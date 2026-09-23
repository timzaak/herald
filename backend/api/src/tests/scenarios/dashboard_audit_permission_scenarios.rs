// =============================================================================
// Dashboard & Audit Permission Scenario Tests
// =============================================================================
//
// Verifies that the refactored permission model enforces dashboard.view and
// audit.view as distinct, required permissions for dashboard and audit
// endpoints respectively.
//
// User Stories covered:
// - US-RA-010: Dashboard user metrics access requires dashboard.view
// - US-AU-001: Audit log list access requires audit.view
// - US-AU-003: Audit event detail access requires audit.view
//
// Reference: docs/user-stories/core/realm-admin.md (US-RA-010)
// Reference: docs/user-stories/core/audit.md (US-AU-001, US-AU-003)
//
// Routes:
//   GET /api/dashboard/stats
//   GET /api/audit?page=0&pageSize=20
//   GET /api/audit/{eventId}
//
// =============================================================================

use crate::tests::helpers::auth_helpers::{create_admin_session_with_user, grant_realm_admin_role};
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use herald_core::domain::authorization::permission_service::PermissionService;
use test_context::test_context;
use tower::ServiceExt;

// =============================================================================
// Helper: grant a single permission to a user via a dedicated role
// =============================================================================

/// Creates a role with a single permission and assigns it to the user.
///
/// This avoids granting the full realm-admin role when we need to test
/// access with exactly one specific permission.
async fn grant_single_permission(ctx: &TestContext, user_id: &str, resource: &str, action: &str) {
    // Create a dedicated role with the single permission
    let role_uuid = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO roles (id, name, description, realm_id, client_id, is_builtin)
         VALUES ($1, $2, $3, $4, $5, false)",
    )
    .bind(role_uuid)
    .bind(format!("test-role-{}-{}", resource, action))
    .bind(format!("Test role for {}.{} only", resource, action))
    .bind(&ctx._realm_id)
    .bind(&ctx._client_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to create single-permission role");

    // Add the single permission to the role
    let policy_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO role_policies (id, role_id, realm_id, resource, action)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(policy_id)
    .bind(role_uuid)
    .bind(&ctx._realm_id)
    .bind(resource)
    .bind(action)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to add single permission to role");

    // Assign the role to the user
    let user_role_id = uuid::Uuid::now_v7();
    let user_uuid = uuid::Uuid::parse_str(user_id).expect("Failed to parse user_id as UUID");
    sqlx::query(
        "INSERT INTO user_roles (id, user_id, role_id, realm_id, client_id, principal_type, principal_id)
         VALUES ($1, $2, $3, $4, $5, $6, $2::text)
         ON CONFLICT DO NOTHING",
    )
    .bind(user_role_id)
    .bind(user_uuid)
    .bind(role_uuid)
    .bind(&ctx._realm_id)
    .bind(&ctx._client_id)
    .bind(herald_core::domain::authorization::principal_types::USER)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to assign single-permission role to user");

    // Invalidate permission cache
    let _ = ctx
        ._app_state
        .permission_checker
        .invalidate_user_role_cache(&ctx._realm_id, user_id)
        .await;
}

// =============================================================================
// Scenario 1: dashboard.view grants access to dashboard stats (US-RA-010)
// =============================================================================

/// User Story: docs/user-stories/core/realm-admin.md - Story 10 (US-RA-010)
/// Covers: User with dashboard.view permission can access dashboard stats endpoint
///
/// Given a user with ONLY dashboard.view permission,
/// When calling GET /api/dashboard/stats,
/// Then response is 200 OK.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_dashboard_view_grants_access(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: user with dashboard.view permission
    let (user_token, user_id) =
        create_admin_session_with_user(ctx, "dashboard-view-perm@test.com", 1800).await;
    grant_single_permission(ctx, &user_id, "dashboard", "view").await;

    // When: calling dashboard stats endpoint
    let req = Request::builder()
        .method("GET")
        .uri("/api/dashboard/stats".to_string())
        .header(header::AUTHORIZATION, format!("Bearer {}", user_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 200 OK
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "User with dashboard.view should get 200 OK"
    );
}

// =============================================================================
// Scenario 2: no dashboard.view results in 403 (US-RA-010)
// =============================================================================

