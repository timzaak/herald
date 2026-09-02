// =============================================================================
// Points System Scenario Test 75: Ext/SDK Grant Points
// =============================================================================
//
// **User Story**: US-TP-017 (Third-party app grants points to user via SDK)
// **Priority**: P0
//
// **Covers**:
// - Normal ext grant succeeds with correct balances
// - Ext grant with validity days sets expires_at
// - Amount <= 0 rejected (400)
// - Empty reason rejected (400)
// - Invalid API Key rejected (401)
// - Cross-realm rejected (403)
// - User not found rejected (404)
// - Wallet auto-created on first grant
// - Grant record appears in transaction history with correct types
// - Concurrent grants to same user both succeed
//
// =============================================================================

use crate::tests::helpers::points_grant_helpers::{
    assert_granted_balance, assert_total_balance, grant_points_ext_via_api,
};
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::http::StatusCode;
use sqlx::Row;
use test_context::test_context;
use uuid::Uuid;

// =============================================================================
// Test 75.1: Normal ext grant succeeds
// =============================================================================
// User Story: US-TP-017
// Covers: API Key auth, valid userId, amount=100, reason="sdk grant", 200 OK
// =============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_75_1_normal_ext_grant_succeeds(ctx: &mut TestContext) {
    // Given: Valid API Key with points grant permission, target user exists
    let (api_key, _client_app_id) = create_ext_api_key(ctx).await;
    let target_user_id = create_target_user(ctx, "target_75_1@test.com").await;

    // When: POST /api/ext/points/{realmId}/grant with userId, amount=100, reason="sdk grant"
    let (status, body) = grant_points_ext_via_api(
        ctx,
        &ctx._realm_id,
        target_user_id,
        100,
        "sdk grant",
        None,
        &api_key,
    )
    .await;

    // Then: 200 response with transactionId, grantedBalance=100, balance=100, expiresAt=null
    assert_eq!(status, StatusCode::OK, "Expected 200 OK, got {:?}", status);
    let resp = body.expect("Response body should exist");
    assert!(
        resp["transactionId"].is_string(),
        "transactionId should be a string"
    );
    assert_eq!(resp["userId"], target_user_id.to_string());
    assert_eq!(resp["amount"], 100);
    assert_eq!(resp["grantedBalance"], 100);
    assert_eq!(resp["balance"], 100);
    assert!(
        resp["expiresAt"].is_null(),
        "expiresAt should be null for permanent grant"
    );

    // Verify DB: granted_balance=100, total_balance=100
    assert_granted_balance(&ctx._app_state.pool, target_user_id, 100).await;
    assert_total_balance(&ctx._app_state.pool, target_user_id, 100).await;
}

// =============================================================================
// Test 75.2: Ext grant with validity days
// =============================================================================
// User Story: US-TP-017
// Covers: Grant with validityDays=7 sets expiresAt approximately now+7 days
// =============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_75_2_ext_grant_with_validity_days(ctx: &mut TestContext) {
    // Given: Valid API Key, target user exists
    let (api_key, _) = create_ext_api_key(ctx).await;
    let target_user_id = create_target_user(ctx, "target_75_2@test.com").await;

    // When: POST with amount=200, reason="campaign bonus", validityDays=7
    let (status, body) = grant_points_ext_via_api(
        ctx,
        &ctx._realm_id,
        target_user_id,
        200,
        "campaign bonus",
        Some(7),
        &api_key,
    )
    .await;

    // Then: 200 response, expiresAt is approximately now+7 days
    assert_eq!(status, StatusCode::OK, "Expected 200 OK, got {:?}", status);
    let resp = body.expect("Response body should exist");
    assert_eq!(resp["amount"], 200);
    assert_eq!(resp["grantedBalance"], 200);
    assert_eq!(resp["balance"], 200);

    let expires_at_str = resp["expiresAt"]
        .as_str()
        .expect("expiresAt should be set when validityDays is provided");
    let expires_at = expires_at_str
        .parse::<chrono::DateTime<chrono::Utc>>()
        .expect("expiresAt should be a valid ISO 8601 datetime");

    let now = chrono::Utc::now();
    let lower_bound = now + chrono::Duration::days(6);
    let upper_bound = now + chrono::Duration::days(8);
    assert!(
        expires_at > lower_bound && expires_at < upper_bound,
        "expiresAt should be approximately now+7 days, got {:?}",
        expires_at
    );
}

