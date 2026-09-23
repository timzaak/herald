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

    // Invalidate cache
    let _ = ctx
        ._app_state
        .permission_checker
        .invalidate_user_role_cache(realm_id, &user_id.to_string())
        .await;
}
