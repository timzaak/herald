// =============================================================================
// Invoice Admin Scenario Tests
// =============================================================================
//
// Tests for admin invoice management: seller config, CRUD, status transitions.
//
// User Story: docs/user-stories/13-invoice-user-stories.md
// Covers: US-IV-010, US-IV-001, US-IV-002, US-IV-003, US-IV-004,
//         US-IV-005, US-IV-006, US-IV-007, US-IV-012
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

    // Helper: create a draft invoice via API, return the full response JSON
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
        .bind(format!("invoice-test-{}@example.com", account_id))
        .bind("$2a$12$dummy_hash")
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
        account_id
    }

    // =========================================================================
    // Test: Seller Config Upsert (US-IV-010)
    // =========================================================================
    // User Story: docs/user-stories/13-invoice-user-stories.md
    // Covers: US-IV-010
    //
    // Given: An authenticated admin
    // When: PUT seller config with name/address/email/phone
    // Then: GET returns all fields matching
    // When: PUT again with updated name
    // Then: GET returns updated name

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_scenario_seller_config_upsert(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let _realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-seller@test.com").await;

        // Step 1: PUT seller config with all fields
        let put_payload = json!({
            "sellerName": "Acme Corp",
            "sellerAddress": "123 Main St, Springfield",
            "sellerEmail": "billing@acme.com",
            "sellerPhone": "+1-555-0100",
            "sellerTaxId": "TAX-ACME-001",
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/bill/invoice-seller-config".to_string())
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(put_payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = parse_body(response.into_body()).await;
        assert_eq!(body["sellerName"], "Acme Corp");
        assert_eq!(body["sellerAddress"], "123 Main St, Springfield");
        assert_eq!(body["sellerEmail"], "billing@acme.com");
        assert_eq!(body["sellerPhone"], "+1-555-0100");

        // Step 2: GET seller config, verify all fields match
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/bill/invoice-seller-config".to_string())
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = parse_body(response.into_body()).await;
        assert_eq!(body["sellerName"], "Acme Corp");
        assert_eq!(body["sellerAddress"], "123 Main St, Springfield");
        assert_eq!(body["sellerEmail"], "billing@acme.com");
        assert_eq!(body["sellerPhone"], "+1-555-0100");

        // Step 3: PUT again with updated name
        let put_payload_updated = json!({
            "sellerName": "Acme Corp International",
            "sellerAddress": "123 Main St, Springfield",
            "sellerEmail": "billing@acme.com",
            "sellerPhone": "+1-555-0100",
            "sellerTaxId": "TAX-ACME-001",
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/bill/invoice-seller-config".to_string())
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(put_payload_updated.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Step 4: GET again, verify updated
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/bill/invoice-seller-config".to_string())
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = parse_body(response.into_body()).await;
        assert_eq!(body["sellerName"], "Acme Corp International");
    }

    // =========================================================================
    // Test: Create Invoice (US-IV-001)
    // =========================================================================
    // User Story: docs/user-stories/13-invoice-user-stories.md
    // Covers: US-IV-001
    //
    // Given: An authenticated admin
    // When: POST /invoices with line items
    // Then: Returns 201, invoiceNumber starts with "INV-", status "draft"
    // And: Amount calculations are correct
    // When: Create second invoice
    // Then: Invoice number increments

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_scenario_create_invoice(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-create@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        let line_items = vec![
            json!({
                "name": "Consulting Hours",
                "description": "10 hours of dev work",
                "quantity": "10",
                "unitPrice": 15000, // 150.00 in cents
            }),
            json!({
                "name": "Server Hosting",
                "quantity": "1",
                "unitPrice": 5000, // 50.00 in cents
            }),
        ];

        // Create first invoice
        let inv1 = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items,
            "Jane Doe",
        )
        .await;

        // Verify basic fields
        assert!(inv1["invoiceNumber"].is_string());
        let invoice_number_1 = inv1["invoiceNumber"].as_str().unwrap();
        assert!(
            invoice_number_1.starts_with("INV-"),
            "Expected invoice number to start with INV-, got: {}",
            invoice_number_1
        );
        assert_eq!(inv1["status"], "draft");
        assert_eq!(inv1["currency"], "USD");

        // Verify amount calculations: 10*15000 + 1*5000 = 155000
        assert_eq!(inv1["subtotal"], 155000);
        assert_eq!(inv1["total"], 155000);

        // Verify line items returned
        let items = inv1["lineItems"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["subtotal"], 150000);
        assert_eq!(items[1]["subtotal"], 5000);

        // Create second invoice -- verify number increments
        let line_items_2 = vec![json!({
            "name": "License Fee",
            "quantity": "1",
            "unitPrice": 9900,
        })];

        let inv2 = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items_2,
            "Jane Doe",
        )
        .await;

        let invoice_number_2 = inv2["invoiceNumber"].as_str().unwrap();
        assert!(
            invoice_number_2.starts_with("INV-"),
            "Expected second invoice number to start with INV-, got: {}",
            invoice_number_2
        );
        // Verify the second invoice number differs from the first
        assert_ne!(
            invoice_number_1, invoice_number_2,
            "Second invoice should have a different number"
        );
    }

    // =========================================================================
    // Test: Edit Draft Invoice (US-IV-002)
    // =========================================================================
    // User Story: docs/user-stories/13-invoice-user-stories.md
    // Covers: US-IV-002
    //
    // Given: A draft invoice
    // When: PATCH with updated line items and billing name
    // Then: Response has updated billing name and recalculated amounts
    // When: Issue the invoice, then try to PATCH again
    // Then: Returns 409

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_scenario_edit_draft_invoice(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-edit@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        // Create a draft invoice
        let line_items = vec![json!({
            "name": "Item A",
            "quantity": "2",
            "unitPrice": 10000,
        })];

        let inv = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items,
            "Original Name",
        )
        .await;

        let invoice_id = inv["id"].as_str().unwrap();

        // PATCH with updated line items and billing name
        let patch_payload = json!({
            "billingName": "Updated Client Name",
            "billingTaxId": "TAX-001",
            "sellerTaxId": "SELLER-TAX-001",
            "lineItems": [
                {
                    "name": "Item A (updated)",
                    "quantity": "3",
                    "unitPrice": 12000,
                },
                {
                    "name": "Item B (new)",
                    "quantity": "1",
                    "unitPrice": 5000,
                }
            ]
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/bill/invoices/{}", invoice_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(patch_payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = parse_body(response.into_body()).await;

        // Verify updated billing name
        assert_eq!(body["billingName"], "Updated Client Name");

        // Verify recalculated amounts: 3*12000 + 1*5000 = 41000
        assert_eq!(body["subtotal"], 41000);
        assert_eq!(body["total"], 41000);

        // Verify updated line items
        let items = body["lineItems"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["name"], "Item A (updated)");
        assert_eq!(items[1]["name"], "Item B (new)");

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

        // Try to PATCH again on issued invoice -> 409
        let patch_after_issue = json!({
            "billingName": "Should Not Work",
            "billingTaxId": "TAX-001",
            "sellerTaxId": "SELLER-TAX-001",
        });

        let conflict_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/bill/invoices/{}", invoice_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(patch_after_issue.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            conflict_response.status(),
            StatusCode::CONFLICT,
            "Expected 409 Conflict when PATCHing a non-draft invoice"
        );
    }

    // =========================================================================
    // Test: List Invoices (US-IV-003)
    // =========================================================================
    // User Story: docs/user-stories/13-invoice-user-stories.md
    // Covers: US-IV-003
    //
    // Given: 3 invoices: 1 draft, 1 issued, 1 voided
    // When: GET /invoices (no filter)
    // Then: total = 3
    // When: GET /invoices?status=issued
    // Then: total = 1
    // When: GET /invoices with pageSize=2&page=1
    // Then: 2 items returned, total=3

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_scenario_list_invoices(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-list@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        let line_items = vec![json!({
            "name": "Service",
            "quantity": "1",
            "unitPrice": 10000,
        })];

        // Invoice 1: leave as draft
        let inv1 = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items.clone(),
            "Draft Client",
        )
        .await;
        let inv1_id = inv1["id"].as_str().unwrap();

        // Invoice 2: issue it
        let inv2 = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items.clone(),
            "Issued Client",
        )
        .await;
        let inv2_id = inv2["id"].as_str().unwrap();

        let issue_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/bill/invoices/{}/issue", inv2_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(json!({}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(issue_response.status(), StatusCode::OK);

        // Invoice 3: void it
        let inv3 = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items,
            "Voided Client",
        )
        .await;
        let inv3_id = inv3["id"].as_str().unwrap();

        // Void invoice 3 (can void from draft)
        let void_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/bill/invoices/{}/void", inv3_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(json!({ "voidReason": "Test void" }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(void_response.status(), StatusCode::OK);

        // GET /invoices (no filter) -- total = 3
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
        assert_eq!(list_body["total"], 3);

        // GET /invoices?status=issued -- total = 1
        let filtered_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/bill/invoices?status=issued".to_string())
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(filtered_response.status(), StatusCode::OK);
        let filtered_body = parse_body(filtered_response.into_body()).await;
        assert_eq!(
            filtered_body["total"], 1,
            "Expected exactly 1 issued invoice, got: {:?}",
            filtered_body
        );

        // GET /invoices?pageSize=2&page=1 -- 2 items, total=3
        let paged_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/bill/invoices?pageSize=2&page=1".to_string())
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(paged_response.status(), StatusCode::OK);
        let paged_body = parse_body(paged_response.into_body()).await;
        assert_eq!(paged_body["total"], 3);
        assert_eq!(
            paged_body["data"].as_array().unwrap().len(),
            2,
            "Expected 2 items on page 1"
        );

        // Suppress unused variable warnings
        let _ = inv1_id;
    }

    // =========================================================================
    // Test: Get Invoice Detail (US-IV-004)
    // =========================================================================
    // User Story: docs/user-stories/13-invoice-user-stories.md
    // Covers: US-IV-004
    //
    // Given: An invoice with line items
    // When: GET /invoices/{id}
    // Then: Full detail returned: invoice fields, line items array, history array
    // When: GET /invoices/{nonexistent-id}
    // Then: Returns 404

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_scenario_get_invoice_detail(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-detail@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        let line_items = vec![
            json!({
                "name": "Design Work",
                "description": "UI/UX design",
                "quantity": "20",
                "unitPrice": 8000,
            }),
            json!({
                "name": "Project Management",
                "description": "PM oversight",
                "quantity": "5",
                "unitPrice": 12000,
            }),
        ];

        // Create invoice
        let inv = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items,
            "Detail Test Client",
        )
        .await;

        let invoice_id = inv["id"].as_str().unwrap();

        // GET detail
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

        // Verify invoice fields
        assert_eq!(detail["status"], "draft");
        assert_eq!(detail["currency"], "USD");
        assert_eq!(detail["billingName"], "Detail Test Client");
        assert_eq!(detail["sellerName"], "Test Seller Inc.");
        assert!(detail["invoiceNumber"].is_string());
        assert!(detail["id"].is_string());

        // Verify amounts: 20*8000 + 5*12000 = 220000
        assert_eq!(detail["subtotal"], 220000);
        assert_eq!(detail["total"], 220000);

        // Verify line items array
        let items = detail["lineItems"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["name"], "Design Work");
        assert_eq!(items[0]["description"], "UI/UX design");
        // quantity is stored as NUMERIC(12,3), so "20" comes back as "20.000"
        let qty_str = items[0]["quantity"].as_str().unwrap();
        let qty_val: f64 = qty_str.parse().unwrap();
        assert!(
            (qty_val - 20.0).abs() < 0.001,
            "Expected quantity ~20, got {}",
            qty_str
        );
        assert_eq!(items[0]["unitPrice"], 8000);
        assert_eq!(items[0]["subtotal"], 160000);

        assert_eq!(items[1]["name"], "Project Management");
        assert_eq!(items[1]["subtotal"], 60000);

        // Verify history array (should have at least a "created" event)
        let history = detail["history"].as_array().unwrap();
        assert!(
            !history.is_empty(),
            "Expected at least one history event (created)"
        );
        let has_created_event = history.iter().any(|h| h["eventType"] == "created");
        assert!(has_created_event, "Expected a 'created' event in history");

        // GET nonexistent invoice -> 404
        let nonexistent_id = Uuid::now_v7();
        let not_found_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/bill/invoices/{}", nonexistent_id))
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            not_found_response.status(),
            StatusCode::NOT_FOUND,
            "Expected 404 for nonexistent invoice ID"
        );
    }

    // =========================================================================
    // Test: Issue Invoice (US-IV-005)
    // =========================================================================
    // User Story: docs/user-stories/13-invoice-user-stories.md
    // Covers: US-IV-005 验收标准 1, 2
    //
    // Scenario 1: Issue a draft invoice with line items -> status "issued"
    // Scenario 2: Issue an empty invoice (no line items) -> rejected with 400

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_admin_issue_draft_invoice(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-issue@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        let line_items = vec![json!({
            "name": "Hosting Fee",
            "quantity": "1",
            "unitPrice": 9900,
        })];

        let inv = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items,
            "Issue Test Client",
        )
        .await;

        let invoice_id = inv["id"].as_str().unwrap();
        assert_eq!(inv["status"], "draft");

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
        let body = parse_body(issue_response.into_body()).await;

        // Verify status changed to "issued"
        assert_eq!(body["status"], "issued");
        assert_eq!(body["id"], invoice_id);

        // Verify issued_at is set
        assert!(
            body["issuedAt"].is_string(),
            "issuedAt should be set after issuing"
        );

        // Verify issue_date is set
        assert!(
            body["issueDate"].is_string(),
            "issueDate should be set after issuing"
        );

        // Verify history contains an "issued" event
        let history = body["history"].as_array().unwrap();
        let has_issued_event = history.iter().any(|h| h["eventType"] == "issued");
        assert!(
            has_issued_event,
            "Expected an 'issued' event in history after issuing"
        );
    }

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_admin_issue_empty_invoice_rejected(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-issue-empty@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        // Create an invoice with an empty line_items array via direct API call
        // (create_draft_invoice helper requires line items and asserts 201,
        //  so we create manually to send empty line items)
        let payload = json!({
            "accountId": account_id.to_string(),
            "currency": "USD",
            "lineItems": [],
            "billingName": "Empty Client",
            "billingAddress": "123 Empty St",
            "billingTaxId": "TAX-EMPTY",
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
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // The create endpoint may succeed with empty items (validation requires length >= 1).
        // If it does, we cannot test issuing it. If validation rejects, we verify that.
        // Based on CreateInvoiceRequest validation: #[validate(length(min = 1))] on line_items
        // So the creation itself should fail with 400.
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "Expected 400 when creating invoice with empty line items"
        );
    }

    // =========================================================================
    // Test: Void Invoice (US-IV-006)
    // =========================================================================
    // User Story: docs/user-stories/13-invoice-user-stories.md
    // Covers: US-IV-006 验收标准 1, 2, 3
    //
    // Scenario 1: Void a draft invoice -> status "void"
    // Scenario 2: Void an issued invoice with reason -> status "void"
    // Scenario 3: Void a paid invoice -> rejected (terminal state)

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_admin_void_draft(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-void-draft@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        let line_items = vec![json!({
            "name": "Item to Void",
            "quantity": "1",
            "unitPrice": 5000,
        })];

        let inv = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items,
            "Void Draft Client",
        )
        .await;

        let invoice_id = inv["id"].as_str().unwrap();
        assert_eq!(inv["status"], "draft");

        // Void the draft invoice
        let void_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/bill/invoices/{}/void", invoice_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(
                        json!({ "voidReason": "No longer needed" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(void_response.status(), StatusCode::OK);
        let body = parse_body(void_response.into_body()).await;

        assert_eq!(body["status"], "void");

        // Verify voided_at is set
        assert!(
            body["voidedAt"].is_string(),
            "voidedAt should be set after voiding"
        );

        // Verify history contains a "voided" event
        let history = body["history"].as_array().unwrap();
        let has_voided_event = history.iter().any(|h| h["eventType"] == "voided");
        assert!(has_voided_event, "Expected a 'voided' event in history");
    }

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_admin_void_issued(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-void-issued@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        let line_items = vec![json!({
            "name": "Issued Item",
            "quantity": "1",
            "unitPrice": 10000,
        })];

        let inv = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items,
            "Void Issued Client",
        )
        .await;

        let invoice_id = inv["id"].as_str().unwrap();

        // First issue the invoice
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

        // Now void the issued invoice with a reason
        let void_reason = "Customer requested cancellation";
        let void_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/bill/invoices/{}/void", invoice_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(json!({ "voidReason": void_reason }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(void_response.status(), StatusCode::OK);
        let body = parse_body(void_response.into_body()).await;

        assert_eq!(body["status"], "void");
        assert_eq!(body["voidReason"], void_reason);

        // Verify voided_at is set
        assert!(
            body["voidedAt"].is_string(),
            "voidedAt should be set after voiding an issued invoice"
        );
    }

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_admin_void_paid_rejected(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-void-paid@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        let line_items = vec![json!({
            "name": "Paid Item",
            "quantity": "1",
            "unitPrice": 7500,
        })];

        let inv = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items,
            "Void Paid Client",
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

        // Mark as paid
        let paid_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/bill/invoices/{}/mark-paid", invoice_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(json!({}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(paid_response.status(), StatusCode::OK);

        // Try to void the paid invoice -> should be rejected (409 terminal state)
        let void_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/bill/invoices/{}/void", invoice_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(
                        json!({ "voidReason": "Attempt on paid" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            void_response.status(),
            StatusCode::CONFLICT,
            "Expected 409 Conflict when voiding a paid invoice (terminal state)"
        );
    }

    // =========================================================================
    // Test: Mark Paid (US-IV-007)
    // =========================================================================
    // User Story: docs/user-stories/13-invoice-user-stories.md
    // Covers: US-IV-007 验收标准 1, 3
    //
    // Scenario 1: Mark an issued invoice as paid -> status "paid"
    // Scenario 2: Mark a draft invoice as paid -> rejected (invalid transition)

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_admin_mark_issued_as_paid(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-mark-paid@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        let line_items = vec![json!({
            "name": "Service Fee",
            "quantity": "2",
            "unitPrice": 5000,
        })];

        let inv = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items,
            "Mark Paid Client",
        )
        .await;

        let invoice_id = inv["id"].as_str().unwrap();

        // Issue first
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

        // Mark as paid
        let paid_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/bill/invoices/{}/mark-paid", invoice_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(json!({}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(paid_response.status(), StatusCode::OK);
        let body = parse_body(paid_response.into_body()).await;

        assert_eq!(body["status"], "paid");

        // Verify paid_at is set
        assert!(
            body["paidAt"].is_string(),
            "paidAt should be set after marking as paid"
        );

        // Verify history contains a "paid" event
        let history = body["history"].as_array().unwrap();
        let has_paid_event = history.iter().any(|h| h["eventType"] == "paid");
        assert!(
            has_paid_event,
            "Expected a 'paid' event in history after marking as paid"
        );
    }

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_admin_mark_draft_as_paid_rejected(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token =
            setup_billing_admin_session(ctx, "invoice-mark-paid-draft@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        let line_items = vec![json!({
            "name": "Draft Item",
            "quantity": "1",
            "unitPrice": 3000,
        })];

        let inv = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items,
            "Draft Pay Client",
        )
        .await;

        let invoice_id = inv["id"].as_str().unwrap();
        assert_eq!(inv["status"], "draft");

        // Try to mark draft as paid -> should be rejected (409 invalid transition)
        let paid_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/bill/invoices/{}/mark-paid", invoice_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(json!({}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            paid_response.status(),
            StatusCode::CONFLICT,
            "Expected 409 Conflict when marking a draft invoice as paid (invalid transition)"
        );
    }

    // =========================================================================
    // Test: Review User Application (US-IV-012)
    // =========================================================================
    // User Story: docs/user-stories/13-invoice-user-stories.md
    // Covers: US-IV-012 验收标准 1, 2
    //
    // Scenario 1: List invoices filtered by source=user_application + status=draft
    //             -> returns pending review invoices
    // Scenario 2: Issue a user application invoice -> status "issued"

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_admin_review_user_application_list(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-review-list@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        // Create 2 admin manual invoices (source = admin_manual)
        let line_items = vec![json!({
            "name": "Admin Item",
            "quantity": "1",
            "unitPrice": 10000,
        })];

        let _admin_inv1 = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items.clone(),
            "Admin Client 1",
        )
        .await;

        let _admin_inv2 = create_draft_invoice(
            &app,
            &admin_token,
            &realm_id,
            account_id,
            line_items,
            "Admin Client 2",
        )
        .await;

        // Create 2 user application invoices via direct SQL
        // (apply_invoice endpoint requires payment_attempt_id or subscription_id
        //  which are complex to set up; direct SQL is cleaner for this test)
        let user_inv_id_1 = Uuid::now_v7();
        let user_inv_id_2 = Uuid::now_v7();

        for (inv_id, idx) in [(user_inv_id_1, 1u32), (user_inv_id_2, 2)] {
            // Get next invoice number counter
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
                    $6, '123 User St', 'TAX-USER-001',
                    'Seller Inc', '456 Seller Ave', 'SELLER-TAX-001',
                    CURRENT_DATE + INTERVAL '30 days', NOW(), NOW()
                )",
            )
            .bind(inv_id)
            .bind(realm_id.as_str())
            .bind(&invoice_number)
            .bind(account_id)
            .bind(Uuid::now_v7()) // applicant_user_id
            .bind(format!("User Client {}", idx))
            .execute(&ctx.app_state.pool)
            .await
            .unwrap();

            // Insert a line item for each
            sqlx::query(
                "INSERT INTO invoice_line_item (id, invoice_id, sort_order, name, quantity, unit_price, subtotal)
                 VALUES ($1, $2, 1, $3, '1', 5000, 5000)",
            )
            .bind(Uuid::now_v7())
            .bind(inv_id)
            .bind(format!("User Line Item {}", idx))
            .execute(&ctx.app_state.pool)
            .await
            .unwrap();

            // Insert history record
            sqlx::query(
                "INSERT INTO invoice_history (id, invoice_id, event_type, actor_user_id, actor_type, changes, created_at)
                 VALUES ($1, $2, 'created', $3, 'user', '{\"field\":\"status\",\"from\":null,\"to\":\"draft\"}', NOW())",
            )
            .bind(Uuid::now_v7())
            .bind(inv_id)
            .bind(Uuid::now_v7())
            .execute(&ctx.app_state.pool)
            .await
            .unwrap();
        }

        // List all invoices with no filter -> total should be 4
        let list_all_response = app
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

        assert_eq!(list_all_response.status(), StatusCode::OK);
        let list_all_body = parse_body(list_all_response.into_body()).await;
        assert_eq!(
            list_all_body["total"], 4,
            "Expected 4 total invoices (2 admin + 2 user_application)"
        );

        // List filtered by source=user_application -> total should be 2
        let list_user_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/bill/invoices?source=user_application".to_string())
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(list_user_response.status(), StatusCode::OK);
        let list_user_body = parse_body(list_user_response.into_body()).await;
        assert_eq!(
            list_user_body["total"], 2,
            "Expected 2 user_application invoices"
        );

        // Verify the data items have source = "user_application"
        let user_items = list_user_body["data"].as_array().unwrap();
        for item in user_items {
            assert_eq!(item["source"], "user_application");
        }

        // List filtered by source=user_application AND status=draft -> pending review
        let list_pending_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/bill/invoices?source=user_application&status=draft".to_string())
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(list_pending_response.status(), StatusCode::OK);
        let list_pending_body = parse_body(list_pending_response.into_body()).await;
        assert_eq!(
            list_pending_body["total"], 2,
            "Expected 2 pending review invoices (user_application + draft)"
        );
    }

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_admin_issue_user_application(ctx: &mut InvoiceTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "invoice-review-issue@test.com").await;
        let account_id = ensure_test_account(ctx, &realm_id).await;

        // Create a user application invoice via direct SQL
        let user_inv_id = Uuid::now_v7();

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
                billing_name, billing_address, billing_email, billing_tax_id,
                seller_name, seller_address, seller_tax_id,
                due_date, created_at, updated_at
            ) VALUES (
                $1, $2, $3, 'user_application', $4, $5,
                'draft', 'USD', 25000, 0, 0, 0, 25000,
                'User Apply Client', '123 User St', 'user-apply@test.com', 'TAX-USER-001',
                'Seller Inc', '456 Seller Ave', 'SELLER-TAX-001',
                CURRENT_DATE + INTERVAL '30 days', NOW(), NOW()
            )",
        )
        .bind(user_inv_id)
        .bind(realm_id.as_str())
        .bind(&invoice_number)
        .bind(account_id)
        .bind(Uuid::now_v7())
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        // Insert line item
        sqlx::query(
            "INSERT INTO invoice_line_item (id, invoice_id, sort_order, name, quantity, unit_price, subtotal)
             VALUES ($1, $2, 1, 'Pro Plan Subscription', '1', 25000, 25000)",
        )
        .bind(Uuid::now_v7())
        .bind(user_inv_id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        // Insert history record
        sqlx::query(
            "INSERT INTO invoice_history (id, invoice_id, event_type, actor_user_id, actor_type, changes, created_at)
             VALUES ($1, $2, 'created', $3, 'user', '{\"field\":\"status\",\"from\":null,\"to\":\"draft\"}', NOW())",
        )
        .bind(Uuid::now_v7())
        .bind(user_inv_id)
        .bind(Uuid::now_v7())
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        // Verify it is draft before issuing
        let detail_before = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/bill/invoices/{}", user_inv_id))
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(detail_before.status(), StatusCode::OK);
        let before_body = parse_body(detail_before.into_body()).await;
        assert_eq!(before_body["status"], "draft");
        assert_eq!(before_body["source"], "user_application");

        // Issue the user application invoice
        let issue_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/bill/invoices/{}/issue", user_inv_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(json!({}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(issue_response.status(), StatusCode::OK);
        let issue_body = parse_body(issue_response.into_body()).await;

        // Verify status changed to "issued" and source remains "user_application"
        assert_eq!(issue_body["status"], "issued");
        assert_eq!(issue_body["source"], "user_application");

        // Verify issued_at is set
        assert!(
            issue_body["issuedAt"].is_string(),
            "issuedAt should be set after issuing user application invoice"
        );

        // Verify history has both "created" and "issued" events
        let history = issue_body["history"].as_array().unwrap();
        let has_created = history.iter().any(|h| h["eventType"] == "created");
        let has_issued = history.iter().any(|h| h["eventType"] == "issued");
        assert!(has_created, "Expected 'created' event in history");
        assert!(
            has_issued,
            "Expected 'issued' event in history after issuing"
        );
    }
}
