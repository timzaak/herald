// =============================================================================
// Invoice PDF Endpoint Scenario Tests
// =============================================================================
//
// Tests for PDF download endpoints: admin download, user download, status
// validation, content-type, content-disposition, and access control.
//
// User Story: docs/user-stories/13-invoice-user-stories.md
// Covers: US-IV-009 (PDF Download) -- all acceptance criteria
//
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tests::helpers::*;
    use crate::tests::schema_test_context::SchemaTestContext;
    use axum::{
        body::Body,
        http::{Request, StatusCode, header},
    };
    use chrono::Datelike;
    use serde_json::json;
    use test_context::test_context;
    use tower::ServiceExt;
    use uuid::Uuid;

    use SchemaTestContext as InvoiceTestContext;

    // Helper: parse response body to JSON
    async fn parse_body(body: axum::body::Body) -> serde_json::Value {
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    // Helper: create a test account row (invoice FK target)
    async fn ensure_test_account(ctx: &InvoiceTestContext, realm_id: &str) -> Uuid {
        let account_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO account (id, realm_id, email, password, status)
             VALUES ($1, $2, $3, $4, 1)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(account_id)
        .bind(realm_id)
        .bind(format!("invoice-pdf-test-{}@example.com", account_id))
        .bind("$2a$12$dummy_hash")
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
        account_id
    }

    /// Helper: create a draft invoice via API, return the full response JSON
    async fn create_draft_invoice(
        app: &axum::Router,
        token: &str,
        _realm_id: &str,
        account_id: Uuid,
        line_items: Vec<serde_json::Value>,
        billing_name: &str,
    ) -> serde_json::Value {
        let payload = json!({
            "accountId": account_id.to_string(),
            "currency": "USD",
            "lineItems": line_items,
            "billingName": billing_name,
            "billingAddress": "123 Test St",
            "billingEmail": "billing@test.com",
            "billingTaxId": "TAX-001",
            "sellerName": "Test Seller Inc.",
            "sellerAddress": "456 Seller Ave",
            "sellerTaxId": "SELLER-TAX-001",
            "dueDate": "2099-12-31",
        });

        let response = app
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

        assert_eq!(response.status(), StatusCode::CREATED);
        parse_body(response.into_body()).await
    }

    /// Create a regular user session (no admin role).
    /// Returns (token, user_id).
    async fn create_regular_user_session(ctx: &InvoiceTestContext, email: &str) -> (String, Uuid) {
        let (token, user_id_str) =
            crate::tests::helpers::create_admin_session_with_user(ctx, email, 1800).await;
        let user_id = Uuid::parse_str(&user_id_str).expect("Invalid user_id format");
        (token, user_id)
    }

    /// Create a user_application invoice directly in the DB and issue it.
    /// Returns the invoice ID.
    async fn create_issued_invoice_for_user(
        ctx: &InvoiceTestContext,
        realm_id: &str,
        applicant_user_id: Uuid,
    ) -> Uuid {
        let invoice_id = Uuid::now_v7();

        let year = chrono::Utc::now().year();
        let seq: i64 = sqlx::query_scalar(
            "INSERT INTO invoice_number_counter (realm_id, year, next_seq, updated_at)
             VALUES ($1, $2, 2, NOW())
             ON CONFLICT (realm_id, year) DO UPDATE SET next_seq = invoice_number_counter.next_seq + 1, updated_at = NOW()
             RETURNING next_seq - 1",
        )
        .bind(realm_id)
        .bind(year)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();

        let invoice_number = format!("INV-{}-{:04}", year, seq);

        sqlx::query(
            "INSERT INTO invoice (
                id, realm_id, invoice_number, source, account_id, applicant_user_id,
                status, currency, subtotal, discount_amount, tax_amount, shipping_amount, total,
                billing_name, billing_address, billing_tax_id,
                seller_name, seller_address, seller_tax_id,
                issued_at, issue_date, due_date, created_at, updated_at
            ) VALUES (
                $1, $2, $3, 'user_application', $4, $5,
                'issued', 'USD', 10000, 0, 0, 0, 10000,
                'PDF Test Client', '123 User St', 'TAX-PDF-001',
                'Seller Inc', '456 Seller Ave', 'SELLER-TAX-001',
                NOW(), CURRENT_DATE, CURRENT_DATE + INTERVAL '30 days', NOW(), NOW()
            )",
        )
        .bind(invoice_id)
        .bind(realm_id)
        .bind(&invoice_number)
        .bind(Uuid::nil())
        .bind(applicant_user_id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        // Insert a line item
        sqlx::query(
            "INSERT INTO invoice_line_item (id, invoice_id, sort_order, name, quantity, unit_price, subtotal)
             VALUES ($1, $2, 1, 'Service Fee', '1', 10000, 10000)",
        )
        .bind(Uuid::now_v7())
        .bind(invoice_id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        // Insert history record
        sqlx::query(
            "INSERT INTO invoice_history (id, invoice_id, event_type, actor_user_id, actor_type, changes, created_at)
             VALUES ($1, $2, 'issued', $3, 'user', '{\"field\":\"status\",\"from\":\"draft\",\"to\":\"issued\"}', NOW())",
        )
        .bind(Uuid::now_v7())
        .bind(invoice_id)
        .bind(applicant_user_id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        invoice_id
    }

    // =========================================================================
    // Admin PDF Download Tests
    // =========================================================================

    // User Story: docs/user-stories/13-invoice-user-stories.md
    // Covers: US-IV-009 -- admin download issued invoice returns application/pdf
    //
    // Given: An admin with billing.view permission and an issued invoice
    // When: GET /api/bill/invoices/{invoiceId}/pdf
    // Then: Returns 200 with Content-Type: application/pdf

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_admin_download_issued_invoice_pdf(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token =
            setup_billing_admin_session(ctx, "invoice-pdf-admin-issued@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        // Create and issue an invoice
        let line_items = vec![json!({
            "name": "Service Fee",
            "quantity": "1",
            "unitPrice": 9900,
        })];

        let inv = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items,
            "PDF Test Client",
        )
        .await;

        let invoice_id = inv["id"].as_str().unwrap();

        // Issue the invoice
        let issue_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/bill/invoices/{}/issue", invoice_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(json!({}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(issue_response.status(), StatusCode::OK);

        // Download the PDF
        let pdf_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/bill/invoices/{}/pdf", invoice_id))
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(pdf_response.status(), StatusCode::OK);

        // Verify Content-Type
        let content_type = pdf_response
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("Missing Content-Type header")
            .to_str()
            .unwrap();
        assert!(
            content_type.contains("application/pdf"),
            "Expected Content-Type to contain 'application/pdf', got: {}",
            content_type
        );

        // Verify body starts with %PDF
        let body_bytes = axum::body::to_bytes(pdf_response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(body_bytes.len() > 100, "PDF body should have content");
        assert_eq!(&body_bytes[0..4], b"%PDF", "PDF body must start with %PDF");
    }

    // User Story: docs/user-stories/13-invoice-user-stories.md
    // Covers: US-IV-009 -- draft invoice PDF download returns 409
    //
    // Given: An admin with billing.view permission and a draft invoice
    // When: GET /api/bill/invoices/{invoiceId}/pdf
    // Then: Returns 409 Conflict

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_admin_download_draft_invoice_pdf_rejected(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token =
            setup_billing_admin_session(ctx, "invoice-pdf-admin-draft@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        let line_items = vec![json!({
            "name": "Draft Item",
            "quantity": "1",
            "unitPrice": 5000,
        })];

        let inv = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items,
            "Draft PDF Client",
        )
        .await;

        let invoice_id = inv["id"].as_str().unwrap();
        assert_eq!(inv["status"], "draft");

        // Try to download PDF for a draft invoice
        let pdf_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/bill/invoices/{}/pdf", invoice_id))
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            pdf_response.status(),
            StatusCode::CONFLICT,
            "Expected 409 Conflict when downloading PDF for a draft invoice"
        );
    }

    // User Story: docs/user-stories/13-invoice-user-stories.md
    // Covers: US-IV-009 -- Content-Disposition has correct filename format
    //
    // Given: An admin and an issued invoice with number INV-YYYY-NNNN
    // When: GET /api/bill/invoices/{invoiceId}/pdf
    // Then: Content-Disposition header contains "attachment; filename=\"INV-YYYY-NNNN.pdf\""

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_pdf_content_disposition_filename(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token =
            setup_billing_admin_session(ctx, "invoice-pdf-disposition@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        let line_items = vec![json!({
            "name": "Service",
            "quantity": "1",
            "unitPrice": 10000,
        })];

        let inv = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items,
            "Disposition Test Client",
        )
        .await;

        let invoice_id = inv["id"].as_str().unwrap();
        let invoice_number = inv["invoiceNumber"].as_str().unwrap();

        // Issue the invoice
        let issue_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/bill/invoices/{}/issue", invoice_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(json!({}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(issue_response.status(), StatusCode::OK);

        // Download PDF and check Content-Disposition
        let pdf_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/bill/invoices/{}/pdf", invoice_id))
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(pdf_response.status(), StatusCode::OK);

        let content_disposition = pdf_response
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .expect("Missing Content-Disposition header")
            .to_str()
            .unwrap();

        let expected_filename = format!("attachment; filename=\"{}.pdf\"", invoice_number);
        assert_eq!(
            content_disposition, expected_filename,
            "Content-Disposition should match expected filename format"
        );
    }

    // =========================================================================
    // User PDF Download Tests
    // =========================================================================

    // User Story: docs/user-stories/13-invoice-user-stories.md
    // Covers: US-IV-009 -- user downloads own issued invoice PDF
    //
    // Given: A regular user with an issued invoice they own
    // When: GET /api/bill/my/invoices/{invoiceId}/pdf
    // Then: Returns 200 with Content-Type: application/pdf

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_user_download_own_invoice_pdf(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        // Create a regular user
        let (user_token, user_id) =
            create_regular_user_session(ctx, "invoice-pdf-user-own@test.com").await;

        // Create an issued invoice belonging to this user
        let invoice_id = create_issued_invoice_for_user(ctx, &realm_id, user_id).await;

        // User downloads their own PDF
        let pdf_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/user/bill/invoices/{}/pdf", invoice_id))
                    .header("authorization", format!("Bearer {}", user_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(pdf_response.status(), StatusCode::OK);

        let content_type = pdf_response
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("Missing Content-Type header")
            .to_str()
            .unwrap();
        assert!(
            content_type.contains("application/pdf"),
            "Expected Content-Type to contain 'application/pdf', got: {}",
            content_type
        );

        // Verify body is a valid PDF
        let body_bytes = axum::body::to_bytes(pdf_response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(body_bytes.len() > 100, "PDF body should have content");
        assert_eq!(&body_bytes[0..4], b"%PDF", "PDF body must start with %PDF");
    }

    // User Story: docs/user-stories/13-invoice-user-stories.md
    // Covers: US-IV-009 -- user cannot download other users' invoice PDF
    //
    // Given: User A and User B, User B has an issued invoice
    // When: User A calls GET /api/bill/my/invoices/{user_b_invoice_id}/pdf
    // Then: Returns 403 Forbidden

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_user_download_others_invoice_pdf_rejected(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        // Create two regular users
        let (user_a_token, _user_a_id) =
            create_regular_user_session(ctx, "invoice-pdf-user-a@test.com").await;
        let (_user_b_token, user_b_id) =
            create_regular_user_session(ctx, "invoice-pdf-user-b@test.com").await;

        // Create an issued invoice for User B
        let invoice_id = create_issued_invoice_for_user(ctx, &realm_id, user_b_id).await;

        // User A tries to download User B's invoice PDF
        let pdf_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/user/bill/invoices/{}/pdf", invoice_id))
                    .header("authorization", format!("Bearer {}", user_a_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            pdf_response.status(),
            StatusCode::FORBIDDEN,
            "Expected 403 when user tries to download another user's invoice PDF"
        );
    }

    // User Story: docs/user-stories/13-invoice-user-stories.md
    // Covers: US-IV-009 -- user tries to download their own draft invoice PDF
    //
    // Given: A regular user with a draft invoice they own
    // When: GET /api/bill/my/invoices/{invoiceId}/pdf
    // Then: Returns 409 Conflict

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_user_download_own_draft_invoice_pdf_rejected(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        let (user_token, user_id) =
            create_regular_user_session(ctx, "invoice-pdf-user-draft@test.com").await;

        // Create a draft invoice directly in the DB
        let invoice_id = Uuid::now_v7();
        let year = chrono::Utc::now().year();
        let seq: i64 = sqlx::query_scalar(
            "INSERT INTO invoice_number_counter (realm_id, year, next_seq, updated_at)
             VALUES ($1, $2, 2, NOW())
             ON CONFLICT (realm_id, year) DO UPDATE SET next_seq = invoice_number_counter.next_seq + 1, updated_at = NOW()
             RETURNING next_seq - 1",
        )
        .bind(realm_id.as_str())
        .bind(year)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();

        let invoice_number = format!("INV-{}-{:04}", year, seq);

        sqlx::query(
            "INSERT INTO invoice (
                id, realm_id, invoice_number, source, account_id, applicant_user_id,
                status, currency, subtotal, discount_amount, tax_amount, shipping_amount, total,
                billing_name, billing_address, billing_tax_id,
                seller_name, seller_address, seller_tax_id,
                due_date, created_at, updated_at
            ) VALUES (
                $1, $2, $3, 'user_application', $4, $5,
                'draft', 'USD', 5000, 0, 0, 0, 5000,
                'Draft User Client', '123 User St', 'TAX-DRAFT-001',
                'Seller Inc', '456 Seller Ave', 'SELLER-TAX-001',
                CURRENT_DATE + INTERVAL '30 days', NOW(), NOW()
            )",
        )
        .bind(invoice_id)
        .bind(realm_id.as_str())
        .bind(&invoice_number)
        .bind(Uuid::nil())
        .bind(user_id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        // User tries to download PDF for their draft invoice
        let pdf_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/user/bill/invoices/{}/pdf", invoice_id))
                    .header("authorization", format!("Bearer {}", user_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            pdf_response.status(),
            StatusCode::CONFLICT,
            "Expected 409 when user tries to download PDF for their own draft invoice"
        );
    }
}
