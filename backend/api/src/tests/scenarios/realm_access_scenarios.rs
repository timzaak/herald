// =============================================================================
// Realm Access - Scenario Tests
// =============================================================================
//
// 测试 Realm 创建和跨 Realm 用户访问功能
//
// **问题背景**:
// - Demo 测试失败: realm1 创建后,访问 /api/roles/realm1/users 时页面显示 "Something went wrong!"
// - Setup 脚本在创建 realm 后访问用户页面时超时
// - 错误特征: 页面无法加载 h1 标题,说明路由或数据查询有问题
//
// **测试覆盖**:
// - Realm 创建功能: 验证 admin realm 的管理员能否创建新 realm (realm1, realm2)
// - Realm 切换功能: 验证用户能否在创建的 realm 之间切换
// - Realm 下的用户管理: 验证在特定 realm (realm1) 下能否访问用户管理页面
//
// **相关 API 端点**:
// - POST /api/realms - 创建 realm
// - GET /api/realms - 查询 realm 列表
// - GET /api/users - 查询 realm 下的用户
// - GET /api/roles/dashboard - 访问 realm dashboard
//
// **运行方式**:
// cargo nextest run --package herald-api realm_access
//
// =============================================================================

use crate::tests::helpers::test_setup_helpers::record_test_user_consent;
use crate::tests::helpers::*;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use herald_core::domain::{
    authentication::BrowserTokenService, client::ports::ClientService, user::UserRepository,
};
use herald_core::infrastructure::authentication::RedisBrowserTokenService;
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;

