// =============================================================================
// Points System Scenario Test 2: View Balance
// =============================================================================
//
// **User Story**: docs/user-stories/billing/points-admin.md (US-PO-02, admin
// views a user's wallet)
// **Priority**: P0
//
// **Scenario**: Admin views a user's wallet via the admin-console surface
//
// `GET /api/points/{realmId}/wallets/{userId}` is the admin wallet view: it
// sits behind the `require_admin_console_token` gate, so callers need a
// FirstParty admin-console token (plain HTTP login tokens are
// CredentialClass::CustomUserUi and are rejected by design). The regular-user
// "view my own balance" story (US-PU-01,
// docs/user-stories/billing/points-user.md) is served by the self-service
// `GET /api/user/wallets` endpoint and covered in
// `credit_bucket/bucket_query_scenarios.rs` (US-CB-005,
// docs/user-stories/billing/credit-bucket.md).
//
// **Given**:
// - A user with an existing points wallet (balance 5000)
// - Total recharged: 10000 / Total consumed: 5000
// - An admin (points.view) admin-console session
//
// **When**:
// - The admin calls `GET /api/points/{realmId}/wallets/{userId}`
//
// **Then**:
// - The response returns balance: 5000
// - The response returns total_recharged: 10000
// - The response returns total_consumed: 5000
// - The response returns currency: "points"
// - HTTP status is 200 OK
//
// =============================================================================

use crate::tests::helpers::auth_helpers::{create_admin_session_with_user, grant_realm_admin_role};
use crate::tests::helpers::points_helpers::{
    assert_derived_balance, count_future_effective_active_rows,
    create_credit_ledger_entry_with_effective_at,
};
use crate::tests::scenarios::points::fixtures::*;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::{Duration, Utc};
use test_context::test_context;
use tower::ServiceExt;

