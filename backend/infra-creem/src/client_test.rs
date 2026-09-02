// =============================================================================
// CreemClient Unit Tests
// =============================================================================
//
// Unit tests for Creem API client using wiremock for HTTP mocking
//
// =============================================================================

use super::*;
use herald_domain::common::entities::app_errors::CoreError;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path, query_param},
};

/// Helper function to create a test client
fn create_test_client(mock_server: &MockServer) -> CreemClient {
    CreemClient {
        http: reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("Failed to create test HTTP client"),
        api_key: "test_api_key".to_string(),
        base_url: mock_server.uri(),
    }
}

/// Test successful checkout session creation
#[tokio::test]
async fn test_unit_create_checkout_session_success() {
    // Arrange
    let mock_server = MockServer::start().await;
    let client = create_test_client(&mock_server);

    Mock::given(method("POST"))
        .and(path("/v1/checkouts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "chk_test123",
            "checkout_url": "https://checkout.test.creem.io/abc123",
            "status": "pending"
        })))
        .mount(&mock_server)
        .await;

    let request = CreateCheckoutRequest {
        product_id: "prod_starter_monthly".to_string(),
        success_url: Some("https://example.com/success".to_string()),
        customer: crate::models::CreemCheckoutCustomer {
            email: Some("test@example.com".to_string()),
        },
        metadata: None,
    };

    // Act
    let result = client.create_checkout_session(&request).await;

    // Assert
    assert!(result.is_ok());
    let session = result.unwrap();
    assert_eq!(session.id, "chk_test123");
    assert_eq!(
        session.checkout_url,
        "https://checkout.test.creem.io/abc123"
    );
    assert_eq!(session.status, "pending");
}

/// Test API authentication failure (401)
#[tokio::test]
async fn test_unit_create_checkout_session_unauthorized() {
    // Arrange
    let mock_server = MockServer::start().await;
    let client = CreemClient {
        http: reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("Failed to create test HTTP client"),
        api_key: "invalid_key".to_string(),
        base_url: mock_server.uri(),
    };

    Mock::given(method("POST"))
        .and(path("/v1/checkouts"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error": "Unauthorized",
            "message": "Invalid API key"
        })))
        .mount(&mock_server)
        .await;

    let request = CreateCheckoutRequest {
        product_id: "prod_test".to_string(),
        success_url: Some("https://example.com/success".to_string()),
        customer: crate::models::CreemCheckoutCustomer {
            email: Some("test@example.com".to_string()),
        },
        metadata: None,
    };

    // Act
    let result = client.create_checkout_session(&request).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        CoreError::InternalServerError(msg) => {
            assert!(msg.contains("401"));
            assert!(msg.contains("Unauthorized"));
        }
        _ => panic!("Expected InternalServerError for 401 response"),
    }
}

/// Test invalid JSON response
#[tokio::test]
async fn test_unit_create_checkout_session_invalid_json() {
    // Arrange
    let mock_server = MockServer::start().await;
    let client = create_test_client(&mock_server);

    Mock::given(method("POST"))
        .and(path("/v1/checkouts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "chk_test",
            "checkout_url": "https://checkout.test.creem.io/test",
            // Missing "status" field
        })))
        .mount(&mock_server)
        .await;

    let request = CreateCheckoutRequest {
        product_id: "prod_test".to_string(),
        success_url: Some("https://example.com/success".to_string()),
        customer: crate::models::CreemCheckoutCustomer {
            email: Some("test@example.com".to_string()),
        },
        metadata: None,
    };

    // Act
    let result = client.create_checkout_session(&request).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        CoreError::InternalServerError(msg) => {
            assert!(msg.contains("Invalid Creem response"), "actual msg: {msg}");
        }
        _ => panic!("Expected InternalServerError for invalid JSON"),
    }
}

// =============================================================================
// search_transactions tests
// =============================================================================

// User Story: As a billing system, I need to search Creem transactions so that
// I can reconcile payment states and detect missing webhook events.
// Covers: correct HTTP method, path, query params, and API key header.

