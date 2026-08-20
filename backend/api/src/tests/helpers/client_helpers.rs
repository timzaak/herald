// =============================================================================
// Client API 测试辅助函数
// =============================================================================
//
// 提供客户端 API 密钥管理相关的测试辅助函数，包括：
// - 创建测试 API Key
// - 清理缓存
// - 查询 API Key 统计
//
// =============================================================================

use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use chrono::{Duration, Utc};
use herald_core::domain::authorization::principal_types;
use herald_core::domain::client_api_keys::entities::ClientApiKey;
use herald_core::domain::client_api_keys::services::ClientApiKeyService;
use uuid::Uuid;

/// ============================================================================
// Client API Key 辅助函数
/// ============================================================================
///
/// 创建测试用 API Key
///
/// # Arguments
/// * `ctx` - 测试上下文
/// * `name` - API Key 名称
/// * `enabled` - 是否启用
/// * `expires_at` - 过期时间（None 表示永不过期）
///
/// # Returns
/// 返回 (api_key_plaintext, api_key_entity) 元组
///
/// # Example
/// ```rust,no_run
/// let (api_key, entity) = create_test_api_key(ctx, "Test Key", true, None).await;
/// // 使用 api_key 调用 API
/// // entity 包含数据库中的完整信息
/// ```
pub async fn create_test_api_key(
    ctx: &TestContext,
    name: &str,
    enabled: bool,
    expires_at: Option<chrono::DateTime<Utc>>,
) -> (String, ClientApiKey) {
    // 1. 生成 API Key (UUID v7)
    let api_key_plaintext = ClientApiKeyService::generate_api_key();

    // 2. 哈希 API Key
    let api_key_hash = ClientApiKeyService::hash_api_key(&api_key_plaintext);

    // 3. 创建数据库记录
    let id = Uuid::now_v7();
    let api_key_entity = ClientApiKey {
        id: id.to_string(),
        name: name.to_string(),
        api_key_hash,
        realm_id: ctx._realm_id.clone(),
        client_app_id: None,
        enabled,
        expires_at,
        created_at: Utc::now(),
        last_used_at: None,
    };

    sqlx::query(
        r#"
        INSERT INTO client_api_keys (id, name, api_key_hash, realm_id, client_app_id, enabled, expires_at, created_at, last_used_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(Uuid::parse_str(&api_key_entity.id).expect("Failed to parse API key ID as UUID"))
    .bind(&api_key_entity.name)
    .bind(&api_key_entity.api_key_hash)
    .bind(&api_key_entity.realm_id)
    .bind(api_key_entity.client_app_id)
    .bind(api_key_entity.enabled)
    .bind(api_key_entity.expires_at)
    .bind(api_key_entity.created_at)
    .bind(api_key_entity.last_used_at)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to create API key");

    (api_key_plaintext, api_key_entity)
}

/// 创建带权限的测试用户（简化版）
///
/// # Arguments
/// * `ctx` - 测试上下文
/// * `email` - 用户邮箱
/// * `permissions` - 权限列表（格式：[(resource, action), ...]）
///
/// # Returns
/// 返回 (user_id, session_token) 元组
///
/// # Example
/// ```rust,no_run
/// let (user_id, token) = create_test_user_with_permissions(
///     ctx,
///     "test@example.com",
///     &[("article", "read"), ("article", "write")]
/// ).await;
/// ```
pub async fn create_test_user_with_permissions(
    ctx: &TestContext,
    email: &str,
    permissions: &[(&str, &str)],
) -> (String, String) {
    use herald_core::domain::authentication::BrowserTokenService;
    use herald_core::domain::client::ports::ClientService;
    use herald_core::domain::user::UserRepository;
    use herald_core::infrastructure::authentication::RedisBrowserTokenService;

    // 1. 创建用户
    let user_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status) VALUES ($1, $2, $3, $4, 1)",
    )
    .bind(user_id)
    .bind(&ctx._realm_id)
    .bind(email)
    .bind("$2a$12$dummy_password_hash") // 假密码哈希
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to create user");

    // 2. 创建测试角色
    let role_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO roles (id, name, realm_id, client_id, is_builtin) VALUES ($1, $2, $3, $4, false)"
    )
    .bind(role_id)
    .bind(format!("test-role-{}", email))
    .bind(&ctx._realm_id)
    .bind(&ctx._client_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to create role");

    // 3. 授予角色权限（插入 role_policies 表）
    for (resource, action) in permissions {
        if let Err(e) = sqlx::query(
            "INSERT INTO role_policies (id, role_id, realm_id, resource, action) VALUES ($1, $2, $3, $4, $5)"
        )
        .bind(Uuid::now_v7())
        .bind(role_id)
        .bind(&ctx._realm_id)
        .bind(resource)
        .bind(action)
        .execute(&ctx._app_state.pool)
        .await
        {
            tracing::warn!("Failed to grant permission: {:?}", e);
        }
    }

    // 4. 将用户分配到角色（插入 user_roles 表）
    sqlx::query(
        "INSERT INTO user_roles (id, user_id, role_id, realm_id, client_id, principal_type, principal_id) VALUES ($1, $2, $3, $4, $5, $6, $2::text)"
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(role_id)
    .bind(&ctx._realm_id)
    .bind(&ctx._client_id)
    .bind(principal_types::USER)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to assign role to user");

    // 5. 为该用户签发 FirstParty Bearer token
    let user = ctx
        ._app_state
        .user_repository
        .get_user_by_id(user_id)
        .await
        .expect("Failed to load test user");
    let client_app = ctx
        ._app_state
        .service
        .client_service()
        .get_client_app_by_client_id(&ctx._realm_id, &ctx._client_id)
        .await
        .expect("Failed to load test client app");
    let session_token = RedisBrowserTokenService::new(ctx._app_state.redis_manager.clone())
        .create_first_party_token_family(&user, &client_app, None, None)
        .await
        .expect("Failed to create FirstParty token family")
        .access_token;

    (user_id.to_string(), session_token)
}

/// 创建测试订阅（第三方 API 专用）
///
/// # Arguments
/// * `ctx` - 测试上下文
/// * `client_app_id` - 客户端应用 ID
/// * `status` - 订阅状态 ("active", "canceled", "expired", "trialing")
/// * `entitlement_key` - 权益标识 (e.g. "professional", "starter")
/// * `plan_name` - 方案名称（未使用，保留签名兼容）
///
/// # Returns
/// 返回 subscription_id
///
/// # Note
/// 此函数命名为 `create_third_party_test_subscription` 以避免与
/// `billing_helpers::create_test_subscription` 冲突。
///
/// # Example
/// ```rust,no_run
/// let subscription_id = create_third_party_test_subscription(
///     ctx,
///     &client_app_id,
///     "active",
///     "professional",
///     None
/// ).await;
/// ```
pub async fn create_third_party_test_subscription(
    ctx: &TestContext,
    client_app_id: &uuid::Uuid,
    status: &str,
    entitlement_key: &str,
    _plan_name: Option<&str>,
) -> String {
    let subscription_id = Uuid::now_v7();
    let user_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status)
         VALUES ($1, $2, $3, '$2a$12$dummy_password_hash', 1)",
    )
    .bind(user_id)
    .bind(&ctx._realm_id)
    .bind(format!("thirdparty-sub-owner-{}@test.com", user_id))
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to create subscription owner");

    // subscription.bucket_id was removed by the distribution-rules refactor;
    // grant routing is configured via distribution rules.
    // Note: external_product_id is required by the schema but not used for third-party API tests
    sqlx::query(
        r#"
        INSERT INTO subscription
            (id, realm_id, user_id, external_subscription_id, external_product_id, payment_provider,
             client_app_id, status, entitlement_key, current_period_start, current_period_end,
             created_at, updated_at, billing_type)
        VALUES ($1, $2, $3, $4, $5, 'creem', $6, $7, $8, $9, $10, $11, $11, 'recurring')
        "#,
    )
    .bind(subscription_id)
    .bind(&ctx._realm_id)
    .bind(user_id)
    .bind(format!("ext-sub-thirdparty-{}", subscription_id))
    .bind("test_product_dummy")
    .bind(*client_app_id)
    .bind(status)
    .bind(entitlement_key)
    .bind(Utc::now())
    .bind(Utc::now() + Duration::days(30))
    .bind(Utc::now())
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to create subscription");

    subscription_id.to_string()
}

