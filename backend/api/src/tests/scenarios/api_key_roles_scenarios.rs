// =============================================================================
// API Key Roles CRUD + Permission + Cache Scenario Tests
// =============================================================================
//
// Verifies API Key role assignment endpoints:
//   GET  /api/api-keys/{realmId}/{apiKeyId}/roles  -> requires api_keys.view
//   PUT  /api/api-keys/{realmId}/{apiKeyId}/roles  -> requires roles.manage
//
// User Stories covered:
// - US-RA-006: User role assignment (API Key reuses same model)
// - US-TP-012: API Key as principal with realm.manage via RBAC
// - US-TP-013: API Key as principal with users.manage/users.view via RBAC
// - US-TP-014: API Key as principal with clients.manage/clients.view via RBAC
//
//
// =============================================================================

use crate::tests::helpers::auth_helpers::{create_admin_session_with_user, grant_realm_admin_role};
use crate::tests::helpers::client_helpers::create_test_api_key;
use crate::tests::response_json;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use herald_core::domain::authorization::permission_service::PermissionService;
use herald_core::domain::authorization::principal_types;
use herald_core::infrastructure::client_api_keys::cache::ApiKeyCacheValue;
use serde_json::json;
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
    .bind(principal_types::USER)
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
// Helper: seed an API key role binding via direct DB insert
// =============================================================================

/// Inserts a role binding for an API key principal into user_roles.
async fn seed_api_key_role(ctx: &TestContext, api_key_id: &str, role_id: uuid::Uuid) {
    let binding_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO user_roles (id, user_id, role_id, realm_id, client_id, principal_type, principal_id)
         VALUES ($1, NULL, $2, $3, $4, $5, $6)",
    )
    .bind(binding_id)
    .bind(role_id)
    .bind(&ctx._realm_id)
    .bind("admin-api-client")
    .bind(principal_types::API_KEY)
    .bind(api_key_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to seed API key role binding");
}

// =============================================================================
// Helper: create a non-builtin role via direct DB insert and return its ID
// =============================================================================

/// Creates a non-builtin role in the test realm and returns its UUID.
async fn seed_role(ctx: &TestContext, name: &str) -> uuid::Uuid {
    let role_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO roles (id, name, description, realm_id, client_id, is_builtin)
         VALUES ($1, $2, $3, $4, $5, false)",
    )
    .bind(role_id)
    .bind(name)
    .bind(format!("Test role: {}", name))
    .bind(&ctx._realm_id)
    .bind(&ctx._client_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to seed role");

    role_id
}

// =============================================================================
// Scenario 1: GET API Key roles returns full list when roles assigned
// =============================================================================

// User Story: docs/user-stories/core/realm-admin.md - US-RA-006
// Covers: US-RA-006, US-TP-012
//
// Given an API Key with assigned roles,
// When calling GET /api/api-keys/{realmId}/{apiKeyId}/roles,
// Then response is 200 OK with the full role list.
#[test_context(TestContext)]
#[tokio::test]
async fn test_get_api_key_roles_with_roles(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: admin user with api_keys.view
    let (token, admin_id) =
        create_admin_session_with_user(ctx, "apikey-roles-view@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_id).await;

    // Given: an API Key with two roles assigned
    let key_id = seed_api_key(ctx).await;
    let role_a = seed_role(ctx, "role-a-get").await;
    let role_b = seed_role(ctx, "role-b-get").await;
    seed_api_key_role(ctx, &key_id, role_a).await;
    seed_api_key_role(ctx, &key_id, role_b).await;

    // When: calling GET roles endpoint
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/api-keys/{}/{}", ctx._realm_id, key_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    // Verify the API Key detail is accessible (sanity check)
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "API Key detail should be accessible"
    );

    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/api-keys/{}/{}/roles", ctx._realm_id, key_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 200 OK with roles
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET roles should return 200 OK"
    );

    let resp_json: serde_json::Value = response_json(resp).await;
    let roles = resp_json["roles"]
        .as_array()
        .expect("roles should be an array");
    assert_eq!(roles.len(), 2, "Should return exactly 2 roles");

    let returned_ids: Vec<&str> = roles.iter().filter_map(|r| r["id"].as_str()).collect();
    assert!(
        returned_ids.contains(&role_a.to_string().as_str()),
        "Role A should be in the response"
    );
    assert!(
        returned_ids.contains(&role_b.to_string().as_str()),
        "Role B should be in the response"
    );
}

// =============================================================================
// Scenario 2: GET API Key roles returns empty array when no roles
// =============================================================================

