// =============================================================================
// Billing Statistics Scenario Tests
// =============================================================================
//
// Verifies the two read-only statistics endpoints backing the admin billing
// statistics page:
//
//   GET /api/bill/{realmId}/stats/payments      -> billing.view
//   GET /api/points/{realmId}/stats/consumption -> points.view
//
// =============================================================================

use crate::tests::helpers::async_payment_helpers::create_test_user;
use crate::tests::helpers::auth_helpers::{
    create_admin_session_with_user, grant_realm_admin_role, mint_first_party_session,
};
use crate::tests::helpers::credit_bucket_helpers::{
    CreditBucketOpts, create_test_credit_bucket, seed_transaction_on_bucket,
};
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use herald_core::domain::authorization::permission_service::PermissionService;
use test_context::test_context;
use tower::ServiceExt;

// =============================================================================
// Seeding helpers
// =============================================================================

async fn seed_realm(ctx: &TestContext, realm_id: &str, name: &str) {
    sqlx::query("INSERT INTO realm (id, name) VALUES ($1, $2)")
        .bind(realm_id)
        .bind(name)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to seed realm");
}

/// Create a role carrying exactly the given permissions and assign it to the
/// user. Used to pin that billing.view and points.view never imply each other.
async fn grant_permissions(
    ctx: &TestContext,
    realm_id: &str,
    user_id: uuid::Uuid,
    permissions: &[(&str, &str)],
) {
    let role_uuid = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO roles (id, name, description, realm_id, client_id, is_builtin)
         VALUES ($1, $2, $3, $4, $5, false)",
    )
    .bind(role_uuid)
    .bind(format!("stats-role-{}", uuid::Uuid::now_v7().simple()))
    .bind("Scoped role for statistics scenario")
    .bind(realm_id)
    .bind(&ctx._client_id)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create scoped role");

    for (resource, action) in permissions {
        sqlx::query(
            "INSERT INTO role_policies (id, role_id, realm_id, resource, action)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(uuid::Uuid::now_v7())
        .bind(role_uuid)
        .bind(realm_id)
        .bind(resource)
        .bind(action)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to add policy to scoped role");
    }

    sqlx::query(
        "INSERT INTO user_roles (id, user_id, role_id, realm_id, client_id, principal_type, principal_id)
         VALUES ($1, $2, $3, $4, $5, $6, $2::text)
         ON CONFLICT DO NOTHING",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(user_id)
    .bind(role_uuid)
    .bind(realm_id)
    .bind(&ctx._client_id)
    .bind(herald_core::domain::authorization::principal_types::USER)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to assign scoped role");

    let _ = ctx
        ._app_state
        .permission_checker
        .invalidate_user_role_cache(realm_id, &user_id.to_string())
        .await;
}

/// Insert a finalized/pending payment attempt attributed to `created_at`.
///
/// Statistics attribute attempts to their initiation day, so the seed date is
/// the created_at, not completed_at.
async fn seed_payment_attempt(
    ctx: &TestContext,
    realm_id: &str,
    user_id: uuid::Uuid,
    provider: &str,
    currency: &str,
    amount: i64,
    status: &str,
    created_at: chrono::DateTime<chrono::Utc>,
) {
    sqlx::query(
        "INSERT INTO payment_attempts
            (id, realm_id, user_id, payment_provider, target_type, target_id,
             amount, currency, status, expires_at, completed_at, created_at, updated_at)
         VALUES ($1, $2, $3, $4, 'entitlement_mapping', $5, $6, $7, $8,
                 NOW() + INTERVAL '2 hours', $9, $10, NOW())",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(realm_id)
    .bind(user_id)
    .bind(provider)
    .bind(uuid::Uuid::now_v7())
    .bind(amount)
    .bind(currency)
    .bind(status)
    .bind((status == "Succeeded" || status == "Failed").then_some(created_at))
    .bind(created_at)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to seed payment attempt");
}

/// Create a named credit bucket; consumption grouping reads bucket_key/name.
async fn seed_credit_bucket(
    ctx: &TestContext,
    realm_id: &str,
    key: &str,
    name: &str,
) -> uuid::Uuid {
    create_test_credit_bucket(
        &ctx.app_state.pool,
        realm_id,
        CreditBucketOpts {
            bucket_key: Some(key.into()),
            name: Some(name.into()),
            ..Default::default()
        },
    )
    .await
}

/// Insert a points transaction attributed to `created_at`, ensuring the
/// (realm, user, bucket) wallet row exists for the FK.
async fn seed_points_transaction(
    ctx: &TestContext,
    realm_id: &str,
    user_id: uuid::Uuid,
    bucket_id: uuid::Uuid,
    transaction_type: &str,
    amount: i64,
    created_at: chrono::DateTime<chrono::Utc>,
) {
    seed_transaction_on_bucket(
        &ctx.app_state.pool,
        realm_id,
        user_id,
        bucket_id,
        transaction_type,
        amount,
        created_at,
    )
    .await;
}

async fn get_json(app: axum::Router, uri: String, token: &str) -> (StatusCode, serde_json::Value) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    if status == StatusCode::OK {
        (status, crate::tests::response_json(resp).await)
    } else {
        (status, serde_json::Value::Null)
    }
}

// =============================================================================
// Scenario 1: payment stats aggregate by currency, provider and day
// =============================================================================

/// Covers: multi-currency / multi-provider aggregation, per-currency amounts
/// without any cross-currency total, provider nesting, zero-filled trend, and
/// the pending-attempt exclusion.
///
/// Given a realm with finalized attempts across two days, two providers and
/// two currencies plus one pending attempt,
/// When calling GET /api/bill/{realmId}/stats/payments,
/// Then counts, per-currency amounts, provider groups and the daily trend
/// reflect only the finalized attempts and the response carries no
/// cross-currency amount field.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_payment_stats_aggregates_by_currency_provider_and_day(
    ctx: &mut TestContext,
) {
    let app = ctx.create_unified_test_router();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "stats-pay-agg@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let realm_id = ctx._realm_id.clone();
    let payer = create_test_user(ctx, &realm_id, "stats-pay-payer@test.com").await;
    let now = chrono::Utc::now();
    let two_days_ago = now - chrono::Duration::days(2);

    // Two days ago: 3 succeeded (USD stripe x2, CNY wechat x1) + 2 failed.
    seed_payment_attempt(
        ctx,
        &realm_id,
        payer,
        "stripe",
        "USD",
        1000,
        "Succeeded",
        two_days_ago,
    )
    .await;
    seed_payment_attempt(
        ctx,
        &realm_id,
        payer,
        "stripe",
        "USD",
        2500,
        "Succeeded",
        two_days_ago,
    )
    .await;
    seed_payment_attempt(
        ctx,
        &realm_id,
        payer,
        "wechat",
        "CNY",
        800,
        "Succeeded",
        two_days_ago,
    )
    .await;
    seed_payment_attempt(
        ctx,
        &realm_id,
        payer,
        "stripe",
        "USD",
        700,
        "Failed",
        two_days_ago,
    )
    .await;
    seed_payment_attempt(
        ctx,
        &realm_id,
        payer,
        "wechat",
        "CNY",
        300,
        "Failed",
        two_days_ago,
    )
    .await;
    // Today: 1 succeeded + 1 pending that must not be counted anywhere.
    seed_payment_attempt(
        ctx,
        &realm_id,
        payer,
        "stripe",
        "USD",
        500,
        "Succeeded",
        now,
    )
    .await;
    seed_payment_attempt(ctx, &realm_id, payer, "stripe", "USD", 999, "Pending", now).await;

    let (status, body) = get_json(
        app,
        format!("/api/bill/{realm_id}/stats/payments"),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "Expected 200 OK, body: {body}");

    assert_eq!(body["windowDays"].as_i64(), Some(7));
    assert_eq!(
        body["succeededCount"].as_i64(),
        Some(4),
        "3 succeeded two days ago + 1 today"
    );
    assert_eq!(body["failedCount"].as_i64(), Some(2));

    // Per-currency amounts with no cross-currency total anywhere: USD 4000
    // (1000+2500+500) and CNY 800 are separate entries (the SQL orders
    // currencies alphabetically), and the response has no other amount-like
    // top-level field.
    let amounts = body["amountsByCurrency"]
        .as_array()
        .expect("amountsByCurrency array");
    assert_eq!(amounts.len(), 2);
    assert_eq!(amounts[0]["currency"], "CNY");
    assert_eq!(amounts[0]["amount"].as_i64(), Some(800));
    assert_eq!(amounts[1]["currency"], "USD");
    assert_eq!(amounts[1]["amount"].as_i64(), Some(4000));
    let mut keys: Vec<&str> = body
        .as_object()
        .expect("response object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "amountsByCurrency",
            "failedCount",
            "paymentTrend",
            "providers",
            "succeededCount",
            "windowDays",
        ],
        "the payment stats contract must not grow a cross-currency total field"
    );

    let providers = body["providers"].as_array().expect("providers array");
    assert_eq!(providers.len(), 2);
    let stripe = providers
        .iter()
        .find(|p| p["paymentProvider"] == "stripe")
        .expect("stripe provider row");
    assert_eq!(stripe["succeededCount"].as_i64(), Some(3));
    assert_eq!(stripe["failedCount"].as_i64(), Some(1));
    let stripe_amounts = stripe["amountsByCurrency"]
        .as_array()
        .expect("stripe amounts");
    assert_eq!(stripe_amounts.len(), 1);
    assert_eq!(stripe_amounts[0]["currency"], "USD");
    assert_eq!(stripe_amounts[0]["amount"].as_i64(), Some(4000));
    let wechat = providers
        .iter()
        .find(|p| p["paymentProvider"] == "wechat")
        .expect("wechat provider row");
    assert_eq!(wechat["succeededCount"].as_i64(), Some(1));
    assert_eq!(wechat["failedCount"].as_i64(), Some(1));
    assert_eq!(wechat["amountsByCurrency"][0]["amount"].as_i64(), Some(800));

    // Trend: full 7-day window, seeded day aggregated, gap days zero-filled.
    let trend = body["paymentTrend"].as_array().expect("paymentTrend array");
    assert_eq!(
        trend.len(),
        7,
        "7-day window must yield 7 ascending daily points"
    );
    let two_days_ago_str = two_days_ago.date_naive().to_string();
    let today_str = now.date_naive().to_string();
    let window_start_str = (now - chrono::Duration::days(6)).date_naive().to_string();
    assert_eq!(trend[0]["date"].as_str().unwrap(), window_start_str);
    assert_eq!(trend[6]["date"].as_str().unwrap(), today_str);
    let entry_two_days_ago = trend
        .iter()
        .find(|p| p["date"].as_str() == Some(two_days_ago_str.as_str()))
        .expect("trend entry for seeded day");
    assert_eq!(entry_two_days_ago["succeededCount"].as_i64(), Some(3));
    assert_eq!(entry_two_days_ago["failedCount"].as_i64(), Some(2));
    let entry_today = trend
        .iter()
        .find(|p| p["date"].as_str() == Some(today_str.as_str()))
        .expect("trend entry for today");
    assert_eq!(
        entry_today["succeededCount"].as_i64(),
        Some(1),
        "pending attempt must not be counted"
    );
    assert_eq!(entry_today["failedCount"].as_i64(), Some(0));
    let yesterday_str = (now - chrono::Duration::days(1)).date_naive().to_string();
    let entry_yesterday = trend
        .iter()
        .find(|p| p["date"].as_str() == Some(yesterday_str.as_str()))
        .expect("trend entry for the gap day");
    assert_eq!(entry_yesterday["succeededCount"].as_i64(), Some(0));
    assert_eq!(entry_yesterday["failedCount"].as_i64(), Some(0));
}