#[tokio::test]
async fn test_search_transactions_sends_correct_params() {
    let mock_server = MockServer::start().await;
    let client = create_test_client(&mock_server);

    Mock::given(method("GET"))
        .and(path("/v1/transactions/search"))
        .and(query_param("page_number", "1"))
        .and(query_param("page_size", "20"))
        .and(header("x-api-key", "test_api_key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [],
            "pagination": {
                "total_records": 0,
                "total_pages": 0,
                "current_page": 1,
                "next_page": null,
                "prev_page": null
            }
        })))
        .mount(&mock_server)
        .await;

    let params = SearchTransactionsParams {
        page_number: 1,
        page_size: 20,
        created_after: None,
    };
    let result = client.search_transactions(&params).await;

    assert!(result.is_ok());
}

// User Story: As a billing system, I need to correctly parse Creem transaction
// data so that downstream reconciliation can distinguish payment types and amounts.
// Covers: parsing of mixed transaction types, pagination metadata.

#[tokio::test]
async fn test_search_transactions_parses_response() {
    let mock_server = MockServer::start().await;
    let client = create_test_client(&mock_server);

    Mock::given(method("GET"))
        .and(path("/v1/transactions/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                {
                    "id": "txn_payment_001",
                    "mode": "live",
                    "object": "transaction",
                    "amount": 2999,
                    "currency": "USD",
                    "type": "payment",
                    "status": "succeeded",
                    "created_at": 1717920000,
                    "amount_paid": 2999,
                    "refunded_amount": null,
                    "order": { "order_id": "ord_001" },
                    "subscription": null,
                    "customer": { "customer_id": "cus_001" }
                },
                {
                    "id": "txn_invoice_002",
                    "mode": "live",
                    "object": "transaction",
                    "amount": 1999,
                    "currency": "EUR",
                    "type": "invoice",
                    "status": "pending",
                    "created_at": 1718006400,
                    "amount_paid": 0,
                    "refunded_amount": null,
                    "order": { "order_id": "ord_002" },
                    "subscription": { "subscription_id": "sub_001" },
                    "customer": null
                }
            ],
            "pagination": {
                "total_records": 2,
                "total_pages": 1,
                "current_page": 1,
                "next_page": null,
                "prev_page": null
            }
        })))
        .mount(&mock_server)
        .await;

    let params = SearchTransactionsParams {
        page_number: 1,
        page_size: 20,
        created_after: None,
    };
    let result = client.search_transactions(&params).await.unwrap();

    assert_eq!(result.data.len(), 2);
    assert_eq!(result.data[0].id, "txn_payment_001");
    assert_eq!(result.data[0].r#type, "payment");
    assert_eq!(result.data[0].status, "succeeded");
    assert_eq!(result.data[0].amount, 2999);
    assert_eq!(result.data[1].r#type, "invoice");

    assert_eq!(result.pagination.total_records, 2);
    assert!(result.pagination.next_page.is_none());
}

// User Story: As a billing system, I need to correctly parse pagination metadata
// so that the compensation job knows whether more pages exist.
// Covers: next_page presence indicates more results; method itself is single-page.

#[tokio::test]
async fn test_search_transactions_handles_pagination() {
    let mock_server = MockServer::start().await;
    let client = create_test_client(&mock_server);

    Mock::given(method("GET"))
        .and(path("/v1/transactions/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                {
                    "id": "txn_010",
                    "mode": "live",
                    "object": "transaction",
                    "amount": 1000,
                    "currency": "USD",
                    "type": "payment",
                    "status": "succeeded",
                    "created_at": 1717920000,
                    "amount_paid": 1000,
                    "refunded_amount": null,
                    "order": null,
                    "subscription": null,
                    "customer": null
                }
            ],
            "pagination": {
                "total_records": 50,
                "total_pages": 3,
                "current_page": 1,
                "next_page": 2,
                "prev_page": null
            }
        })))
        .mount(&mock_server)
        .await;

    let params = SearchTransactionsParams {
        page_number: 1,
        page_size: 20,
        created_after: None,
    };
    let result = client.search_transactions(&params).await.unwrap();

    assert_eq!(result.data.len(), 1);
    assert_eq!(result.pagination.total_records, 50);
    assert_eq!(result.pagination.total_pages, 3);
    assert_eq!(result.pagination.current_page, 1);
    assert_eq!(result.pagination.next_page, Some(2));
    assert!(result.pagination.prev_page.is_none());
}