// User Story: docs/user-stories/core/realm-admin.md - US-RA-006
// Covers: US-RA-006
//
// Given an API Key with no roles assigned,
// When calling GET /api/api-keys/{realmId}/{apiKeyId}/roles,
// Then response is 200 OK with an empty roles array.
#[test_context(TestContext)]
#[tokio::test]
async fn test_get_api_key_roles_empty(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: admin user with api_keys.view
    let (token, admin_id) =
        create_admin_session_with_user(ctx, "apikey-roles-empty@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_id).await;

    // Given: an API Key with NO roles
    let key_id = seed_api_key(ctx).await;

    // When: calling GET roles endpoint
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/api-keys/{}/{}/roles", ctx._realm_id, key_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 200 OK with empty array
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET roles should return 200 OK even when empty"
    );

    let resp_json: serde_json::Value = response_json(resp).await;
    let roles = resp_json["roles"]
        .as_array()
        .expect("roles should be an array");
    assert!(
        roles.is_empty(),
        "Roles array should be empty when no roles assigned"
    );
}

// =============================================================================
// Scenario 3: GET API Key roles returns 404 for nonexistent API Key
// =============================================================================

// User Story: docs/user-stories/core/realm-admin.md - US-RA-006
// Covers: US-RA-006
//
// Given a nonexistent API Key ID,
// When calling GET /api/api-keys/{realmId}/{apiKeyId}/roles,
// Then response is 404 Not Found.
#[test_context(TestContext)]
#[tokio::test]
async fn test_get_api_key_roles_not_found(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: admin user with api_keys.view
    let (token, admin_id) =
        create_admin_session_with_user(ctx, "apikey-roles-nf@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_id).await;

    // Given: a nonexistent API Key ID
    let fake_key_id = uuid::Uuid::now_v7().to_string();

    // When: calling GET roles for nonexistent key
    let req = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/api-keys/{}/{}/roles",
            ctx._realm_id, fake_key_id
        ))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 404 Not Found
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "GET roles should return 404 for nonexistent API Key"
    );
}

// =============================================================================
// Scenario 4: PUT adds new role bindings
// =============================================================================

// User Story: docs/user-stories/core/realm-admin.md - US-RA-006
// Covers: US-RA-006, US-TP-012, US-TP-013, US-TP-014
//
// Given an API Key with no roles and a valid role in the realm,
// When calling PUT /api/api-keys/{realmId}/{apiKeyId}/roles with the role ID,
// Then roles are assigned and a subsequent GET confirms the assignment.
#[test_context(TestContext)]
#[tokio::test]
async fn test_replace_api_key_roles_add(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: admin user with roles.manage
    let (token, admin_id) =
        create_admin_session_with_user(ctx, "apikey-roles-add@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_id).await;

    // Given: an API Key with no roles and a role to assign
    let key_id = seed_api_key(ctx).await;
    let role_id = seed_role(ctx, "role-to-add").await;

    // When: calling PUT to add the role
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/api-keys/{}/{}/roles", ctx._realm_id, key_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "roleIds": [role_id] }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 200 OK
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "PUT add roles should return 200 OK"
    );

    // And: GET confirms the role is assigned
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/api-keys/{}/{}/roles", ctx._realm_id, key_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp_json: serde_json::Value = response_json(resp).await;
    let roles = resp_json["roles"]
        .as_array()
        .expect("roles should be an array");
    assert_eq!(roles.len(), 1, "Should have exactly 1 role after add");
    assert_eq!(roles[0]["id"].as_str().unwrap(), role_id.to_string());
}

// =============================================================================
// Scenario 5: PUT clears all roles with empty array
// =============================================================================

// User Story: docs/user-stories/core/realm-admin.md - US-RA-006
// Covers: US-RA-006
//
// Given an API Key with existing roles,
// When calling PUT /api/api-keys/{realmId}/{apiKeyId}/roles with an empty array,
// Then all roles are cleared and GET returns an empty array.
#[test_context(TestContext)]
#[tokio::test]
async fn test_replace_api_key_roles_clear(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: admin user with roles.manage
    let (token, admin_id) =
        create_admin_session_with_user(ctx, "apikey-roles-clear@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_id).await;

    // Given: an API Key with a role
    let key_id = seed_api_key(ctx).await;
    let role_id = seed_role(ctx, "role-to-clear").await;
    seed_api_key_role(ctx, &key_id, role_id).await;

    // When: calling PUT with empty array to clear all roles
    let empty_roles: Vec<uuid::Uuid> = vec![];
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/api-keys/{}/{}/roles", ctx._realm_id, key_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "roleIds": empty_roles }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 200 OK
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "PUT clear roles should return 200 OK"
    );

    // And: GET confirms no roles
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/api-keys/{}/{}/roles", ctx._realm_id, key_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp_json: serde_json::Value = response_json(resp).await;
    let roles = resp_json["roles"]
        .as_array()
        .expect("roles should be an array");
    assert!(roles.is_empty(), "Roles should be empty after clear");
}

// =============================================================================
// Scenario 6: PUT replaces existing roles with new ones (swap)
// =============================================================================

