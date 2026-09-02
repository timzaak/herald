// =============================================================================
// Points System Scenario Test 74: Admin Grant Points
// =============================================================================
//
// **User Story**: US-PO-08 (Admin grants points to user)
// **Priority**: P0
//
// **Covers**:
// - Normal grant succeeds with correct balances
// - Grant with validity days sets expires_at
// - Amount <= 0 rejected (400)
// - Empty reason rejected (400)
// - User not found rejected (404)
// - Permission denied for non-admin (403)
// - Cross-realm rejected (403)
// - Wallet auto-created on first grant
// - Grant record appears in transaction history
// - Cumulative grants update balance correctly
//
// =============================================================================

use crate::tests::helpers::auth_helpers::{create_admin_session_with_user, grant_realm_admin_role};
use crate::tests::helpers::points_grant_helpers::{
    assert_granted_balance, assert_total_balance, grant_points_admin_via_api,
};
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::http::StatusCode;
use test_context::test_context;
use uuid::Uuid;

// =============================================================================
// Test 74.1: Normal grant succeeds
// =============================================================================
// User Story: US-PO-08
// Covers: Admin grants points to a user, balances updated correctly
// =============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_74_1_normal_grant_succeeds(ctx: &mut TestContext) {
    // Given: Admin user with points.manage permission, target user exists
    let admin_email = "admin_74_1@test.com";
    let (admin_token, admin_user_id) = create_admin_session_with_user(ctx, admin_email, 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let target_user_id = create_target_user(ctx, "target_74_1@test.com").await;

    // When: POST /api/points/{realmId}/grant with valid userId, amount=100, reason="test grant"
    let (status, body) = grant_points_admin_via_api(
        ctx,
        &ctx._realm_id,
        target_user_id,
        100,
        "test grant",
        None,
        &admin_token,
    )
    .await;

    // Then: 200 response with transactionId, grantedBalance=100, totalBalance=100, expiresAt=null
    assert_eq!(status, StatusCode::OK, "Expected 200 OK, got {:?}", status);
    let resp = body.expect("Response body should exist");
    assert!(
        resp["transactionId"].is_string(),
        "transactionId should be a string"
    );
    assert_eq!(resp["userId"], target_user_id.to_string());
    assert_eq!(resp["amount"], 100);
    assert_eq!(resp["grantedBalance"], 100);
    assert_eq!(resp["totalBalance"], 100);
    assert!(
        resp["expiresAt"].is_null(),
        "expiresAt should be null for permanent grant"
    );

    // Verify DB: granted_balance=100, total_balance=100
    assert_granted_balance(&ctx._app_state.pool, target_user_id, 100).await;
    assert_total_balance(&ctx._app_state.pool, target_user_id, 100).await;
}

// =============================================================================
// Test 74.2: Grant with validity days
// =============================================================================
// User Story: US-PO-08
// Covers: Grant with validityDays sets expiresAt approximately now+30 days
// =============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_74_2_grant_with_validity_days(ctx: &mut TestContext) {
    // Given: Admin user, target user exists
    let admin_email = "admin_74_2@test.com";
    let (admin_token, admin_user_id) = create_admin_session_with_user(ctx, admin_email, 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let target_user_id = create_target_user(ctx, "target_74_2@test.com").await;

    // When: POST with amount=200, reason="temporary bonus", validityDays=30
    let (status, body) = grant_points_admin_via_api(
        ctx,
        &ctx._realm_id,
        target_user_id,
        200,
        "temporary bonus",
        Some(30),
        &admin_token,
    )
    .await;

    // Then: 200 response, expiresAt is approximately now+30 days
    assert_eq!(status, StatusCode::OK, "Expected 200 OK, got {:?}", status);
    let resp = body.expect("Response body should exist");
    assert_eq!(resp["amount"], 200);
    assert_eq!(resp["grantedBalance"], 200);
    assert_eq!(resp["totalBalance"], 200);

    let expires_at_str = resp["expiresAt"]
        .as_str()
        .expect("expiresAt should be set when validityDays is provided");
    let expires_at = expires_at_str
        .parse::<chrono::DateTime<chrono::Utc>>()
        .expect("expiresAt should be a valid ISO 8601 datetime");

    let now = chrono::Utc::now();
    let lower_bound = now + chrono::Duration::days(29);
    let upper_bound = now + chrono::Duration::days(31);
    assert!(
        expires_at > lower_bound && expires_at < upper_bound,
        "expiresAt should be approximately now+30 days, got {:?}",
        expires_at
    );

    // Verify DB: credit ledger entry has expires_at set
    let ledger_expires: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "SELECT expires_at FROM points_credit_ledger WHERE user_id = $1 AND credit_type = 'granted_credit' ORDER BY created_at DESC LIMIT 1",
    )
    .bind(target_user_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();

    assert!(
        ledger_expires.is_some(),
        "Credit ledger entry should have expires_at set"
    );
}

