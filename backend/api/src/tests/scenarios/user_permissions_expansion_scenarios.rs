// =============================================================================
// User Permissions Hierarchy Expansion Scenario Tests
// =============================================================================
//
// Verifies that GET /api/user/permissions returns the hierarchy-EXPANDED
// effective permission set.
//
// WHY: exact-string consumers (frontend menu/entry gating, admin-console
// eligibility) read this list. A custom role holding only `resource.manage`
// passes backend checks via the manage→view/create hierarchy; if the
// endpoint returned raw policy strings, the frontend would hide every
// view-gated entry for that role — authorized on the server, invisible in
// the UI.
//
// Rules: docs/prd/auth/permissions.md §4.1（manage 隐含 view/create），
// §5（权限层级规则正确生效）
//
// Route: GET /api/user/permissions

use crate::tests::helpers::auth_helpers::create_admin_session_with_user;
use crate::tests::helpers::rbac_helpers::grant_single_permission;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use test_context::test_context;
use tower::ServiceExt;

/// GET /api/user/permissions for the session user, returning the permission
/// strings carried in the response envelope.
async fn fetch_own_permissions(app: axum::Router, token: &str) -> Vec<String> {
    let req = Request::builder()
        .method("GET")
        .uri("/api/user/permissions")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "permissions endpoint must answer 200"
    );
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    let data = body.get("data").cloned().unwrap_or(body);
    data["permissions"]
        .as_array()
        .expect("permissions must be an array")
        .iter()
        .map(|p| p.as_str().unwrap_or_default().to_string())
        .collect()
}

// Scenario 1: manage-only role lists view/create (expanded effective set)
//
// Given a user whose only grant is users.manage (no explicit users.view),
// When GET /api/user/permissions,
// Then the list contains users.manage, users.view AND users.create — the
// same access the check path would enforce.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_manage_only_role_lists_derived_view_and_create(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    let (user_token, user_id) =
        create_admin_session_with_user(ctx, "perm-expansion-manage@test.com", 1800).await;
    grant_single_permission(ctx, &user_id, "users", "manage").await;

    let permissions = fetch_own_permissions(app, &user_token).await;

    assert!(
        permissions.contains(&"users.manage".to_string()),
        "raw grant must be listed; got {permissions:?}"
    );
    assert!(
        permissions.contains(&"users.view".to_string()),
        "manage must expand to view for exact-string frontend gating; got {permissions:?}"
    );
    assert!(
        permissions.contains(&"users.create".to_string()),
        "manage must expand to create; got {permissions:?}"
    );
}

// Scenario 2: view-only role does NOT list manage/create (no upward expansion)
//
// Given a user whose only grant is users.view,
// When GET /api/user/permissions,
// Then the list contains users.view but neither users.manage nor users.create.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_view_only_role_lists_no_derived_permissions(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    let (user_token, user_id) =
        create_admin_session_with_user(ctx, "perm-expansion-view@test.com", 1800).await;
    grant_single_permission(ctx, &user_id, "users", "view").await;

    let permissions = fetch_own_permissions(app, &user_token).await;

    assert!(
        permissions.contains(&"users.view".to_string()),
        "raw grant must be listed; got {permissions:?}"
    );
    assert!(
        !permissions.contains(&"users.manage".to_string()),
        "view must not expand upward to manage; got {permissions:?}"
    );
    assert!(
        !permissions.contains(&"users.create".to_string()),
        "view must not expand to create; got {permissions:?}"
    );
}