/// Mint an admin-console (FirstParty) session for a caller with points.view.
///
/// `GET /api/points/{realmId}/wallets/{userId}` is the admin wallet view: it
/// sits behind the `require_admin_console_token` gate, so a plain HTTP login
/// token (CredentialClass::CustomUserUi since the credential-class split) is
/// rejected with 403 by design. The regular-user "view my own balance" story
/// (US-PU-01, docs/user-stories/billing/points-user.md) is served by the
/// self-service `GET /api/user/wallets` endpoint and covered by
/// `credit_bucket/bucket_query_scenarios.rs` (US-CB-005,
/// docs/user-stories/billing/credit-bucket.md).
async fn admin_points_view_session(ctx: &mut TestContext) -> String {
    let (token, admin_user_id) =
        create_admin_session_with_user(ctx, "points-view-admin@example.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;
    token
}

/// ============================================================================
/// Scenario 1.2: Admin views a user's wallet (flat DTO incl. lifetime totals)
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_admin_view_user_wallet(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // ============================================================================
    // Given: A user with a points account, and an admin (points.view) session
    // ============================================================================
    println!("[Step 1] Create test user and points account");

    let email = "user2@example.com";
    let user_id =
        create_test_user_with_auth(&ctx._app_state.pool, &ctx._realm_id, email, "password123")
            .await;

    let balance = 5000;
    let total_recharged = 10000;
    let total_consumed = 5000;

    let wallet_id = create_test_points_wallet(&ctx._app_state.pool, user_id, balance).await;

    // Update total_recharged and total_consumed
    sqlx::query(
        "UPDATE points_wallets SET total_recharged = $1, total_consumed = $2 WHERE id = $3",
    )
    .bind(total_recharged)
    .bind(total_consumed)
    .bind(wallet_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to update account totals");

    println!(
        "[Step 1] ✓ Test data created: user={}, account={}, balance={}, recharged={}, consumed={}",
        user_id, wallet_id, balance, total_recharged, total_consumed
    );

    println!("[Step 2] Admin opens an admin-console session");
    let token = admin_points_view_session(ctx).await;

    println!("[Step 3] Admin requests the user's wallet");

    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/points/{}/wallets/{}", ctx._realm_id, user_id))
        .header("authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();

    // ============================================================================
    // Then: Verify balance response
    // ============================================================================
    println!("[Step 4] Verify balance response");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Get balance should return 200 OK"
    );

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let body: serde_json::Value =
        serde_json::from_slice(&body_bytes).expect("Failed to parse JSON");

    assert_eq!(
        body["balance"].as_i64(),
        Some(balance),
        "Balance should be 5000"
    );
    assert_eq!(
        body["totalRecharged"].as_i64(),
        Some(total_recharged),
        "Total recharged should be 10000"
    );
    assert_eq!(
        body["totalConsumed"].as_i64(),
        Some(total_consumed),
        "Total consumed should be 5000"
    );
    assert_eq!(
        body["currency"].as_str(),
        Some("points"),
        "Currency should be 'points'"
    );
    assert_eq!(
        body["userId"].as_str(),
        Some(user_id.to_string().as_str()),
        "User ID should match"
    );

    println!(
        "[Step 4] ✓ Balance verified: balance={}, recharged={}, consumed={}",
        balance, total_recharged, total_consumed
    );

    println!("\n✅ Scenario 1.2 完成：管理员成功查看用户积分钱包");
}

/// ============================================================================
/// Scenario 1.3: GET wallet returns a zero-balance user-total view (no row)
/// ============================================================================
///
/// Credit Buckets model: a wallet is per-(user, bucket).
/// A bare GET for a user with no wallet returns a synthesized zero-balance
/// user-total view and does NOT persist a `bucket_id = NULL` row (the column is
/// NOT NULL). Wallet rows are created lazily only when a grant/consume targets
/// a specific bucket.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_get_wallet_auto_creates_empty_wallet(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: A user exists but has no points wallet yet.
    let user_id = create_test_user_with_auth(
        &ctx._app_state.pool,
        &ctx._realm_id,
        "user2-auto-create@example.com",
        "password123",
    )
    .await;

    let wallet_count_before: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM points_wallets WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&ctx._app_state.pool)
            .await
            .expect("Failed to count wallets before request");
    assert_eq!(
        wallet_count_before, 0,
        "Precondition matters: this scenario verifies GET returns a usable view when no wallet exists"
    );

    // When: an admin (points.view) requests the user's wallet.
    let token = admin_points_view_session(ctx).await;

    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/points/{}/wallets/{}", ctx._realm_id, user_id))
        .header("authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();

    // Then: The response is a zero-balance active view (no row persisted).
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Get wallet should return 200 OK with a zero-balance view"
    );

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let body: serde_json::Value =
        serde_json::from_slice(&body_bytes).expect("Failed to parse JSON");

    assert_eq!(body["balance"].as_i64(), Some(0));
    assert_eq!(body["status"].as_str(), Some("active"));
    assert_eq!(body["totalRecharged"].as_i64(), Some(0));
    assert_eq!(body["totalConsumed"].as_i64(), Some(0));
    assert_eq!(body["userId"].as_str(), Some(user_id.to_string().as_str()));

    // No wallet row is persisted by a bare GET — the bucket-wallet is created
    // lazily only when a grant/consume targets a specific bucket.
    let wallet_count_after: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM points_wallets WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&ctx._app_state.pool)
            .await
            .expect("Failed to count wallets after request");
    assert_eq!(
        wallet_count_after, 0,
        "GET must NOT persist a bucket-less wallet row; bucket-wallets are created lazily on grant/consume"
    );
}