// =============================================================================
// Scenario 2: invalid window values are rejected on both endpoints
// =============================================================================

/// Covers: days validation — only 7 and 30 are legal, anything else 400s and
/// the omitted parameter defaults to a 7-day window, on both endpoints.
///
/// Given an admin holding both permissions,
/// When calling either stats endpoint with days=45 or days=0,
/// Then the response is 400; without days the response is 200 with
/// windowDays=7.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_stats_rejects_invalid_window(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "stats-window@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;
    let realm_id = ctx._realm_id.clone();

    for endpoint in [
        format!("/api/bill/{realm_id}/stats/payments"),
        format!("/api/points/{realm_id}/stats/consumption"),
    ] {
        for days in ["45", "0", "-7", "8", "abc"] {
            let (status, _) =
                get_json(app.clone(), format!("{endpoint}?days={days}"), &admin_token).await;
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "days={days} must be rejected with 400 on {endpoint}"
            );
        }

        let (status, body) = get_json(app.clone(), endpoint.clone(), &admin_token).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "omitted days must default, body: {body}"
        );
        assert_eq!(
            body["windowDays"].as_i64(),
            Some(7),
            "omitted days must yield the 7-day default window"
        );

        let (status, body) =
            get_json(app.clone(), format!("{endpoint}?days=30"), &admin_token).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["windowDays"].as_i64(), Some(30));
    }
}