// User Story: docs/user-stories/core/realm-admin.md - US-RA-006
// Covers: US-RA-006, US-TP-013
//
// Given an API Key with role A assigned,
// When calling PUT /api/api-keys/{realmId}/{apiKeyId}/roles with role B (and not A),
// Then only role B remains assigned.
#[test_context(TestContext)]
#[tokio::test]
async fn test_replace_api_key_roles_swap(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: admin user with roles.manage
    let (token, admin_id) =
        create_admin_session_with_user(ctx, "apikey-roles-swap@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_id).await;

    // Given: an API Key with role A
    let key_id = seed_api_key(ctx).await;
    let role_a = seed_role(ctx, "role-a-swap").await;
    let role_b = seed_role(ctx, "role-b-swap").await;
    seed_api_key_role(ctx, &key_id, role_a).await;

    // When: calling PUT to swap to role B only
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/api-keys/{}/{}/roles", ctx._realm_id, key_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "roleIds": [role_b] }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 200 OK
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "PUT swap roles should return 200 OK"
    );

    // And: GET confirms only role B
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/api-keys/{}/{}/roles", ctx._realm_id, key_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp_json: serde_json::Value = response_json(resp).await;
    let roles = resp_json["roles"]
        .as_array()
        .expect("roles should be an array");
    assert_eq!(roles.len(), 1, "Should have exactly 1 role after swap");
    assert_eq!(roles[0]["id"].as_str().unwrap(), role_b.to_string());
}

// =============================================================================
// Scenario 7: PUT returns 400 for nonexistent role UUID
// =============================================================================

// User Story: docs/user-stories/core/realm-admin.md - US-RA-006
// Covers: US-RA-006
//
// Given an API Key and a nonexistent role UUID,
// When calling PUT /api/api-keys/{realmId}/{apiKeyId}/roles with that UUID,
// Then response is 400 Bad Request.
#[test_context(TestContext)]
#[tokio::test]
async fn test_replace_api_key_roles_not_found_role(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: admin user with roles.manage
    let (token, admin_id) =
        create_admin_session_with_user(ctx, "apikey-roles-nfrole@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_id).await;

    // Given: an API Key and a nonexistent role UUID
    let key_id = seed_api_key(ctx).await;
    let fake_role_id = uuid::Uuid::now_v7();

    // When: calling PUT with the nonexistent role
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/api-keys/{}/{}/roles", ctx._realm_id, key_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "roleIds": [fake_role_id] }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 400 Bad Request
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "PUT with nonexistent role should return 400"
    );
}

// =============================================================================
// Scenario 8: PUT rejects builtin roles
// =============================================================================

// User Story: docs/user-stories/core/realm-admin.md - US-RA-006
// Covers: US-RA-006, design section 4.2.2 builtin role rejection
//
// Given an API Key and a builtin role (is_builtin=true),
// When calling PUT /api/api-keys/{realmId}/{apiKeyId}/roles with that role,
// Then response is 400 Bad Request and no role bindings are written.
#[test_context(TestContext)]
#[tokio::test]
async fn test_replace_api_key_roles_rejects_builtin_roles(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: admin user with roles.manage
    let (token, admin_id) =
        create_admin_session_with_user(ctx, "apikey-roles-builtin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_id).await;

    // Given: an API Key and a builtin role
    let key_id = seed_api_key(ctx).await;
    let builtin_role_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO roles (id, name, description, realm_id, client_id, is_builtin)
         VALUES ($1, $2, $3, $4, $5, true)",
    )
    .bind(builtin_role_id)
    .bind("builtin-test-role")
    .bind("A builtin role for testing")
    .bind(&ctx._realm_id)
    .bind(&ctx._client_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to seed builtin role");

    // When: calling PUT with the builtin role
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/api-keys/{}/{}/roles", ctx._realm_id, key_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "roleIds": [builtin_role_id] }).to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 400 Bad Request
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "PUT with builtin role should return 400"
    );

    // And: no roles are assigned (GET returns empty)
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/api-keys/{}/{}/roles", ctx._realm_id, key_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp_json: serde_json::Value = response_json(resp).await;
    let roles = resp_json["roles"]
        .as_array()
        .expect("roles should be an array");
    assert!(
        roles.is_empty(),
        "No roles should be assigned after builtin rejection"
    );
}

// =============================================================================
// Scenario 9: GET returns 403 without api_keys.view
// =============================================================================

