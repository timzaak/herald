// =============================================================================
// Points Grant Test Helpers
// =============================================================================
//
// Helpers for admin grant points API tests.
// Provides functions for building requests, calling the grant endpoint,
// and asserting balance state in the database.
//
// =============================================================================

#![allow(dead_code)]

use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

/// Build a grant points admin HTTP request with session auth.
///
/// Returns a Request<Body> ready to send via oneshot.
pub fn grant_points_admin_request(
    _realm_id: &str,
    user_id: Uuid,
    bucket_id: Uuid,
    amount: i64,
    reason: &str,
    validity_days: Option<i64>,
    session_token: &str,
) -> Request<Body> {
    let mut body = json!({
        "userId": user_id.to_string(),
        "bucketId": bucket_id.to_string(),
        "amount": amount,
        "reason": reason,
    });
    if let Some(days) = validity_days {
        body["validityDays"] = json!(days);
    }

    Request::builder()
        .method("POST")
        .uri("/api/points/grant".to_string())
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {}", session_token))
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Call the admin grant endpoint via the unified test router and return
/// the parsed GrantPointsResponse on success, or the status code on failure.
pub async fn grant_points_admin_via_api(
    ctx: &TestContext,
    realm_id: &str,
    user_id: Uuid,
    amount: i64,
    reason: &str,
    validity_days: Option<i64>,
    session_token: &str,
) -> (StatusCode, Option<serde_json::Value>) {
    // Credit Buckets model: every grant targets an explicit bucket.
    // Bind the realm's legacy test bucket so the grant succeeds.
    let bucket_id = crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
        &ctx._app_state.pool,
        realm_id,
    )
    .await;
    let app = ctx.create_unified_test_router();
    let request = grant_points_admin_request(
        realm_id,
        user_id,
        bucket_id,
        amount,
        reason,
        validity_days,
        session_token,
    );
    let response = app.oneshot(request).await.unwrap();

    let status = response.status();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: Option<serde_json::Value> = if body_bytes.is_empty() {
        None
    } else {
        Some(serde_json::from_slice(&body_bytes).unwrap())
    };
    (status, body)
}

/// Assert granted_balance for a user matches the expected value.
pub async fn assert_granted_balance(pool: &sqlx::PgPool, user_id: Uuid, expected: i64) {
    // `points_wallets.granted_balance` was dropped; derive available
    // granted credit from `points_credit_ledger` (credit_type = 'granted_credit').
    let row: Option<i64> = sqlx::query_scalar(
        "SELECT COALESCE(SUM(l.remaining_amount) FILTER (
                        WHERE l.status = 'active' AND l.remaining_amount > 0
                          AND l.credit_type = 'granted_credit'
                          AND (l.effective_at IS NULL OR l.effective_at <= NOW())
                          AND (l.expires_at  IS NULL OR l.expires_at  >  NOW())
                    ), 0)::BIGINT
             FROM points_wallets w
             LEFT JOIN points_credit_ledger l
               ON l.realm_id = w.realm_id AND l.user_id = w.user_id AND l.bucket_id = w.bucket_id
             WHERE w.user_id = $1
             GROUP BY w.id",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .unwrap();

    let actual = row.unwrap_or(0);
    assert_eq!(
        actual, expected,
        "granted_balance mismatch: expected {}, got {}",
        expected, actual
    );
}

/// Assert total_balance for a user matches the expected value.
///
/// `points_wallets.total_balance` was dropped; derive the total
/// available balance from `points_credit_ledger`.
pub async fn assert_total_balance(pool: &sqlx::PgPool, user_id: Uuid, expected: i64) {
    let row: Option<i64> = sqlx::query_scalar(
        "SELECT COALESCE(SUM(l.remaining_amount) FILTER (
                        WHERE l.status = 'active' AND l.remaining_amount > 0
                          AND (l.effective_at IS NULL OR l.effective_at <= NOW())
                          AND (l.expires_at  IS NULL OR l.expires_at  >  NOW())
                    ), 0)::BIGINT
             FROM points_wallets w
             LEFT JOIN points_credit_ledger l
               ON l.realm_id = w.realm_id AND l.user_id = w.user_id AND l.bucket_id = w.bucket_id
             WHERE w.user_id = $1
             GROUP BY w.id",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .unwrap();

    let actual = row.unwrap_or(0);
    assert_eq!(
        actual, expected,
        "total_balance mismatch: expected {}, got {}",
        expected, actual
    );
}

// =============================================================================
// Ext/SDK Grant Points Helpers
// =============================================================================
//
// Helpers for ext/SDK grant points API tests.
// Uses X-API-Key header for authentication (same pattern as test_14).
//
// =============================================================================

/// Build an ext grant points HTTP request with API Key auth.
///
/// Returns a Request<Body> ready to send via oneshot.
pub fn grant_points_ext_request(
    realm_id: &str,
    user_id: Uuid,
    bucket_id: Uuid,
    amount: i64,
    reason: &str,
    validity_days: Option<i64>,
    api_key: &str,
) -> Request<Body> {
    let mut body = json!({
        "userId": user_id.to_string(),
        "bucketId": bucket_id.to_string(),
        "amount": amount,
        "reason": reason,
    });
    if let Some(days) = validity_days {
        body["validityDays"] = json!(days);
    }

    Request::builder()
        .method("POST")
        .uri(format!("/api/ext/points/{}/grant", realm_id))
        .header("content-type", "application/json")
        .header("X-API-Key", api_key)
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Call the ext grant endpoint via the unified test router and return
/// the status code and optional parsed response body.
pub async fn grant_points_ext_via_api(
    ctx: &TestContext,
    realm_id: &str,
    user_id: Uuid,
    amount: i64,
    reason: &str,
    validity_days: Option<i64>,
    api_key: &str,
) -> (StatusCode, Option<serde_json::Value>) {
    // Credit Buckets model: every grant targets an explicit bucket.
    // Bind the realm's legacy test bucket so the grant succeeds.
    let bucket_id = crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
        &ctx._app_state.pool,
        realm_id,
    )
    .await;
    let app = ctx.create_unified_test_router();
    let request = grant_points_ext_request(
        realm_id,
        user_id,
        bucket_id,
        amount,
        reason,
        validity_days,
        api_key,
    );
    let response = app.oneshot(request).await.unwrap();

    let status = response.status();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: Option<serde_json::Value> = if body_bytes.is_empty() {
        None
    } else {
        Some(serde_json::from_slice(&body_bytes).unwrap())
    };
    (status, body)
}