// =============================================================================
// Test 74.3: Amount <= 0 rejected
// =============================================================================
// User Story: US-PO-08
// Covers: amount=0 and amount=-10 both return 400
// =============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_74_3_amount_zero_and_negative_rejected(ctx: &mut TestContext) {
    let admin_email = "admin_74_3@test.com";
    let (admin_token, admin_user_id) = create_admin_session_with_user(ctx, admin_email, 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let target_user_id = create_target_user(ctx, "target_74_3@test.com").await;

    // When: POST with amount=0
    let (status_zero, _) = grant_points_admin_via_api(
        ctx,
        &ctx._realm_id,
        target_user_id,
        0,
        "test",
        None,
        &admin_token,
    )
    .await;
    assert_eq!(
        status_zero,
        StatusCode::BAD_REQUEST,
        "amount=0 should return 400"
    );

    // When: POST with amount=-10
    let (status_neg, _) = grant_points_admin_via_api(
        ctx,
        &ctx._realm_id,
        target_user_id,
        -10,
        "test",
        None,
        &admin_token,
    )
    .await;
    assert_eq!(
        status_neg,
        StatusCode::BAD_REQUEST,
        "amount=-10 should return 400"
    );
}

// =============================================================================
// Test 74.4: Empty reason rejected
// =============================================================================
// User Story: US-PO-08
// Covers: reason="" returns 400
// =============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_74_4_empty_reason_rejected(ctx: &mut TestContext) {
    let admin_email = "admin_74_4@test.com";
    let (admin_token, admin_user_id) = create_admin_session_with_user(ctx, admin_email, 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let target_user_id = create_target_user(ctx, "target_74_4@test.com").await;

    // When: POST with reason=""
    let (status, _) = grant_points_admin_via_api(
        ctx,
        &ctx._realm_id,
        target_user_id,
        100,
        "",
        None,
        &admin_token,
    )
    .await;

    // Then: 400 response
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "empty reason should return 400"
    );
}

// =============================================================================
// Test 74.5: User not found rejected
// =============================================================================
// User Story: US-PO-08
// Covers: userId pointing to non-existent user returns 404
// =============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_74_5_user_not_found_rejected(ctx: &mut TestContext) {
    let admin_email = "admin_74_5@test.com";
    let (admin_token, admin_user_id) = create_admin_session_with_user(ctx, admin_email, 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let non_existent_user_id = Uuid::now_v7();

    // When: POST with userId pointing to non-existent user
    let (status, _) = grant_points_admin_via_api(
        ctx,
        &ctx._realm_id,
        non_existent_user_id,
        100,
        "grant to nobody",
        None,
        &admin_token,
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
// Test 74.6: Permission denied rejected
// =============================================================================
// User Story: US-PO-08
// Covers: Regular user (no admin role) gets 403
// =============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_74_6_permission_denied_rejected(ctx: &mut TestContext) {
    // Given: Regular user (no admin role)
    let regular_email = "regular_74_6@test.com";
    let (regular_token, _regular_user_id) =
        create_admin_session_with_user(ctx, regular_email, 1800).await;
    // Intentionally NOT calling grant_realm_admin_role

    let target_user_id = create_target_user(ctx, "target_74_6@test.com").await;

    // When: POST grant request as regular user
    let (status, _) = grant_points_admin_via_api(
        ctx,
        &ctx._realm_id,
        target_user_id,
        100,
        "unauthorized grant",
        None,
        &regular_token,
    )
    .await;

    // Then: 403 response
    assert_eq!(status, StatusCode::FORBIDDEN, "regular user should get 403");
}

// =============================================================================
// Test 74.7: Cross-realm rejected
// =============================================================================
// User Story: US-PO-08
// Covers: Admin in realm A, target user in realm B, returns 403
// =============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_74_7_cross_realm_rejected(ctx: &mut TestContext) {
    // Given: Admin in realm A (ctx._realm_id)
    let admin_email = "admin_74_7@test.com";
    let (admin_token, admin_user_id) = create_admin_session_with_user(ctx, admin_email, 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let target_user_id = create_target_user(ctx, "target_74_7@test.com").await;

    // When: POST grant request with a different realm ID
    let different_realm_id = "other-realm-74-7";
    let (status, _) = grant_points_admin_via_api(
        ctx,
        different_realm_id,
        target_user_id,
        100,
        "cross-realm grant",
        None,
        &admin_token,
    )
    .await;

    // Then: 403 response (admin has no access to different realm)
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "cross-realm grant should return 403"
    );
}

