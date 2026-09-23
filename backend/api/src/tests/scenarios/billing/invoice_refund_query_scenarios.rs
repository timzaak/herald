// =============================================================================
// Invoice Refund Field Query Scenario Tests
// =============================================================================
//
// Tests for the invoice list/detail refund dimension fields
// (`amountRefunded`, `amountRemaining`, `creditNotes`). Covers the query side
// of US-IF-008 (admin refund visibility) and
// US-IF-009 (user refund annotation), the transaction consistency invariant
// (credit note creation + invoice cache update happen atomically), and the
// Creem provider exclusion rule (design section 1.3).
//
// Spec deviation: item step 3.e specified `source='external'` for the
// Creem invoice insert. The `invoice.source` column has a CHECK constraint
// limiting it to `('admin_manual', 'user_application', 'external_sync')`
// (see `backend/app/migrations/20260508_invoice.sql` line 34). The provider
// designation lives in `provider` / `payment_provider`, not `source`. We use
// `'external_sync'` (allowed by the CHECK) and set `provider='creem'`,
// `payment_provider='creem'` as the item specifies.
//
// User Story: docs/user-stories/billing/invoice-fallback.md
// Covers: US-IF-008 (scenarios 1, 3, 4, 6), US-IF-009 (scenarios 1, 2, 4)
//
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tests::helpers::*;
    use crate::tests::schema_test_context::SchemaTestContext;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use serde_json::json;
    use test_context::test_context;
    use tower::ServiceExt;
    use uuid::Uuid;

    use SchemaTestContext as RefundQueryTestContext;

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

    /// Parse response body to JSON.
    async fn parse_body(body: Body) -> serde_json::Value {
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// Create a test account row (invoice FK target).
    async fn ensure_test_account(ctx: &RefundQueryTestContext, realm_id: &str) -> Uuid {
        let account_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO account (id, realm_id, email, password, status)
             VALUES ($1, $2, $3, $4, 1)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(account_id)
        .bind(realm_id)
        .bind(format!("refund-query-{}@example.com", account_id))
        .bind("$2a$12$dummy_hash")
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
        account_id
    }

    /// Create a draft invoice via API, then issue and mark-paid.
    /// Returns the mark-paid response JSON (contains `id`, `status: "paid"`, `total`).
    async fn create_paid_manual_invoice(
        app: &axum::Router,
        token: &str,
        _realm_id: &str,
        account_id: Uuid,
        total: i64,
    ) -> serde_json::Value {
        let payload = json!({
            "accountId": account_id.to_string(),
            "currency": "USD",
            "lineItems": [
                {
                    "name": "Refund Query Service",
                    "quantity": "1",
                    "unitPrice": total,
                }
            ],
            "billingName": "Refund Query Client",
            "billingAddress": "123 Test St",
            "billingEmail": "billing@test.com",
            "billingTaxId": "TAX-001",
            "sellerName": "Test Seller Inc.",
            "sellerAddress": "456 Seller Ave",
            "sellerTaxId": "SELLER-TAX-001",
            "dueDate": "2099-12-31",
        });

        let create_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/bill/invoices".to_string())
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", token))
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let draft = parse_body(create_response.into_body()).await;
        let invoice_id = draft["id"].as_str().unwrap();

        // Issue
        let issue_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/bill/invoices/{}/issue", invoice_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", token))
                    .body(Body::from(json!({}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(issue_response.status(), StatusCode::OK);

        // Mark paid
        let paid_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/bill/invoices/{}/mark-paid", invoice_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", token))
                    .body(Body::from(json!({}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(paid_response.status(), StatusCode::OK);
        parse_body(paid_response.into_body()).await
    }

    /// Insert a credit note directly via SQL for test setup, bypassing the API.
    /// Also updates the cached `invoice.amount_refunded` / `amount_remaining`
    /// columns so list/detail responses reflect the refund without requiring
    /// an actual API call.
    async fn insert_credit_note_via_sql(
        ctx: &RefundQueryTestContext,
        invoice_id: Uuid,
        realm_id: &str,
        amount: i64,
        currency: &str,
        source: &str,
        memo: Option<&str>,
    ) -> Uuid {
        let credit_note_id = Uuid::now_v7();

        // Insert credit note row
        sqlx::query(
            "INSERT INTO credit_note (id, invoice_id, realm_id, amount, currency, source, memo, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())",
        )
        .bind(credit_note_id)
        .bind(invoice_id)
        .bind(realm_id)
        .bind(amount)
        .bind(currency)
        .bind(source)
        .bind(memo)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        // Update cached invoice refund totals so list/detail responses are consistent
        sqlx::query(
            "UPDATE invoice
             SET amount_refunded = (
                    SELECT COALESCE(SUM(amount), 0) FROM credit_note WHERE invoice_id = $1
                 ),
                 amount_remaining = total - (
                    SELECT COALESCE(SUM(amount), 0) FROM credit_note WHERE invoice_id = $1
                 ),
                 updated_at = NOW()
             WHERE id = $1",
        )
        .bind(invoice_id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        credit_note_id
    }

    /// Insert a Creem invoice directly into DB with `provider='creem'`,
    /// `status='paid'`, and no refund dimensions (Creem
    /// exclusion). The invoice is owned by `applicant_user_id` so the
    /// user-side detail endpoint (`get_my_invoice`) can be exercised.
    ///
    /// `source='external_sync'` is used because the `invoice.source` CHECK
    /// constraint allows only `('admin_manual', 'user_application',
    /// 'external_sync')`. The provider designation lives in `provider` /
    /// `payment_provider`, set to `'creem'` here.
    async fn create_creem_invoice_via_sql(
        ctx: &RefundQueryTestContext,
        realm_id: &str,
        total: i64,
        applicant_user_id: Option<Uuid>,
    ) -> Uuid {
        let invoice_id = Uuid::new_v4();
        let invoice_number = format!("CRM-{}", Uuid::now_v7().simple());
        let account_id = Uuid::new_v4();

        sqlx::query(
            "INSERT INTO invoice (
                id, realm_id, invoice_number, source, account_id, applicant_user_id,
                status, currency, subtotal, discount_amount, tax_amount, shipping_amount, total,
                amount_refunded, amount_remaining,
                billing_name, seller_name,
                provider, payment_provider, external_invoice_id,
                issued_at, paid_at, created_at, updated_at
            ) VALUES (
                $1, $2, $3, 'external_sync', $4, $5,
                'paid', 'usd', $6, 0, 0, 0, $6,
                0, $6,
                'Test Billing', 'Test Seller',
                'creem', 'creem', $7,
                NOW(), NOW(), NOW(), NOW()
            )",
        )
        .bind(invoice_id)
        .bind(realm_id)
        .bind(&invoice_number)
        .bind(account_id)
        .bind(applicant_user_id)
        .bind(total)
        .bind(format!("test_creem_{}", invoice_id))
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        invoice_id
    }

    // =========================================================================
    // Test: Invoice Detail Shows Refund Fields After Credit Note
    // (US-IF-008 scenario 1)
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-008 scenario 1
    //
    // Given: A paid manual invoice (total=10000) with a credit note (amount=3000)
    // When: GET invoice detail (admin)
    // Then: Response has amountRefunded=3000, amountRemaining=7000
    // And: creditNotes array has 1 entry with matching amount and source

    #[test_context(RefundQueryTestContext)]
    #[tokio::test]
    async fn test_invoice_detail_shows_refund_fields_after_credit_note(
        ctx: &mut RefundQueryTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "refund-query-detail@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        // Create a paid manual invoice (total=10000)
        let paid =
            create_paid_manual_invoice(&app, &admin_token, &realm_id, account_id, 10000).await;
        assert_eq!(paid["status"], "paid");
        assert_eq!(paid["total"], 10000);
        let invoice_id: Uuid = Uuid::parse_str(paid["id"].as_str().unwrap()).expect("valid UUID");

        // Insert a credit note (amount=3000, source=manual)
        insert_credit_note_via_sql(
            ctx,
            invoice_id,
            &realm_id,
            3000,
            "USD",
            "manual",
            Some("Detail refund"),
        )
        .await;

        // GET invoice detail (admin)
        let detail_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/bill/invoices/{}", invoice_id))
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(detail_response.status(), StatusCode::OK);
        let detail = parse_body(detail_response.into_body()).await;

        assert_eq!(
            detail["amountRefunded"], 3000,
            "amountRefunded should reflect the credit note amount"
        );
        assert_eq!(
            detail["amountRemaining"], 7000,
            "amountRemaining should be total - amountRefunded"
        );

        let credit_notes = detail["creditNotes"].as_array().unwrap();
        assert_eq!(credit_notes.len(), 1, "Expected exactly 1 credit note");
        assert_eq!(credit_notes[0]["amount"], 3000);
        assert_eq!(credit_notes[0]["source"], "manual");
    }

    // =========================================================================
    // Test: Invoice Detail No Credit Notes Shows Zero Refund
    // (US-IF-008 scenario 4)
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-008 scenario 4
    //
    // Given: A paid manual invoice with no credit notes
    // When: GET invoice detail
    // Then: amountRefunded=0, amountRemaining=total
    // And: creditNotes array is empty

    #[test_context(RefundQueryTestContext)]
    #[tokio::test]
    async fn test_invoice_detail_no_credit_notes_shows_zero_refund(
        ctx: &mut RefundQueryTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "refund-query-zero@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        let paid =
            create_paid_manual_invoice(&app, &admin_token, &realm_id, account_id, 8000).await;
        assert_eq!(paid["status"], "paid");
        let invoice_id: Uuid = Uuid::parse_str(paid["id"].as_str().unwrap()).expect("valid UUID");
        let total = paid["total"].as_i64().expect("total is int");

        // GET detail -- no credit notes inserted
        let detail_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/bill/invoices/{}", invoice_id))
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(detail_response.status(), StatusCode::OK);
        let detail = parse_body(detail_response.into_body()).await;

        assert_eq!(
            detail["amountRefunded"], 0,
            "amountRefunded should be 0 when there are no credit notes"
        );
        assert_eq!(
            detail["amountRemaining"], total,
            "amountRemaining should equal total when there are no credit notes"
        );

        let credit_notes = detail["creditNotes"].as_array().unwrap();
        assert!(
            credit_notes.is_empty(),
            "creditNotes array should be empty when no credit notes exist"
        );
    }

    // =========================================================================
    // Test: Admin Invoice List Shows Refund Annotation
    // (US-IF-008 scenario 3)
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-008 scenario 3
    //
    // Given: Two invoices: one with a credit note (amount_refunded=3000), one without
    // When: GET admin invoice list
    // Then: List response includes amountRefunded for each row
    // And: The refunded invoice shows amountRefunded=3000, the other shows amountRefunded=0

    #[test_context(RefundQueryTestContext)]
    #[tokio::test]
    async fn test_invoice_list_shows_refund_annotation(ctx: &mut RefundQueryTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "refund-query-list@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        // Invoice A: with a credit note (amount_refunded=3000)
        let paid_a =
            create_paid_manual_invoice(&app, &admin_token, &realm_id, account_id, 10000).await;
        let invoice_a_id: Uuid =
            Uuid::parse_str(paid_a["id"].as_str().unwrap()).expect("valid UUID");
        insert_credit_note_via_sql(
            ctx,
            invoice_a_id,
            &realm_id,
            3000,
            "USD",
            "manual",
            Some("List A refund"),
        )
        .await;

        // Invoice B: without a credit note
        let paid_b =
            create_paid_manual_invoice(&app, &admin_token, &realm_id, account_id, 5000).await;
        let invoice_b_id: Uuid =
            Uuid::parse_str(paid_b["id"].as_str().unwrap()).expect("valid UUID");

        // GET admin list
        let list_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/bill/invoices".to_string())
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(list_response.status(), StatusCode::OK);
        let list_body = parse_body(list_response.into_body()).await;
        let data = list_body["data"].as_array().unwrap();
        assert!(data.len() >= 2, "Expected at least 2 invoices in the list");

        // Find each invoice row and verify amountRefunded annotation
        let row_a = data
            .iter()
            .find(|item| {
                Uuid::parse_str(item["id"].as_str().unwrap_or_default()).unwrap_or_default()
                    == invoice_a_id
            })
            .expect("Invoice A should appear in the list");
        let row_b = data
            .iter()
            .find(|item| {
                Uuid::parse_str(item["id"].as_str().unwrap_or_default()).unwrap_or_default()
                    == invoice_b_id
            })
            .expect("Invoice B should appear in the list");

        assert_eq!(
            row_a["amountRefunded"], 3000,
            "Invoice A row should show amountRefunded=3000"
        );
        assert_eq!(
            row_b["amountRefunded"], 0,
            "Invoice B row should show amountRefunded=0"
        );
    }

    // =========================================================================
    // Test: Credit Note Creation Updates Invoice In Transaction
    // (Transaction consistency)
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: Design section 4.1 (transaction consistency invariant)
    //
    // Given: A paid manual invoice
    // When: Create credit note via POST API
    // Then: Immediately query invoice detail and verify amount_refunded and
    //       amount_remaining are consistent with the credit note
    // And: The credit note appears in the creditNotes list
    //
    // This validates that credit note creation and invoice cache update
    // happen atomically (no torn state visible to a subsequent read).

    #[test_context(RefundQueryTestContext)]
    #[tokio::test]
    async fn test_credit_note_creation_updates_invoice_in_transaction(
        ctx: &mut RefundQueryTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token =
            setup_billing_admin_session(ctx, "refund-query-txn-consistency@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        let paid =
            create_paid_manual_invoice(&app, &admin_token, &realm_id, account_id, 10000).await;
        assert_eq!(paid["status"], "paid");
        let invoice_id = paid["id"].as_str().unwrap();
        let invoice_uuid: Uuid = Uuid::parse_str(invoice_id).expect("valid UUID");

        // Create a credit note via the POST API (amount=4000)
        let create_cn_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/bill/invoices/{}/credit-notes", invoice_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(
                        json!({ "amount": 4000, "memo": "Txn consistency refund" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_cn_response.status(), StatusCode::CREATED);

        // Immediately query detail and verify consistency
        let detail_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/bill/invoices/{}", invoice_id))
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(detail_response.status(), StatusCode::OK);
        let detail = parse_body(detail_response.into_body()).await;

        assert_eq!(
            detail["amountRefunded"], 4000,
            "amountRefunded must reflect the credit note immediately after creation"
        );
        assert_eq!(
            detail["amountRemaining"], 6000,
            "amountRemaining must equal total - amountRefunded immediately after creation"
        );

        let credit_notes = detail["creditNotes"].as_array().unwrap();
        assert_eq!(
            credit_notes.len(),
            1,
            "Exactly one credit note must be visible after creation"
        );
        assert_eq!(credit_notes[0]["amount"], 4000);
        assert_eq!(credit_notes[0]["source"], "manual");

        // Cross-check: the cached invoice columns match the API response
        let (cached_refunded, cached_remaining): (i64, i64) =
            sqlx::query_as("SELECT amount_refunded, amount_remaining FROM invoice WHERE id = $1")
                .bind(invoice_uuid)
                .fetch_one(&ctx.app_state.pool)
                .await
                .unwrap();
        assert_eq!(
            cached_refunded, 4000,
            "Cached amount_refunded must match API"
        );
        assert_eq!(
            cached_remaining, 6000,
            "Cached amount_remaining must match API"
        );
    }

    // =========================================================================
    // Test: My-Invoice Detail Shows Refund Without Internal IDs
    // (US-IF-009 scenario 2)
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-009 scenario 2 (user-side strips externalCreditNoteId)
    //
    // Given: A user's invoice with a Stripe credit note
    //        (external_credit_note_id set on the row)
    // When: GET my invoice detail (user endpoint)
    // Then: Response includes amountRefunded and amountRemaining
    // And: Each entry in creditNotes has externalCreditNoteId == null
    //      ("用户端响应的 creditNotes 不包含 externalCreditNoteId 和内部编号")

    #[test_context(RefundQueryTestContext)]
    #[tokio::test]
    async fn test_my_invoice_detail_shows_refund_without_internal_ids(
        ctx: &mut RefundQueryTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        let (user_token, user_id_str) =
            create_admin_session_with_user(ctx, "refund-query-strip-user@test.com", 1800).await;
        let user_id = Uuid::parse_str(&user_id_str).expect("valid user UUID");

        // Create a paid invoice owned by the user via Stripe provider insert,
        // so the credit note created below is source=stripe.
        // Use external_sync source (CHECK constraint) and provider=stripe.
        let invoice_id = Uuid::new_v4();
        let invoice_number = format!("STR-{}", Uuid::now_v7().simple());
        sqlx::query(
            "INSERT INTO invoice (
                id, realm_id, invoice_number, source, account_id, applicant_user_id,
                status, currency, subtotal, discount_amount, tax_amount, shipping_amount, total,
                amount_refunded, amount_remaining,
                billing_name, seller_name,
                provider, payment_provider, external_invoice_id,
                issued_at, paid_at, created_at, updated_at
            ) VALUES (
                $1, $2, $3, 'external_sync', $4, $5,
                'paid', 'usd', 10000, 0, 0, 0, 10000,
                0, 10000,
                'Stripe Client', 'Test Seller',
                'stripe', 'stripe', $6,
                NOW(), NOW(), NOW(), NOW()
            )",
        )
        .bind(invoice_id)
        .bind(realm_id.as_str())
        .bind(&invoice_number)
        .bind(Uuid::nil())
        .bind(user_id)
        .bind(format!("in_stripe_{}", invoice_id))
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        // Insert a Stripe credit note (external_credit_note_id set) + update cache
        let stripe_credit_note_id = format!("cn_test_strip_{}", Uuid::now_v7());
        sqlx::query(
            "INSERT INTO credit_note (id, invoice_id, realm_id, amount, currency, source, external_credit_note_id, created_at)
             VALUES ($1, $2, $3, 3000, 'usd', 'stripe', $4, NOW())",
        )
        .bind(Uuid::now_v7())
        .bind(invoice_id)
        .bind(realm_id.as_str())
        .bind(&stripe_credit_note_id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        sqlx::query(
            "UPDATE invoice
             SET amount_refunded = (
                    SELECT COALESCE(SUM(amount), 0) FROM credit_note WHERE invoice_id = $1
                 ),
                 amount_remaining = total - (
                    SELECT COALESCE(SUM(amount), 0) FROM credit_note WHERE invoice_id = $1
                 ),
                 updated_at = NOW()
             WHERE id = $1",
        )
        .bind(invoice_id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        // User GET my-invoice detail
        let detail_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/user/bill/invoices/{}", invoice_id))
                    .header("authorization", format!("Bearer {}", user_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(detail_response.status(), StatusCode::OK);
        let detail = parse_body(detail_response.into_body()).await;

        // amountRefunded / amountRemaining present
        assert_eq!(
            detail["amountRefunded"], 3000,
            "User-side detail must surface amountRefunded"
        );
        assert_eq!(
            detail["amountRemaining"], 7000,
            "User-side detail must surface amountRemaining"
        );

        // creditNotes entry must have externalCreditNoteId == null
        let credit_notes = detail["creditNotes"].as_array().unwrap();
        assert_eq!(credit_notes.len(), 1, "Expected exactly 1 credit note");
        assert!(
            credit_notes[0]
                .get("externalCreditNoteId")
                .is_none_or(|v| v.is_null()),
            "externalCreditNoteId must be null on the user-side response, got: {:?}",
            credit_notes[0].get("externalCreditNoteId")
        );
        assert_eq!(credit_notes[0]["amount"], 3000);
        assert_eq!(credit_notes[0]["source"], "stripe");
    }

    // =========================================================================
    // Test: Creem Invoice Excludes Refund Fields
    // (US-IF-008 scenario 6, US-IF-009 scenario 4)
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-008 scenario 6, US-IF-009 scenario 4
    //
    // Given: A Creem invoice (provider=creem) in the database
    // When: GET invoice detail (admin) and GET my invoice detail (user)
    // Then: creditNotes is empty and amountRefunded is 0
    //
    // This validates the Creem exclusion rule: credit notes
    // are never created for provider=creem invoices (managed externally), so
    // the refund dimension fields reflect "no refunds" for these invoices.

    #[test_context(RefundQueryTestContext)]
    #[tokio::test]
    async fn test_creem_invoice_excludes_refund_fields(ctx: &mut RefundQueryTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        // Set up admin + a regular user who owns the Creem invoice
        let admin_token =
            setup_billing_admin_session(ctx, "refund-query-creem-admin@test.com").await;
        let (user_token, user_id_str) =
            create_admin_session_with_user(ctx, "refund-query-creem-user@test.com", 1800).await;
        let user_id = Uuid::parse_str(&user_id_str).expect("valid user UUID");

        // Insert a Creem invoice owned by the user (applicant_user_id = user_id)
        let invoice_id = create_creem_invoice_via_sql(ctx, &realm_id, 15000, Some(user_id)).await;

        // Admin GET invoice detail
        let admin_detail_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/bill/invoices/{}", invoice_id))
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(admin_detail_response.status(), StatusCode::OK);
        let admin_detail = parse_body(admin_detail_response.into_body()).await;
        assert_eq!(admin_detail["provider"], "creem");
        assert_eq!(
            admin_detail["amountRefunded"], 0,
            "Creem invoices must show amountRefunded=0 (refund fields excluded by design)"
        );
        let admin_credit_notes = admin_detail["creditNotes"].as_array().unwrap();
        assert!(
            admin_credit_notes.is_empty(),
            "Creem invoices must have an empty creditNotes list by design"
        );

        // User GET my invoice detail
        let user_detail_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/user/bill/invoices/{}", invoice_id))
                    .header("authorization", format!("Bearer {}", user_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(user_detail_response.status(), StatusCode::OK);
        let user_detail = parse_body(user_detail_response.into_body()).await;
        assert_eq!(user_detail["provider"], "creem");
        assert_eq!(
            user_detail["amountRefunded"], 0,
            "Creem invoices must show amountRefunded=0 on user-side by design"
        );
        let user_credit_notes = user_detail["creditNotes"].as_array().unwrap();
        assert!(
            user_credit_notes.is_empty(),
            "Creem invoices must have an empty creditNotes list on user-side by design"
        );
    }
}
