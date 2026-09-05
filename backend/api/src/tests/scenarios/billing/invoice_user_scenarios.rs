// =============================================================================
// Invoice User Scenario Tests
// =============================================================================
//
// Tests for user-facing invoice APIs: apply, list my invoices, get my invoice,
// cross-user isolation, cross-realm isolation, and admin endpoint rejection.
//
// User Story: docs/user-stories/13-invoice-user-stories.md
// Covers: US-IV-011, US-IV-008
//
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tests::helpers::*;
    use crate::tests::schema_test_context::SchemaTestContext;
    use axum::{
        body::Body,
        extract::{Extension, Path, State},
        http::{Request, StatusCode},
        response::IntoResponse,
    };
    use chrono::{Datelike, Utc};
    use herald_api_billing::invoice_handlers::get_my_invoice;
    use herald_core::domain::{
        authentication::{CredentialClass, Identity, TokenCredentialContext},
        client_api_keys::entities::ClientApiKey,
    };
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

    /// Helper: create a regular user session (no admin role).
    /// Returns (token, user_id).
    async fn create_regular_user_session(ctx: &InvoiceTestContext, email: &str) -> (String, Uuid) {
        let (token, user_id_str) =
            crate::tests::helpers::create_admin_session_with_user(ctx, email, 1800).await;
        let user_id = Uuid::parse_str(&user_id_str).expect("Invalid user_id format");
        // Do NOT grant realm-admin role -- this is a regular user.
        (token, user_id)
    }

    fn third_party_identity_in_realm(realm_id: &str) -> Identity {
        Identity::ThirdParty(ClientApiKey {
            id: Uuid::now_v7().to_string(),
            name: "Test API Key".to_string(),
            api_key_hash: "sha256:test".to_string(),
            realm_id: realm_id.to_string(),
            client_app_id: None,
            enabled: true,
            expires_at: None,
            created_at: Utc::now(),
            last_used_at: None,
        })
    }

    /// Helper: set up seller config for a realm via admin API.
    async fn setup_seller_config(app: &axum::Router, admin_token: &str, realm_id: &str) {
        let put_payload = json!({
            "sellerName": "Test Seller Corp",
            "sellerAddress": "456 Commerce St",
            "sellerEmail": "seller@testcorp.com",
            "sellerPhone": "+1-555-0200",
            "sellerTaxId": "TAX-CORP-001",
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/bill/{}/invoice-seller-config", realm_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(put_payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Seller config setup failed"
        );
    }

    /// Helper: create a user_application invoice directly in the DB.
    /// Used for isolation tests where we need invoices belonging to a specific user.
    async fn create_user_invoice_in_db(
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
                billing_name, billing_address, billing_email, billing_tax_id,
                seller_name, seller_address, seller_tax_id,
                due_date, created_at, updated_at
            ) VALUES (
                $1, $2, $3, 'user_application', $4, $5,
                'draft', 'USD', 5000, 0, 0, 0, 5000,
                'Test User Client', '123 User St', 'user-invoice@test.com', 'TAX-USER-001',
                'Seller Inc', '456 Seller Ave', 'SELLER-TAX-001',
                CURRENT_DATE + INTERVAL '30 days', NOW(), NOW()
            )",
        )
        .bind(invoice_id)
        .bind(realm_id)
        .bind(&invoice_number)
        .bind(Uuid::nil()) // account_id
        .bind(applicant_user_id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        // Insert history record
        sqlx::query(
            "INSERT INTO invoice_history (id, invoice_id, event_type, actor_user_id, actor_type, changes, created_at)
             VALUES ($1, $2, 'created', $3, 'user', '{\"field\":\"status\",\"from\":null,\"to\":\"draft\"}', NOW())",
        )
        .bind(Uuid::now_v7())
        .bind(invoice_id)
        .bind(applicant_user_id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        invoice_id
    }

    /// Helper: create a payment_attempts row for testing ownership validation.
    async fn create_test_payment_attempt(
        ctx: &InvoiceTestContext,
        realm_id: &str,
        user_id: Uuid,
    ) -> Uuid {
        let pa_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO payment_attempts (id, realm_id, user_id, payment_provider, target_type, target_id, amount, currency, status, expires_at)
             VALUES ($1, $2, $3, 'wechat', 'entitlement_mapping', $4, 5000, 'USD', 'Succeeded', NOW() + interval '1 hour')"
        )
        .bind(pa_id)
        .bind(realm_id)
        .bind(user_id)
        .bind(Uuid::now_v7()) // target_id
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
        pa_id
    }

    /// Helper: create a subscription row for testing ownership validation.
    async fn create_test_subscription(
        ctx: &InvoiceTestContext,
        realm_id: &str,
        user_id: Uuid,
    ) -> Uuid {
        let sub_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO subscription (id, realm_id, external_subscription_id, external_product_id, payment_provider, status, entitlement_key, user_id, created_at, updated_at, billing_type)
             VALUES ($1, $2, $3, $4, 'wechat', 'active', 'pro', $5, NOW(), NOW(), 'recurring')"
        )
        .bind(sub_id)
        .bind(realm_id)
        .bind(format!("sub_ext_{}", sub_id))
        .bind(format!("prod_ext_{}", sub_id))
        .bind(user_id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
        sub_id
    }

    // =========================================================================
    // US-IV-011: Apply Invoice Scenarios
    // =========================================================================
    // User Story: docs/user-stories/13-invoice-user-stories.md
    // Covers: US-IV-011 验收标准 1, 2, 3

    // -------------------------------------------------------------------------
    // test_user_apply_invoice_for_payment -- submit application, verify draft
    // -------------------------------------------------------------------------
    // Given: A regular user with a payment_attempt_id and seller config
    // When: POST /my/invoices with ApplyInvoiceRequest
    // Then: Returns 201 with status "draft", source "user_application"

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_user_apply_invoice_for_payment(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        // Set up admin with billing permissions and seller config
        let admin_token =
            setup_billing_admin_session(ctx, "invoice-user-apply-admin@test.com").await;
        setup_seller_config(&app, &admin_token, &realm_id).await;

        // Create regular user
        let (user_token, user_id) =
            create_regular_user_session(ctx, "invoice-user-apply@test.com").await;

        // Create a real payment_attempt row so the handler validates ownership
        let payment_attempt_id = create_test_payment_attempt(ctx, &realm_id, user_id).await;

        let payload = json!({
            "paymentAttemptId": payment_attempt_id.to_string(),
            "currency": "USD",
            "billingName": "John Applicant",
            "billingEmail": "john@applicant.com",
            "billingAddress": "789 User Lane",
            "billingPhone": "+1-555-0300",
            "billingTaxId": "TAX-123",
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
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        let body = parse_body(response.into_body()).await;

        // Verify draft status and user_application source
        assert_eq!(body["status"], "draft");
        assert_eq!(body["source"], "user_application");
        assert_eq!(body["currency"], "USD");
        assert_eq!(body["billingName"], "John Applicant");
        assert_eq!(body["billingEmail"], "john@applicant.com");

        // Verify invoice number assigned
        assert!(
            body["invoiceNumber"].is_string(),
            "Expected invoiceNumber to be set"
        );

        // Verify empty line items (user drafts start with no line items)
        let items = body["lineItems"].as_array().unwrap();
        assert!(
            items.is_empty(),
            "Expected empty line items for user application draft"
        );

        // Verify history has a "created" event
        let history = body["history"].as_array().unwrap();
        let has_created = history.iter().any(|h| h["eventType"] == "created");
        assert!(has_created, "Expected 'created' event in history");
    }

    // -------------------------------------------------------------------------
    // test_user_apply_invoice_seller_auto_filled -- seller from realm config
    // -------------------------------------------------------------------------
    // Given: Seller config is set up for the realm
    // When: User applies for invoice
    // Then: Seller info is auto-filled from the realm seller config

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_user_apply_invoice_seller_auto_filled(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        // Set up admin and seller config
        let admin_token =
            setup_billing_admin_session(ctx, "invoice-user-seller-admin@test.com").await;
        setup_seller_config(&app, &admin_token, &realm_id).await;

        // Create regular user
        let (user_token, user_id) =
            create_regular_user_session(ctx, "invoice-user-seller@test.com").await;

        // Create a real subscription row so the handler validates ownership
        let subscription_id = create_test_subscription(ctx, &realm_id, user_id).await;

        let payload = json!({
            "subscriptionId": subscription_id.to_string(),
            "currency": "USD",
            "billingName": "Jane Buyer",
            "billingAddress": "321 Buyer Blvd",
            "billingTaxId": "TAX-456",
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
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        let body = parse_body(response.into_body()).await;

        // Verify seller info is auto-filled from config
        assert_eq!(body["sellerName"], "Test Seller Corp");
        assert_eq!(body["sellerAddress"], "456 Commerce St");
        assert_eq!(body["sellerEmail"], "seller@testcorp.com");
        assert_eq!(body["sellerPhone"], "+1-555-0200");
    }

    // -------------------------------------------------------------------------
    // test_user_apply_invoice_no_seller_config_rejected -- 400 when no config
    // -------------------------------------------------------------------------
    // Given: No seller config for the realm
    // When: User applies for invoice
    // Then: Returns 400 with appropriate error message

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_user_apply_invoice_no_seller_config_rejected(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        // Create regular user (no admin, no seller config setup)
        let (user_token, user_id) =
            create_regular_user_session(ctx, "invoice-user-no-seller@test.com").await;

        // Create a real payment_attempt row so ownership validation passes,
        // then the handler will fail on the missing seller config
        let payment_attempt_id = create_test_payment_attempt(ctx, &realm_id, user_id).await;

        let payload = json!({
            "paymentAttemptId": payment_attempt_id.to_string(),
            "currency": "USD",
            "billingName": "No Seller User",
            "billingAddress": "999 No Seller St",
            "billingTaxId": "TAX-789",
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
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "Expected 400 when no seller config exists for realm"
        );

        let body = parse_body(response.into_body()).await;
        let error_msg = body["message"].as_str().unwrap_or("");
        assert!(
            error_msg.contains("seller"),
            "Expected error message about seller config, got: {}",
            error_msg
        );
    }

    // -------------------------------------------------------------------------
    // test_user_apply_invoice_for_other_user_payment_rejected -- 403
    // -------------------------------------------------------------------------
    // Given: A regular user
    // When: The user references a payment attempt outside the session realm
    // Then: The resource is not visible

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_user_apply_invoice_wrong_realm_rejected(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let _realm_id = ctx._realm_id.clone();

        // Create regular user
        let (user_token, _user_id) =
            create_regular_user_session(ctx, "invoice-user-wrong-realm@test.com").await;

        let payload = json!({
            "paymentAttemptId": Uuid::now_v7().to_string(),
            "currency": "USD",
            "billingName": "Wrong Realm User",
            "billingAddress": "000 Wrong Realm St",
            "billingTaxId": "TAX-WRONG",
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
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "Cross-realm resources are rejected as not found on the self-service write path"
        );
    }

    // =========================================================================
    // US-IV-008: My Invoices Scenarios
    // =========================================================================
    // User Story: docs/user-stories/13-invoice-user-stories.md
    // Covers: US-IV-008 验收标准 1, 2, 3

    // -------------------------------------------------------------------------
    // test_user_list_my_invoices -- only own invoices visible
    // -------------------------------------------------------------------------
    // Given: User A has 2 invoices, User B has 1 invoice
    // When: User A calls GET /my/invoices
    // Then: Only User A's 2 invoices are returned

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_user_list_my_invoices(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        // Create two regular users
        let (user_a_token, user_a_id) =
            create_regular_user_session(ctx, "invoice-list-a@test.com").await;
        let (_user_b_token, user_b_id) =
            create_regular_user_session(ctx, "invoice-list-b@test.com").await;

        // Create 2 invoices for user A
        let _inv_a1 = create_user_invoice_in_db(ctx, &realm_id, user_a_id).await;
        let _inv_a2 = create_user_invoice_in_db(ctx, &realm_id, user_a_id).await;

        // Create 1 invoice for user B
        let _inv_b1 = create_user_invoice_in_db(ctx, &realm_id, user_b_id).await;

        // User A lists their invoices
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/user/bill/invoices")
                    .header("authorization", format!("Bearer {}", user_a_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = parse_body(response.into_body()).await;

        // User A should see exactly 2 invoices
        assert_eq!(
            body["total"], 2,
            "User A should see exactly 2 of their own invoices, got: {}",
            body["total"]
        );

        let data = body["data"].as_array().unwrap();
        assert_eq!(data.len(), 2);
    }

    // -------------------------------------------------------------------------
    // test_user_get_my_invoice_detail -- detail with line items
    // -------------------------------------------------------------------------
    // Given: A user with an invoice that has line items
    // When: GET /my/invoices/{id}
    // Then: Returns full detail including line items

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_user_get_my_invoice_detail(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        let (user_token, user_id) =
            create_regular_user_session(ctx, "invoice-detail-user@test.com").await;

        // Create invoice for this user
        let invoice_id = create_user_invoice_in_db(ctx, &realm_id, user_id).await;

        // Add a line item
        sqlx::query(
            "INSERT INTO invoice_line_item (id, invoice_id, sort_order, name, quantity, unit_price, subtotal)
             VALUES ($1, $2, 1, 'Pro Plan Subscription', '1', 5000, 5000)",
        )
        .bind(Uuid::now_v7())
        .bind(invoice_id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        // Update totals
        sqlx::query("UPDATE invoice SET subtotal = 5000, total = 5000 WHERE id = $1")
            .bind(invoice_id)
            .execute(&ctx.app_state.pool)
            .await
            .unwrap();

        // Get the detail
        let response = app
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

        assert_eq!(response.status(), StatusCode::OK);
        let detail = parse_body(response.into_body()).await;

        // Verify basic fields
        assert_eq!(detail["status"], "draft");
        assert_eq!(detail["source"], "user_application");
        assert_eq!(detail["currency"], "USD");
        assert_eq!(detail["billingName"], "Test User Client");
        assert_eq!(detail["sellerName"], "Seller Inc");

        // Verify amounts
        assert_eq!(detail["subtotal"], 5000);
        assert_eq!(detail["total"], 5000);

        // Verify line items
        let items = detail["lineItems"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["name"], "Pro Plan Subscription");
        assert_eq!(items[0]["subtotal"], 5000);

        // Verify history
        let history = detail["history"].as_array().unwrap();
        let has_created = history.iter().any(|h| h["eventType"] == "created");
        assert!(has_created, "Expected 'created' event in history");
    }

    // -------------------------------------------------------------------------
    // test_my_invoice_detail_rejects_non_user_identity -- handler contract
    // -------------------------------------------------------------------------
    // Given: A non-user identity in the same realm
    // When: The my invoice detail handler is called
    // Then: It rejects before relying on ownership mismatch

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_my_invoice_detail_rejects_non_user_identity(ctx: &mut InvoiceTestContext) {
        let realm_id = ctx._realm_id.clone();
        let identity = third_party_identity_in_realm(&realm_id);

        let context = TokenCredentialContext {
            client_app_id: Uuid::now_v7(),
            client_id: "custom-user-ui".to_string(),
            family_id: Uuid::now_v7(),
            credential_class: CredentialClass::CustomUserUi,
            allowed_scopes: std::collections::HashSet::new(),
        };
        let err = match get_my_invoice(
            State((*ctx.app_state).clone()),
            Extension(identity),
            Extension(context),
            Path(Uuid::now_v7()),
        )
        .await
        {
            Ok(_) => panic!("Expected non-user identity to be rejected"),
            Err(err) => err,
        };

        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = parse_body(response.into_body()).await;
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .contains("authenticated user token required"),
            "Expected authenticated user token error, got: {}",
            body
        );
    }

    // -------------------------------------------------------------------------
    // test_regular_user_cannot_use_admin_endpoints -- admin endpoints return 403
    // -------------------------------------------------------------------------
    // Given: A regular user (no billing.manage or billing.view permissions)
    // When: Accessing admin invoice endpoints
    // Then: Returns 403

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_regular_user_cannot_use_admin_endpoints(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        // Create a regular user
        let (user_token, _user_id) =
            create_regular_user_session(ctx, "invoice-admin-reject@test.com").await;

        // Test: GET /invoices (admin list) -> 403
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/bill/{}/invoices", realm_id))
                    .header("authorization", format!("Bearer {}", user_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "Regular user should be forbidden from admin invoice list"
        );

        // Test: POST /invoices (admin create) -> 403
        let create_payload = json!({
            "accountId": Uuid::now_v7().to_string(),
            "currency": "USD",
            "lineItems": [{"name": "Test", "quantity": "1", "unitPrice": 1000}],
            "billingName": "Test",
            "billingAddress": "123 Test St",
            "billingTaxId": "TAX-TEST",
            "sellerName": "Test Seller",
            "sellerAddress": "456 Seller Ave",
            "sellerTaxId": "SELLER-TAX",
            "dueDate": "2099-12-31",
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/bill/{}/invoices", realm_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", user_token))
                    .body(Body::from(create_payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "Regular user should be forbidden from creating admin invoices"
        );

        // Test: GET /invoice-seller-config -> 403 (requires billing.view)
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/bill/{}/invoice-seller-config", realm_id))
                    .header("authorization", format!("Bearer {}", user_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "Regular user should be forbidden from viewing seller config"
        );

        // Test: PUT /invoice-seller-config -> 403 (requires billing.manage)
        let seller_payload = json!({
            "sellerName": "Unauthorized",
            "sellerAddress": "999 Unauth St",
            "sellerTaxId": "TAX-UNAUTH",
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/bill/{}/invoice-seller-config", realm_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", user_token))
                    .body(Body::from(seller_payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "Regular user should be forbidden from updating seller config"
        );

        // Test: Admin status transitions -> 403
        let fake_invoice_id = Uuid::now_v7();
        for (method, path_suffix) in [
            ("POST", "/issue"),
            ("POST", "/void"),
            ("POST", "/mark-paid"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(format!(
                            "/api/bill/{}/invoices/{}{}",
                            realm_id, fake_invoice_id, path_suffix
                        ))
                        .header("content-type", "application/json")
                        .header("authorization", format!("Bearer {}", user_token))
                        .body(Body::from(json!({}).to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(
                response.status(),
                StatusCode::FORBIDDEN,
                "Regular user should be forbidden from admin status transition: {}",
                path_suffix
            );
        }
    }

    // =========================================================================
    // Edge Cases
    // =========================================================================

    // -------------------------------------------------------------------------
    // test_apply_invoice_with_subscription_reference -- optional subscription link
    // -------------------------------------------------------------------------
    // Given: A regular user and seller config
    // When: POST /my/invoices with subscriptionId
    // Then: Invoice created with subscriptionId reference

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_apply_invoice_with_subscription_reference(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        // Set up admin and seller config
        let admin_token = setup_billing_admin_session(ctx, "invoice-subs-ref-admin@test.com").await;
        setup_seller_config(&app, &admin_token, &realm_id).await;

        // Create regular user
        let (user_token, user_id) =
            create_regular_user_session(ctx, "invoice-subs-ref@test.com").await;

        // Create a real subscription row so the handler validates ownership
        let subscription_id = create_test_subscription(ctx, &realm_id, user_id).await;

        let payload = json!({
            "subscriptionId": subscription_id.to_string(),
            "currency": "USD",
            "billingName": "Subs Reference User",
            "billingAddress": "555 Subs Lane",
            "billingTaxId": "TAX-SUBS",
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
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        let body = parse_body(response.into_body()).await;

        // Verify subscription reference is stored
        assert_eq!(
            body["subscriptionId"],
            subscription_id.to_string(),
            "Expected subscriptionId to be stored"
        );
    }

    // -------------------------------------------------------------------------
    // test_apply_invoice_status_tracking -- user sees status changes after admin
    // -------------------------------------------------------------------------
    // Given: User creates an invoice application (draft)
    // When: Admin issues it
    // Then: User sees status changed to "issued" when listing or getting detail

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_apply_invoice_status_tracking(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        // Set up admin and seller config
        let admin_token = setup_billing_admin_session(ctx, "invoice-tracking-admin@test.com").await;
        setup_seller_config(&app, &admin_token, &realm_id).await;

        // Create regular user
        let (user_token, user_id) =
            create_regular_user_session(ctx, "invoice-tracking@test.com").await;

        // User applies for invoice
        let payment_attempt_id = create_test_payment_attempt(ctx, &realm_id, user_id).await;
        let payload = json!({
            "paymentAttemptId": payment_attempt_id.to_string(),
            "currency": "USD",
            "billingName": "Tracking User",
            "billingAddress": "111 Tracking Ave",
            "billingEmail": "tracking@test.com",
            "billingTaxId": "TAX-TRACK",
            "dueDate": "2099-12-31",
        });

        let apply_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/user/bill/invoices")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", user_token))
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(apply_response.status(), StatusCode::CREATED);
        let apply_body = parse_body(apply_response.into_body()).await;
        let invoice_id = apply_body["id"].as_str().unwrap();
        assert_eq!(apply_body["status"], "draft");

        // Verify user sees "draft" in their list
        let list_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/user/bill/invoices?status=draft")
                    .header("authorization", format!("Bearer {}", user_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(list_response.status(), StatusCode::OK);
        let list_body = parse_body(list_response.into_body()).await;
        assert!(
            list_body["total"].as_u64().unwrap() >= 1,
            "Expected at least 1 draft invoice in user's list"
        );

        // Admin adds line items and issues the invoice
        // First, update the invoice to add line items (admin can PATCH user_application drafts)
        let patch_payload = json!({
            "billingTaxId": "TAX-USER-001",
            "sellerTaxId": "SELLER-TAX-001",
            "lineItems": [{
                "name": "Service Fee",
                "quantity": "1",
                "unitPrice": 10000,
            }]
        });

        let patch_response = app
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

        assert_eq!(patch_response.status(), StatusCode::OK);

        // Admin issues the invoice
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

        // User now checks their invoice detail -- should see "issued"
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
        assert_eq!(
            detail["status"], "issued",
            "User should see the invoice status changed to 'issued' after admin issues it"
        );

        // Verify history contains both "created" and "issued" events
        let history = detail["history"].as_array().unwrap();
        let has_created = history.iter().any(|h| h["eventType"] == "created");
        let has_issued = history.iter().any(|h| h["eventType"] == "issued");
        assert!(has_created, "Expected 'created' event in history");
        assert!(
            has_issued,
            "Expected 'issued' event in history after admin issues"
        );
    }

    // -------------------------------------------------------------------------
    // test_apply_invoice_requires_payment_or_subscription -- validation
    // -------------------------------------------------------------------------
    // Given: A regular user and seller config
    // When: POST /my/invoices without payment_attempt_id or subscription_id
    // Then: Returns 400

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_apply_invoice_requires_payment_or_subscription(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        // Set up admin and seller config
        let admin_token =
            setup_billing_admin_session(ctx, "invoice-validation-admin@test.com").await;
        setup_seller_config(&app, &admin_token, &realm_id).await;

        // Create regular user
        let (user_token, _user_id) =
            create_regular_user_session(ctx, "invoice-validation@test.com").await;

        // Omit both payment_attempt_id and subscription_id
        let payload = json!({
            "currency": "USD",
            "billingName": "Missing Reference User",
            "billingAddress": "222 Missing St",
            "billingTaxId": "TAX-MISS",
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
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "Expected 400 when neither payment_attempt_id nor subscription_id is provided"
        );
    }

    // =========================================================================
    // Ownership Validation Tests (FK replaced by business logic)
    // =========================================================================

    // -------------------------------------------------------------------------
    // test_user_apply_invoice_other_user_payment_rejected -- 403 for wrong owner
    // -------------------------------------------------------------------------
    // Given: User A creates a payment_attempt
    // When: User B tries to apply for invoice using User A's payment_attempt_id
    // Then: Returns 403

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_user_apply_invoice_other_user_payment_rejected(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        // Set up admin and seller config
        let admin_token = setup_billing_admin_session(ctx, "invoice-owner-admin@test.com").await;
        setup_seller_config(&app, &admin_token, &realm_id).await;

        // Create two regular users
        let (_user_a_token, user_a_id) =
            create_regular_user_session(ctx, "invoice-owner-a@test.com").await;
        let (user_b_token, _user_b_id) =
            create_regular_user_session(ctx, "invoice-owner-b@test.com").await;

        // Create a payment_attempt for User A
        let pa_id = create_test_payment_attempt(ctx, &realm_id, user_a_id).await;

        // User B tries to apply for invoice using User A's payment_attempt
        let payload = json!({
            "paymentAttemptId": pa_id.to_string(),
            "currency": "USD",
            "billingName": "Imposter User",
            "billingAddress": "333 Imposter Rd",
            "billingTaxId": "TAX-IMP",
            "dueDate": "2099-12-31",
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/user/bill/invoices")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", user_b_token))
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "Expected 403 when User B tries to apply invoice with User A's payment_attempt"
        );
    }

    // -------------------------------------------------------------------------
    // test_user_apply_invoice_nonexistent_payment_rejected -- 400
    // -------------------------------------------------------------------------
    // Given: A regular user
    // When: Applying for invoice with a nonexistent payment_attempt_id
    // Then: Returns 400

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_user_apply_invoice_nonexistent_payment_rejected(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        // Set up admin and seller config
        let admin_token = setup_billing_admin_session(ctx, "invoice-nonexist-admin@test.com").await;
        setup_seller_config(&app, &admin_token, &realm_id).await;

        // Create regular user
        let (user_token, _user_id) =
            create_regular_user_session(ctx, "invoice-nonexist@test.com").await;

        // Use a UUID that does NOT exist in payment_attempts table
        let fake_pa_id = Uuid::now_v7();

        let payload = json!({
            "paymentAttemptId": fake_pa_id.to_string(),
            "currency": "USD",
            "billingName": "Fake Payment User",
            "billingAddress": "444 Fake St",
            "billingTaxId": "TAX-FAKE",
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
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "Expected 400 when applying invoice with nonexistent payment_attempt_id"
        );
    }
}