// =============================================================================
// Scenario 3: window boundary day-20 counts only in the 30-day window
// =============================================================================

/// Covers: the two-window semantics — a record 20 days back is inside the
/// 30-day window and outside the 7-day window, for both endpoints.
///
/// Given a succeeded payment and a consume transaction 20 days ago,
/// When calling either stats endpoint with days=30 vs days=7,
/// Then the day-20 records count only in the 30-day window.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_stats_window_boundary(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "stats-boundary@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let realm_id = ctx._realm_id.clone();
    let actor = create_test_user(ctx, &realm_id, "stats-boundary-actor@test.com").await;
    let bucket = seed_credit_bucket(ctx, &realm_id, "boundary-bucket", "Boundary Bucket").await;
    let twenty_days_ago = chrono::Utc::now() - chrono::Duration::days(20);

    seed_payment_attempt(
        ctx,
        &realm_id,
        actor,
        "stripe",
        "USD",
        1500,
        "Succeeded",
        twenty_days_ago,
    )
    .await;
    seed_points_transaction(
        ctx,
        &realm_id,
        actor,
        bucket,
        "consume",
        -400,
        twenty_days_ago,
    )
    .await;

    let (status, body) = get_json(
        app.clone(),
        format!("/api/bill/{realm_id}/stats/payments?days=30"),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["succeededCount"].as_i64(),
        Some(1),
        "day-20 payment counts in the 30-day window"
    );

    let (status, body) = get_json(
        app.clone(),
        format!("/api/bill/{realm_id}/stats/payments?days=7"),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["succeededCount"].as_i64(),
        Some(0),
        "day-20 payment is outside the 7-day window"
    );
    assert_eq!(body["paymentTrend"].as_array().expect("trend").len(), 7);

    let (status, body) = get_json(
        app.clone(),
        format!("/api/points/{realm_id}/stats/consumption?days=30"),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["totalConsumedPoints"].as_i64(),
        Some(400),
        "day-20 consumption counts in the 30-day window"
    );

    let (status, body) = get_json(
        app,
        format!("/api/points/{realm_id}/stats/consumption?days=7"),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["totalConsumedPoints"].as_i64(), Some(0));
    assert_eq!(body["consumptionTrend"].as_array().expect("trend").len(), 7);
}

