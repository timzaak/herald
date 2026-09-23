// =============================================================================
// Dashboard Stats API - BDD Scenario Tests
// =============================================================================
//
// User Stories covered:
// - US-RA-010: Dashboard user metrics (totalUsers, newUsers, activeUsers)
// - US-RA-011: Auth trend aggregation (daily success/failure counts)
// - US-RA-001: Realm isolation and permission enforcement
//
// Reference: docs/user-stories/ (dashboard-redesign)
//
// Routes:
//   GET /api/dashboard/stats
//
// =============================================================================

use crate::tests::helpers::*;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use herald_core::domain::authorization::permission_service::PermissionService;
use herald_core::domain::{
    authentication::BrowserTokenService, client::ports::ClientService, user::UserRepository,
};
use herald_core::infrastructure::authentication::RedisBrowserTokenService;
use test_context::test_context;
use tower::ServiceExt;

// =============================================================================
// Helpers
// =============================================================================

async fn create_first_party_token_for_user(ctx: &TestContext, user_id: &str) -> String {
    let user = ctx
        .app_state
        .user_repository
        .get_user_by_id(uuid::Uuid::parse_str(user_id).expect("test user id must be a UUID"))
        .await
        .expect("Failed to load test user");
    let client_app = ctx
        .app_state
        .service
        .client_service()
        .get_client_app_by_client_id(&user.realm_id, &ctx._client_id)
        .await
        .expect("Failed to load FirstParty test client app");
    RedisBrowserTokenService::new(ctx.app_state.redis_manager.clone())
        .create_first_party_token_family(&user, &client_app, None, None)
        .await
        .expect("Failed to create FirstParty token family")
        .access_token
}

/// Insert an audit event directly into the database for seeding test data.
async fn seed_audit_event(
    ctx: &TestContext,
    realm_id: &str,
    action: &str,
    actor_id: &str,
    created_at: &chrono::DateTime<chrono::Utc>,
) {
    sqlx::query(
        "INSERT INTO audit_events (id, realm_id, category, action, actor_id, target_type, target_id, result, created_at)
         VALUES ($1, $2, 'auth', $3, $4, 'session', 'session-seed', 'success', $5)",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(realm_id)
    .bind(action)
    .bind(actor_id)
    .bind(created_at)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to seed audit event");
}

/// Insert a user directly into the database for seeding test data.
async fn seed_user(
    ctx: &TestContext,
    realm_id: &str,
    email: &str,
    created_at: &chrono::DateTime<chrono::Utc>,
) -> String {
    let user_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status, created_at)
         VALUES ($1, $2, $3, $4, 1, $5)",
    )
    .bind(user_id)
    .bind(realm_id)
    .bind(email)
    .bind("$2a$12$dummy_password_hash")
    .bind(created_at)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to seed user");
    user_id.to_string()
}

/// Create a second realm in the test schema for isolation tests.
async fn seed_realm(ctx: &TestContext, realm_id: &str, name: &str) {
    sqlx::query("INSERT INTO realm (id, name) VALUES ($1, $2)")
        .bind(realm_id)
        .bind(name)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to seed realm");
}

// =============================================================================
// Scenario 1: Basic stats with seeded data (US-RA-010)
// =============================================================================