/// ============================================================================
/// Scenario 1.4: user PointsWalletResponse does NOT leak
/// future-effective rows
/// ============================================================================
///
/// User Story: docs/user-stories/billing/points-user.md — US-PU-001 /
/// US-PU-004 / US-PU-005 (future-period credits must not be visible to
/// regular users before their effective time).
///
/// Covers the "响应不泄漏未来期积分" invariant and the wallet Stored 列读点
/// 遗漏 risk: `get_balance` 之外的 `list_wallets` 等读路径若继续读
/// `points_wallets.total_balance` 会泄漏未来期积分.
///
/// Why this test exists: the GET `/wallets/{userId}` response assembles
/// `balance`/typed balances from the DERIVED SUM (`compute_available_balance`,
/// derived predicate), whose predicate includes
/// `(effective_at IS NULL OR effective_at <= NOW())`. A future-effective
/// pre-grant row must therefore NOT show up in the regular-user balance
/// response — otherwise the invariant "balance you see == balance you can
/// spend" breaks and a future period silently leaks into the user-visible
/// balance. The test also confirms the same row IS present in the ledger
/// (so the non-leak is a predicate effect, not "row missing").
#[test_context(TestContext)]
#[tokio::test]
async fn test_user_balance_excludes_future_effective(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();

    // Given: A user with two subscription_credit ledger rows on the same
    // (user, realm, bucket): one immediately available (effective_at=NULL,
    // amount A), one future-effective (effective_at=now+1d, amount B).
    let user_id = create_test_user_with_auth(
        &ctx._app_state.pool,
        &realm_id,
        "user-be-t09-noleak@example.com",
        "password123",
    )
    .await;

    let amount_immediate = 2_000;
    let amount_future = 3_000;
    let future_effective_at = Utc::now() + Duration::days(1);

    let _imm_ledger = create_credit_ledger_entry_with_effective_at(
        ctx,
        user_id,
        &realm_id,
        herald_core::domain::points::entities::CreditType::SubscriptionCredit,
        herald_core::domain::points::entities::CreditSourceType::SubscriptionInitial,
        format!("be-t09-noleak-imm-{}", uuid::Uuid::now_v7()),
        amount_immediate,
        None,
        None, // effective_at=NULL ⟺ immediately available
    )
    .await;
    let _fut_ledger = create_credit_ledger_entry_with_effective_at(
        ctx,
        user_id,
        &realm_id,
        herald_core::domain::points::entities::CreditType::SubscriptionCredit,
        herald_core::domain::points::entities::CreditSourceType::SubscriptionInitial,
        format!("be-t09-noleak-fut-{}", uuid::Uuid::now_v7()),
        amount_future,
        None,
        Some(future_effective_at), // future ⟺ excluded from derived SUM
    )
    .await;

    // Cross-check (a): the derived balance predicate excludes B; the row IS
    // present in the ledger (count=1 future-effective active row).
    assert_derived_balance(
        ctx,
        user_id,
        &realm_id,
        herald_core::domain::points::entities::CreditType::SubscriptionCredit,
        amount_immediate,
    )
    .await;
    assert_eq!(
        count_future_effective_active_rows(ctx, user_id, &realm_id).await,
        1,
        "future-effective row is present in the ledger (the non-leak is a predicate effect, not a missing row)"
    );

    // When: an admin (points.view) requests the user's wallet. The derived-SUM
    // predicate is caller-independent, so the non-leak property under test is
    // identical to the (admin-console-gated) user-visible wallet view.
    let token = admin_points_view_session(ctx).await;

    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/points/{}/wallets/{}", realm_id, user_id))
        .header("authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();

    // Then: the response balance == A (immediate only), NOT A+B — the future
    // row does not leak into the user-visible PointsWalletResponse.balance.
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Get wallet should return 200 OK"
    );
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let body: serde_json::Value =
        serde_json::from_slice(&body_bytes).expect("Failed to parse JSON");

    assert_eq!(
        body["balance"].as_i64(),
        Some(amount_immediate),
        "PointsWalletResponse.balance must be the derived SUM excluding future-effective rows; \
         expected {} (immediate only), got {:?}",
        amount_immediate,
        body["balance"].as_i64()
    );

    println!(
        "\n✅ Scenario 1.4 完成：钱包视图 PointsWalletResponse 不含 future-effective（balance={}，未泄漏未来期 {}）",
        amount_immediate, amount_future
    );
}