// =============================================================================
// Scenario 4: consumption counts consume rows only
// =============================================================================

/// Covers: consume-only caliber — recharge and refund_revoke rows are excluded
/// and a refund revocation never offsets consumption — plus per-bucket
/// grouping and distinct consuming-user counting.
///
/// Given consume, recharge and refund_revoke transactions in the same window
/// across two users and two buckets,
/// When calling GET /api/points/{realmId}/stats/consumption,
/// Then totals, trend and buckets reflect only the consume rows and
/// consumingUsers counts distinct users.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_points_stats_count_consume_only(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "stats-consume@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let realm_id = ctx._realm_id.clone();
    let user_a = create_test_user(ctx, &realm_id, "stats-consume-a@test.com").await;
    let user_b = create_test_user(ctx, &realm_id, "stats-consume-b@test.com").await;
    let bucket_a = seed_credit_bucket(ctx, &realm_id, "consume-main", "Main Bucket").await;
    let bucket_b = seed_credit_bucket(ctx, &realm_id, "consume-second", "Second Bucket").await;
    let now = chrono::Utc::now();

    seed_points_transaction(ctx, &realm_id, user_a, bucket_a, "consume", -100, now).await;
    seed_points_transaction(ctx, &realm_id, user_a, bucket_a, "consume", -50, now).await;
    seed_points_transaction(ctx, &realm_id, user_b, bucket_b, "consume", -70, now).await;
    // Non-consume rows in the same window: a recharge and a 30-point refund
    // revocation. Neither may enter totals, and the revocation must not
    // subtract from the consumed figures.
    seed_points_transaction(ctx, &realm_id, user_a, bucket_a, "recharge", 200, now).await;
    seed_points_transaction(ctx, &realm_id, user_a, bucket_a, "refund_revoke", -30, now).await;

    let (status, body) = get_json(
        app,
        format!("/api/points/{realm_id}/stats/consumption"),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");

    assert_eq!(
        body["totalConsumedPoints"].as_i64(),
        Some(220),
        "100+50+70 consumed; the 30-point refund_revoke must not offset it"
    );
    assert_eq!(
        body["consumingUsers"].as_i64(),
        Some(2),
        "distinct users with consume rows"
    );

    let buckets = body["buckets"].as_array().expect("buckets array");
    assert_eq!(buckets.len(), 2, "one row per bucket with consumption");
    assert_eq!(buckets[0]["bucketKey"], "consume-main");
    assert_eq!(buckets[0]["bucketName"], "Main Bucket");
    assert_eq!(buckets[0]["consumedPoints"].as_i64(), Some(150));
    assert_eq!(buckets[1]["bucketKey"], "consume-second");
    assert_eq!(buckets[1]["consumedPoints"].as_i64(), Some(70));

    let trend = body["consumptionTrend"].as_array().expect("trend array");
    assert_eq!(trend.len(), 7);
    let today_str = now.date_naive().to_string();
    let today_entry = trend
        .iter()
        .find(|p| p["date"].as_str() == Some(today_str.as_str()))
        .expect("trend entry for today");
    assert_eq!(today_entry["consumedPoints"].as_i64(), Some(220));
}

