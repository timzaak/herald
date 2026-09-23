// =============================================================================
// User List API 场景测试
// =============================================================================
//
// 测试用户列表 API 的功能性和错误处理
//
// **问题背景**：
// API 端点：GET /api/users?page=0&pageSize=20
// 错误：返回 500 Internal Server Error
//
// **测试目标**：
// 1. 验证 admin realm 的管理员可以成功访问用户列表
// 2. 验证 API 返回 200 而不是 500
// 3. 如果测试失败，记录详细的错误堆栈和根本原因
//
// **运行方式**：
// ```bash
// cargo nextest run --workspace user_list_scenarios
// ```
//
// =============================================================================

use crate::tests::helpers::*;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use test_context::test_context;
use tower::ServiceExt;

/// ============================================================================
/// User Story: Admin 管理员可以访问用户列表
///
/// **场景描述**：
/// Admin realm 的管理员登录后，访问 /api/roles/admin/users 端点
/// 来获取用户列表，期望成功返回 200 OK 和用户数据。
///
/// **测试步骤**：
/// 1. 创建管理员会话（使用 admin realm）
/// 2. 授予管理员 realm-admin 角色（获得 users.view 权限）
/// 3. 调用 GET /api/roles/admin/users?page=0&pageSize=20
/// 4. 验证返回 200 OK（不是 500 Internal Server Error）
/// 5. 验证响应包含 users 数组和 pagination 信息
///
/// **验收标准**：
/// - API 返回 200 OK
/// - 响应包含 users 数组
/// - 响应包含正确的 pagination 信息
/// - 如果失败，输出详细的错误信息和堆栈
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_admin_list_users_success
/// ```
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_admin_list_users_success(ctx: &mut TestContext) {
    // 使用 admin router（包含权限中间件）
    let app = ctx.create_unified_test_router();

    // ============================================================================
    // Setup: 创建管理员会话并授予权限
    // ============================================================================
    println!("[Setup] 创建管理员会话");

    // 创建管理员会话
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "admin-list-users@test.com", 1800).await;

    // 授予 realm-admin 角色（包含 users.view 权限）
    grant_realm_admin_role(ctx, &admin_user_id).await;

    println!("[Setup] ✓ 管理员会话创建成功，user_id={}", admin_user_id);

    // ============================================================================
    // Step 1: 调用用户列表 API
    // ============================================================================
    println!("[Step 1] 调用 GET /api/users?page=0&pageSize=20");

    let req = Request::builder()
        .method("GET")
        .uri("/api/users?page=0&pageSize=20".to_string())
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();

    println!("[Step 1] 响应状态码: {}", status);

    // ============================================================================
    // Step 2: 验证响应状态码
    // ============================================================================
    println!("[Step 2] 验证响应状态码");

    if status == StatusCode::INTERNAL_SERVER_ERROR {
        // 读取错误响应
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str =
            String::from_utf8(body_bytes.to_vec()).unwrap_or_else(|_| "<invalid utf8>".to_string());

        println!("\n❌ 测试失败：API 返回 500 Internal Server Error");
        println!("\n错误响应内容:");
        println!("{}", body_str);

        // 尝试查询数据库，确认数据存在
        println!("\n=== 数据库调试信息 ===");

        // 检查 realm 是否存在
        let realm_check: Option<(String, String)> =
            sqlx::query_as("SELECT id, name FROM realm WHERE id::text = $1")
                .bind(&ctx._realm_id)
                .fetch_optional(&ctx._app_state.pool)
                .await
                .unwrap_or_else(|e| {
                    println!("数据库查询失败: {}", e);
                    None
                });

        if let Some((id, name)) = realm_check {
            println!("Realm 存在: {} ({})", name, id);
        } else {
            println!("⚠️  Realm 不存在或查询失败: {}", ctx._realm_id);
        }

        // 检查用户表
        let user_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM account WHERE realm_id::text = $1")
                .bind(&ctx._realm_id)
                .fetch_one(&ctx._app_state.pool)
                .await
                .unwrap_or_else(|e| {
                    println!("查询用户数量失败: {}", e);
                    -1
                });

        println!("用户表记录数: {}", user_count);

        // 检查权限配置
        println!("\n=== 权限配置检查 ===");

        // 检查用户的角色分配
        let user_roles: Vec<(String, String)> = sqlx::query_as(
            "SELECT r.name, r.id::text
             FROM user_roles ur
             JOIN roles r ON ur.role_id = r.id
             WHERE ur.user_id::text = $1 AND ur.realm_id::text = $2",
        )
        .bind(&admin_user_id)
        .bind(&ctx._realm_id)
        .fetch_all(&ctx._app_state.pool)
        .await
        .unwrap_or_else(|e| {
            println!("查询用户角色失败: {}", e);
            vec![]
        });

        println!("管理员的角色:");
        for (role_name, role_id) in &user_roles {
            println!("  - {} (ID: {})", role_name, role_id);
        }

        // 检查 realm-admin 角色的权限
        if !user_roles.is_empty() {
            let role_policies: Vec<(String, String)> = sqlx::query_as(
                "SELECT resource, action
                 FROM role_policies
                 WHERE role_id::text = $1 AND realm_id::text = $2",
            )
            .bind(&user_roles[0].1) // 使用第一个角色的 ID
            .bind(&ctx._realm_id)
            .fetch_all(&ctx._app_state.pool)
            .await
            .unwrap_or_else(|e| {
                println!("查询角色权限失败: {}", e);
                vec![]
            });

            println!("角色权限:");
            for (resource, action) in &role_policies {
                println!("  - {} : {}", resource, action);
            }

            // 检查是否有 users.view 权限
            let has_users_view = role_policies
                .iter()
                .any(|(r, a)| r == "users" && a == "view");

            if has_users_view {
                println!("✓ 找到 users.view 权限");
            } else {
                println!("⚠️  缺少 users.view 权限");
            }
        }

        // 断言失败
        panic!(
            "用户列表 API 应该返回 200 OK，但返回了 500 Internal Server Error\n\n\
             请查看上述调试信息来定位问题根源。"
        );
    } else if status == StatusCode::FORBIDDEN {
        // 权限不足 - 读取响应体
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let _body_str =
            String::from_utf8(body_bytes.to_vec()).unwrap_or_else(|_| "<invalid utf8>".to_string());

        println!("\n❌ 测试失败：API 返回 403 Forbidden");
        println!("原因：管理员缺少 users.view 权限");

        // 检查权限配置
        let user_roles: Vec<(String, String)> = sqlx::query_as(
            "SELECT r.name, r.id::text
             FROM user_roles ur
             JOIN roles r ON ur.role_id = r.id
             WHERE ur.user_id::text = $1 AND ur.realm_id::text = $2",
        )
        .bind(&admin_user_id)
        .bind(&ctx._realm_id)
        .fetch_all(&ctx._app_state.pool)
        .await
        .unwrap_or_default();

        println!("管理员的角色:");
        for (role_name, role_id) in &user_roles {
            println!("  - {} (ID: {})", role_name, role_id);

            // 显示该角色的权限
            let role_policies: Vec<(String, String)> = sqlx::query_as(
                "SELECT resource, action
                 FROM role_policies
                 WHERE role_id::text = $1 AND realm_id::text = $2",
            )
            .bind(role_id)
            .bind(&ctx._realm_id)
            .fetch_all(&ctx._app_state.pool)
            .await
            .unwrap_or_default();

            println!("  权限:");
            for (resource, action) in &role_policies {
                println!("    - {} : {}", resource, action);
            }
        }

        panic!(
            "用户列表 API 应该返回 200 OK，但返回了 403 Forbidden\n\n\
             请确保管理员拥有 users.view 权限。"
        );
    } else if status != StatusCode::OK {
        // 其他错误状态码
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str =
            String::from_utf8(body_bytes.to_vec()).unwrap_or_else(|_| "<invalid utf8>".to_string());

        println!("\n❌ 测试失败：API 返回意外的状态码");
        println!("状态码: {}", status);
        println!("响应内容: {}", body_str);

        panic!(
            "用户列表 API 返回了意外的状态码: {} (期望 200 OK)\n响应: {}",
            status, body_str
        );
    }

    println!("[Step 2] ✅ 状态码验证通过: 200 OK");

    // ============================================================================
    // Step 3: 验证响应内容
    // ============================================================================
    println!("[Step 3] 验证响应内容");

    let body: serde_json::Value = crate::tests::response_json(resp).await;

    assert!(body["items"].is_array(), "响应应包含 items 数组");
    let users = body["items"].as_array().unwrap();
    println!("[Step 3] ✅ items 数组存在，共 {} 个用户", users.len());

    assert_eq!(body["page"], 0, "page 应该是 0 (从请求参数 page=0)");
    assert_eq!(
        body["pageSize"], 20,
        "pageSize 应该是 20 (从请求参数 pageSize=20)"
    );

    let total_count = body["total"].as_i64().unwrap_or(0);

    println!(
        "[Step 3] ✅ page 信息正确: page={}, pageSize={}, total_count={}",
        body["page"], body["pageSize"], total_count
    );

    // ============================================================================
    // 测试通过
    // ============================================================================
    println!("\n✅ 用户列表 API 测试通过！");
    println!("   - 状态码: 200 OK");
    println!("   - 用户数量: {}", users.len());
    println!("   - 总记录数: {}", total_count);
}
/// ============================================================================
/// User Story: User list includes nickname field
///
/// **场景描述**：
/// 验证用户列表 API 返回的每个用户都包含正确的 nickname 字段。
///
/// **测试步骤**：
/// 1. 创建测试用户（有些有 nickname，有些没有）
/// 2. 更新用户的 nickname
/// 3. 调用用户列表 API
/// 4. 验证返回的用户包含正确的 nickname 值
///
/// **验收标准**：
/// - 有 nickname 的用户显示正确的值
/// - 没有 nickname 的用户显示 null
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_user_list_includes_nicknames
/// ```
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_user_list_includes_nicknames(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Setup: Create admin session
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "admin-nick@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    // Create test users
    let user1_id = create_simple_test_user(ctx, "user1@test.com")
        .await
        .to_string();
    let user2_id = create_simple_test_user(ctx, "user2@test.com")
        .await
        .to_string();

    // Update user1 nickname
    let update_payload = serde_json::json!({
        "nickname": "Alice"
    });

    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/users/{}", user1_id))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::from(serde_json::to_string(&update_payload).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Fetch user list
    let req = Request::builder()
        .method("GET")
        .uri("/api/users?page=0&pageSize=20".to_string())
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = crate::tests::response_json(resp).await;
    let users = body["items"].as_array().unwrap();

    // Find our test users
    let user1 = users
        .iter()
        .find(|u| u["id"].as_str() == Some(user1_id.as_str()))
        .unwrap();
    let user2 = users
        .iter()
        .find(|u| u["id"].as_str() == Some(user2_id.as_str()))
        .unwrap();

    // Verify nicknames
    assert_eq!(
        user1["nickname"], "Alice",
        "User1 should have nickname 'Alice'"
    );
    assert!(
        user2["nickname"].is_null(),
        "User2 should have null nickname"
    );

    println!("\n✅ 用户列表 nickname 字段测试通过！");
    println!(
        "   - User1 ({}): nickname = {}",
        user1_id, user1["nickname"]
    );
    println!("   - User2 ({}): nickname = null", user2_id);
}
