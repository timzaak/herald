// =============================================================================
// 通用认证辅助函数
// =============================================================================
//
// ## 权限授予方式（基于 RBAC 角色）
//
// - `grant_realm_admin_role` - 授予 Realm Admin 角色（获得所有标准权限）
//
// ## ⚠️ 重要：Realm 严格隔离
//
// - **没有跨 Realm 访问**：所有用户只能访问自己所属的 Realm
// - **admin realm 是普通 realm**：只有 `realm.manage` 权限可以创建新 Realm
// - **权限验证在 Service 层**：不在 HTTP middleware
//
// ## 参考
// - 权限标准文档: `docs/permission-standard.md`
// - RBAC 产品文档: `docs/permission.md`
// - RBAC 初始化: `core/src/domain/rbac_init/services.rs`

#![allow(dead_code)]

use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use herald_core::domain::authentication::BrowserTokenService;
use herald_core::domain::authorization::permission_service::PermissionService;
use herald_core::domain::authorization::principal_types;
use herald_core::domain::client::ports::ClientService;
use herald_core::domain::user::UserRepository;
use herald_core::infrastructure::authentication::RedisBrowserTokenService;

/// ============================================================================
/// 会话管理
/// ============================================================================
///
/// 创建管理员会话并在数据库中创建对应的测试用户
///
/// # Arguments
/// * `ctx` - 测试上下文
/// * `email` - 用户邮箱（用作标识）
/// * `ttl_seconds` - 会话过期时间（秒）
///
/// # 返回
/// 返回 (token, user_id) 元组，其中 user_id 是有效的 UUID
///
/// # 注意
/// 此函数会在数据库中创建一个测试用户，适用于需要用户身份验证的路由。
pub async fn create_admin_session_with_user(
    ctx: &TestContext,
    email: &str,
    ttl_seconds: usize,
) -> (String, String) {
    use uuid::Uuid;

    // 检查用户是否已存在
    let existing_user: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM account WHERE email = $1 AND realm_id = $2")
            .bind(email)
            .bind(&ctx._realm_id)
            .fetch_optional(&ctx._app_state.pool)
            .await
            .unwrap();

    let user_uuid = if let Some(uuid) = existing_user {
        uuid
    } else {
        // 创建新用户
        let new_user_uuid = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO account (id, realm_id, email, password, status) VALUES ($1, $2, $3, $4, 1)"
        )
        .bind(new_user_uuid)
        .bind(&ctx._realm_id)
        .bind(email)
        .bind("$2a$12$dummy_password_hash") // 假密码哈希
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
        new_user_uuid
    };

    let user_id_str = user_uuid.to_string();
    let _ = ttl_seconds;
    let user = ctx
        ._app_state
        .user_repository
        .get_user_by_id(user_uuid)
        .await
        .unwrap();
    let client_app = ctx
        ._app_state
        .service
        .client_service()
        .get_client_app_by_client_id(&ctx._realm_id, &ctx._client_id)
        .await
        .unwrap();
    let token = RedisBrowserTokenService::new(ctx._app_state.redis_manager.clone())
        .create_first_party_token_family(&user, &client_app, None, None)
        .await
        .unwrap()
        .access_token;

    (token, user_id_str)
}

/// 为既有用户直接铸 FirstParty browser token family。
///
/// 双轨凭证类：密码登录签发的是 CustomUserUi family（scope 上限不含
/// ChangeEmail）。自助改邮箱等 FirstParty-only 端点在测试里通过该 helper
/// 取得 FirstParty 会话（生产路径是各直登/换客户端入口）。
pub async fn mint_first_party_session(ctx: &TestContext, user_id: uuid::Uuid) -> String {
    let user = ctx
        ._app_state
        .user_repository
        .get_user_by_id(user_id)
        .await
        .unwrap();
    let client_app = ctx
        ._app_state
        .service
        .client_service()
        .get_client_app_by_client_id(&ctx._realm_id, &ctx._client_id)
        .await
        .unwrap();
    RedisBrowserTokenService::new(ctx._app_state.redis_manager.clone())
        .create_first_party_token_family(&user, &client_app, None, None)
        .await
        .unwrap()
        .access_token
}

