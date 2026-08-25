// =============================================================================
// User Roles Scenario Tests (GWT Format)
// =============================================================================
//
// Tests for user role management API
// Based on design document Section 5.7.2
//
// =============================================================================

use crate::tests::helpers::*;
use crate::tests::response_json;
use crate::tests::schema_test_context::SchemaTestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;

// ============================================================================
// Scenario 1: Assign Role to User
// ============================================================================

/// **Given**: 用户 user-1 没有任何角色
/// **When**: POST /api/admin/users/user-1/roles, body: { role_ids: ["role-a"] }
/// **Then**: HTTP 200 OK
/// **And**: GET /api/admin/users/user-1/roles 返回包含 role-a
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_assign_role_to_user(ctx: &mut SchemaTestContext) {
    let (token, user_id_str) = create_admin_session_with_user(ctx, "test-admin", 1800).await;
    grant_realm_admin_role(ctx, &user_id_str).await;

    // Given: Create user without roles
    let user_id = create_simple_test_user(ctx, "user-1@example.com")
        .await
        .to_string();

    // Given: Create role
    let role_id = create_role(ctx, &ctx._realm_id, &token, "role-a", "Role A").await;

    // When: Assign role to user
    let app = ctx.create_unified_test_router();
    let req_body = json!({
        "roleIds": [role_id]
    })
    .to_string();

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/permission/users/{}/roles", user_id))
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::from(req_body))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: HTTP 201 CREATED
    let status = resp.status();
    if status != StatusCode::CREATED {
        eprintln!("Error response status: {}", status);
        let error_json: serde_json::Value = response_json(resp).await;
        eprintln!("Error response body: {}", error_json);
    }
    assert_eq!(status, StatusCode::CREATED);

    // And: Verify role is assigned
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/permission/users/{}/roles", user_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp_json: serde_json::Value = response_json(resp).await;
    assert_eq!(resp_json["roles"].as_array().unwrap().len(), 1);
    assert_eq!(resp_json["roles"][0]["id"], serde_json::json!(role_id));
}

// ============================================================================
// Scenario 2: Remove Role from User
// ============================================================================

/// **Given**: 用户 user-1 有 role-a 角色
/// **When**: DELETE /api/admin/users/user-1/roles/role-a
/// **Then**: HTTP 204 No Content
/// **And**: GET /api/admin/users/user-1/roles 不包含 role-a
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_remove_role_from_user(ctx: &mut SchemaTestContext) {
    let (token, user_id_str) = create_admin_session_with_user(ctx, "test-admin", 1800).await;
    grant_realm_admin_role(ctx, &user_id_str).await;

    // Given: Create user with role
    let user_id = create_simple_test_user(ctx, "user-2@example.com").await;
    let role_id = create_role(ctx, &ctx._realm_id, &token, "role-a", "Role A").await;
    assign_role_to_user(ctx, &ctx._realm_id, &token, user_id, role_id).await;

    // When: Remove role from user
    let app = ctx.create_unified_test_router();
    let req = Request::builder()
        .method("DELETE")
        .uri(format!(
            "/api/permission/users/{}/roles/{}",
            user_id, role_id
        ))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: HTTP 204 No Content
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // And: Verify role is removed
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/permission/users/{}/roles", user_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let resp_json: serde_json::Value = response_json(resp).await;

    assert_eq!(resp_json["roles"].as_array().unwrap().len(), 0);
}
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_assign_duplicate_role_ids_in_single_request(ctx: &mut SchemaTestContext) {
    let (token, user_id_str) = create_admin_session_with_user(ctx, "test-admin", 1800).await;
    grant_realm_admin_role(ctx, &user_id_str).await;

    let user_id = create_simple_test_user(ctx, "user-duplicate-role-request@example.com").await;
    let role_id = create_role(ctx, &ctx._realm_id, &token, "role-dup", "Role Dup").await;

    let app = ctx.create_unified_test_router();
    let req_body = json!({
        "roleIds": [role_id, role_id]
    })
    .to_string();

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/permission/users/{}/roles", user_id))
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::from(req_body))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/permission/users/{}/roles", user_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let resp_json: serde_json::Value = response_json(resp).await;

    let roles = resp_json["roles"].as_array().unwrap();
    assert!(roles.len() == 1, "Expected 1 role, got {}", roles.len());
    assert_eq!(resp_json["roles"][0]["id"], serde_json::json!(role_id));
}
// ============================================================================
// Scenario 5: Assign Multiple Roles
// ============================================================================

/// **Given**: 用户 user-1 没有任何角色
/// **When**: POST /api/admin/users/user-1/roles, body: { role_ids: ["role-a", "role-b", "role-c"] }
/// **Then**: HTTP 200 OK
/// **And**: 用户拥有所有三个角色
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_assign_multiple_roles(ctx: &mut SchemaTestContext) {
    let (token, user_id_str) = create_admin_session_with_user(ctx, "test-admin", 1800).await;
    grant_realm_admin_role(ctx, &user_id_str).await;

    // Given: User without roles
    let user_id = create_simple_test_user(ctx, "multi-roles-user@example.com").await;

    // Given: Create three roles
    let role_a = create_role(ctx, &ctx._realm_id, &token, "role-a", "Role A").await;
    let role_b = create_role(ctx, &ctx._realm_id, &token, "role-b", "Role B").await;
    let role_c = create_role(ctx, &ctx._realm_id, &token, "role-c", "Role C").await;

    // When: Assign all three roles
    let app = ctx.create_unified_test_router();
    let req_body = json!({
        "roleIds": [role_a.clone(), role_b.clone(), role_c.clone()]
    })
    .to_string();

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/permission/users/{}/roles", user_id))
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::from(req_body))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Then: Verify user has all three roles
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/permission/users/{}/roles", user_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let resp_json: serde_json::Value = response_json(resp).await;

    let roles = resp_json["roles"].as_array().unwrap();
    assert_eq!(roles.len(), 3);

    let role_ids: Vec<uuid::Uuid> = roles
        .iter()
        .filter_map(|r| r["id"].as_str().and_then(|s| uuid::Uuid::parse_str(s).ok()))
        .collect();

    assert!(role_ids.contains(&role_a));
    assert!(role_ids.contains(&role_b));
    assert!(role_ids.contains(&role_c));
}