// =============================================================================
// Scenario 5: billing.view and points.view never imply each other
// =============================================================================

/// Covers: the two statistics endpoints are gated by independent permissions —
/// a billing.view-only admin cannot read consumption stats and a points.view
/// -only admin cannot read payment stats.
///
/// Given one admin holding only billing.view and another holding only
/// points.view,
/// When each calls both statistics endpoints,
/// Then each gets 200 only on the endpoint their permission covers and 403 on
/// the other.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_stats_permissions_are_separate(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();

    let (billing_token, billing_user) =
        create_admin_session_with_user(ctx, "stats-only-billing@test.com", 1800).await;
    grant_permissions(
        ctx,
        &realm_id,
        uuid::Uuid::parse_str(&billing_user).unwrap(),
        &[("billing", "view")],
    )
    .await;

    let (points_token, points_user) =
        create_admin_session_with_user(ctx, "stats-only-points@test.com", 1800).await;
    grant_permissions(
        ctx,
        &realm_id,
        uuid::Uuid::parse_str(&points_user).unwrap(),
        &[("points", "view")],
    )
    .await;

    let (status, _) = get_json(
        app.clone(),
        format!("/api/bill/{realm_id}/stats/payments"),
        &billing_token,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "billing.view covers the payments endpoint"
    );

    let (status, _) = get_json(
        app.clone(),
        format!("/api/points/{realm_id}/stats/consumption"),
        &billing_token,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "billing.view must not imply points.view"
    );

    let (status, _) = get_json(
        app.clone(),
        format!("/api/points/{realm_id}/stats/consumption"),
        &points_token,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "points.view covers the consumption endpoint"
    );

    let (status, _) = get_json(
        app,
        format!("/api/bill/{realm_id}/stats/payments"),
        &points_token,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "points.view must not imply billing.view"
    );
}

// =============================================================================
// Scenario 6: statistics never leak across realms
// =============================================================================