// User Story: docs/user-stories/core/realm-admin.md - US-RA-006
// Covers: US-RA-006, design section 4.5 permission enforcement
//
// Given a user without api_keys.view permission,
// When calling GET /api/api-keys/{realmId}/{apiKeyId}/roles,
// Then response is 403 Forbidden.
#[test_context(TestContext)]
#[tokio::test]
async fn test_get_api_key_roles_forbidden(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: a user without api_keys.view (grant roles.manage only, which does NOT cover api_keys.view)
    let (user_token, user_id) =
        create_admin_session_with_user(ctx, "apikey-roles-noperm-get@test.com", 1800).await;
    grant_single_permission(ctx, &user_id, "roles", "manage").await;

    // Given: an API Key (seeded via DB so we do not need api_keys.manage)
    let key_id = seed_api_key(ctx).await;

    // When: calling GET roles without api_keys.view
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/api-keys/{}/{}/roles", ctx._realm_id, key_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", user_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 403 Forbidden
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "GET roles should return 403 without api_keys.view"
    );
}

// =============================================================================
// Scenario 10: PUT returns 403 without roles.manage
// =============================================================================

// User Story: docs/user-stories/core/realm-admin.md - US-RA-006
// Covers: US-RA-006, design section 4.5 permission enforcement
//
// Given a user without roles.manage permission,
// When calling PUT /api/api-keys/{realmId}/{apiKeyId}/roles,
// Then response is 403 Forbidden.
#[test_context(TestContext)]
#[tokio::test]
async fn test_replace_api_key_roles_forbidden(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: a user without roles.manage (grant api_keys.view only)
    let (user_token, user_id) =
        create_admin_session_with_user(ctx, "apikey-roles-noperm-put@test.com", 1800).await;
    grant_single_permission(ctx, &user_id, "api_keys", "view").await;

    // Given: an API Key and a role to attempt assigning
    let key_id = seed_api_key(ctx).await;
    let role_id = seed_role(ctx, "role-forbidden-test").await;

    // When: calling PUT roles without roles.manage
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/api-keys/{}/{}/roles", ctx._realm_id, key_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", user_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "roleIds": [role_id] }).to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();

    // Then: 403 Forbidden
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "PUT roles should return 403 without roles.manage"
    );
}

// =============================================================================
// Scenario 11: Cache invalidated after role replacement
// =============================================================================

// User Story: docs/user-stories/core/realm-admin.md - US-RA-006
// Covers: US-RA-006, design section 4.5 cache invalidation, US-TP-012
//
// Given an API Key with cached permission data,
// When calling PUT /api/api-keys/{realmId}/{apiKeyId}/roles,
// Then the principal role cache is invalidated so subsequent permission
// checks reflect the new role assignments immediately.
#[test_context(TestContext)]
#[tokio::test]
async fn test_api_key_roles_cache_invalidation(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: admin user with roles.manage
    let (token, admin_id) =
        create_admin_session_with_user(ctx, "apikey-roles-cache@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_id).await;

    // Given: an API Key and a role
    let key_id = seed_api_key(ctx).await;
    let role_id = seed_role(ctx, "role-cache-test").await;

    // Given: seed a permission policy for the role so we can check permissions
    let policy_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO role_policies (id, role_id, realm_id, resource, action)
         VALUES ($1, $2, $3, 'realm', 'view')",
    )
    .bind(policy_id)
    .bind(role_id)
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to seed role policy");

    // Given: pre-warm the principal cache by checking permission before role assignment
    let has_before = ctx
        ._app_state
        .permission_checker
        .check_principal_permission(&ctx._realm_id, "api_key", &key_id, "realm", "view")
        .await
        .expect("Permission check should not error");
    assert!(
        !has_before,
        "API Key should NOT have realm.view before role assignment"
    );

    // When: calling PUT to assign the role (this must invalidate cache)
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/api-keys/{}/{}/roles", ctx._realm_id, key_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "roleIds": [role_id] }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "PUT assign role should succeed"
    );

    // Then: permission check after assignment should reflect the new role
    // (cache was invalidated, so the check fetches fresh data including the new role)
    let has_after = ctx
        ._app_state
        .permission_checker
        .check_principal_permission(&ctx._realm_id, "api_key", &key_id, "realm", "view")
        .await
        .expect("Permission check should not error");
    assert!(
        has_after,
        "API Key SHOULD have realm.view after role assignment (cache invalidated)"
    );
}

