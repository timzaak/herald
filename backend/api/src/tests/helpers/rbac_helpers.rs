// =============================================================================
// 通用 RBAC 辅助函数
// =============================================================================

#![allow(dead_code)]

use crate::application::http::role_definitions::types::{RoleCreateRequest, RoleResponse};
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use herald_core::domain::authorization::permission_service::PermissionService;
use herald_core::domain::authorization::principal_types;
use serde_json::json;
use tower::ServiceExt;

/// ============================================================================
/// 角色定义管理
/// ============================================================================
///
/// 创建角色定义
///
/// **返回**: role_id (String)
///
pub async fn create_role(
    ctx: &TestContext,
    _realm_id: &str,
    token: &str,
    name: &str,
    description: &str,
) -> uuid::Uuid {
    let app = ctx.create_unified_test_router();

    let req_body = json!(RoleCreateRequest {
        name: name.to_string(),
        description: Some(description.to_string()),
        client_id: ctx._client_id.clone(),
    })
    .to_string();

    let req = Request::builder()
        .method("POST")
        .uri("/api/roles/define".to_string())
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(req_body))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let role: RoleResponse = crate::tests::response_json(resp).await;
    role.id
}

/// 为用户分配角色（通过 user_roles 表）
///
/// user_roles 格式: {user_id}, {role_id}, {realm_id}, {client_id}
///
pub async fn assign_role_to_user(
    ctx: &TestContext,
    realm_id: &str,
    _token: &str,
    user_id: uuid::Uuid,
    role_id: uuid::Uuid,
) {
    let user_role_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO user_roles (id, user_id, role_id, realm_id, client_id, principal_type, principal_id)
         VALUES ($1, $2, $3, $4, $5, $6, $2::text)",
    )
    .bind(user_role_id)
    .bind(user_id)
    .bind(role_id)
    .bind(realm_id)
    .bind(&ctx._client_id)
    .bind(principal_types::USER)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to add role to user");
    let _ = ctx
        ._app_state
        .permission_checker
        .invalidate_user_role_cache(realm_id, &user_id.to_string())
        .await;
}

/// 创建仅含单个权限（resource.action）的角色并分配给用户
///
/// 场景测试需要"恰好一个特定权限"时使用，避免直接授予 realm-admin 角色。
pub async fn grant_single_permission(
    ctx: &TestContext,
    user_id: &str,
    resource: &str,
    action: &str,
) {
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