// =============================================================================
// Test 75.3: Amount <= 0 rejected
// =============================================================================
// User Story: US-TP-017
// Covers: amount=0 and amount=-1 both return 400
// =============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_75_3_amount_zero_and_negative_rejected(ctx: &mut TestContext) {
    let (api_key, _) = create_ext_api_key(ctx).await;
    let target_user_id = create_target_user(ctx, "target_75_3@test.com").await;

    // When: POST with amount=0
    let (status_zero, _) = grant_points_ext_via_api(
        ctx,
        &ctx._realm_id,
        target_user_id,
        0,
        "test",
        None,
        &api_key,
    )
    .await;
    assert_eq!(
        status_zero,
        StatusCode::BAD_REQUEST,
        "amount=0 should return 400"
    );

    // When: POST with amount=-1
    let (status_neg, _) = grant_points_ext_via_api(
        ctx,
        &ctx._realm_id,
        target_user_id,
        -1,
        "test",
        None,
        &api_key,
    )
    .await;
    assert_eq!(
        status_neg,
        StatusCode::BAD_REQUEST,
        "amount=-1 should return 400"
    );
}

// =============================================================================
// Test 75.4: Empty reason rejected
// =============================================================================
// User Story: US-TP-017
// Covers: reason="" returns 400
// =============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_75_4_empty_reason_rejected(ctx: &mut TestContext) {
    let (api_key, _) = create_ext_api_key(ctx).await;
    let target_user_id = create_target_user(ctx, "target_75_4@test.com").await;

    // When: POST with reason=""
    let (status, _) =
        grant_points_ext_via_api(ctx, &ctx._realm_id, target_user_id, 100, "", None, &api_key)
            .await;

    // Then: 400 response
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "empty reason should return 400"
    );
}

// =============================================================================
// Test 75.5: Invalid API Key rejected
// =============================================================================
// User Story: US-TP-017
// Covers: Invalid/missing API Key returns 401
// =============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_75_5_invalid_api_key_rejected(ctx: &mut TestContext) {
    let target_user_id = create_target_user(ctx, "target_75_5@test.com").await;

    // When: POST with an invalid API Key
    let invalid_api_key = "invalid-api-key-does-not-exist";
    let (status, _) = grant_points_ext_via_api(
        ctx,
        &ctx._realm_id,
        target_user_id,
        100,
        "test grant",
        None,
        invalid_api_key,
    )
    .await;

    // Then: 401 response
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "invalid API Key should return 401"
    );
}

// =============================================================================
// Test 75.6: Cross-realm rejected
// =============================================================================
// User Story: US-TP-017
// Covers: API Key belongs to realm A, request targets realm B, returns 403
// =============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_75_6_cross_realm_rejected(ctx: &mut TestContext) {
    // Given: API Key belongs to ctx._realm_id
    let (api_key, _) = create_ext_api_key(ctx).await;
    let target_user_id = create_target_user(ctx, "target_75_6@test.com").await;

    // When: POST grant request targeting a different realm
    let different_realm_id = "other-realm-75-6";
    let (status, _) = grant_points_ext_via_api(
        ctx,
        different_realm_id,
        target_user_id,
        100,
        "cross-realm grant",
        None,
        &api_key,
    )
    .await;

    // Then: 403 response (API Key does not have access to different realm)
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "cross-realm grant should return 403"
    );
}

// =============================================================================
// Test 75.7: User not found rejected
// =============================================================================
// User Story: US-TP-017
// Covers: userId pointing to non-existent user returns 404
// =============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_75_7_user_not_found_rejected(ctx: &mut TestContext) {
    let (api_key, _) = create_ext_api_key(ctx).await;
    let non_existent_user_id = Uuid::now_v7();

    // When: POST with userId pointing to non-existent user
    let (status, _) = grant_points_ext_via_api(
        ctx,
        &ctx._realm_id,
        non_existent_user_id,
        100,
        "grant to nobody",
        None,
        &api_key,
    )
    .await;

    // Then: 404 response
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "non-existent user should return 404"
    );
}