// =============================================================================
// Scenario: Cache invalidation after role removal (full round-trip via ext API)
// =============================================================================
//
// Regression test: after removing an API key's role via PUT with roleIds: [],
// the ext API must return 403 (not 200).
//
// This exercises the full stack: admin API role assignment -> cache invalidation
// -> ext API permission check via X-API-Key header.
//
// User Story: docs/user-stories/core/realm-admin.md - US-RA-006
// Covers: US-RA-006, US-TP-012
//
#[test_context(TestContext)]
#[tokio::test]
async fn test_api_key_roles_cache_invalidation_on_remove(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: admin user with roles.manage
    let (token, _admin_id) =
        create_admin_session_with_user(ctx, "apikey-roles-remove-cache@test.com", 1800).await;
    grant_realm_admin_role(ctx, &_admin_id).await;

    // Given: an API Key with a known plaintext value (for X-API-Key header)
    let (api_key_plaintext, api_key_entity) =
        create_test_api_key(ctx, "cache-remove-test-key", true, None).await;
    let key_id = &api_key_entity.id;

    // Given: a role with realm:view policy
    let role_id = seed_role(ctx, "role-cache-remove-test").await;
    let policy_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO role_policies (id, role_id, realm_id, resource, action)
         VALUES ($1, $2, $3, 'realm', 'view')",
    )
    .bind(policy_id)
    .bind(role_id)
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to seed role policy");

    // Step 1: Assign the role to the API key
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/api-keys/{}/{}/roles", ctx._realm_id, key_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "roleIds": [role_id] }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "PUT assign role should succeed"
    );

    // Step 2: Verify ext API returns 200 (permission granted)
    let req = Request::builder()
        .method("GET")
        .uri("/api/ext/realms")
        .header("X-API-Key", &api_key_plaintext)
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET /api/ext/realms should return 200 after role assignment"
    );

    // Step 3: Remove the role (empty roleIds)
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/api-keys/{}/{}/roles", ctx._realm_id, key_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "roleIds": [] }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "PUT remove role should succeed"
    );

    // Step 4: Verify ext API returns 403 (permission denied after removal)
    let req = Request::builder()
        .method("GET")
        .uri("/api/ext/realms")
        .header("X-API-Key", &api_key_plaintext)
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "GET /api/ext/realms should return 403 after role removal (cache invalidated)"
    );
}

// =============================================================================
// API Key Client App Lifecycle Scenarios (tests 12-16)
// =============================================================================
//
// Tests the relationship between API Keys and the realm's built-in API Key
// Client App (client_id='admin-api-client').
//
// User Stories: US-RA-006, US-TP-012, US-TP-013, US-TP-014
//
// =============================================================================

// =============================================================================
// Helper: seed the realm's built-in API Key Client App
// =============================================================================

/// Creates the built-in API Key Client App (client_id='admin-api-client', enabled=true)
/// for the test realm and returns its UUID. Idempotent: if the row already exists (e.g.
/// created by realm init), returns the existing UUID instead of failing.
async fn seed_realm_api_key_client(ctx: &TestContext) -> uuid::Uuid {
    let app_id = uuid::Uuid::now_v7();
    let inserted: Option<(uuid::Uuid,)> = sqlx::query_as(
        "INSERT INTO client_app (id, realm_id, client_id, name, enabled, redirect_uris, browser_refresh_absolute_ttl_seconds)
         VALUES ($1, $2, 'admin-api-client', 'API Key Client', true, '[]'::jsonb, 86400)
         ON CONFLICT (realm_id, client_id) DO NOTHING
         RETURNING id",
    )
    .bind(app_id)
    .bind(&ctx._realm_id)
    .fetch_optional(&ctx._app_state.pool)
    .await
    .expect("Failed to seed realm API Key Client App");

    if let Some((id,)) = inserted {
        return id;
    }

    // Row already exists (created by realm init); fetch its id.
    let (existing_id,): (uuid::Uuid,) = sqlx::query_as(
        "SELECT id FROM client_app WHERE realm_id = $1 AND client_id = 'admin-api-client'",
    )
    .bind(&ctx._realm_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("Failed to find existing realm API Key Client App");

    existing_id
}

/// Creates an ordinary Client App for API key scoping tests.
async fn seed_scoped_client_app(ctx: &TestContext, client_id: &str, name: &str) -> uuid::Uuid {
    let app_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO client_app (id, realm_id, client_id, name, enabled, redirect_uris, browser_refresh_absolute_ttl_seconds)
         VALUES ($1, $2, $3, $4, true, '[]'::jsonb, 86400)",
    )
    .bind(app_id)
    .bind(&ctx._realm_id)
    .bind(client_id)
    .bind(name)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to seed scoped Client App");

    app_id
}

// =============================================================================
// Scenario 12: Creating an API Key uses the realm's built-in API Key Client App
// =============================================================================

