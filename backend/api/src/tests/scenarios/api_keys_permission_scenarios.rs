// =============================================================================
// API Keys View/Manage Permission Split Scenario Tests
// =============================================================================
//
// Verifies that api_keys.view and api_keys.manage are enforced as distinct
// permissions for API key management endpoints.
//
// User Stories covered:
// - US-RA-009: Permission hierarchy — manage covers view, view does not cover manage
//
// Reference: docs/user-stories/core/realm-admin.md (US-RA-009)
//
// Routes:
//   GET    /api/api-keys              → requires api_keys.view
//   GET    /api/api-keys/{apiKeyId}   → requires api_keys.view
//   POST   /api/api-keys              → requires api_keys.manage
//   DELETE /api/api-keys/{apiKeyId}   → requires api_keys.manage
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

    let _ = ctx
        ._app_state
        .permission_checker
        .invalidate_user_role_cache(&ctx._realm_id, user_id)
        .await;
}

// =============================================================================
// Helper: seed an API key via direct DB insert
// =============================================================================

/// Inserts an API key row directly into the database and returns its ID.
async fn seed_api_key(ctx: &TestContext) -> String {
    let key_id = uuid::Uuid::now_v7().to_string();
    let fake_hash = format!("sha256:{}", uuid::Uuid::now_v7());

    sqlx::query(
        "INSERT INTO client_api_keys (id, name, api_key_hash, realm_id, enabled, created_at)
         VALUES ($1, $2, $3, $4, true, NOW())",
    )
    .bind(&key_id)
    .bind("seeded-test-key")
    .bind(&fake_hash)
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to seed API key");

    key_id
}

// =============================================================================
// Scenario 1: api_keys.view grants list access (US-RA-009)
// =============================================================================

// User Story: docs/user-stories/core/realm-admin.md - Story 9 (US-RA-009)
// Covers: User with api_keys.view can list API keys
//
// Given a user with ONLY api_keys.view permission,
// When calling GET /api/api-keys,
// Then response is 200 OK.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_api_keys_view_grants_list(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: user with api_keys.view permission
    let (user_token, user_id) =
        create_admin_session_with_user(ctx, "apikeys-view-list@test.com", 1800).await;
    grant_single_permission(ctx, &user_id, "api_keys", "view").await;

    // When: calling API keys list endpoint
    let req = Request::builder()
        .method("GET")
        .uri("/api/api-keys".to_string())
        .header(header::AUTHORIZATION, format!("Bearer {}", user_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 200 OK
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "User with api_keys.view should get 200 OK for list"
    );
}

// =============================================================================
// Scenario 2: api_keys.view grants get detail (US-RA-009)
// =============================================================================

// User Story: docs/user-stories/core/realm-admin.md - Story 9 (US-RA-009)
// Covers: User with api_keys.view can get API key detail
//
// Given a user with ONLY api_keys.view permission and an existing API key,
// When calling GET /api/api-keys/{apiKeyId},
// Then response is 200 OK.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_api_keys_view_grants_get_detail(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: seed an API key (need realm admin to do this via DB)
    let (_admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "apikeys-detail-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;
    let key_id = seed_api_key(ctx).await;

    // Given: a second user with ONLY api_keys.view permission
    let (user_token, user_id) =
        create_admin_session_with_user(ctx, "apikeys-view-detail@test.com", 1800).await;
    grant_single_permission(ctx, &user_id, "api_keys", "view").await;

    // When: calling API key detail endpoint
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/api-keys/{}", key_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", user_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 200 OK
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "User with api_keys.view should get 200 OK for get detail"
    );
}

// =============================================================================
// Scenario 3: api_keys.view cannot create (US-RA-009)
// =============================================================================

// User Story: docs/user-stories/core/realm-admin.md - Story 9 (US-RA-009)
// Covers: User with api_keys.view only cannot create API keys
//
// Given a user with ONLY api_keys.view permission,
// When calling POST /api/api-keys,
// Then response is 403 Forbidden.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_api_keys_view_cannot_create(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: user with api_keys.view permission only
    let (user_token, user_id) =
        create_admin_session_with_user(ctx, "apikeys-view-nocreate@test.com", 1800).await;
    grant_single_permission(ctx, &user_id, "api_keys", "view").await;

    // When: attempting to create an API key
    let body = r#"{"name": "should-not-succeed"}"#;
    let req = Request::builder()
        .method("POST")
        .uri("/api/api-keys".to_string())
        .header(header::AUTHORIZATION, format!("Bearer {}", user_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 403 Forbidden
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "User with api_keys.view only should get 403 Forbidden for create"
    );
}

// =============================================================================
// Scenario 4: api_keys.view cannot delete (US-RA-009)
// =============================================================================

// User Story: docs/user-stories/core/realm-admin.md - Story 9 (US-RA-009)
// Covers: User with api_keys.view only cannot delete API keys
//
// Given a user with ONLY api_keys.view permission and an existing API key,
// When calling DELETE /api/api-keys/{apiKeyId},
// Then response is 403 Forbidden.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_api_keys_view_cannot_delete(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: seed an API key
    let (_admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "apikeys-delseed-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;
    let key_id = seed_api_key(ctx).await;

    // Given: a second user with ONLY api_keys.view permission
    let (user_token, user_id) =
        create_admin_session_with_user(ctx, "apikeys-view-nodelete@test.com", 1800).await;
    grant_single_permission(ctx, &user_id, "api_keys", "view").await;

    // When: attempting to delete the API key
    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/api-keys/{}", key_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", user_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 403 Forbidden
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "User with api_keys.view only should get 403 Forbidden for delete"
    );
}

// =============================================================================
// Scenario 5: api_keys.manage covers view (hierarchy) (US-RA-009)
// =============================================================================

// User Story: docs/user-stories/core/realm-admin.md - Story 9 (US-RA-009)
// Covers: api_keys.manage hierarchy grants api_keys.view access
//
// Given a user with ONLY api_keys.manage permission (no explicit api_keys.view),
// When calling GET /api/api-keys,
// Then response is 200 OK (manage hierarchy grants view).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_api_keys_manage_covers_view(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: user with api_keys.manage permission only (no explicit view)
    let (user_token, user_id) =
        create_admin_session_with_user(ctx, "apikeys-manage-hierarchy@test.com", 1800).await;
    grant_single_permission(ctx, &user_id, "api_keys", "manage").await;

    // When: calling API keys list endpoint (which requires api_keys.view)
    let req = Request::builder()
        .method("GET")
        .uri("/api/api-keys".to_string())
        .header(header::AUTHORIZATION, format!("Bearer {}", user_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 200 OK — manage hierarchy covers view
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "User with api_keys.manage should get 200 OK for list (manage covers view)"
    );
}