/// Record legal consent for a realm admin user created via the realm API.
///
/// Realm creation bypasses the normal registration-time consent recording, so
/// tests that log in as the newly-created admin must mirror it here to avoid
/// being blocked by the legal-consent gate (BE-D08).
async fn record_consent_for_email(ctx: &TestContext, email: &str, realm_id: &str) {
    let user_id = sqlx::query_scalar::<_, uuid::Uuid>(
        "SELECT id FROM account WHERE email = $1 AND realm_id = $2",
    )
    .bind(email)
    .bind(realm_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("Failed to find realm admin user");

    record_test_user_consent(&ctx._app_state.pool, user_id, realm_id).await;
}

/// ============================================================================
/// Scenario 1: Create Multiple Realms with Isolated Admin Access
///
/// **User Story**: Admin realm creates multiple realms, each with its own admin
///
/// **Given**: Admin realm 的管理员已登录
/// **When**:
///   1. Create realm1 with admin1
///   2. Create realm2 with admin2
///   3. Admin1 tries to access realm1 users (should succeed)
///   4. Admin1 tries to access realm2 users (should fail - realm isolation)
/// **Then**:
///   - Both realms are created successfully
///   - Each admin can only access their own realm's users
///   - Realm boundary is strictly enforced
///
/// **验收标准**:
/// - Realm 创建成功
/// - Realm 严格隔离：每个 realm 的管理员只能访问自己的 realm
/// - 跨 realm 访问返回 403 Forbidden
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_create_multiple_realms_and_access_users(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // ============================================================================
    // Setup: 创建 Admin realm 的管理员
    // ============================================================================
    println!("[Setup] 创建 Admin realm 的管理员");
    let (admin_token, _admin_user_id) =
        create_admin_session_with_user(ctx, "admin@cas.com", 1800).await;
    grant_realm_admin_role(ctx, &_admin_user_id).await;
    println!("[Setup] ✓ Admin realm 管理员会话创建成功");

    // ============================================================================
    // Step 1: 创建 realm1 (带管理员)
    // ============================================================================
    println!("[Step 1] 创建 realm1 (带管理员)");
    let timestamp = chrono::Utc::now().timestamp_millis();
    let realm1_id = format!("realm1_{}", timestamp % 1000000000);
    let realm1_name = "Test Realm 1";
    let realm1_admin_email = "admin1@realm1.com";
    let realm1_admin_password = "Password123!";

    let create_payload = json!({
        "id": realm1_id,
        "name": realm1_name,
        "adminUser": {
            "email": realm1_admin_email,
            "password": realm1_admin_password
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/realms")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::from(serde_json::to_string(&create_payload).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED, "创建 realm1 应返回 201");
    println!(
        "[Step 1] ✓ realm1 创建成功: {} ({})",
        realm1_name, realm1_id
    );
    record_consent_for_email(ctx, realm1_admin_email, &realm1_id).await;

    // ============================================================================
    // Step 2: 创建 realm2 (带管理员)
    // ============================================================================
    println!("[Step 2] 创建 realm2 (带管理员)");
    let realm2_id = format!("realm2_{}", (timestamp + 1) % 1000000000);
    let realm2_name = "Test Realm 2";
    let realm2_admin_email = "admin2@realm2.com";
    let realm2_admin_password = "Password123!";

    let create_payload = json!({
        "id": realm2_id,
        "name": realm2_name,
        "adminUser": {
            "email": realm2_admin_email,
            "password": realm2_admin_password
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/realms")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::from(serde_json::to_string(&create_payload).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED, "创建 realm2 应返回 201");
    println!(
        "[Step 2] ✓ realm2 创建成功: {} ({})",
        realm2_name, realm2_id
    );
    record_consent_for_email(ctx, realm2_admin_email, &realm2_id).await;

    // ============================================================================
    // Step 3: Mint a FirstParty token for the realm1 admin and access realm1 users
    // ============================================================================
    println!("[Step 3] 为 realm1 管理员铸造 FirstParty token 并访问 realm1 用户列表");

    // /api/users 路由要求 FirstParty token；领域服务 create_realm 已把
    // admin-web-console 置为 first-party，此处防御性再确认一次（幂等）。
    sqlx::query(
        "UPDATE client_app SET is_first_party = true WHERE realm_id = $1 AND client_id = 'admin-web-console'",
    )
    .bind(&realm1_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to mark realm1 admin-web-console as first-party");

    let realm1_admin_user_id = sqlx::query_scalar::<_, uuid::Uuid>(
        "SELECT id FROM account WHERE email = $1 AND realm_id = $2",
    )
    .bind(realm1_admin_email)
    .bind(&realm1_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("Failed to find realm1 admin user");

    let realm1_admin_user = ctx
        ._app_state
        .user_repository
        .get_user_by_id(realm1_admin_user_id)
        .await
        .expect("Failed to load realm1 admin user");
    let realm1_client_app = ctx
        ._app_state
        .service
        .client_service()
        .get_client_app_by_client_id(&realm1_id, "admin-web-console")
        .await
        .expect("Failed to load realm1 admin-web-console client app");
    let realm1_admin_token = RedisBrowserTokenService::new(ctx._app_state.redis_manager.clone())
        .create_first_party_token_family(&realm1_admin_user, &realm1_client_app, None, None)
        .await
        .expect("Failed to create FirstParty token family")
        .access_token;
    println!("[Step 3] ✓ realm1 管理员 FirstParty token 铸造成功");

    // Access realm1 users
    let req = Request::builder()
        .method("GET")
        .uri("/api/users?page=0&pageSize=20".to_string())
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", realm1_admin_token),
        )
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "realm1 管理员应能访问 realm1 用户列表"
    );
    println!("[Step 3] ✓ realm1 管理员可以访问 realm1 用户列表");

    // ============================================================================
    // Step 4: realm1 admin's user list must not contain realm2 users
    // ============================================================================
    // The realm is session-derived, so there is no path segment through
    // which realm2 could be requested; isolation is asserted at the content
    // level instead — realm2's seeded admin must never appear in realm1's list.
    println!("[Step 4] realm1 管理员获取用户列表（不应包含 realm2 用户）");

    let req = Request::builder()
        .method("GET")
        .uri("/api/users?page=0&pageSize=100".to_string())
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", realm1_admin_token),
        )
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "realm1 管理员应能访问自己 realm 的用户列表"
    );
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let list: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let items = list["items"].as_array().cloned().unwrap_or_default();
    let leaked = items.iter().any(|u| {
        u["realmId"].as_str().is_some_and(|r| r != realm1_id)
            || u["email"].as_str().is_some_and(|e| e.contains("realm2"))
    });
    assert!(
        !leaked,
        "realm1 用户列表不应包含 realm2 用户（Realm 隔离生效）"
    );
    println!("[Step 4] ✓ realm1 用户列表不含 realm2 用户（Realm 隔离生效）");

    // ============================================================================
    // Cleanup
    // ============================================================================
    println!("[Cleanup] 删除测试 Realm");
    sqlx::query("DELETE FROM realm WHERE id::text = $1")
        .bind(&realm1_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM realm WHERE id::text = $1")
        .bind(&realm2_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
    println!("[Cleanup] ✓ 测试 Realm 已删除");

    println!("\n✅ Realm 隔离测试通过！");
}

/// ============================================================================
/// Scenario 4: Admin can list all realms via paginated endpoint
///
/// **User Story**: As a super-admin in the admin realm, when I create a new
/// realm, the paginated realm list (`GET /api/realms/paginated`) must include
/// BOTH the admin realm and the newly created realm.
///
/// **Regression intent**: `list_realms_paginated` previously scoped the SQL
/// query to `WHERE realm.id = <identity.realm_id()>` even for the admin
/// identity, so the admin could only ever see their own "admin" realm — never
/// the realm they just created. This test encodes WHY that is wrong: a super
/// admin's realm list must reflect every realm in the system, mirroring the
/// non-paginated `list_realms` access semantics.
///
/// **Given**: A super-admin session in the admin realm
/// **When**:
///   1. Create a new realm
///   2. Call `GET /api/realms/paginated`
/// **Then**:
///   - `total >= 2` (admin realm + new realm, at minimum)
///   - The new realm id is present in `items`
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_admin_list_realms_paginated_includes_all_realms(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Setup: super-admin session in the admin realm
    println!("[Setup] 创建 Admin realm 管理员会话");
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "admin-paginated@cas.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;
    println!("[Setup] ✓ Admin realm 管理员会话创建成功");

    // Step 1: create a new realm
    println!("[Step 1] 创建新 Realm");
    let timestamp = chrono::Utc::now().timestamp_millis();
    let new_realm_id = format!("paglist_{}", timestamp % 1000000000);
    let create_payload = json!({
        "id": new_realm_id,
        "name": "Paginated List Test Realm",
        "adminUser": {
            "email": format!("admin@{}.com", new_realm_id),
            "password": "Password123!"
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/realms")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::from(serde_json::to_string(&create_payload).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "创建新 Realm 应返回 201"
    );
    println!("[Step 1] ✓ 新 Realm 创建成功: {}", new_realm_id);
    record_consent_for_email(ctx, &format!("admin@{}.com", new_realm_id), &new_realm_id).await;

    // Step 2: call the paginated realm list as the admin identity
    println!("[Step 2] 调用 GET /api/realms/paginated");
    let req = Request::builder()
        .method("GET")
        .uri("/api/realms/paginated?page=0&pageSize=100")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "分页列表请求应返回 200");

    let body: serde_json::Value = crate::tests::response_json(resp).await;
    let total = body["total"].as_i64().expect("响应应包含 total");

    // Intent: a super-admin must see every realm, not be scoped to its own.
    // With the bug, `accessible_realm_id = "admin"` scopes the SQL to a single
    // row, so `total` would be exactly 1 and the new realm would be missing.
    assert!(
        total >= 2,
        "admin 应至少看到 admin realm 与新建 realm 共 {} 个 (total >= 2)，实际 total = {}",
        2,
        total
    );

    let items = body["items"].as_array().expect("响应应包含 items 数组");
    let returned_ids: Vec<&str> = items
        .iter()
        .map(|item| item["id"].as_str().unwrap_or(""))
        .collect();
    assert!(
        returned_ids.iter().any(|id| *id == new_realm_id),
        "admin 分页列表应包含新建的 realm id {}，实际返回: {:?}",
        new_realm_id,
        returned_ids
    );
    println!("[Step 2] ✓ 分页列表 total = {}, 包含新建 realm", total);

    // Cleanup
    println!("[Cleanup] 删除测试 Realm");
    sqlx::query("DELETE FROM realm WHERE id::text = $1")
        .bind(&new_realm_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
    println!("[Cleanup] ✓ 测试 Realm 已删除");

    println!("\n✅ Admin 分页 Realm 列表测试通过！");
}