/// ============================================================================
/// 基于 RBAC 角色系统的权限授予
/// ============================================================================
///
/// 创建管理员会话（简化版）
///
/// **返回**: token (String)
///
pub async fn create_admin_session(ctx: &TestContext, email: &str, ttl_seconds: usize) -> String {
    let (token, _user_id) = create_admin_session_with_user(ctx, email, ttl_seconds).await;
    token
}

/// 授予 Realm Admin 角色（通过 user_roles 表）
///
/// 将用户加入 realm-admin 角色，自动获得标准权限：
/// - realm.view, dashboard.view, audit.view
/// - users.view, users.manage
/// - clients.view, clients.manage
/// - roles.view, roles.manage
/// - permissions.view, permissions.manage
/// - policies.view, policies.manage
/// - settings.view, settings.manage
/// - api_keys.view, api_keys.manage
/// - billing.view, billing.manage
/// - points.view, points.manage
///
/// **特殊权限（仅 admin realm）**：
/// - realm.manage (仅当 realm_id == "admin" 时授予)
///
/// **参数**:
/// - `ctx`: 测试上下文
/// - `user_id`: 用户 ID
///
/// **参考**:
/// - RBAC 初始化: `core/src/domain/rbac_init/services.rs` 第 75-265 行
/// - 权限标准文档: `docs/permission-standard.md` 第 52-66 行
///
/// **示例**:
/// ```no_run
/// # use crate::tests::helpers::auth_helpers::grant_realm_admin_role;
/// # async fn test(ctx: &mut TestContext) {
/// let (token, user_id) = create_admin_session_with_user(ctx, "admin@test.com", 1800).await;
/// grant_realm_admin_role(ctx, &user_id).await;
/// // 用户现在拥有 Realm Admin 的所有权限
/// // （如果是 admin realm，还包括创建 Realm 的权限）
/// # }
/// ```
pub async fn grant_realm_admin_role(ctx: &TestContext, user_id: &str) {
    // 检查 realm-admin 角色是否存在，不存在则创建
    let realm_admin_role_id: String = match sqlx::query_scalar(
        "SELECT id::text FROM roles WHERE realm_id = $1 AND name = 'realm-admin'",
    )
    .bind(&ctx._realm_id)
    .fetch_optional(&ctx._app_state.pool)
    .await
    .unwrap()
    {
        Some(id) => {
            tracing::debug!(realm_id = %ctx._realm_id, "realm-admin role exists: {}", id);
            id
        }
        None => {
            tracing::debug!(realm_id = %ctx._realm_id, "realm-admin role not found, creating it");
            // 创建 realm-admin 角色
            let role_uuid = uuid::Uuid::now_v7();
            sqlx::query(
                "INSERT INTO roles (id, name, description, realm_id, client_id, is_builtin)
                 VALUES ($1, 'realm-admin', 'Realm Administrator with full access to this realm', $2, $3, false)"
            )
            .bind(role_uuid)
            .bind(&ctx._realm_id)
            .bind(&ctx._client_id)
            .execute(&ctx._app_state.pool)
            .await
            .expect("Failed to create realm-admin role");
            let role_id = role_uuid.to_string();

            // 为 realm-admin 角色添加标准权限（统一 action: view, manage）
            // 注意：只有 admin realm 才有 realm.manage 权限
            let mut permissions = vec![
                ("realm", "view"),
                ("dashboard", "view"),
                ("audit", "view"),
                ("users", "view"),
                ("users", "manage"),
                ("clients", "view"),
                ("clients", "manage"),
                ("roles", "view"),
                ("roles", "manage"),
                ("permissions", "view"),
                ("permissions", "manage"),
                ("policies", "view"),
                ("policies", "manage"),
                ("settings", "view"),
                ("settings", "manage"),
                ("api_keys", "view"),
                ("api_keys", "manage"),
                ("billing", "view"),
                ("billing", "manage"),
                ("points", "view"),
                ("points", "manage"),
            ];

            // 只有 admin realm 才有管理 Realm 的权限
            if ctx._realm_id == "admin" {
                permissions.push(("realm", "manage"));
            }

            // 为每个权限添加策略到 role_policies 表
            for (resource, action) in &permissions {
                let policy_id = uuid::Uuid::now_v7();
                let role_uuid =
                    uuid::Uuid::parse_str(&role_id).expect("Failed to parse role_id as UUID");
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
                .expect("Failed to add policy for realm-admin role");
            }

            let permission_count = if ctx._realm_id == "admin" { 23 } else { 22 };
            tracing::debug!(
                realm_id = %ctx._realm_id,
                role_id = %role_id,
                "Created realm-admin role with {} standard permissions{}",
                permission_count,
                if ctx._realm_id == "admin" { " (including realm.manage)" } else { "" }
            );

            role_id
        }
    };

    // 通过 user_roles 表将用户加入角色
    let user_role_id = uuid::Uuid::now_v7();
    let user_uuid = uuid::Uuid::parse_str(user_id).expect("Failed to parse user_id as UUID");
    let role_uuid =
        uuid::Uuid::parse_str(&realm_admin_role_id).expect("Failed to parse role_id as UUID");
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
    .expect("Failed to add user to realm-admin role");

    // Invalidate cache
    let _ = ctx
        ._app_state
        .permission_checker
        .invalidate_user_role_cache(&ctx._realm_id, user_id)
        .await;

    tracing::debug!(
        user_id = %user_id,
        realm_id = %ctx._realm_id,
        role_id = %realm_admin_role_id,
        "Granted realm-admin role to user"
    );
}