// =============================================================================
// search_subscriptions tests
// =============================================================================

// User Story: As a billing system, I need to search Creem subscriptions so that
// I can detect status changes (active, canceled) that may not have arrived via webhook.
// Covers: correct HTTP method, path, query params, and API key header.

#[tokio::test]
async fn test_search_subscriptions_sends_correct_params() {
    let mock_server = MockServer::start().await;
    let client = create_test_client(&mock_server);

    Mock::given(method("GET"))
        .and(path("/v1/subscriptions/search"))
        .and(query_param("page_number", "1"))
        .and(query_param("page_size", "20"))
        .and(header("x-api-key", "test_api_key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [],
            "pagination": {
                "total_records": 0,
                "total_pages": 0,
                "current_page": 1,
                "next_page": null,
                "prev_page": null
            }
        })))
        .mount(&mock_server)
        .await;

    let params = SearchSubscriptionsParams {
        page_number: 1,
        page_size: 20,
        created_after: None,
    };
    let result = client.search_subscriptions(&params).await;

    assert!(result.is_ok());
}

// User Story: As a billing system, I need to correctly parse active subscription
// data so that downstream logic can compare subscription state with local records.
// Covers: subscription fields, nested customer/product objects, pagination.

#[tokio::test]
async fn test_search_subscriptions_parses_response() {
    let mock_server = MockServer::start().await;
    let client = create_test_client(&mock_server);

    Mock::given(method("GET"))
        .and(path("/v1/subscriptions/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                {
                    "id": "sub_active_001",
                    "status": "active",
                    "customer": {
                        "email": "alice@example.com"
                    },
                    "product": {
                        "id": "prod_monthly",
                        "name": "Monthly Plan",
                        "price": 2999,
                        "currency": "USD",
                        "billing_type": "recurring",
                        "billing_period": "month"
                    },
                    "canceled_at": null,
                    "current_period_start_date": "2026-06-01",
                    "current_period_end_date": "2026-07-01",
                    "next_transaction_date": "2026-07-01",
                    "last_transaction_date": "2026-06-01",
                    "created_at": "2026-01-15T08:00:00Z",
                    "updated_at": "2026-06-01T00:00:00Z"
                }
            ],
            "pagination": {
                "total_records": 1,
                "total_pages": 1,
                "current_page": 1,
                "next_page": null,
                "prev_page": null
            }
        })))
        .mount(&mock_server)
        .await;

    let params = SearchSubscriptionsParams {
        page_number: 1,
        page_size: 20,
        created_after: None,
    };
    let result = client.search_subscriptions(&params).await.unwrap();

    assert_eq!(result.data.len(), 1);
    let sub = &result.data[0];
    assert_eq!(sub.id, "sub_active_001");
    assert_eq!(sub.status, "active");
    assert!(sub.canceled_at.is_none());
    assert_eq!(sub.current_period_start_date.as_deref(), Some("2026-06-01"));
    assert_eq!(sub.customer.as_ref().unwrap().email, "alice@example.com");

    assert_eq!(result.pagination.total_pages, 1);
}

// User Story: As a billing system, I need to correctly parse canceled subscriptions
// so that the compensation job can detect churn events missed by webhooks.
// Covers: canceled_at field present, status "canceled".