// ============================================================================
// Scenario 6: Role Assignment Requires Permission
// ============================================================================

/// **Given**: 普通用户没有 `roles.manage` 权限
/// **When**: POST /api/permission/users/{userId}/roles
/// **Then**: HTTP 403 Forbidden
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_assign_role_requires_roles_manage(ctx: &mut SchemaTestContext) {
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "setup-admin@example.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let (user_token, _user_id) =
        create_admin_session_with_user(ctx, "plain-user@example.com", 1800).await;

    let target_user_id = create_simple_test_user(ctx, "assign-target@example.com").await;
    let role_id = create_role(ctx, &ctx._realm_id, &admin_token, "limited-role", "Limited").await;

    let app = ctx.create_unified_test_router();
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/permission/users/{}/roles", target_user_id))
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {}", user_token))
        .body(Body::from(
            json!({
                "roleIds": [role_id]
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ============================================================================
// Scenario 6: 自助角色列表过滤跨 realm 引用（仓储层 realm 谓词兜底）
// ============================================================================

/// **WHY**: `RoleRepository::find_by_ids` 曾按 ID 全表取角色（无 realm 谓词），
/// 自助角色列表 `/api/user/roles` 的租户隔离完全依赖"user_roles 不会出现跨
/// realm 引用"这一上游数据完整性假设。一旦任一写入路径出现缺陷（或数据被
/// 直接污染），A realm 用户就能看到 B realm 的角色名。修复后仓储层强制
/// realm 过滤：即使关联表存在脏数据，外域角色也不会出现在返回中——租户
/// 边界由读取路径自身保证，而非依赖上游无缺陷。
///
/// **Given**: 用户在本 realm 拥有一个正常角色
/// **And**: user_roles 中存在一条指向外 realm 角色的脏引用
/// **When**: 用户 GET /api/user/roles
/// **Then**: 本 realm 角色正常返回，外 realm 角色名不出现
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_user_roles_hides_cross_realm_role_references(ctx: &mut SchemaTestContext) {
    // Given: 创建并登录普通用户
    let (user_uuid, token) = crate::tests::helpers::test_setup_helpers::create_user_and_login(
        ctx,
        "roles-leak-canary@example.com",
        "SecurePassword123!",
    )
    .await;

    // 本 realm 的正常角色（保证返回列表非空，避免测试假阳性）
    let local_role_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO roles (id, name, description, realm_id, client_id, is_builtin)
         VALUES ($1, 'legit-local-role', NULL, $2, $3, false)",
    )
    .bind(local_role_id)
    .bind(&ctx._realm_id)
    .bind(&ctx._client_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to create local role");

    // And: 外 realm 的金丝雀角色 + 一条脏的 user_roles 引用（模拟上游写入
    // 缺陷或数据污染：本 realm 的 user_roles 行指向外 realm 角色）
    let foreign_realm = format!("{}-foreign", ctx._realm_id);
    let foreign_role_id = uuid::Uuid::now_v7();
    let foreign_role_name = format!("foreign-canary-{}", foreign_role_id.simple());
    sqlx::query(
        "INSERT INTO roles (id, name, description, realm_id, client_id, is_builtin)
         VALUES ($1, $2, NULL, $3, $4, false)",
    )
    .bind(foreign_role_id)
    .bind(&foreign_role_name)
    .bind(&foreign_realm)
    .bind(&ctx._client_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to create foreign realm role");

    for role_id in [local_role_id, foreign_role_id] {
        sqlx::query(
            "INSERT INTO user_roles (id, user_id, role_id, realm_id, client_id, principal_type, principal_id)
             VALUES ($1, $2, $3, $4, $5, 'user', $2::text)",
        )
        .bind(uuid::Uuid::now_v7())
        .bind(user_uuid)
        .bind(role_id)
        .bind(&ctx._realm_id)
        .bind(&ctx._client_id)
        .execute(&ctx._app_state.pool)
        .await
        .expect("Failed to create user role binding");
    }

    // 直插 SQL 绕过了缓存失效路径，主动失效以保证读取走 DB
    use herald_core::domain::authorization::permission_service::PermissionService;
    ctx._app_state
        .permission_checker
        .invalidate_user_role_cache(&ctx._realm_id, &user_uuid.to_string())
        .await
        .expect("Failed to invalidate user role cache");

    // When: 获取自助角色列表
    let app = ctx.create_unified_test_router();
    let req = Request::builder()
        .method("GET")
        .uri("/api/user/roles")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 本 realm 角色返回，外 realm 角色名被过滤
    assert_eq!(resp.status(), StatusCode::OK, "获取自助角色应返回 200");
    let result: serde_json::Value = response_json(resp).await;

    let roles: Vec<String> = result["roles"]
        .as_array()
        .expect("roles should be an array")
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    assert!(
        roles.contains(&"legit-local-role".to_string()),
        "same-realm role must be returned, got: {roles:?}"
    );
    assert!(
        !roles.contains(&foreign_role_name),
        "cross-realm role name must not leak into the self-service role list, got: {roles:?}"
    );
}