/// 授予 points-viewer 角色（仅 `points.view`，**不含** `points.manage`）。
///
/// 与 `grant_realm_admin_role` 同构，但角色只绑定单条策略 `(points, view)`，
/// 用于构造"普通用户"（非 admin）测试身份：能查询自己的钱包/交易，但无跨用户
/// 管理权限。角色创建幂等（不存在则建，存在则复用）；用户绑定走 `user_roles`
/// 的 `ON CONFLICT DO NOTHING`。授予后失效该用户的角色缓存，确保当次会话立即生效。
pub async fn grant_points_view_role(ctx: &TestContext, user_id: &str) {
    // 复用或创建 points-viewer 角色
    let role_id: String = match sqlx::query_scalar(
        "SELECT id::text FROM roles WHERE realm_id = $1 AND name = 'points-viewer'",
    )
    .bind(&ctx._realm_id)
    .fetch_optional(&ctx._app_state.pool)
    .await
    .unwrap()
    {
        Some(id) => id,
        None => {
            let role_uuid = uuid::Uuid::now_v7();
            sqlx::query(
                "INSERT INTO roles (id, name, description, realm_id, client_id, is_builtin)
                 VALUES ($1, 'points-viewer', 'Points viewer (points.view only)', $2, $3, false)",
            )
            .bind(role_uuid)
            .bind(&ctx._realm_id)
            .bind(&ctx._client_id)
            .execute(&ctx._app_state.pool)
            .await
            .expect("Failed to create points-viewer role");

            // 关键：只授予 (points, view)，不含 (points, manage)
            let policy_id = uuid::Uuid::now_v7();
            sqlx::query(
                "INSERT INTO role_policies (id, role_id, realm_id, resource, action)
                 VALUES ($1, $2, $3, 'points', 'view')",
            )
            .bind(policy_id)
            .bind(role_uuid)
            .bind(&ctx._realm_id)
            .execute(&ctx._app_state.pool)
            .await
            .expect("Failed to add points.view policy for points-viewer role");

            role_uuid.to_string()
        }
    };

    // 将用户加入 points-viewer 角色
    let user_uuid = uuid::Uuid::parse_str(user_id).expect("Failed to parse user_id as UUID");
    let role_uuid = uuid::Uuid::parse_str(&role_id).expect("Failed to parse role_id as UUID");
    sqlx::query(
        "INSERT INTO user_roles (id, user_id, role_id, realm_id, client_id, principal_type, principal_id)
         VALUES ($1, $2, $3, $4, $5, $6, $2::text)
         ON CONFLICT DO NOTHING",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(user_uuid)
    .bind(role_uuid)
    .bind(&ctx._realm_id)
    .bind(&ctx._client_id)
    .bind(principal_types::USER)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to add user to points-viewer role");

    // 失效权限缓存，确保本次会话立即生效
    let _ = ctx
        ._app_state
        .permission_checker
        .invalidate_user_role_cache(&ctx._realm_id, user_id)
        .await;

    tracing::debug!(
        user_id = %user_id,
        realm_id = %ctx._realm_id,
        role_id = %role_id,
        "Granted points-viewer role (points.view only) to user"
    );
}

/// ============================================================================
/// 权限检查辅助函数
/// ============================================================================
/// 简化版权限检查请求
///
/// 向 `/api/permission/check` 端点发送请求，自动添加 client_id
///
/// # Arguments
/// * `ctx` - 测试上下文
/// * `token` - 用户会话令牌
/// * `resource` - 资源名称（如 "users"）
/// * `action` - 操作名称（如 "view", "manage"）
///
/// # 返回
/// 返回权限检查结果的 JSON 响应
///
/// # 示例
/// ```no_run
/// # use crate::tests::helpers::auth_helpers::check_permission;
/// # async fn test(ctx: &mut TestContext) {
/// let (token, _) = create_admin_session_with_user(ctx, "user@test.com", 1800).await;
/// let result = check_permission(ctx, &token, "users", "view").await;
/// assert_eq!(result["allowed"], true);
/// # }
/// ```
pub async fn check_permission(
    ctx: &TestContext,
    token: &str,
    resource: &str,
    action: &str,
) -> serde_json::Value {
    use axum::body::Body;
    use axum::http::Request;
    use serde_json::json;
    use tower::ServiceExt;

    let app = ctx.create_unified_test_router();
    let req_body = json!({
        "token": token,
        "clientId": ctx._client_id.clone(),
        "rules": [{
            "resource": resource,
            "action": action
        }]
    })
    .to_string();

    let req = Request::builder()
        .method("POST")
        .uri("/api/permission/check")
        // /api/permission/check is self-introspection: the caller must present
        // the same token it is probing via the Authorization header.
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(req_body))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    body.get("data").cloned().unwrap_or(body)
}