// User Story: docs/user-stories/core/realm-admin.md - US-RA-006
// Covers: US-RA-006, US-TP-012, US-TP-013, US-TP-014
//
// Given: Realm has built-in API Key Client App (client_id='admin-api-client', enabled=true)
// When: POST /api/api-keys/{realmId} to create new API Key
// Then: 201 Created, DB row client_api_keys.client_app_id points to built-in Client App UUID
#[test_context(TestContext)]
#[tokio::test]
async fn test_create_api_key_uses_realm_api_key_client(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: admin user with api_keys.manage
    let (token, admin_id) =
        create_admin_session_with_user(ctx, "apikey-lifecycle-create@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_id).await;

    // Given: realm has built-in API Key Client App
    let builtin_client_app_id = seed_realm_api_key_client(ctx).await;

    // When: creating an API Key via POST endpoint
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/api-keys/{}", ctx._realm_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "name": "lifecycle-test-key" }).to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 201 Created
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "POST create API key should return 201 Created"
    );

    let resp_json: serde_json::Value = response_json(resp).await;
    let created_id = resp_json["id"].as_str().expect("Response should have id");

    // Then: DB row client_app_id points to the built-in Client App UUID
    let db_client_app_id: Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT client_app_id FROM client_api_keys WHERE id = $1")
            .bind(created_id)
            .fetch_one(&ctx._app_state.pool)
            .await
            .expect("Failed to query client_app_id");

    assert_eq!(
        db_client_app_id,
        Some(builtin_client_app_id),
        "client_api_keys.client_app_id must point to the realm's built-in API Key Client App"
    );
}

/// Role binding is a post-create operation. If it fails, the API must still
/// return the one-time plaintext key and identify the partial failure.
#[test_context(TestContext)]
#[tokio::test]
async fn test_create_api_key_returns_plaintext_when_role_binding_fails(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, admin_id) =
        create_admin_session_with_user(ctx, "apikey-create-role-failure@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_id).await;
    seed_realm_api_key_client(ctx).await;
    let missing_role_id = uuid::Uuid::now_v7();

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/api-keys/{}", ctx._realm_id))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "name": "partial-role-binding-key",
                "roleIds": [missing_role_id]
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = response_json(resp).await;
    assert!(body["key"].as_str().is_some_and(|key| !key.is_empty()));
    assert!(
        body["roleBindingError"]
            .as_str()
            .is_some_and(|error| { error.contains(&missing_role_id.to_string()) })
    );

    let persisted: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM client_api_keys WHERE id = $1)")
            .bind(body["id"].as_str().unwrap())
            .fetch_one(&ctx._app_state.pool)
            .await
            .unwrap();
    assert!(
        persisted,
        "the response describes a created key, not a rollback"
    );
}

// =============================================================================
// Scenario 12b: Creating an API Key can bind an ordinary Client App
// =============================================================================

// User Story: docs/user-stories/core/realm-admin.md - US-RA-018
// Covers: US-RA-018
//
// Given: Realm has an ordinary Client App
// When: POST /api/api-keys/{realmId} with clientAppId
// Then: 201 Created, response and DB row point to the selected Client App
#[test_context(TestContext)]
#[tokio::test]
async fn test_create_api_key_accepts_client_app_scope(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: admin user with api_keys.manage
    let (token, admin_id) =
        create_admin_session_with_user(ctx, "apikey-client-scope@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_id).await;

    // Given: realm has an ordinary Client App to scope the API Key to
    let client_app_id = seed_scoped_client_app(ctx, "scoped-api-client", "Scoped API Client").await;

    // When: creating an API Key with clientAppId
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/api-keys/{}", ctx._realm_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "name": "scoped-client-app-key",
                "clientAppId": client_app_id
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 201 Created with the selected Client App echoed for the UI table
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "POST create API key with clientAppId should return 201 Created"
    );

    let resp_json: serde_json::Value = response_json(resp).await;
    let created_id = resp_json["id"].as_str().expect("Response should have id");
    assert_eq!(
        resp_json["clientAppId"].as_str(),
        Some(client_app_id.to_string().as_str()),
        "Response should expose the selected Client App id"
    );
    assert_eq!(
        resp_json["clientAppName"].as_str(),
        Some("Scoped API Client"),
        "Response should expose the selected Client App name"
    );

    // Then: DB row client_app_id points to the selected Client App UUID
    let db_client_app_id: Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT client_app_id FROM client_api_keys WHERE id = $1")
            .bind(created_id)
            .fetch_one(&ctx._app_state.pool)
            .await
            .expect("Failed to query client_app_id");

    assert_eq!(
        db_client_app_id,
        Some(client_app_id),
        "client_api_keys.client_app_id must point to the selected Client App"
    );
}

// =============================================================================
// Scenario 13: Creating an API Key fails when realm's built-in Client App missing
// =============================================================================

