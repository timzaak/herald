// =============================================================================
// Invoice Provider & Policy Guard Scenario Tests
// =============================================================================
//
// Tests for provider model, invoice policy logic matrix, Creem MoR protection,
// external invoice readonly guards, provider filtering, PDF dual-track behavior,
// and user-facing external invoice display.
//
// User Story: docs/user-stories/billing/invoice-fallback.md
// Covers: US-IF-001, US-IF-003, US-IF-004, US-IF-005, US-IF-006
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
        .bind(format!("invoice-provider-test-{}@example.com", account_id))
        .bind("$2a$12$dummy_hash")
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
        account_id
    }

    // Helper: create a draft invoice via API, return the full response JSON
    async fn create_draft_invoice(
        app: &axum::Router,
        token: &str,
        realm_id: &str,
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
                    .uri(format!("/api/bill/{}/invoices", realm_id))
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

    /// Insert an external invoice record directly into the database,
    /// simulating a provider sync (Stripe/Creem webhook).
    /// Returns the invoice UUID.
    async fn create_external_invoice_in_db(
        ctx: &InvoiceTestContext,
        realm_id: &str,
        provider: &str,
        external_invoice_id: &str,
        status: &str,
        external_hosted_url: Option<&str>,
        external_pdf_url: Option<&str>,
    ) -> Uuid {
        let invoice_id = Uuid::now_v7();

        // Generate a sequential invoice number
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

        let invoice_number = format!("EXT-{}-{}", provider.to_uppercase(), external_invoice_id);

        sqlx::query(
            "INSERT INTO invoice (
                id, realm_id, invoice_number, source, status, currency,
                subtotal, discount_amount, tax_amount, shipping_amount, total,
                provider, payment_provider, external_invoice_id,
                external_hosted_url, external_pdf_url,
                created_at, updated_at
            ) VALUES (
                $1, $2, $3, 'external_sync', $4, 'USD',
                10000, 0, 0, 0, 10000,
                $5, $5, $6,
                $7, $8,
                NOW(), NOW()
            )",
        )
        .bind(invoice_id)
        .bind(realm_id)
        .bind(&invoice_number)
        .bind(status)
        .bind(provider)
        .bind(external_invoice_id)
        .bind(external_hosted_url)
        .bind(external_pdf_url)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        // Insert a line item so the invoice has content
        sqlx::query(
            "INSERT INTO invoice_line_item (id, invoice_id, sort_order, name, quantity, unit_price, subtotal)
             VALUES ($1, $2, 1, 'External Service', '1', 10000, 10000)",
        )
        .bind(Uuid::now_v7())
        .bind(invoice_id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        // Insert history record
        sqlx::query(
            "INSERT INTO invoice_history (id, invoice_id, event_type, actor_type, changes, created_at)
             VALUES ($1, $2, 'created', 'system', '{\"field\":\"status\",\"from\":null,\"to\":\"draft\"}', NOW())",
        )
        .bind(Uuid::now_v7())
        .bind(invoice_id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        // Suppress unused warning for seq
        let _ = seq;

        invoice_id
    }

    /// Set the invoice policy for a realm by inserting/updating realm_config.
    /// provider_capabilities example: `r#"{"stripe":{"external_invoice_enabled":true}}"#`
    async fn set_invoice_policy(
        ctx: &InvoiceTestContext,
        realm_id: &str,
        policy: &str,
        provider_capabilities: &str,
    ) {
        let config_value = json!({
            "policy": policy,
            "provider_capabilities": serde_json::from_str::<serde_json::Value>(provider_capabilities).unwrap(),
        })
        .to_string();

        sqlx::query(
            "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled, created_at, updated_at)
             VALUES ($1, 'invoice_policy', 'policy', $2, true, NOW(), NOW())
             ON CONFLICT (realm_id, config_type, config_key) DO UPDATE SET config_value = $2, enabled = true, updated_at = NOW()",
        )
        .bind(realm_id)
        .bind(&config_value)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
    }

    // =========================================================================
    // Group 1: Provider Model & Policy Matrix (US-IF-001)
    // =========================================================================

    // =========================================================================
    // Test: Invoice list shows provider field
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-004 scenario 1 -- invoice list displays provider field
    //
    // Given: A manual invoice and an external Stripe invoice in the database
    // When: GET /invoices (admin)
    // Then: Each invoice in the list has a "provider" field
    // And: Manual invoices show provider="manual", Stripe invoices show provider="stripe"

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_invoice_list_shows_provider_field(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-provider-list@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        // Create a manual invoice via API
        let line_items = vec![json!({
            "name": "Manual Service",
            "quantity": "1",
            "unitPrice": 5000,
        })];
        let _manual_inv = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items,
            "Manual Client",
        )
        .await;

        // Create an external Stripe invoice via direct SQL
        let _stripe_inv_id = create_external_invoice_in_db(
            ctx,
            &realm_id,
            "stripe",
            "in_test_stripe_001",
            "issued",
            Some("https://stripe.com/invoice/001"),
            Some("https://stripe.com/invoice/001/pdf"),
        )
        .await;

        // GET /invoices
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/bill/{}/invoices", realm_id))
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = parse_body(response.into_body()).await;
        assert_eq!(body["total"], 2);

        let data = body["data"].as_array().unwrap();
        let has_manual = data.iter().any(|inv| inv["provider"] == "manual");
        let has_stripe = data.iter().any(|inv| inv["provider"] == "stripe");
        assert!(has_manual, "Expected at least one manual invoice in list");
        assert!(has_stripe, "Expected at least one stripe invoice in list");
        let stripe_invoice = data
            .iter()
            .find(|inv| inv["provider"] == "stripe")
            .expect("Expected a stripe invoice in list");
        assert_eq!(
            stripe_invoice["externalPdfUrl"], "https://stripe.com/invoice/001/pdf",
            "Expected list response to include externalPdfUrl for provider invoices"
        );
    }

    // =========================================================================
    // Test: Invoice detail shows provider and external URLs
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-004 scenario 2 -- external invoice detail includes provider and URLs
    //
    // Given: An external Stripe invoice in the database with hosted_url and pdf_url
    // When: GET /invoices/{id} (admin)
    // Then: Response contains provider="stripe", externalHostedUrl, externalPdfUrl

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_invoice_detail_shows_provider_and_external_urls(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token =
            setup_billing_admin_session(ctx, "invoice-provider-detail@test.com").await;

        // Create an external Stripe invoice via direct SQL
        let stripe_inv_id = create_external_invoice_in_db(
            ctx,
            &realm_id,
            "stripe",
            "in_test_stripe_002",
            "issued",
            Some("https://stripe.com/invoice/002"),
            Some("https://stripe.com/invoice/002/pdf"),
        )
        .await;

        // GET /invoices/{id}
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/bill/{}/invoices/{}", realm_id, stripe_inv_id))
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = parse_body(response.into_body()).await;

        // Verify provider field
        assert_eq!(body["provider"], "stripe");

        // Verify external URL fields are present
        assert_eq!(
            body["externalHostedUrl"], "https://stripe.com/invoice/002",
            "Expected externalHostedUrl to be set"
        );
        assert_eq!(
            body["externalPdfUrl"], "https://stripe.com/invoice/002/pdf",
            "Expected externalPdfUrl to be set"
        );

        // Verify payment provider is also set
        assert_eq!(body["paymentProvider"], "stripe");
    }

    // =========================================================================
    // Test: provider_first policy allows manual fallback
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-001 scenario 1 -- provider_first strategy allows manual creation
    //
    // Given: invoice_policy = provider_first
    // When: Admin creates a manual invoice (no external provider involved)
    // Then: Returns 201, invoice created successfully

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_invoice_policy_provider_first_allows_manual_fallback(
        ctx: &mut InvoiceTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token =
            setup_billing_admin_session(ctx, "invoice-policy-provider-first@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        // Set policy to provider_first
        set_invoice_policy(
            ctx,
            &realm_id,
            "provider_first",
            r#"{"stripe":{"external_invoice_enabled":true},"creem":{"external_invoice_enabled":true}}"#,
        )
        .await;

        let line_items = vec![json!({
            "name": "Manual Service",
            "quantity": "1",
            "unitPrice": 5000,
        })];

        let inv = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items,
            "Manual Fallback Client",
        )
        .await;

        assert_eq!(inv["status"], "draft");
        assert_eq!(inv["provider"], "manual");
    }

    // =========================================================================
    // Test: manual_only policy allows creation
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-001 scenario 2 -- manual_only strategy allows manual creation,
    //         but Creem transactions are still rejected
    //
    // Given: invoice_policy = manual_only
    // When: Admin creates a manual invoice (no Creem link)
    // Then: Returns 201, invoice created successfully

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_invoice_policy_manual_only_allows_creation(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token =
            setup_billing_admin_session(ctx, "invoice-policy-manual-only@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        // Set policy to manual_only
        set_invoice_policy(
            ctx,
            &realm_id,
            "manual_only",
            r#"{"stripe":{"external_invoice_enabled":false},"creem":{"external_invoice_enabled":false}}"#,
        )
        .await;

        let line_items = vec![json!({
            "name": "Manual Only Service",
            "quantity": "1",
            "unitPrice": 8000,
        })];

        let inv = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items,
            "Manual Only Client",
        )
        .await;

        assert_eq!(inv["status"], "draft");
        assert_eq!(inv["provider"], "manual");
    }

    // =========================================================================
    // Test: none policy rejects manual creation
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-001 scenario 6 -- none policy forbids invoice creation
    //
    // Given: invoice_policy = none
    // When: Admin creates a manual invoice
    // Then: Returns 403 with "Invoice creation is disabled by policy"

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_invoice_policy_none_rejects_manual_creation(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-policy-none@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        // Set policy to none
        set_invoice_policy(
            ctx,
            &realm_id,
            "none",
            r#"{"stripe":{"external_invoice_enabled":false}}"#,
        )
        .await;

        let payload = json!({
            "accountId": account_id.to_string(),
            "currency": "USD",
            "lineItems": [{
                "name": "Forbidden Item",
                "quantity": "1",
                "unitPrice": 5000,
            }],
            "billingName": "None Policy Client",
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
                    .uri(format!("/api/bill/{}/invoices", realm_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "Expected 403 Forbidden when invoice_policy is 'none'"
        );

        let body = parse_body(response.into_body()).await;
        let error_msg = body["error"].as_str().unwrap_or("");
        assert!(
            error_msg.contains("disabled by policy") || error_msg.contains("policy"),
            "Expected error about disabled policy, got: {}",
            error_msg
        );
    }

    // =========================================================================
    // Test: none policy rejects user apply
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-005 scenario -- user apply invoice rejected when policy=none
    //
    // Given: invoice_policy = none
    // And: A regular user session
    // When: POST /my/invoices (apply for invoice)
    // Then: Returns 403 with "Invoice creation is disabled by policy"

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_invoice_policy_none_rejects_user_apply(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let _admin_token =
            setup_billing_admin_session(ctx, "invoice-policy-none-admin@test.com").await;

        // Set policy to none
        set_invoice_policy(
            ctx,
            &realm_id,
            "none",
            r#"{"stripe":{"external_invoice_enabled":false}}"#,
        )
        .await;

        // Create a regular user session
        let (user_token, user_id) = crate::tests::helpers::create_admin_session_with_user(
            ctx,
            "user-apply-none@test.com",
            1800,
        )
        .await;
        let user_uuid = Uuid::parse_str(&user_id).expect("Invalid user_id format");

        // Create a payment_attempt to link (needed by apply_invoice endpoint)
        let pa_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO payment_attempts (id, realm_id, user_id, payment_provider, target_type, target_id, amount, currency, status, expires_at, created_at)
             VALUES ($1, $2, $3, 'stripe', 'entitlement_mapping', $4, 5000, 'USD', 'completed', NOW() + INTERVAL '1 hour', NOW())",
        )
        .bind(pa_id)
        .bind(realm_id.as_str())
        .bind(user_uuid)
        .bind(Uuid::now_v7())
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        let apply_payload = json!({
            "paymentAttemptId": pa_id.to_string(),
            "currency": "USD",
            "billingName": "User Apply Client",
            "billingAddress": "123 User St",
            "billingTaxId": "TAX-USER-001",
            "dueDate": "2099-12-31",
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/user/bill/invoices")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", user_token))
                    .body(Body::from(apply_payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "Expected 403 Forbidden when user applies invoice with policy=none"
        );
    }

    // =========================================================================
    // Group 2: Creem MoR Protection (US-IF-001 scenario 4, US-IF-003)
    // =========================================================================

    // =========================================================================
    // Test: Create invoice for Creem payment rejected
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-001 scenario 4, US-IF-003 -- Creem MoR protection on admin create
    //
    // Given: A payment_attempt with payment_provider = 'creem'
    // When: Admin creates a manual invoice linked to that payment_attempt
    // Then: Returns 400 with "creem transactions are managed by the platform
    //       as Merchant of Record"

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_create_invoice_for_creem_payment_rejected(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-creem-mor@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        // Create a payment_attempt with payment_provider = 'creem'
        let pa_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO payment_attempts (id, realm_id, user_id, payment_provider, target_type, target_id, amount, currency, status, expires_at, created_at)
             VALUES ($1, $2, $3, 'creem', 'entitlement_mapping', $4, 10000, 'USD', 'completed', NOW() + INTERVAL '1 hour', NOW())",
        )
        .bind(pa_id)
        .bind(realm_id.as_str())
        .bind(account_id)
        .bind(Uuid::now_v7())
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        let payload = json!({
            "accountId": account_id.to_string(),
            "paymentAttemptId": pa_id.to_string(),
            "currency": "USD",
            "lineItems": [{
                "name": "Creem Item",
                "quantity": "1",
                "unitPrice": 10000,
            }],
            "billingName": "Creem Client",
            "billingAddress": "123 Creem St",
            "billingTaxId": "TAX-CREEM-001",
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
                    .uri(format!("/api/bill/{}/invoices", realm_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "Expected 400 when creating invoice for Creem payment"
        );

        let body = parse_body(response.into_body()).await;
        let error_msg = body["error"].as_str().unwrap_or("");
        assert!(
            error_msg.contains("Merchant of Record"),
            "Expected MoR rejection message, got: {}",
            error_msg
        );
    }

    // =========================================================================
    // Test: User apply invoice for Creem payment rejected
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-003 scenario 3 -- user apply rejected for Creem transactions
    //
    // Given: A payment_attempt with payment_provider = 'creem'
    // And: A regular user session
    // When: POST /my/invoices (apply for invoice)
    // Then: Returns 400 with "creem transactions are managed by the platform
    //       as Merchant of Record"

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_user_apply_invoice_for_creem_payment_rejected(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let _admin_token = setup_billing_admin_session(ctx, "invoice-creem-user@test.com").await;

        // Create a regular user session
        let (user_token, user_id) = crate::tests::helpers::create_admin_session_with_user(
            ctx,
            "user-creem-apply@test.com",
            1800,
        )
        .await;
        let user_uuid = Uuid::parse_str(&user_id).expect("Invalid user_id format");

        // Create a payment_attempt with payment_provider = 'creem'
        let pa_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO payment_attempts (id, realm_id, user_id, payment_provider, target_type, target_id, amount, currency, status, expires_at, created_at)
             VALUES ($1, $2, $3, 'creem', 'entitlement_mapping', $4, 5000, 'USD', 'completed', NOW() + INTERVAL '1 hour', NOW())",
        )
        .bind(pa_id)
        .bind(realm_id.as_str())
        .bind(user_uuid)
        .bind(Uuid::now_v7())
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        let apply_payload = json!({
            "paymentAttemptId": pa_id.to_string(),
            "currency": "USD",
            "billingName": "Creem User Client",
            "billingAddress": "123 User St",
            "billingTaxId": "TAX-USER-001",
            "dueDate": "2099-12-31",
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/user/bill/invoices")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", user_token))
                    .body(Body::from(apply_payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "Expected 400 when user applies invoice for Creem payment"
        );

        let body = parse_body(response.into_body()).await;
        let error_msg = body["error"].as_str().unwrap_or("");
        assert!(
            error_msg.contains("Merchant of Record"),
            "Expected MoR rejection message, got: {}",
            error_msg
        );
    }

    // =========================================================================
    // Group 3: External Invoice Readonly Guards (US-IF-003, US-IF-004)
    // =========================================================================

    // =========================================================================
    // Test: External Stripe invoice update rejected
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-004 scenario 2 -- external invoice cannot be edited
    //
    // Given: An external Stripe invoice (provider != manual)
    // When: PATCH /invoices/{id} with updated data
    // Then: Returns 403 with "managed by the payment provider"

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_external_stripe_invoice_update_rejected(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token =
            setup_billing_admin_session(ctx, "invoice-readonly-update@test.com").await;

        let stripe_inv_id = create_external_invoice_in_db(
            ctx,
            &realm_id,
            "stripe",
            "in_test_readonly_001",
            "draft",
            None,
            None,
        )
        .await;

        let patch_payload = json!({
            "billingName": "Should Not Work",
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/bill/{}/invoices/{}", realm_id, stripe_inv_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(patch_payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "Expected 403 when updating external Stripe invoice"
        );
    }

    // =========================================================================
    // Test: External Creem invoice update rejected
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-003 scenario 2 -- Creem external invoice readonly
    //
    // Given: An external Creem invoice (provider != manual)
    // When: PATCH /invoices/{id}
    // Then: Returns 403

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_external_creem_invoice_update_rejected(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-readonly-creem@test.com").await;

        let creem_inv_id = create_external_invoice_in_db(
            ctx,
            &realm_id,
            "creem",
            "order_creem_001",
            "paid",
            None,
            None,
        )
        .await;

        let patch_payload = json!({
            "billingName": "Should Not Work",
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/bill/{}/invoices/{}", realm_id, creem_inv_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(patch_payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "Expected 403 when updating external Creem invoice"
        );
    }

    // =========================================================================
    // Test: Manual invoice operations still work
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-001 -- manual invoices remain fully operational
    //
    // Given: A manual invoice (provider=manual)
    // When: Admin edits, issues, and marks it as paid
    // Then: All operations succeed (regression guard)

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_manual_invoice_operations_still_work(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-manual-ops@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        let line_items = vec![json!({
            "name": "Manual Service",
            "quantity": "1",
            "unitPrice": 5000,
        })];

        // Create
        let inv = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items,
            "Manual Ops Client",
        )
        .await;
        let invoice_id = inv["id"].as_str().unwrap();
        assert_eq!(inv["provider"], "manual");

        // Edit
        let patch_payload = json!({
            "billingName": "Updated Manual Client",
        });
        let edit_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/bill/{}/invoices/{}", realm_id, invoice_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(patch_payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(edit_response.status(), StatusCode::OK);

        // Issue
        let issue_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/bill/{}/invoices/{}/issue",
                        realm_id, invoice_id
                    ))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
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
                    .uri(format!(
                        "/api/bill/{}/invoices/{}/mark-paid",
                        realm_id, invoice_id
                    ))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(json!({}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(paid_response.status(), StatusCode::OK);
        let paid_body = parse_body(paid_response.into_body()).await;
        assert_eq!(paid_body["status"], "paid");
    }

    // =========================================================================
    // Group 4: Provider Filtering (US-IF-004)
    // =========================================================================

    // =========================================================================
    // Test: List invoices filter by provider=stripe
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-004 scenario 4 -- filter invoices by provider
    //
    // Given: One manual invoice and two external Stripe invoices
    // When: GET /invoices?provider=stripe
    // Then: Only Stripe invoices are returned (total=2)

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_list_invoices_filter_provider_stripe(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-filter-stripe@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        // Create a manual invoice
        let line_items = vec![json!({
            "name": "Manual Item",
            "quantity": "1",
            "unitPrice": 3000,
        })];
        let _manual_inv = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items,
            "Filter Manual Client",
        )
        .await;

        // Create two Stripe external invoices
        let _stripe1 = create_external_invoice_in_db(
            ctx,
            &realm_id,
            "stripe",
            "in_filter_stripe_001",
            "issued",
            None,
            None,
        )
        .await;
        let _stripe2 = create_external_invoice_in_db(
            ctx,
            &realm_id,
            "stripe",
            "in_filter_stripe_002",
            "paid",
            None,
            None,
        )
        .await;

        // Filter by provider=stripe
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/bill/{}/invoices?provider=stripe", realm_id))
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = parse_body(response.into_body()).await;
        assert_eq!(body["total"], 2, "Expected exactly 2 Stripe invoices");

        let data = body["data"].as_array().unwrap();
        for inv in data {
            assert_eq!(
                inv["provider"], "stripe",
                "All filtered results should have provider=stripe"
            );
        }
    }

    // =========================================================================
    // Test: List invoices filter by provider=manual
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-004 scenario 4 -- filter invoices by provider=manual
    //
    // Given: One manual invoice and one Stripe external invoice
    // When: GET /invoices?provider=manual
    // Then: Only manual invoices are returned (total=1)

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_list_invoices_filter_provider_manual(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-filter-manual@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        // Create a manual invoice
        let line_items = vec![json!({
            "name": "Manual Item",
            "quantity": "1",
            "unitPrice": 4000,
        })];
        let _manual_inv = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items,
            "Filter Manual Client 2",
        )
        .await;

        // Create a Stripe external invoice
        let _stripe_inv = create_external_invoice_in_db(
            ctx,
            &realm_id,
            "stripe",
            "in_filter_manual_001",
            "draft",
            None,
            None,
        )
        .await;

        // Filter by provider=manual
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/bill/{}/invoices?provider=manual", realm_id))
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = parse_body(response.into_body()).await;
        assert_eq!(body["total"], 1, "Expected exactly 1 manual invoice");

        let data = body["data"].as_array().unwrap();
        assert_eq!(data[0]["provider"], "manual");
    }

    // =========================================================================
    // Test: List invoices without provider filter returns all
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-004 scenario 1 -- unfiltered list includes all providers
    //
    // Given: One manual, one Stripe, and one Creem invoice
    // When: GET /invoices (no provider filter)
    // Then: Total=3, all providers represented

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_list_invoices_no_filter_returns_all(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-filter-all@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        // Create a manual invoice
        let line_items = vec![json!({
            "name": "Manual Item",
            "quantity": "1",
            "unitPrice": 6000,
        })];
        let _manual_inv = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items,
            "All Filter Client",
        )
        .await;

        // Create Stripe external invoice
        let _stripe_inv = create_external_invoice_in_db(
            ctx,
            &realm_id,
            "stripe",
            "in_filter_all_001",
            "issued",
            None,
            None,
        )
        .await;

        // Create Creem external invoice
        let _creem_inv = create_external_invoice_in_db(
            ctx,
            &realm_id,
            "creem",
            "order_filter_all_001",
            "paid",
            None,
            None,
        )
        .await;

        // GET /invoices without filter
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/bill/{}/invoices", realm_id))
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = parse_body(response.into_body()).await;
        assert_eq!(body["total"], 3, "Expected all 3 invoices");

        let data = body["data"].as_array().unwrap();
        let providers: std::collections::HashSet<&str> = data
            .iter()
            .map(|inv| inv["provider"].as_str().unwrap())
            .collect();
        assert!(
            providers.contains("manual"),
            "Should contain manual invoice"
        );
        assert!(
            providers.contains("stripe"),
            "Should contain stripe invoice"
        );
        assert!(providers.contains("creem"), "Should contain creem invoice");
    }

    // =========================================================================
    // Group 5: PDF Dual-Track (US-IF-006)
    // =========================================================================

    // =========================================================================
    // Test: External invoice PDF redirects to external URL
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-006 -- external invoice with PDF URL returns redirect
    //
    // Given: An external Stripe invoice with external_pdf_url set, status=issued
    // When: GET /invoices/{id}/pdf
    // Then: Returns 302 redirect to the external PDF URL

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_external_invoice_pdf_redirects_to_external_url(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-pdf-redirect@test.com").await;

        let stripe_inv_id = create_external_invoice_in_db(
            ctx,
            &realm_id,
            "stripe",
            "in_pdf_redirect_001",
            "issued",
            Some("https://stripe.com/invoice/pdf-redirect"),
            Some("https://stripe.com/invoice/pdf-redirect/pdf"),
        )
        .await;

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/bill/{}/invoices/{}/pdf",
                        realm_id, stripe_inv_id
                    ))
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // External invoice with PDF URL should redirect (302)
        assert_eq!(
            response.status(),
            StatusCode::FOUND,
            "Expected 302 redirect for external invoice with PDF URL"
        );

        let location = response
            .headers()
            .get("location")
            .expect("Location header should be present")
            .to_str()
            .unwrap();
        assert!(
            location.contains("stripe.com"),
            "Location should point to Stripe URL, got: {}",
            location
        );
    }

    // =========================================================================
    // Test: External invoice PDF with no URL returns 404
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-006 -- external invoice without PDF URL returns 404
    //
    // Given: An external Creem invoice with no external_pdf_url, status=paid
    // When: GET /invoices/{id}/pdf
    // Then: Returns 404 with "managed by" message

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_external_invoice_pdf_no_url_returns_404(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-pdf-no-url@test.com").await;

        let creem_inv_id = create_external_invoice_in_db(
            ctx,
            &realm_id,
            "creem",
            "order_pdf_no_url_001",
            "paid",
            None,
            None,
        )
        .await;

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/bill/{}/invoices/{}/pdf",
                        realm_id, creem_inv_id
                    ))
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "Expected 404 for external invoice without PDF URL"
        );

        let body = parse_body(response.into_body()).await;
        let error_msg = body["error"].as_str().unwrap_or("");
        assert!(
            error_msg.contains("managed by"),
            "Expected 'managed by' error message, got: {}",
            error_msg
        );
    }

    // =========================================================================
    // Test: Manual invoice PDF generates via IronPress
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-006 -- manual invoice PDF generation unchanged (regression)
    //
    // Given: A manual issued invoice (provider=manual)
    // When: GET /invoices/{id}/pdf
    // Then: Returns 200 with PDF content-type

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_manual_invoice_pdf_generates_ironpress(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-pdf-manual@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        let line_items = vec![json!({
            "name": "PDF Service",
            "quantity": "1",
            "unitPrice": 5000,
        })];

        let inv = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items,
            "PDF Manual Client",
        )
        .await;
        let invoice_id = inv["id"].as_str().unwrap();

        // Issue the invoice first (PDF requires non-draft status)
        let issue_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/bill/{}/invoices/{}/issue",
                        realm_id, invoice_id
                    ))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(json!({}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(issue_response.status(), StatusCode::OK);

        // GET PDF
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/bill/{}/invoices/{}/pdf",
                        realm_id, invoice_id
                    ))
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Expected 200 for manual invoice PDF generation"
        );

        let content_type = response
            .headers()
            .get("content-type")
            .expect("Content-Type header should be present")
            .to_str()
            .unwrap();
        assert!(
            content_type.contains("pdf"),
            "Expected PDF content-type, got: {}",
            content_type
        );
    }

    // =========================================================================
    // Group 6: User External Invoice Readonly (US-IF-005)
    // =========================================================================

    // =========================================================================
    // Test: User invoice list shows external provider field
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-005 -- user invoice list includes provider field
    //
    // Given: A user with a manual invoice and an external Stripe invoice linked to them
    // When: GET /my/invoices
    // Then: Each invoice has a "provider" field
    // And: External invoices show provider="stripe"

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_user_invoice_list_shows_external_provider_field(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let _admin_token =
            setup_billing_admin_session(ctx, "invoice-user-list-admin@test.com").await;

        // Create a regular user
        let (user_token, user_id) = crate::tests::helpers::create_admin_session_with_user(
            ctx,
            "user-list-provider@test.com",
            1800,
        )
        .await;
        let user_uuid = Uuid::parse_str(&user_id).expect("Invalid user_id format");

        // Ensure the user has an account row
        sqlx::query(
            "INSERT INTO account (id, realm_id, email, password, status)
             VALUES ($1, $2, $3, $4, 1)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(user_uuid)
        .bind(realm_id.as_str())
        .bind(format!("user-list-provider-{}@example.com", user_uuid))
        .bind("$2a$12$dummy_hash")
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        // Create an external Stripe invoice linked to the user's account
        let _stripe_inv_id = {
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

            let invoice_number = format!("EXT-STRIPE-user-{}", user_uuid);
            let _ = seq;

            sqlx::query(
                "INSERT INTO invoice (
                    id, realm_id, invoice_number, source, account_id, status, currency,
                    subtotal, discount_amount, tax_amount, shipping_amount, total,
                    provider, payment_provider, external_invoice_id,
                    external_hosted_url, external_pdf_url,
                    created_at, updated_at
                ) VALUES (
                    $1, $2, $3, 'external_sync', $4, 'issued', 'USD',
                    10000, 0, 0, 0, 10000,
                    'stripe', 'stripe', 'in_user_list_001',
                    'https://stripe.com/invoice/user-list', NULL,
                    NOW(), NOW()
                )",
            )
            .bind(invoice_id)
            .bind(realm_id.as_str())
            .bind(&invoice_number)
            .bind(user_uuid)
            .execute(&ctx.app_state.pool)
            .await
            .unwrap();

            invoice_id
        };

        // GET /my/invoices
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/user/bill/invoices")
                    .header("authorization", format!("Bearer {}", user_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = parse_body(response.into_body()).await;
        assert!(
            body["total"].as_u64().unwrap() >= 1,
            "Expected at least 1 invoice in user list"
        );

        let data = body["data"].as_array().unwrap();
        let has_stripe = data.iter().any(|inv| inv["provider"] == "stripe");
        assert!(
            has_stripe,
            "Expected at least one Stripe invoice in user list"
        );

        // Verify the Stripe invoice has externalHostedUrl
        let stripe_inv = data.iter().find(|inv| inv["provider"] == "stripe").unwrap();
        assert_eq!(
            stripe_inv["externalHostedUrl"], "https://stripe.com/invoice/user-list",
            "External hosted URL should be present for Stripe invoice"
        );
    }

    // =========================================================================
    // Test: User external invoice detail is readonly
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: US-IF-005 -- user views external invoice detail (readonly)
    //
    // Given: An external Stripe invoice linked to a user
    // When: GET /my/invoices/{id} (user detail view)
    // Then: Returns 200 with provider="stripe" and externalHostedUrl
    // And: The invoice detail shows it is an external invoice (provider!=manual)

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_user_external_invoice_detail_readonly(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let _admin_token =
            setup_billing_admin_session(ctx, "invoice-user-detail-admin@test.com").await;

        // Create a regular user
        let (user_token, user_id) = crate::tests::helpers::create_admin_session_with_user(
            ctx,
            "user-detail-provider@test.com",
            1800,
        )
        .await;
        let user_uuid = Uuid::parse_str(&user_id).expect("Invalid user_id format");

        // Ensure the user has an account row
        sqlx::query(
            "INSERT INTO account (id, realm_id, email, password, status)
             VALUES ($1, $2, $3, $4, 1)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(user_uuid)
        .bind(realm_id.as_str())
        .bind(format!("user-detail-provider-{}@example.com", user_uuid))
        .bind("$2a$12$dummy_hash")
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        // Create an external Stripe invoice linked to the user
        let stripe_inv_id = {
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

            let invoice_number = format!("EXT-STRIPE-user-det-{}", user_uuid);
            let _ = seq;

            sqlx::query(
                "INSERT INTO invoice (
                    id, realm_id, invoice_number, source, account_id, status, currency,
                    subtotal, discount_amount, tax_amount, shipping_amount, total,
                    provider, payment_provider, external_invoice_id,
                    external_hosted_url, external_pdf_url,
                    created_at, updated_at
                ) VALUES (
                    $1, $2, $3, 'external_sync', $4, 'issued', 'USD',
                    10000, 0, 0, 0, 10000,
                    'stripe', 'stripe', 'in_user_detail_001',
                    'https://stripe.com/invoice/user-detail', NULL,
                    NOW(), NOW()
                )",
            )
            .bind(invoice_id)
            .bind(realm_id.as_str())
            .bind(&invoice_number)
            .bind(user_uuid)
            .execute(&ctx.app_state.pool)
            .await
            .unwrap();

            // Insert line item
            sqlx::query(
                "INSERT INTO invoice_line_item (id, invoice_id, sort_order, name, quantity, unit_price, subtotal)
                 VALUES ($1, $2, 1, 'External Service', '1', 10000, 10000)",
            )
            .bind(Uuid::now_v7())
            .bind(invoice_id)
            .execute(&ctx.app_state.pool)
            .await
            .unwrap();

            // Insert history record
            sqlx::query(
                "INSERT INTO invoice_history (id, invoice_id, event_type, actor_type, changes, created_at)
                 VALUES ($1, $2, 'created', 'system', '{\"field\":\"status\",\"from\":null,\"to\":\"draft\"}', NOW())",
            )
            .bind(Uuid::now_v7())
            .bind(invoice_id)
            .execute(&ctx.app_state.pool)
            .await
            .unwrap();

            invoice_id
        };

        // GET /my/invoices/{id}
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/user/bill/invoices/{}", stripe_inv_id))
                    .header("authorization", format!("Bearer {}", user_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = parse_body(response.into_body()).await;

        // Verify provider field
        assert_eq!(
            body["provider"], "stripe",
            "External invoice should show provider=stripe"
        );

        // Verify external hosted URL is present
        assert_eq!(
            body["externalHostedUrl"], "https://stripe.com/invoice/user-detail",
            "External hosted URL should be present"
        );

        // Verify provider indicates it is NOT manual (readonly for UI)
        assert_ne!(
            body["provider"], "manual",
            "External invoice provider should NOT be manual"
        );
    }
}