// =============================================================================
// Test 75.9: Grant record in transaction history
// =============================================================================
// User Story: US-TP-017
// Covers: points_transactions has row with type='grant', credit_type='granted_credit',
//         source_type='sdk_grant' in credit ledger
// =============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_75_9_grant_record_in_transaction_history(ctx: &mut TestContext) {
    // Given: Ext grant succeeds
    let (api_key, _) = create_ext_api_key(ctx).await;
    let target_user_id = create_target_user(ctx, "target_75_9@test.com").await;
    let grant_reason = "sdk promotional bonus";

    let (status, _) = grant_points_ext_via_api(
        ctx,
        &ctx._realm_id,
        target_user_id,
        50,
        grant_reason,
        None,
        &api_key,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Then: points_transactions has row with type='grant', credit_type='granted_credit'
    let tx_row = sqlx::query(
        "SELECT type, credit_type, amount, description \
         FROM points_transactions \
         WHERE user_id = $1 AND type = 'grant' \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(target_user_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("Grant transaction should exist");

    let tx_type: String = tx_row.get("type");
    let credit_type: Option<String> = tx_row.get("credit_type");
    let amount: i64 = tx_row.get("amount");
    let description: Option<String> = tx_row.get("description");

    assert_eq!(tx_type, "grant", "Transaction type should be 'grant'");
    assert_eq!(
        credit_type.as_deref(),
        Some("granted_credit"),
        "Credit type should be 'granted_credit'"
    );
    assert_eq!(amount, 50, "Amount should match grant amount");

    let desc = description.expect("Description should be set");
    assert!(
        desc.contains(grant_reason),
        "Description should contain the grant reason, got: {:?}",
        desc
    );

    // Also verify credit ledger has source_type='sdk_grant'
    let ledger_row = sqlx::query(
        "SELECT source_type, credit_type \
         FROM points_credit_ledger \
         WHERE user_id = $1 AND credit_type = 'granted_credit' \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(target_user_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("Credit ledger entry should exist");

    let source_type: String = ledger_row.get("source_type");
    assert_eq!(
        source_type, "sdk_grant",
        "Credit ledger source_type should be 'sdk_grant'"
    );
}

// =============================================================================
// Test 75.10: Concurrent grants to same user
// =============================================================================
// User Story: US-TP-017
// Covers: Two concurrent ext grant requests for same user, both succeed,
//         final granted_balance = sum of both amounts
// =============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_75_10_concurrent_grants_to_same_user(ctx: &mut TestContext) {
    // Given: Target user exists
    let (api_key, _) = create_ext_api_key(ctx).await;
    let target_user_id = create_target_user(ctx, "target_75_10@test.com").await;

    // When: Two concurrent ext grant requests for same user
    let grant1 = grant_points_ext_via_api(
        ctx,
        &ctx._realm_id,
        target_user_id,
        60,
        "concurrent grant 1",
        None,
        &api_key,
    );
    let grant2 = grant_points_ext_via_api(
        ctx,
        &ctx._realm_id,
        target_user_id,
        40,
        "concurrent grant 2",
        None,
        &api_key,
    );

    let (result1, result2) = tokio::join!(grant1, grant2);

    // Then: Both succeed
    assert_eq!(
        result1.0,
        StatusCode::OK,
        "First grant should succeed, got {:?}",
        result1.0
    );
    assert_eq!(
        result2.0,
        StatusCode::OK,
        "Second grant should succeed, got {:?}",
        result2.0
    );

    // Final granted_balance = 60 + 40 = 100
    assert_granted_balance(&ctx._app_state.pool, target_user_id, 100).await;
    assert_total_balance(&ctx._app_state.pool, target_user_id, 100).await;
}

// =============================================================================
// Helpers
// =============================================================================

/// Create a target user without any roles.
async fn create_target_user(ctx: &TestContext, email: &str) -> Uuid {
    let user_uuid = Uuid::now_v7();
    let password_hash =
        bcrypt::hash("password123", bcrypt::DEFAULT_COST).expect("Failed to hash password");

    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status)
         VALUES ($1, $2, $3, $4, 1)",
    )
    .bind(user_uuid)
    .bind(&ctx._realm_id)
    .bind(email)
    .bind(&password_hash)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to create target user");

    user_uuid
}

/// Create an API Key for ext/SDK use in the test realm.
///
/// Returns (api_key_plaintext, client_app_id).
async fn create_ext_api_key(ctx: &TestContext) -> (String, Uuid) {
    use crate::tests::scenarios::points::fixtures::create_test_api_key;

    let client_app_id = Uuid::now_v7();
    let api_key = create_test_api_key(&ctx._app_state.pool, &ctx._realm_id, client_app_id).await;
    (api_key, client_app_id)
}