// User Story: docs/user-stories/core/realm-admin.md - US-RA-006
// Covers: US-RA-006, US-TP-012
//
// Given: Realm's built-in Client App does not exist
// When: POST /api/api-keys/{realmId} to create new API Key
// Then: Error response (400), no new API Key created
#[test_context(TestContext)]
#[tokio::test]
async fn test_create_api_key_fails_when_realm_api_key_client_missing(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: admin user with api_keys.manage
    let (token, admin_id) =
        create_admin_session_with_user(ctx, "apikey-lifecycle-noclient@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_id).await;

    // Given: realm does NOT have the built-in API Key Client App.
    // Realm init now auto-creates it, so we must explicitly remove it.
    sqlx::query("DELETE FROM client_app WHERE realm_id = $1 AND client_id = 'admin-api-client'")
        .bind(&ctx._realm_id)
        .execute(&ctx._app_state.pool)
        .await
        .expect("Failed to delete auto-created admin-api-client");

    // Count existing API keys before attempt
    let count_before: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM client_api_keys WHERE realm_id = $1")
            .bind(&ctx._realm_id)
            .fetch_one(&ctx._app_state.pool)
            .await
            .expect("Failed to count API keys");

    // When: attempting to create an API Key
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/api-keys/{}", ctx._realm_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "name": "should-not-be-created" }).to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: error response (400 Bad Request based on create.rs returning bad_request)
    assert!(
        resp.status() == StatusCode::BAD_REQUEST
            || resp.status() == StatusCode::INTERNAL_SERVER_ERROR,
        "POST create API key should fail with 400 or 500 when built-in Client App is missing, got {}",
        resp.status()
    );

    // Then: no new API Key was created
    let count_after: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM client_api_keys WHERE realm_id = $1")
            .bind(&ctx._realm_id)
            .fetch_one(&ctx._app_state.pool)
            .await
            .expect("Failed to count API keys after attempt");

    assert_eq!(
        count_before, count_after,
        "No new API key should be created when built-in Client App is missing"
    );
}

// =============================================================================
// Scenario 14: Deleting an API Key keeps the realm's built-in API Key Client App
// =============================================================================

// User Story: docs/user-stories/core/realm-admin.md - US-RA-006
// Covers: US-RA-006, US-TP-014
//
// Given: API Key exists linked to built-in Client App
// When: DELETE /api/api-keys/{realmId}/{apiKeyId}
// Then: 204 No Content, built-in Client App still exists, API Key removed
#[test_context(TestContext)]
#[tokio::test]
async fn test_delete_api_key_keeps_realm_api_key_client(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: admin user with api_keys.manage
    let (token, admin_id) =
        create_admin_session_with_user(ctx, "apikey-lifecycle-delete@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_id).await;

    // Given: realm has built-in API Key Client App
    let builtin_client_app_id = seed_realm_api_key_client(ctx).await;

    // Given: seed an API Key linked to the built-in Client App
    let key_id = uuid::Uuid::now_v7().to_string();
    let fake_hash = format!("sha256:{}", uuid::Uuid::now_v7());
    sqlx::query(
        "INSERT INTO client_api_keys (id, name, api_key_hash, realm_id, client_app_id, enabled, created_at)
         VALUES ($1, $2, $3, $4, $5, true, NOW())",
    )
    .bind(&key_id)
    .bind("key-to-delete")
    .bind(&fake_hash)
    .bind(&ctx._realm_id)
    .bind(builtin_client_app_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to seed API key with client_app_id");

    // When: deleting the API Key
    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/api-keys/{}/{}", ctx._realm_id, key_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 204 No Content
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "DELETE API key should return 204 No Content"
    );

    // Then: built-in Client App still exists
    let client_still_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM client_app WHERE id = $1)")
            .bind(builtin_client_app_id)
            .fetch_one(&ctx._app_state.pool)
            .await
            .expect("Failed to check Client App existence");

    assert!(
        client_still_exists,
        "Built-in API Key Client App must NOT be deleted when its API Key is deleted"
    );

    // Then: API Key is removed
    let key_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM client_api_keys WHERE id = $1)")
            .bind(&key_id)
            .fetch_one(&ctx._app_state.pool)
            .await
            .expect("Failed to check API key existence");

    assert!(!key_exists, "API Key should be removed after DELETE");
}

// =============================================================================
// Scenario 15: Disabling an API Key does not update the realm's built-in Client App
// =============================================================================