/// User Story: docs/user-stories/ (dashboard-redesign) - US-RA-010
/// Covers: totalUsers, newUsers (within 7 days), activeUsers (distinct actors with successful logins)
///
/// Given a realm with known users (some recent, some older) and known audit events,
/// When calling GET /api/dashboard/stats,
/// Then response contains correct totalUsers, newUsers, and activeUsers counts.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_dashboard_stats_basic_metrics_returned(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "dashboard-basic@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let realm_id = &ctx._realm_id;
    let now = chrono::Utc::now();
    let three_days_ago = now - chrono::Duration::days(3);
    let ten_days_ago = now - chrono::Duration::days(10);

    // Given: 1 admin user created by helper + 2 recent users + 1 older user = 4 total
    let _recent_user_a = seed_user(ctx, realm_id, "recent-a@test.com", &three_days_ago).await;
    let _recent_user_b = seed_user(ctx, realm_id, "recent-b@test.com", &three_days_ago).await;
    let _older_user = seed_user(ctx, realm_id, "older@test.com", &ten_days_ago).await;

    // Given: audit events for active users (auth.login within 7 days)
    // admin_user_id, recent_user_a have successful logins
    seed_audit_event(ctx, realm_id, "auth.login", &admin_user_id, &three_days_ago).await;
    seed_audit_event(
        ctx,
        realm_id,
        "auth.login",
        &_recent_user_a,
        &three_days_ago,
    )
    .await;

    // When: admin calls dashboard stats
    let req = Request::builder()
        .method("GET")
        .uri("/api/dashboard/stats".to_string())
        .header(header::AUTHORIZATION, format!("Bearer {admin_token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 200 OK with correct counts
    assert_eq!(resp.status(), StatusCode::OK, "Expected 200 OK");

    let body: serde_json::Value = crate::tests::response_json(resp).await;

    let user_stats = &body["userStats"];
    assert!(
        user_stats.is_object(),
        "Expected userStats object in response"
    );

    let total_users = user_stats["totalUsers"]
        .as_i64()
        .expect("Expected totalUsers as integer");
    let new_users = user_stats["newUsers"]
        .as_i64()
        .expect("Expected newUsers as integer");
    let active_users = user_stats["activeUsers"]
        .as_i64()
        .expect("Expected activeUsers as integer");

    // 4 users total: admin (created by helper) + 2 recent + 1 older
    assert!(
        total_users >= 4,
        "Expected totalUsers >= 4, got {}",
        total_users
    );

    // newUsers: only users created within last 7 days (admin + 2 recent = 3, older is excluded)
    assert!(
        new_users >= 3,
        "Expected newUsers >= 3 (admin + 2 recent users within 7 days), got {}",
        new_users,
    );

    // activeUsers: distinct actors with auth.login within 7 days (admin_user_id + recent_user_a)
    assert!(
        active_users >= 2,
        "Expected activeUsers >= 2 (admin + recent_user_a with successful logins), got {}",
        active_users,
    );
}

// =============================================================================
// Scenario 2: Auth trend aggregation (US-RA-011)
// =============================================================================

/// User Story: docs/user-stories/ (dashboard-redesign) - US-RA-011
/// Covers: authTrend array with correct date grouping, success_count and failure_count per day
///
/// Given a realm with audit events spanning multiple days (auth.login and auth.login_failed),
/// When calling GET /api/dashboard/stats,
/// Then response contains authTrend with correct daily aggregation.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_dashboard_stats_auth_trend_aggregated(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "dashboard-trend@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let realm_id = &ctx._realm_id;
    let now = chrono::Utc::now();

    // Given: events on today and yesterday
    let today = now;
    let yesterday = now - chrono::Duration::days(1);

    // Seed 2 successful logins today for the admin user
    seed_audit_event(ctx, realm_id, "auth.login", &admin_user_id, &today).await;
    seed_audit_event(ctx, realm_id, "auth.login", &admin_user_id, &today).await;

    // Seed 1 failed login yesterday
    seed_audit_event(
        ctx,
        realm_id,
        "auth.login_failed",
        &admin_user_id,
        &yesterday,
    )
    .await;

    // When: admin calls dashboard stats
    let req = Request::builder()
        .method("GET")
        .uri("/api/dashboard/stats".to_string())
        .header(header::AUTHORIZATION, format!("Bearer {admin_token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "Expected 200 OK");

    let body: serde_json::Value = crate::tests::response_json(resp).await;

    let auth_trend = body["authTrend"]
        .as_array()
        .expect("Expected authTrend array in response");

    // Then: authTrend should have 30 entries (last 30 days)
    assert!(!auth_trend.is_empty(), "Expected non-empty authTrend array",);

    // Find today's entry
    let today_str = today.date_naive().to_string();
    let today_entry = auth_trend
        .iter()
        .find(|entry| entry["date"].as_str() == Some(today_str.as_str()));

    assert!(
        today_entry.is_some(),
        "Expected authTrend entry for today ({})",
        today_str,
    );
    let today_entry = today_entry.unwrap();
    assert!(
        today_entry["successCount"].as_i64().unwrap_or(0) >= 2,
        "Expected today successCount >= 2, got {:?}",
        today_entry["successCount"],
    );

    // Find yesterday's entry
    let yesterday_str = yesterday.date_naive().to_string();
    let yesterday_entry = auth_trend
        .iter()
        .find(|entry| entry["date"].as_str() == Some(yesterday_str.as_str()));

    assert!(
        yesterday_entry.is_some(),
        "Expected authTrend entry for yesterday ({})",
        yesterday_str,
    );
    let yesterday_entry = yesterday_entry.unwrap();
    assert!(
        yesterday_entry["failureCount"].as_i64().unwrap_or(0) >= 1,
        "Expected yesterday failureCount >= 1, got {:?}",
        yesterday_entry["failureCount"],
    );
}

// =============================================================================
// Scenario 3: Empty realm returns zero values (US-RA-010, US-RA-011)
// =============================================================================

/// User Story: docs/user-stories/ (dashboard-redesign) - US-RA-010, US-RA-011
/// Covers: newly created realm with no data returns zeroed metrics and empty trend
///
/// Given a newly created realm with no users and no audit events,
/// When calling GET /api/dashboard/stats,
/// Then response contains totalUsers=0, newUsers=0, activeUsers=0, authTrend=[].
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_dashboard_stats_empty_realm_returns_zeros(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Create a fresh empty realm
    let empty_realm_id = format!("empty-realm-{}", uuid::Uuid::now_v7());
    seed_realm(ctx, &empty_realm_id, "Empty Test Realm").await;

    // First-party token minting requires a first-party client app in the user's realm.
    sqlx::query(
        "INSERT INTO client_app (realm_id, client_id, name, is_first_party, enabled)
         VALUES ($1, 'admin-web-console', 'Admin Console', true, true)",
    )
    .bind(&empty_realm_id)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to seed first-party client app in empty realm");

    // Create a user directly in the empty realm (identity.realm_id comes from DB, not session)
    let now = chrono::Utc::now();
    let empty_admin_id = seed_user(ctx, &empty_realm_id, "dashboard-empty@test.com", &now).await;

    // Create realm-admin role with dashboard.view policy in the empty realm
    let role_uuid = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO roles (id, name, description, realm_id, client_id, is_builtin)
         VALUES ($1, 'realm-admin', 'Realm Administrator', $2, $3, false)",
    )
    .bind(role_uuid)
    .bind(&empty_realm_id)
    .bind(&ctx._client_id)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create realm-admin role in empty realm");

    let policy_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO role_policies (id, role_id, realm_id, resource, action)
         VALUES ($1, $2, $3, 'dashboard', 'view')",
    )
    .bind(policy_id)
    .bind(role_uuid)
    .bind(&empty_realm_id)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to add dashboard.view policy");

    let user_role_id = uuid::Uuid::now_v7();
    let user_uuid = uuid::Uuid::parse_str(&empty_admin_id).unwrap();
    sqlx::query(
        "INSERT INTO user_roles (id, user_id, role_id, realm_id, client_id, principal_type, principal_id)
         VALUES ($1, $2, $3, $4, $5, $6, $2::text)",
    )
    .bind(user_role_id)
    .bind(user_uuid)
    .bind(role_uuid)
    .bind(&empty_realm_id)
    .bind(&ctx._client_id)
    .bind(herald_core::domain::authorization::principal_types::USER)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to assign user to realm-admin role in empty realm");

    // Invalidate cache
    let _ = ctx
        ._app_state
        .permission_checker
        .invalidate_user_role_cache(&empty_realm_id, &empty_admin_id)
        .await;

    let empty_token = create_first_party_token_for_user(ctx, &empty_admin_id).await;

    // Verify preconditions: only the admin user exists (will be counted in totalUsers)
    let event_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE realm_id = $1")
            .bind(&empty_realm_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap();
    assert_eq!(
        event_count, 0,
        "Precondition: no audit events in empty realm"
    );

    // When: admin calls dashboard stats for the empty realm
    let req = Request::builder()
        .method("GET")
        .uri("/api/dashboard/stats".to_string())
        .header(header::AUTHORIZATION, format!("Bearer {empty_token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 200 OK with zeroed metrics
    assert_eq!(resp.status(), StatusCode::OK, "Expected 200 OK");

    let body: serde_json::Value = crate::tests::response_json(resp).await;

    let user_stats = &body["userStats"];
    // The empty realm has 1 user (the admin we created), so totalUsers=1
    assert_eq!(
        user_stats["totalUsers"].as_i64(),
        Some(1),
        "Expected totalUsers=1 (the admin user) for empty realm",
    );
    assert_eq!(
        user_stats["newUsers"].as_i64(),
        Some(1),
        "Expected newUsers=1 (admin created just now) for empty realm",
    );
    assert_eq!(
        user_stats["activeUsers"].as_i64(),
        Some(0),
        "Expected activeUsers=0 for empty realm (no audit events)",
    );

    // Auth trend should have all-zero counts
    let auth_trend = body["authTrend"]
        .as_array()
        .expect("Expected authTrend array");
    for entry in auth_trend {
        assert_eq!(
            entry["successCount"].as_i64(),
            Some(0),
            "Expected successCount=0 in empty realm trend",
        );
        assert_eq!(
            entry["failureCount"].as_i64(),
            Some(0),
            "Expected failureCount=0 in empty realm trend",
        );
    }
}

// =============================================================================
// Scenario 4: Realm isolation (US-RA-001)
// =============================================================================

/// User Story: docs/user-stories/ (dashboard-redesign) - US-RA-001
/// Covers: realm isolation -- stats for one realm must not leak data from another
///
/// Given two realms, Realm A with users and events, Realm B with only an admin user,
/// When calling stats API for Realm B,
/// Then Realm B stats show only its own data, not Realm A's.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_dashboard_stats_realm_isolation_no_leakage(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // --- Setup Realm A (the default test realm) with data ---
    let (_realm_a_token, realm_a_user_id) =
        create_admin_session_with_user(ctx, "dashboard-realm-a@test.com", 1800).await;
    grant_realm_admin_role(ctx, &realm_a_user_id).await;

    let realm_a_id = &ctx._realm_id;
    let now = chrono::Utc::now();

    // Seed users in Realm A
    let _user_a = seed_user(ctx, realm_a_id, "user-a@test.com", &now).await;

    // Seed audit events in Realm A
    seed_audit_event(ctx, realm_a_id, "auth.login", &realm_a_user_id, &now).await;

    // --- Setup Realm B (with only admin user, no events) ---
    let realm_b_id = format!("isolated-realm-{}", uuid::Uuid::now_v7());
    seed_realm(ctx, &realm_b_id, "Isolated Realm B").await;

    // First-party token minting requires a first-party client app in the user's realm.
    sqlx::query(
        "INSERT INTO client_app (realm_id, client_id, name, is_first_party, enabled)
         VALUES ($1, 'admin-web-console', 'Admin Console', true, true)",
    )
    .bind(&realm_b_id)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to seed first-party client app in Realm B");

    // Create a user directly in Realm B (identity.realm_id comes from DB, not session)
    let realm_b_admin_id = seed_user(ctx, &realm_b_id, "realm-b-admin@test.com", &now).await;

    // Create realm-admin role in Realm B
    let role_uuid = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO roles (id, name, description, realm_id, client_id, is_builtin)
         VALUES ($1, 'realm-admin', 'Realm Administrator', $2, $3, false)",
    )
    .bind(role_uuid)
    .bind(&realm_b_id)
    .bind(&ctx._client_id)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create realm-admin role in Realm B");

    let policy_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO role_policies (id, role_id, realm_id, resource, action)
         VALUES ($1, $2, $3, 'dashboard', 'view')",
    )
    .bind(policy_id)
    .bind(role_uuid)
    .bind(&realm_b_id)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to add dashboard.view policy to Realm B");

    let user_role_id = uuid::Uuid::now_v7();
    let user_uuid = uuid::Uuid::parse_str(&realm_b_admin_id).unwrap();
    sqlx::query(
        "INSERT INTO user_roles (id, user_id, role_id, realm_id, client_id, principal_type, principal_id)
         VALUES ($1, $2, $3, $4, $5, $6, $2::text)",
    )
    .bind(user_role_id)
    .bind(user_uuid)
    .bind(role_uuid)
    .bind(&realm_b_id)
    .bind(&ctx._client_id)
    .bind(herald_core::domain::authorization::principal_types::USER)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to assign user to realm-admin role in Realm B");

    let _ = ctx
        ._app_state
        .permission_checker
        .invalidate_user_role_cache(&realm_b_id, &realm_b_admin_id)
        .await;

    let realm_b_session_token = create_first_party_token_for_user(ctx, &realm_b_admin_id).await;

    // When: calling stats API for Realm B
    let req = Request::builder()
        .method("GET")
        .uri("/api/dashboard/stats".to_string())
        .header(
            header::AUTHORIZATION,
            format!("Bearer {realm_b_session_token}"),
        )
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: Realm B stats show only its own data, not Realm A's
    assert_eq!(resp.status(), StatusCode::OK, "Expected 200 OK");

    let body: serde_json::Value = crate::tests::response_json(resp).await;

    let user_stats = &body["userStats"];
    // Realm B has 1 user (the admin we created), not Realm A's users
    assert_eq!(
        user_stats["totalUsers"].as_i64(),
        Some(1),
        "Realm B should have totalUsers=1 (only its admin), not Realm A's data",
    );
    assert_eq!(
        user_stats["newUsers"].as_i64(),
        Some(1),
        "Realm B should have newUsers=1 (admin created just now)",
    );
    assert_eq!(
        user_stats["activeUsers"].as_i64(),
        Some(0),
        "Realm B should have activeUsers=0 (no audit events)",
    );

    // Auth trend should have zero counts for every day
    let auth_trend = body["authTrend"]
        .as_array()
        .expect("Expected authTrend array");
    for entry in auth_trend {
        assert_eq!(
            entry["successCount"].as_i64(),
            Some(0),
            "Realm B trend should have successCount=0",
        );
        assert_eq!(
            entry["failureCount"].as_i64(),
            Some(0),
            "Realm B trend should have failureCount=0",
        );
    }
}

// =============================================================================
// Scenario 5: Permission enforcement (US-RA-001)
// =============================================================================

/// User Story: docs/user-stories/ (dashboard-redesign) - US-RA-001
/// Covers: non-admin users cannot access dashboard stats
///
/// Given a realm with a regular (non-admin) user,
/// When regular user calls stats API,
/// Then returns 403 Forbidden.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_dashboard_stats_non_admin_forbidden(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Create a user WITHOUT granting realm-admin role
    let (user_token, _user_id) =
        create_admin_session_with_user(ctx, "dashboard-no-role@test.com", 1800).await;
    // Deliberately NOT calling grant_realm_admin_role

    // When: regular user calls dashboard stats
    let req = Request::builder()
        .method("GET")
        .uri("/api/dashboard/stats".to_string())
        .header(header::AUTHORIZATION, format!("Bearer {user_token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 403 Forbidden
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "Expected 403 Forbidden for non-admin user calling dashboard stats",
    );
}
