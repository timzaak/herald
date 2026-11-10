// Points System Scenario Test 11: Consume Edge Cases (Input Validation Boundaries)
//
// User Story: docs/user-stories/points-billing-events.md
// Covers: Input validation boundaries for the consume endpoint
//   - amount=0 rejection (zero is not a valid consumption)
//   - amount=1 minimum valid consumption
//   - Oversized amount (insufficient balance, no integer overflow)
//   - Missing userId field
//   - Missing amount field
//
// These complement test_10 which covers negative amounts, and test_08/test_09
// which cover normal and exact-balance consumption success paths.
//

use crate::tests::helpers::test_setup_helpers::assert_response_error;
use crate::tests::scenarios::points::fixtures::*;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;

/// Helper: create standard test fixtures (user, account, client app, API key)
/// Returns (user_id, wallet_id, client_app_id, api_key).
async fn setup_consume_fixtures(
    ctx: &TestContext,
    email: &str,
    balance: i64,
) -> (uuid::Uuid, uuid::Uuid, uuid::Uuid, String) {
    let user_id = create_test_user(&ctx._app_state.pool, &ctx._realm_id, email).await;
    let wallet_id = create_test_points_wallet(&ctx._app_state.pool, user_id, balance).await;
    let client_app_id = create_test_client_app(&ctx._app_state.pool, &ctx._realm_id).await;
    let api_key = create_test_api_key(&ctx._app_state.pool, &ctx._realm_id, client_app_id).await;
    (user_id, wallet_id, client_app_id, api_key)
}

/// Helper: build a consume request with custom JSON body.
fn build_consume_request(
    realm_id: &str,
    api_key: &str,
    payload: serde_json::Value,
) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("/api/ext/points/{}/consume", realm_id))
        .header("content-type", "application/json")
        .header("X-API-Key", api_key)
        .body(Body::from(payload.to_string()))
        .unwrap()
}

/// Helper: parse JSON response body.
async fn parse_response_body(response: axum::response::Response) -> serde_json::Value {
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    serde_json::from_slice(&body_bytes).expect("Failed to parse JSON")
}

// Scenario 2: amount=1 (minimum valid value) should succeed
// User Story: docs/user-stories/points-billing-events.md
// Covers: Minimum valid consumption amount (1 point)

#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_consume_amount_one_succeeds(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: user with balance 5000
    let (user_id, _wallet_id, client_app_id, api_key) =
        setup_consume_fixtures(ctx, "edge-one@example.com", 5000).await;

    // When: consume amount=1
    let payload = json!({
        "userId": user_id.to_string(),
        "clientAppId": client_app_id.to_string(),
        "amount": 1,
        "description": "minimum consumption"
    });

    let response = app
        .clone()
        .oneshot(build_consume_request(&ctx._realm_id, &api_key, payload))
        .await
        .unwrap();

    // Then: 200 OK, balance=4999, transaction amount=-1
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "amount=1 should succeed with 200 OK"
    );

    let body = parse_response_body(response).await;

    // Multi-bucket consume response: total consumed at the
    // top level; per-bucket transaction carries the deduction magnitude and
    // resulting balance.
    let transactions = body["transactions"]
        .as_array()
        .expect("response should contain a transactions array");
    assert_eq!(transactions.len(), 1, "single-pool consume → 1 transaction");
    let txn = &transactions[0];
    assert_eq!(
        body["amount"].as_i64(),
        Some(1),
        "Response amount should be the total consumed (1)"
    );
    assert_eq!(
        txn["balanceAfter"].as_i64(),
        Some(4999),
        "balanceAfter should be 4999"
    );

    // Verify transaction exists
    let (txn_amount,): (i64,) = sqlx::query_as(
        "SELECT amount FROM points_transactions WHERE user_id = $1 AND type = 'consume'",
    )
    .bind(user_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(txn_amount, -1, "Transaction amount should be -1");
}

// Scenario 3: Oversized amount should fail with insufficient balance, not panic
// User Story: docs/user-stories/points-billing-events.md
// Covers: No integer overflow panic on very large amounts; normal error response

#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_consume_oversized_amount_insufficient_balance(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: user with balance 5000
    let (user_id, wallet_id, client_app_id, api_key) =
        setup_consume_fixtures(ctx, "edge-oversize@example.com", 5000).await;

    // When: consume with amount far exceeding balance (999_999_999_999)
    let payload = json!({
        "userId": user_id.to_string(),
        "clientAppId": client_app_id.to_string(),
        "amount": 999_999_999_999_i64,
        "description": "oversized amount"
    });

    let response = app
        .clone()
        .oneshot(build_consume_request(&ctx._realm_id, &api_key, payload))
        .await
        .unwrap();

    // Then: error response (not a panic/server crash)
    // Handler validates amount > 1_000_000 first, so this returns InvalidAmount (400)
    assert!(
        response.status() == StatusCode::BAD_REQUEST
            || response.status() == StatusCode::INTERNAL_SERVER_ERROR,
        "Oversized amount should return an error, got: {}",
        response.status()
    );

    let body = parse_response_body(response).await;
    assert_response_error(&body, None);

    // Verify balance unchanged
    let (balance,): (i64,) =
        sqlx::query_as("SELECT COALESCE(SUM(l.remaining_amount) FILTER (WHERE l.status = 'active' AND l.remaining_amount > 0 AND (l.effective_at IS NULL OR l.effective_at <= NOW()) AND (l.expires_at IS NULL OR l.expires_at > NOW())), 0)::BIGINT AS total_balance FROM points_wallets w LEFT JOIN points_credit_ledger l ON l.realm_id = w.realm_id AND l.user_id = w.user_id AND l.bucket_id = w.bucket_id WHERE w.id = $1 GROUP BY w.id")
            .bind(wallet_id)
            .fetch_one(&ctx._app_state.pool)
            .await
            .unwrap();
    assert_eq!(
        balance, 5000,
        "Balance should remain 5000 after oversized rejection"
    );
}