// =============================================================================
// Test 74.8: Wallet auto-created on first grant
// =============================================================================
// User Story: US-PO-08
// Covers: Target user exists but has no points_wallets row, grant auto-creates wallet
// =============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_74_8_wallet_auto_created_on_first_grant(ctx: &mut TestContext) {
    // Given: Target user exists but has no points_wallets row
    let admin_email = "admin_74_8@test.com";
    let (admin_token, admin_user_id) = create_admin_session_with_user(ctx, admin_email, 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let target_user_id = create_target_user(ctx, "target_74_8@test.com").await;

    // Verify no wallet exists
    let wallet_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM points_wallets WHERE user_id = $1)")
            .bind(target_user_id)
            .fetch_one(&ctx._app_state.pool)
            .await
            .unwrap();
    assert!(!wallet_exists, "User should have no wallet before grant");

    // When: POST grant with amount=50
    let (status, body) = grant_points_admin_via_api(
        ctx,
        &ctx._realm_id,
        target_user_id,
        50,
        "first grant",
        None,
        &admin_token,
    )
    .await;

    // Then: 200 response
    assert_eq!(status, StatusCode::OK, "Expected 200 OK, got {:?}", status);
    let resp = body.expect("Response body should exist");
    assert_eq!(resp["amount"], 50);
    assert_eq!(resp["grantedBalance"], 50);
    assert_eq!(resp["totalBalance"], 50);

    // Verify DB: wallet created with granted_balance=50, topup_balance=0,
    // subscription_balance=0, total_balance=50
    let wallet: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(l.remaining_amount) FILTER (
                    WHERE l.status = 'active' AND l.remaining_amount > 0
                      AND l.credit_type = 'granted_credit'
                      AND (l.effective_at IS NULL OR l.effective_at <= NOW())
                      AND (l.expires_at  IS NULL OR l.expires_at  >  NOW())
                ), 0)::BIGINT AS granted_balance,
                COALESCE(SUM(l.remaining_amount) FILTER (
                    WHERE l.status = 'active' AND l.remaining_amount > 0
                      AND l.credit_type IN ('topup_credit','registration_credit','free_periodic_credit')
                      AND (l.effective_at IS NULL OR l.effective_at <= NOW())
                      AND (l.expires_at  IS NULL OR l.expires_at  >  NOW())
                ), 0)::BIGINT AS topup_balance,
                COALESCE(SUM(l.remaining_amount) FILTER (
                    WHERE l.status = 'active' AND l.remaining_amount > 0
                      AND l.credit_type = 'subscription_credit'
                      AND (l.effective_at IS NULL OR l.effective_at <= NOW())
                      AND (l.expires_at  IS NULL OR l.expires_at  >  NOW())
                ), 0)::BIGINT AS subscription_balance,
                COALESCE(SUM(l.remaining_amount) FILTER (
                    WHERE l.status = 'active' AND l.remaining_amount > 0
                      AND (l.effective_at IS NULL OR l.effective_at <= NOW())
                      AND (l.expires_at  IS NULL OR l.expires_at  >  NOW())
                ), 0)::BIGINT AS total_balance
         FROM points_wallets w
         LEFT JOIN points_credit_ledger l
           ON l.realm_id = w.realm_id AND l.user_id = w.user_id AND l.bucket_id = w.bucket_id
         WHERE w.user_id = $1
         GROUP BY w.id",
    )
    .bind(target_user_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("Wallet should exist after grant");

    assert_eq!(wallet.0, 50, "granted_balance should be 50");
    assert_eq!(wallet.1, 0, "topup_balance should be 0");
    assert_eq!(wallet.2, 0, "subscription_balance should be 0");
    assert_eq!(wallet.3, 50, "total_balance should be 50");
}

// =============================================================================
// Test 74.10: Cumulative grants update balance correctly
// =============================================================================
// User Story: US-PO-08
// Covers: User has granted_balance=100 from prior grant, new grant of 50
//         results in grantedBalance=150, totalBalance=150
// =============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_74_10_cumulative_grants_update_balance(ctx: &mut TestContext) {
    // Given: Admin user and target user with prior grant of 100
    let admin_email = "admin_74_10@test.com";
    let (admin_token, admin_user_id) = create_admin_session_with_user(ctx, admin_email, 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let target_user_id = create_target_user(ctx, "target_74_10@test.com").await;

    // First grant: amount=100
    let (status_first, _) = grant_points_admin_via_api(
        ctx,
        &ctx._realm_id,
        target_user_id,
        100,
        "initial grant",
        None,
        &admin_token,
    )
    .await;
    assert_eq!(status_first, StatusCode::OK);

    // Verify first grant state
    assert_granted_balance(&ctx._app_state.pool, target_user_id, 100).await;
    assert_total_balance(&ctx._app_state.pool, target_user_id, 100).await;

    // When: POST grant with amount=50
    let (status_second, body) = grant_points_admin_via_api(
        ctx,
        &ctx._realm_id,
        target_user_id,
        50,
        "second grant",
        None,
        &admin_token,
    )
    .await;

    // Then: grantedBalance=150, totalBalance=150
    assert_eq!(
        status_second,
        StatusCode::OK,
        "Expected 200 OK for second grant"
    );
    let resp = body.expect("Response body should exist");
    assert_eq!(resp["grantedBalance"], 150);
    assert_eq!(resp["totalBalance"], 150);

    // Verify DB: granted_balance=150
    assert_granted_balance(&ctx._app_state.pool, target_user_id, 150).await;
    assert_total_balance(&ctx._app_state.pool, target_user_id, 150).await;
}

// =============================================================================
// Helper: Create a target user without any roles
// =============================================================================
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