#[tokio::test]
async fn test_search_subscriptions_handles_canceled_subscription() {
    let mock_server = MockServer::start().await;
    let client = create_test_client(&mock_server);

    Mock::given(method("GET"))
        .and(path("/v1/subscriptions/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                {
                    "id": "sub_canceled_001",
                    "status": "canceled",
                    "customer": {
                        "email": "bob@example.com"
                    },
                    "product": {
                        "id": "prod_yearly",
                        "name": "Yearly Plan",
                        "price": 29999,
                        "currency": "USD",
                        "billing_type": "recurring",
                        "billing_period": "year"
                    },
                    "canceled_at": "2026-06-05T10:00:00Z",
                    "current_period_start_date": "2025-06-05",
                    "current_period_end_date": "2026-06-05",
                    "next_transaction_date": null,
                    "last_transaction_date": "2025-06-05",
                    "created_at": "2025-06-05T10:00:00Z",
                    "updated_at": "2026-06-05T10:00:00Z"
                }
            ],
            "pagination": {
                "total_records": 1,
                "total_pages": 1,
                "current_page": 1,
                "next_page": null,
                "prev_page": null
            }
        })))
        .mount(&mock_server)
        .await;

    let params = SearchSubscriptionsParams {
        page_number: 1,
        page_size: 20,
        created_after: None,
    };
    let result = client.search_subscriptions(&params).await.unwrap();

    let sub = &result.data[0];
    assert_eq!(sub.id, "sub_canceled_001");
    assert_eq!(sub.status, "canceled");
    assert_eq!(sub.canceled_at.as_deref(), Some("2026-06-05T10:00:00Z"));
}

// =============================================================================
// cancel_subscription tests
// =============================================================================
//
// User Story: As a billing system, I need to cancel a Creem subscription on
// behalf of a user so that provider-side cancellation propagates back via the
// Creem webhook. Covers: correct method/path/api-key, immediate vs scheduled
// mode bodies, and provider error surfacing.

#[tokio::test]
async fn test_cancel_subscription_immediate_sends_correct_body() {
    let mock_server = MockServer::start().await;
    let client = create_test_client(&mock_server);

    Mock::given(method("POST"))
        .and(path("/v1/subscriptions/sub_abc/cancel"))
        .and(header("x-api-key", "test_api_key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "sub_abc",
            "status": "canceled",
            "canceled_at": "2026-08-03T12:00:00Z"
        })))
        .mount(&mock_server)
        .await;

    let result = client
        .cancel_subscription("sub_abc", CreemCancelMode::Immediate)
        .await
        .expect("immediate cancel should succeed");

    assert_eq!(result.id, "sub_abc");
    assert_eq!(result.status.as_deref(), Some("canceled"));
    assert_eq!(result.canceled_at.as_deref(), Some("2026-08-03T12:00:00Z"));

    // Body must carry mode=immediate and NOT carry on_execute (immediate ignores it).
    let requests = mock_server
        .received_requests()
        .await
        .expect("request recording should be enabled");
    assert_eq!(requests.len(), 1);
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["mode"], "immediate");
    assert!(body.get("onExecute").is_none());
}

#[tokio::test]
async fn test_cancel_subscription_scheduled_sends_correct_body() {
    let mock_server = MockServer::start().await;
    let client = create_test_client(&mock_server);

    Mock::given(method("POST"))
        .and(path("/v1/subscriptions/sub_def/cancel"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "sub_def",
            "status": "active",
            "canceled_at": null
        })))
        .mount(&mock_server)
        .await;

    let result = client
        .cancel_subscription("sub_def", CreemCancelMode::Scheduled)
        .await
        .expect("scheduled cancel should succeed");

    assert_eq!(result.id, "sub_def");
    assert_eq!(result.status.as_deref(), Some("active"));

    // Body must carry mode=scheduled and onExecute=cancel (we never pause).
    let requests = mock_server
        .received_requests()
        .await
        .expect("request recording should be enabled");
    assert_eq!(requests.len(), 1);
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["mode"], "scheduled");
    assert_eq!(body["onExecute"], "cancel");
}

#[tokio::test]
async fn test_cancel_subscription_surfaces_provider_error() {
    let mock_server = MockServer::start().await;
    let client = create_test_client(&mock_server);

    Mock::given(method("POST"))
        .and(path("/v1/subscriptions/sub_missing/cancel"))
        .respond_with(ResponseTemplate::new(404).set_body_string("subscription not found"))
        .mount(&mock_server)
        .await;

    let result = client
        .cancel_subscription("sub_missing", CreemCancelMode::Immediate)
        .await;

    assert!(
        matches!(result, Err(CoreError::InternalServerError(_))),
        "provider error must surface, got {:?}",
        result
    );
}