/// User Story: docs/user-stories/core/realm-admin.md - Story 10 (US-RA-010)
/// Covers: User without dashboard.view permission cannot access dashboard stats
///
/// Given a user with ONLY users.view permission (no dashboard.view),
/// When calling GET /api/dashboard/stats,
/// Then response is 403 Forbidden.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_dashboard_access_denied_without_view(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: user with only users.view (no dashboard.view)
    let (user_token, user_id) =
        create_admin_session_with_user(ctx, "dashboard-no-view-perm@test.com", 1800).await;
    grant_single_permission(ctx, &user_id, "users", "view").await;

    // When: calling dashboard stats endpoint
    let req = Request::builder()
        .method("GET")
        .uri("/api/dashboard/stats".to_string())
        .header(header::AUTHORIZATION, format!("Bearer {}", user_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 403 Forbidden
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "User without dashboard.view should get 403 Forbidden"
    );
}

// =============================================================================
// Scenario 3: audit.view grants access to audit list (US-AU-001)
// =============================================================================

/// User Story: docs/user-stories/core/audit.md - Story 1 (US-AU-001)
/// Covers: User with audit.view permission can list audit events
///
/// Given a user with ONLY audit.view permission,
/// When calling GET /api/audit?page=0&pageSize=20,
/// Then response is 200 OK.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_audit_view_grants_list_access(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: user with audit.view permission
    let (user_token, user_id) =
        create_admin_session_with_user(ctx, "audit-view-list-perm@test.com", 1800).await;
    grant_single_permission(ctx, &user_id, "audit", "view").await;

    // When: calling audit list endpoint
    let req = Request::builder()
        .method("GET")
        .uri("/api/audit?page=0&pageSize=20".to_string())
        .header(header::AUTHORIZATION, format!("Bearer {}", user_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 200 OK
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "User with audit.view should get 200 OK for audit list"
    );
}

// =============================================================================
// Scenario 4: audit.view grants access to audit detail (US-AU-003)
// =============================================================================

/// User Story: docs/user-stories/core/audit.md - Story 3 (US-AU-003)
/// Covers: User with audit.view permission can view audit event detail
///
/// Given a user with audit.view permission and an existing audit event,
/// When calling GET /api/audit/{eventId},
/// Then response is 200 OK.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_audit_view_grants_detail_access(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: first create a full admin to seed an audit event
    let (_admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "audit-detail-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    // Insert an audit event directly into the database
    let event_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO audit_events (id, realm_id, category, action, actor_id, target_type, target_id, result, created_at)
         VALUES ($1, $2, 'auth', 'auth.login', $3, 'session', 'session-test', 'success', NOW())",
    )
    .bind(event_id)
    .bind(&ctx._realm_id)
    .bind(&admin_user_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to seed audit event");

    // Given: a second user with ONLY audit.view permission
    let (user_token, user_id) =
        create_admin_session_with_user(ctx, "audit-view-detail-perm@test.com", 1800).await;
    grant_single_permission(ctx, &user_id, "audit", "view").await;

    // When: calling audit detail endpoint
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/audit/{}", event_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", user_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 200 OK
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "User with audit.view should get 200 OK for audit event detail"
    );
}

// =============================================================================
// Scenario 5: users.view is insufficient for audit access (US-AU-001)
// =============================================================================

/// User Story: docs/user-stories/core/audit.md - Story 1 (US-AU-001)
/// Covers: User with ONLY users.view permission cannot access audit list
///
/// Given a user with ONLY users.view permission (no audit.view),
/// When calling GET /api/audit,
/// Then response is 403 Forbidden.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_audit_users_view_insufficient(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: user with only users.view (no audit.view)
    let (user_token, user_id) =
        create_admin_session_with_user(ctx, "audit-users-view-only@test.com", 1800).await;
    grant_single_permission(ctx, &user_id, "users", "view").await;

    // When: calling audit list endpoint
    let req = Request::builder()
        .method("GET")
        .uri("/api/audit?page=0&pageSize=20".to_string())
        .header(header::AUTHORIZATION, format!("Bearer {}", user_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 403 Forbidden
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "User with only users.view should get 403 Forbidden for audit list"
    );
}