/// 清空 API Key 缓存
///
/// 删除 Redis 中的所有 API Key 缓存（用于测试缓存未命中场景）
///
/// # Example
/// ```rust,no_run
/// clear_api_key_cache(ctx).await;
/// // 后续请求将查询数据库
/// ```
pub async fn clear_api_key_cache(ctx: &TestContext) {
    // 删除所有 api_key:* 键
    let redis_manager = ctx._app_state.redis_manager.clone();
    let mut conn = redis_manager
        .get()
        .await
        .expect("Failed to get Redis connection");

    // 使用 SCAN 查找所有 api_key: 键
    let mut keys = Vec::new();
    let mut cursor = 0;
    loop {
        let (next_cursor, batch_keys): (u64, Vec<String>) = redis::cmd("SCAN")
            .arg(cursor)
            .arg("MATCH")
            .arg("api_key:*")
            .arg("COUNT")
            .arg(100)
            .query_async(&mut conn)
            .await
            .expect("Failed to scan keys");
        keys.extend(batch_keys);
        cursor = next_cursor;
        if cursor == 0 {
            break;
        }
    }

    // 删除所有找到的键
    if !keys.is_empty() {
        let _: () = redis::cmd("DEL")
            .arg(keys.as_slice())
            .query_async(&mut conn)
            .await
            .expect("Failed to delete keys");
    }
}