// User Story: docs/user-stories/core/realm-admin.md - US-RA-006
// Covers: US-RA-006, US-TP-013
//
// Given: API Key enabled=true, built-in Client App enabled=true
// When: Disable API Key (set enabled=false)
// Then: client_api_keys.enabled=false, built-in Client App enabled=true unchanged
#[test_context(TestContext)]
#[tokio::test]
async fn test_disable_api_key_does_not_update_realm_api_key_client(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: admin user with api_keys.manage
    let (token, admin_id) =
        create_admin_session_with_user(ctx, "apikey-lifecycle-disable@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_id).await;

    // Given: realm has built-in API Key Client App (enabled=true)
    let builtin_client_app_id = seed_realm_api_key_client(ctx).await;

    // Given: seed an API Key with enabled=true linked to the built-in Client App
    let key_id = uuid::Uuid::now_v7().to_string();
    let fake_hash = format!("sha256:{}", uuid::Uuid::now_v7());
    sqlx::query(
        "INSERT INTO client_api_keys (id, name, api_key_hash, realm_id, client_app_id, enabled, created_at)
         VALUES ($1, $2, $3, $4, $5, true, NOW())",
    )
    .bind(&key_id)
    .bind("key-to-disable")
    .bind(&fake_hash)
    .bind(&ctx._realm_id)
    .bind(builtin_client_app_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to seed API key");

    // Warm the exact authentication cache entry before disabling the key.
    // WHY: a database-only assertion cannot detect the security regression
    // where a compromised key remains usable from Redis for five minutes.
    ctx._app_state
        .api_key_cache
        .set(
            &fake_hash,
            &ApiKeyCacheValue {
                id: key_id.clone(),
                name: "key-to-disable".to_string(),
                api_key_hash: fake_hash.clone(),
                realm_id: ctx._realm_id.clone(),
                client_app_id: Some(builtin_client_app_id),
                enabled: true,
                client_app_enabled: true,
                expires_at: None,
                created_at: chrono::Utc::now().to_rfc3339(),
            },
            300,
        )
        .await
        .expect("Failed to warm API key authentication cache");

    // When: disabling the API Key (PUT with enabled=false)
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/api-keys/{}/{}", ctx._realm_id, key_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "enabled": false }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 200 OK
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "PUT disable API key should return 200 OK"
    );

    // Then: client_api_keys.enabled=false
    let key_enabled: bool = sqlx::query_scalar("SELECT enabled FROM client_api_keys WHERE id = $1")
        .bind(&key_id)
        .fetch_one(&ctx._app_state.pool)
        .await
        .expect("Failed to query API key enabled state");

    assert!(!key_enabled, "API Key should be disabled (enabled=false)");

    let cached = ctx
        ._app_state
        .api_key_cache
        .get(&fake_hash)
        .await
        .expect("Failed to inspect API key authentication cache");
    assert!(
        cached.is_none(),
        "disabling a key must immediately evict its cached enabled state"
    );

    // Then: built-in Client App enabled=true unchanged
    let client_enabled: bool = sqlx::query_scalar("SELECT enabled FROM client_app WHERE id = $1")
        .bind(builtin_client_app_id)
        .fetch_one(&ctx._app_state.pool)
        .await
        .expect("Failed to query Client App enabled state");

    assert!(
        client_enabled,
        "Built-in API Key Client App must remain enabled=true when an API Key is disabled"
    );
}

// =============================================================================
// Scenario 16: API Key role bindings use the realm's built-in API Key Client ID
// =============================================================================

// User Story: docs/user-stories/core/realm-admin.md - US-RA-006
// Covers: US-RA-006, US-TP-012, US-TP-013, US-TP-014
//
// Given: API Key exists, realm has built-in Client App (client_id='admin-api-client')
// When: PUT /api/api-keys/{realmId}/{apiKeyId}/roles with valid role IDs
// Then: user_roles rows have client_id='admin-api-client', principal_type='api_key',
//       principal_id=api_key_id, user_id=NULL
#[test_context(TestContext)]
#[tokio::test]
async fn test_api_key_roles_use_realm_api_key_client_id(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: admin user with roles.manage
    let (token, admin_id) =
        create_admin_session_with_user(ctx, "apikey-lifecycle-roles@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_id).await;

    // Given: realm has built-in API Key Client App
    seed_realm_api_key_client(ctx).await;

    // Given: an API Key and a role to assign
    let key_id = seed_api_key(ctx).await;
    let role_id = seed_role(ctx, "role-client-id-check").await;

    // When: assigning role via PUT endpoint
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/api-keys/{}/{}/roles", ctx._realm_id, key_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "roleIds": [role_id] }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 200 OK
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "PUT assign role should return 200 OK"
    );

    // Then: user_roles row has correct fields
    let row: sqlx::types::Json<serde_json::Value> = sqlx::query_scalar(
        r#"SELECT json_build_object(
            'client_id', client_id,
            'principal_type', principal_type,
            'principal_id', principal_id,
            'user_id', user_id
        ) FROM user_roles
        WHERE principal_type = 'api_key' AND principal_id = $1 AND role_id = $2 AND realm_id = $3
        LIMIT 1"#,
    )
    .bind(&key_id)
    .bind(role_id)
    .bind(&ctx._realm_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("Failed to query user_roles for API key role binding");

    let obj = &row.0;
    assert_eq!(
        obj["client_id"].as_str(),
        Some("admin-api-client"),
        "user_roles.client_id must be 'admin-api-client'"
    );
    assert_eq!(
        obj["principal_type"].as_str(),
        Some("api_key"),
        "user_roles.principal_type must be 'api_key'"
    );
    assert_eq!(
        obj["principal_id"].as_str(),
        Some(key_id.as_str()),
        "user_roles.principal_id must be the API key ID"
    );
    assert!(
        obj["user_id"].is_null(),
        "user_roles.user_id must be NULL for API key principal"
    );
}