/// Covers: strict realm isolation — another realm's payments and consumption
/// never appear in this realm's statistics, and a foreign-realm admin cannot
/// query another realm's stats at all.
///
/// Given realm-2 with a succeeded payment and a consume transaction and
/// realm-1 with none,
/// When realm-1's admin queries both endpoints,
/// Then all figures are zero/empty; and when realm-1's admin targets realm-2's
/// path, the response is 403.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_stats_realm_isolation(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "stats-isolation@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let realm_1 = ctx._realm_id.clone();
    let realm_2 = format!("stats-realm-{}", uuid::Uuid::now_v7().simple());
    seed_realm(ctx, &realm_2, "Statistics Isolation Realm").await;
    let foreign_user = create_test_user(ctx, &realm_2, "stats-foreign@test.com").await;
    let foreign_bucket =
        seed_credit_bucket(ctx, &realm_2, "foreign-bucket", "Foreign Bucket").await;
    let now = chrono::Utc::now();

    seed_payment_attempt(
        ctx,
        &realm_2,
        foreign_user,
        "stripe",
        "USD",
        5000,
        "Succeeded",
        now,
    )
    .await;
    seed_points_transaction(
        ctx,
        &realm_2,
        foreign_user,
        foreign_bucket,
        "consume",
        -900,
        now,
    )
    .await;

    let (status, body) = get_json(
        app.clone(),
        format!("/api/bill/{realm_1}/stats/payments"),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["succeededCount"].as_i64(),
        Some(0),
        "realm-2 payments must not leak into realm-1 stats"
    );

    let (status, body) = get_json(
        app.clone(),
        format!("/api/points/{realm_1}/stats/consumption"),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["totalConsumedPoints"].as_i64(),
        Some(0),
        "realm-2 consumption must not leak into realm-1 stats"
    );

    // Realm boundary: a realm-1 identity querying realm-2's path is rejected
    // before any data access.
    let (status, _) = get_json(
        app.clone(),
        format!("/api/bill/{realm_2}/stats/payments"),
        &admin_token,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "cross-realm stats query must be 403"
    );

    let (status, _) = get_json(
        app,
        format!("/api/points/{realm_2}/stats/consumption"),
        &admin_token,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "cross-realm stats query must be 403"
    );
}

// =============================================================================
// Scenario 7: empty realm returns zeroed stats, not an error
// =============================================================================

/// Covers: the empty-realm contract — a realm without payments or consumption
/// gets 200 with zero counts, empty grouping arrays and a fully zero-filled
/// trend covering the whole window.
///
/// Given a freshly created realm whose admin holds both permissions,
/// When calling both statistics endpoints,
/// Then each responds 200 with zeroed counts, empty groups and a complete
/// zero trend of window-length days.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_stats_empty_realm_returns_zeros(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    let empty_realm = format!("stats-empty-{}", uuid::Uuid::now_v7().simple());
    seed_realm(ctx, &empty_realm, "Statistics Empty Realm").await;

    // First-party token minting requires a first-party client app in the realm.
    sqlx::query(
        "INSERT INTO client_app (realm_id, client_id, name, is_first_party, enabled)
         VALUES ($1, 'admin-web-console', 'Admin Console', true, true)",
    )
    .bind(&empty_realm)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to seed client app in empty realm");

    let empty_admin = create_test_user(ctx, &empty_realm, "stats-empty@test.com").await;
    grant_permissions(
        ctx,
        &empty_realm,
        empty_admin,
        &[("billing", "view"), ("points", "view")],
    )
    .await;
    let token = mint_first_party_session(ctx, empty_admin).await;

    let (status, body) = get_json(
        app.clone(),
        format!("/api/bill/{empty_realm}/stats/payments"),
        &token,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "empty realm must answer 200, body: {body}"
    );
    assert_eq!(body["windowDays"].as_i64(), Some(7));
    assert_eq!(body["succeededCount"].as_i64(), Some(0));
    assert_eq!(body["failedCount"].as_i64(), Some(0));
    assert_eq!(
        body["amountsByCurrency"]
            .as_array()
            .expect("amounts array")
            .len(),
        0
    );
    assert_eq!(
        body["providers"].as_array().expect("providers array").len(),
        0
    );
    let trend = body["paymentTrend"].as_array().expect("trend array");
    assert_eq!(
        trend.len(),
        7,
        "empty realm still gets a complete 7-day axis"
    );
    assert!(
        trend
            .iter()
            .all(|p| p["succeededCount"] == 0 && p["failedCount"] == 0)
    );

    let (status, body) = get_json(
        app,
        format!("/api/points/{empty_realm}/stats/consumption"),
        &token,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "empty realm must answer 200, body: {body}"
    );
    assert_eq!(body["totalConsumedPoints"].as_i64(), Some(0));
    assert_eq!(body["consumingUsers"].as_i64(), Some(0));
    assert_eq!(body["buckets"].as_array().expect("buckets array").len(), 0);
    let trend = body["consumptionTrend"].as_array().expect("trend array");
    assert_eq!(trend.len(), 7);
    assert!(trend.iter().all(|p| p["consumedPoints"] == 0));
}