/// ============================================================================
/// Reauth 票据（高风险自助操作二次认证）
/// ============================================================================
///
/// 高风险自助端点（删除账号、TOTP/passkey 管理、换邮箱、改密码）不接受明文
/// 密码，而是要求携带一次性 reauth ticket（由 `/api/user/reauth/verify`
/// 用密码等因子换取，targetOperation 必须与目标操作匹配）。
///
/// # Arguments
/// * `ctx` - 测试上下文
/// * `session_token` - 用户 Bearer access token
/// * `target_operation` - TargetOperation 的 snake_case 形式（如 "delete_account"）
/// * `password` - 用户明文密码
///
/// # 返回
/// 一次性 reauthToken 字符串
pub async fn obtain_reauth_token(
    ctx: &TestContext,
    session_token: &str,
    target_operation: &str,
    password: &str,
) -> String {
    let resp = attempt_reauth_verify(ctx, session_token, target_operation, password).await;
    assert_eq!(
        resp.status(),
        axum::http::StatusCode::OK,
        "reauth verify must succeed for target '{target_operation}'"
    );
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    body["reauthToken"]
        .as_str()
        .expect("reauth verify should return reauthToken")
        .to_owned()
}

/// 尝试用密码换取 reauth ticket，返回原始响应（用于断言失败场景的负向测试）。
pub async fn attempt_reauth_verify(
    ctx: &TestContext,
    session_token: &str,
    target_operation: &str,
    password: &str,
) -> axum::response::Response {
    use axum::body::Body;
    use axum::http::{Request, header};
    use serde_json::json;
    use tower::ServiceExt;

    let app = ctx.create_unified_test_router();
    let payload = json!({
        "targetOperation": target_operation,
        "factor": "password",
        "password": password,
    });
    let req = Request::builder()
        .method("POST")
        .uri("/api/user/reauth/verify")
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {session_token}"))
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(payload.to_string()))
        .unwrap();

    app.oneshot(req).await.unwrap()
}
