// =============================================================================
// Role Policies Scenario Tests (GWT Format)
// =============================================================================
//
// Tests for role policy management API
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
use herald_core::domain::authorization::permission_service::PermissionService;
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;

// ============================================================================
// Scenario 1: Add Policy to Role
// ============================================================================

/// **Given**: 角色 role-a 没有任何策略
/// **When**: POST /api/admin/roles/role-a/policies, body: { resource: "users", action: "view" }
/// **Then**: HTTP 201 Created
/// **And**: GET /api/admin/roles/role-a/policies 返回包含新策略
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_add_policy_to_role(ctx: &mut SchemaTestContext) {
    let (token, user_id) = create_admin_session_with_user(ctx, "test-admin", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    // Given: Create role without policies
    let role_id = create_role(ctx, &ctx._realm_id, &token, "role-a", "Role A").await;

    // When: Add policy to role
    let app = ctx.create_unified_test_router();
    let req_body = json!({
        "resource": "users",
        "action": "view"
    })
    .to_string();

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/permission/roles/{}/policies", role_id))
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::from(req_body))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: HTTP 201 Created
    assert_eq!(resp.status(), StatusCode::CREATED);
    let resp_json: serde_json::Value = response_json(resp).await;
    assert_eq!(resp_json["resource"], "users");
    assert_eq!(resp_json["action"], "view");
    assert!(resp_json["id"].is_string());
    assert!(resp_json["meta"].is_null());

    // And: Verify policy exists in database
    let policy_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM role_policies
         WHERE role_id = $1 AND resource = $2 AND action = $3",
    )
    .bind(role_id)
    .bind("users")
    .bind("view")
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("Failed to query role_policies");

    assert_eq!(policy_count, 1, "Policy should exist in database");
}

// ============================================================================
// Scenario 2: Delete Policy from Role
// ============================================================================

/// **Given**: 角色 role-a 有 users.view 策略
/// **When**: DELETE /api/admin/roles/role-a/policies/{policy_id}
/// **Then**: HTTP 204 No Content
/// **And**: GET /api/admin/roles/role-a/policies 不包含该策略
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_delete_policy_from_role(ctx: &mut SchemaTestContext) {
    let (token, user_id) = create_admin_session_with_user(ctx, "test-admin", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    // Given: Create role with policy
    let role_id = create_role(ctx, &ctx._realm_id, &token, "role-a", "Role A").await;

    // Insert policy directly
    let policy_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO role_policies (id, realm_id, role_id, resource, action)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(policy_id)
    .bind(&ctx._realm_id)
    .bind(role_id)
    .bind("users")
    .bind("view")
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to insert policy");

    // Invalidate cache
    let _ = ctx
        ._app_state
        .permission_checker
        .invalidate_role_policy_cache(&ctx._realm_id, &role_id.to_string())
        .await;

    // When: Delete policy
    let app = ctx.create_unified_test_router();
    let req = Request::builder()
        .method("DELETE")
        .uri(format!(
            "/api/permission/roles/{}/policies/{}",
            role_id, policy_id
        ))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: HTTP 204 No Content
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // And: Verify policy is deleted
    let policy_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM role_policies WHERE id = $1")
        .bind(policy_id)
        .fetch_one(&ctx._app_state.pool)
        .await
        .expect("Failed to query role_policies");

    assert_eq!(policy_count, 0, "Policy should be deleted");
}

// ============================================================================
// Scenario 3: Delete Policy via Mismatched Role Path
// ============================================================================

/// **Given**: 角色 role-a 与 role-b 各有策略，role-b 的策略 policy-b 仍然生效
/// **When**: DELETE /api/permission/roles/role-a/policies/{policy_b}（policy 属于 role-b）
/// **Then**: HTTP 404，policy-b 保持存在
///
/// WHY: 删除必须同时校验 policy 属于路径中的 role。否则同 realm 管理员可用
/// role-a 的路径删掉 role-b 的策略，且缓存失效会打在未校验的 roleId 上——
/// 真正受影响的 role-b 会继续从缓存拿到已被"撤销"的权限，撤销不即时生效。
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_delete_policy_with_mismatched_role_rejected(ctx: &mut SchemaTestContext) {
    let (token, user_id) = create_admin_session_with_user(ctx, "test-admin", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    // Given: two roles; the policy belongs to role-b
    let role_a = create_role(ctx, &ctx._realm_id, &token, "role-a", "Role A").await;
    let role_b = create_role(ctx, &ctx._realm_id, &token, "role-b", "Role B").await;

    let policy_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO role_policies (id, realm_id, role_id, resource, action)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(policy_id)
    .bind(&ctx._realm_id)
    .bind(role_b)
    .bind("users")
    .bind("view")
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to insert policy");

    // When: address the policy through role-a's path
    let app = ctx.create_unified_test_router();
    let req = Request::builder()
        .method("DELETE")
        .uri(format!(
            "/api/permission/roles/{}/policies/{}",
            role_a, policy_id
        ))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();

    // Then: rejected and the policy survives
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let policy_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM role_policies WHERE id = $1")
        .bind(policy_id)
        .fetch_one(&ctx._app_state.pool)
        .await
        .expect("Failed to query role_policies");
    assert_eq!(
        policy_count, 1,
        "Role-b's policy must not be deletable via role-a's path"
    );
}

// ============================================================================
// Scenario 4: Policy Uniqueness Constraint
// ============================================================================

/// **Given**: 角色 role-a 已有 users.view 策略
/// **When**: POST /api/admin/roles/role-a/policies, body: { resource: "users", action: "view" }
/// **Then**: HTTP 409 Conflict (唯一性约束)
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_policy_uniqueness_constraint(ctx: &mut SchemaTestContext) {
    let (token, user_id) = create_admin_session_with_user(ctx, "test-admin", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    // Given: Create role with policy
    let role_id = create_role(ctx, &ctx._realm_id, &token, "role-a", "Role A").await;

    // Insert first policy
    let policy_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO role_policies (id, realm_id, role_id, resource, action)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(policy_id)
    .bind(&ctx._realm_id)
    .bind(role_id)
    .bind("users")
    .bind("view")
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to insert policy");

    // Invalidate cache
    let _ = ctx
        ._app_state
        .permission_checker
        .invalidate_role_policy_cache(&ctx._realm_id, &role_id.to_string())
        .await;

    // When: Try to add duplicate policy
    let app = ctx.create_unified_test_router();
    let req_body = json!({
        "resource": "users",
        "action": "view"
    })
    .to_string();

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/permission/roles/{}/policies", role_id))
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::from(req_body))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();

    // Then: HTTP 409 Conflict
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}