/// 禁用 API Key
///
/// 将 API Key 的 enabled 字段设置为 false 并删除缓存
///
/// # Arguments
/// * `ctx` - 测试上下文
/// * `api_key_id` - API Key ID
/// * `api_key_plaintext` - 明文 API Key（用于删除缓存）
///
/// # Example
/// ```rust,no_run
/// disable_api_key(ctx, &api_key_id, &api_key_plaintext).await;
/// // 后续请求将返回 401 api_key_disabled
/// ```
pub async fn disable_api_key(ctx: &TestContext, api_key_id: &str, api_key_plaintext: &str) {
    // 更新数据库
    sqlx::query("UPDATE client_api_keys SET enabled = false WHERE id = $1")
        .bind(api_key_id)
        .execute(&ctx._app_state.pool)
        .await
        .expect("Failed to disable API key");

    // 删除缓存（缓存 key 是 API key 的 SHA-256 摘要，与认证中间件一致）
    tracing::debug!(
        api_key_id = %api_key_id,
        api_key_length = api_key_plaintext.len(),
        "Disabling API key and deleting from cache"
    );

    let cache_key = ClientApiKeyService::hash_api_key(api_key_plaintext);
    ctx._app_state
        .api_key_cache
        .delete(&cache_key)
        .await
        .expect("Failed to delete API key from cache");

    tracing::debug!("API key cache deletion completed");
}

/// 删除 API Key（使用明文 Key 删除缓存）
///
/// 从数据库中删除 API Key 并使用明文 Key 删除缓存
///
/// # Arguments
/// * `ctx` - 测试上下文
/// * `api_key_id` - API Key ID
/// * `api_key_plaintext` - 明文 API Key（用于删除缓存）
///
/// # Example
/// ```rust,no_run
/// delete_api_key_with_plaintext(ctx, &api_key_id, &api_key_plaintext).await;
/// // 后续请求将返回 401 invalid_api_key
/// ```
pub async fn delete_api_key_with_plaintext(
    ctx: &TestContext,
    api_key_id: &str,
    api_key_plaintext: &str,
) {
    // 删除数据库记录
    sqlx::query("DELETE FROM client_api_keys WHERE id::text = $1")
        .bind(api_key_id)
        .execute(&ctx._app_state.pool)
        .await
        .expect("Failed to delete API key");

    // 删除缓存（缓存 key 是 API key 的 SHA-256 摘要，与认证中间件一致）
    let cache_key = ClientApiKeyService::hash_api_key(api_key_plaintext);
    ctx._app_state
        .api_key_cache
        .delete(&cache_key)
        .await
        .expect("Failed to delete API key from cache");
}

/// 查询 API Key 使用统计
///
/// # Arguments
/// * `ctx` - 测试上下文
/// * `api_key_id` - API Key ID
///
/// # Returns
/// 返回 last_used_at（None = 从未使用）
///
/// # Example
/// ```rust,no_run
/// let last_used_at = get_api_key_stats(ctx, &api_key_id).await;
/// assert!(last_used_at.is_some());
/// ```
pub async fn get_api_key_stats(
    ctx: &TestContext,
    api_key_id: &str,
) -> Option<chrono::DateTime<Utc>> {
    // usage_count column removed; this helper now returns only last_used_at.
    let row: (Option<chrono::DateTime<Utc>>,) =
        sqlx::query_as("SELECT last_used_at FROM client_api_keys WHERE id::text = $1")
            .bind(api_key_id)
            .fetch_one(&ctx._app_state.pool)
            .await
            .expect("Failed to query API key last_used_at");

    row.0
}
