// =============================================================================
// Schema 隔离测试上下文
// =============================================================================
//
// 每个测试使用独立的 PostgreSQL Schema，实现真正的数据库隔离。
// 这样可以完全并行运行测试，避免唯一约束冲突。
//
// =============================================================================

use crate::application::http::oauth::device_token::init_device_token_function;
use crate::application::http::rate_limit::init_rate_limit_function;
use crate::application::http::state::AppState;
use crate::tests::shared::SharedContainers;
use herald_core::admin::user::init_admin_user;
use herald_core::application::{ApplicationServiceBuilder, WebhookService};
use herald_core::infrastructure::PostgresCustomDomainMappingRepository;
use herald_core::infrastructure::authentication::init_authentication_functions;
use herald_core::infrastructure::authorization::{RedisCache, RedisPermissionChecker};
use herald_core::infrastructure::points::RedisIdempotencyStore;
use herald_core::infrastructure::points::init_idempotency_function;
use herald_core::infrastructure::realm_config::PostgresRealmConfigRepository;
use herald_core::infrastructure::redis::{ManagerConfig, RedisConnectionManager};
use herald_core::infrastructure::user::repositories::PostgresUserRepository;
use herald_core::infrastructure::webhook::WebhookEventRepository;
use herald_test_db::{clone_schema_from_template, create_schema_scoped_connections};
use sqlx::{PgPool, Row};
use std::sync::Arc;
use test_context::AsyncTestContext;

const SCHEMA_POOL_MAX_CONNECTIONS: u32 = 3;
const TEST_JWT_SECRET: &str = "test-jwt-secret-key-for-integration-tests-32b";

/// 确保 Redis Functions 只初始化一次
static RATE_LIMIT_INIT: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
static DEVICE_TOKEN_INIT: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
static IDEMPOTENCY_INIT: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
static AUTHENTICATION_INIT: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

/// Schema 隔离的测试上下文
///
/// 每个测试创建独立的 PostgreSQL Schema，实现数据库隔离。
/// Redis 隔离通过使用 DB 1 实现（test_mode=true），不再需要 UUID key prefix。
pub struct SchemaTestContext {
    /// 应用状态
    pub app_state: Arc<AppState>,
    /// 应用状态的别名（与旧 TestContext 兼容）
    pub _app_state: Arc<AppState>,
    /// Schema 名称
    pub schema_name: String,
    /// Realm ID
    pub _realm_id: String,
    /// Client ID (external identifier like 'admin-web-console')
    pub _client_id: String,
    /// Client App UUID (internal database ID)
    pub _client_app_id: String,
    /// 原始连接池（用于清理）
    cleanup_pool: Arc<PgPool>,
}

// Removed: CACHED_TEST_CONFIG - loading config inline is simple and predictable

impl AsyncTestContext for SchemaTestContext {
    async fn setup() -> Self {
        let _ = tracing_subscriber::fmt().try_init();
        let setup_started = std::time::Instant::now();

        // 1. 获取共享容器（连接到主数据库）
        let shared = SharedContainers::get().await;

        // 2. 生成唯一的 Schema 名称
        let schema_name = format!(
            "test_{}",
            uuid::Uuid::now_v7().to_string().replace("-", "_")
        );

        tracing::debug!("📦 创建测试 Schema: {}", schema_name);

        // 3. 从模板 Schema 克隆新 Schema，避免每个测试重复执行全量迁移
        let clone_started = std::time::Instant::now();
        clone_schema_from_template(&shared.pool, &shared.template_schema_name, &schema_name).await;
        tracing::debug!(
            schema_name = %schema_name,
            elapsed_ms = clone_started.elapsed().as_millis(),
            "Schema clone completed"
        );

        // 4. 创建带 Schema 的连接池
        let pool_started = std::time::Instant::now();
        let (pool_with_schema, sea_conn) = create_schema_scoped_connections(
            &shared.pg_host,
            shared.pg_port,
            &schema_name,
            SCHEMA_POOL_MAX_CONNECTIONS,
        )
        .await;
        tracing::debug!(
            schema_name = %schema_name,
            elapsed_ms = pool_started.elapsed().as_millis(),
            "Schema-scoped pools created"
        );

        // 6. NEW: 创建 RedisConnectionManager（测试模式自动使用 DB 1）
        // 修改 Redis URL 以直接使用 DB 1，确保测试隔离
        // 查找最后一个斜杠后的数字并替换为 1
        let redis_url_with_db = if let Some(last_slash_pos) = shared.redis_url.rfind('/') {
            let after_slash = &shared.redis_url[last_slash_pos + 1..];
            if after_slash.chars().all(|c| c.is_ascii_digit()) {
                // URL 已包含 DB 编号，替换为 /1
                format!("{}{}", &shared.redis_url[..last_slash_pos + 1], "1")
            } else {
                // URL 不包含 DB 编号，添加 /1
                format!("{}/1", shared.redis_url)
            }
        } else {
            // URL 没有斜杠，添加 /1
            format!("{}/1", shared.redis_url)
        };

        tracing::debug!(
            original_url = %shared.redis_url,
            modified_url = %redis_url_with_db,
            "Creating RedisConnectionManager with DB 1"
        );

        let redis_config = ManagerConfig {
            url: redis_url_with_db,
            default_db: 1,    // 测试环境使用 DB 1
            test_mode: false, // 禁用自动 SELECT，因为 URL 已包含 DB
            test_db: 1,
        };

        let redis_manager = Arc::new(
            RedisConnectionManager::new(redis_config)
                .await
                .expect("Failed to create RedisConnectionManager"),
        );

        // 7. 创建 RedisPermissionChecker（使用新的 RedisCache）
        let redis_cache =
            RedisCache::new((*redis_manager).clone()).expect("Failed to create Redis cache");
        let permission_checker = Arc::new(RedisPermissionChecker::new(
            Arc::new(sea_conn.clone()),
            Arc::new(tokio::sync::RwLock::new(redis_cache)),
        ));

        // 8. 构建 ApplicationService（必须在 init_admin_realm_rbac 之前）
        let application_service = ApplicationServiceBuilder::new()
            .with_database(Arc::new(sea_conn.clone()))
            .with_redis((*redis_manager).clone())
            .with_permission_checker(permission_checker.clone())
            .build()
            .expect("Failed to build ApplicationService");

        // 9. 初始化 admin realm RBAC（必须在 init_admin_user 之前）
        //    确保默认角色（realm-admin 和 user）已创建及其权限策略
        let rbac_init_service = application_service.realm_service().get_rbac_init_service();
        herald_core::admin::init_admin_realm_rbac(&pool_with_schema, rbac_init_service)
            .await
            .expect("Failed to initialize admin realm RBAC");

        // 10. 初始化测试数据（创建管理员用户）
        //    init_admin_user 会创建 admin 用户并加入已存在的 realm-admin 角色
        init_admin_user(&pool_with_schema, "test")
            .await
            .expect("Failed to initialize admin user");

        // 11. 获取 realm_id 和 client_id（用于测试）
        let realm_id: String = sqlx::query("select id from realm limit 1")
            .fetch_one(&pool_with_schema)
            .await
            .map(|x| x.get("id"))
            .expect("Failed to get realm_id");
        let (client_id, client_app_id): (String, String) = sqlx::query_as(
            "select client_id, id::text from client_app where client_id = 'admin-web-console' and realm_id = $1 limit 1"
        )
            .bind(&realm_id)
            .fetch_one(&pool_with_schema)
            .await
            .expect("Failed to get admin-web-console client_id and UUID");

        // 12. 构建 AppState
        // NEW: 使用 RedisConnectionManager（测试模式自动使用 DB 1，无需 UUID prefix）
        let user_repository = Arc::new(PostgresUserRepository::new(sea_conn.clone().into()));
        let billing_repository = Arc::new(
            herald_core::infrastructure::billing::PostgresBillingRepository::new(sea_conn.clone()),
        );

        // Create entitlement mapping service with PermissionBasedBillingPolicy for testing
        use herald_core::domain::billing::EntitlementMappingService;
        use herald_core::infrastructure::authorization::policies::PermissionBasedBillingPolicy;
        let billing_policy = PermissionBasedBillingPolicy::new(permission_checker.clone());
        let entitlement_mapping_service = Arc::new(EntitlementMappingService::new(
            billing_repository.clone(),
            Arc::new(billing_policy.clone()),
        ));
        let provider_product_sync_service = Arc::new(
            herald_core::domain::billing::ProviderProductSyncService::new(
                billing_repository.clone(),
                Arc::new(billing_policy),
                Arc::new(
                    herald_core::infrastructure::billing::ConfiguredProviderProductApi::new(
                        pool_with_schema.clone(),
                    ),
                ),
            ),
        );

        // API Key cache and repository
        let api_key_cache = herald_core::infrastructure::client_api_keys::ApiKeyCache::new(
            Arc::clone(&redis_manager),
        );
        let api_key_repo = Arc::new(
            herald_core::infrastructure::client_api_keys::ClientApiKeyRepository::new(Arc::new(
                sea_conn.clone(),
            )),
        );

        // Create admin user repositories
        use herald_core::infrastructure::user::{
            PostgresAdminUserRepository, PostgresRolePolicyRepository, PostgresUserRoleRepository,
        };
        let admin_user_repository =
            Arc::new(PostgresAdminUserRepository::new(pool_with_schema.clone()));
        let user_role_repository =
            Arc::new(PostgresUserRoleRepository::new(pool_with_schema.clone()));
        let role_policy_repository =
            Arc::new(PostgresRolePolicyRepository::new(pool_with_schema.clone()));

        // Create admin user services
        use herald_core::domain::user::services::admin::{
            AdminUserServiceImpl, PermissionManagementServiceImpl, RoleAssignmentServiceImpl,
            UserPermissionServiceImpl,
        };
        let audit_event_repository = Arc::new(
            herald_core::infrastructure::audit::PostgresAuditEventRepository::new(sea_conn.clone()),
        );
        let admin_token_service = Arc::new(
            herald_core::infrastructure::authentication::RedisBrowserTokenService::new(
                (*redis_manager).clone(),
            ),
        );
        let admin_user_service = Arc::new(AdminUserServiceImpl::new(
            admin_user_repository.clone(),
            user_role_repository.clone(),
            role_policy_repository.clone(),
            permission_checker.clone(),
            audit_event_repository.clone(),
            admin_token_service,
        ));
        let role_assignment_service = Arc::new(RoleAssignmentServiceImpl::new(
            user_role_repository.clone(),
            role_policy_repository.clone(),
            permission_checker.clone(),
        ));
        let user_permission_service = Arc::new(UserPermissionServiceImpl::new(
            user_role_repository.clone(),
            role_policy_repository.clone(),
            permission_checker.clone(),
        ));
        let permission_management_service = Arc::new(PermissionManagementServiceImpl::new(
            user_role_repository.clone(),
            role_policy_repository.clone(),
            permission_checker.clone(),
            audit_event_repository.clone(),
        ));

        let payment_attempt_repository = Arc::new(
            herald_core::infrastructure::payment_attempt::PostgresPaymentAttemptRepository::new(
                Arc::new(sea_conn.clone()),
                pool_with_schema.clone(),
            ),
        );
        let fulfillment_service = Arc::new(
            herald_core::infrastructure::purchase::PostgresFulfillmentService::new(
                Arc::new(
                    herald_core::infrastructure::points::PostgresPointsRepository::new(
                        Arc::new(sea_conn.clone()),
                        pool_with_schema.clone(),
                    ),
                ),
                billing_repository.clone(),
                payment_attempt_repository.clone(),
                user_role_repository.clone(),
                permission_checker.clone(),
            ),
        );
        let payment_attempt_service = Arc::new(
            herald_core::domain::payment_attempt::PaymentAttemptService::new(
                payment_attempt_repository.clone(),
            ),
        );
        let purchase_service =
            Arc::new(herald_core::infrastructure::purchase::PurchaseService::new(
                pool_with_schema.clone(),
                "http://localhost:8080".to_string(),
                billing_repository.clone(),
                payment_attempt_service.clone(),
                payment_attempt_repository.clone(),
                user_role_repository.clone(),
                fulfillment_service.clone(),
            ));

        let app_state = Arc::new(AppState {
            service: application_service,
            pool: pool_with_schema.clone(),
            db: Arc::new(sea_conn.clone()),
            http_client: reqwest::Client::new(),
            redis_manager: (*redis_manager).clone(), // NEW: 使用 RedisConnectionManager
            billing_repository: billing_repository.clone(),
            role_policy_repository: role_policy_repository.clone(),
            invoice_repository: Arc::new(
                herald_core::infrastructure::billing::PostgresInvoiceRepository::new(sea_conn.clone()),
            ),
            credit_note_repository: Arc::new(
                herald_core::infrastructure::billing::PostgresCreditNoteRepository::new(
                    sea_conn.clone(),
                ),
            ),
            audit_event_repository: Arc::new(
                herald_core::infrastructure::audit::PostgresAuditEventRepository::new(sea_conn.clone()),
            ),
            entitlement_mapping_service,
            provider_product_sync_service,
            public_base_url: "http://localhost:8080".to_string(),
            permission_checker: permission_checker.clone(),
            app_env: "test".to_string(),
            user_repository: user_repository.clone(),
            api_key_cache,
            api_key_repo,
            startup_time: std::time::Instant::now(),
            points_repository: Arc::new(
                herald_core::infrastructure::points::PostgresPointsRepository::new(
                    Arc::new(sea_conn.clone()),
                    pool_with_schema.clone(),
                ),
            ),
            points_service: Arc::new(herald_core::domain::points::service::PointsService::new(
                Arc::new(
                    herald_core::infrastructure::points::PostgresPointsRepository::new(
                        Arc::new(sea_conn.clone()),
                        pool_with_schema.clone(),
                    ),
                ),
                Arc::new(
                    herald_core::infrastructure::authorization::policies::PermissionBasedPointsPolicy::new(
                        permission_checker.clone(),
                    ),
                ),
            )),
            subscription_service: Arc::new(
                herald_core::domain::points::subscription_service::SubscriptionService::new(
                    Arc::new(herald_core::domain::points::service::PointsService::new(
                        Arc::new(
                            herald_core::infrastructure::points::PostgresPointsRepository::new(
                                Arc::new(sea_conn.clone()),
                                pool_with_schema.clone(),
                            ),
                        ),
                        Arc::new(
                            herald_core::infrastructure::authorization::policies::PermissionBasedPointsPolicy::new(
                                permission_checker.clone(),
                            ),
                        ),
                    )), // SubscriptionService::new() second parameter
                    Arc::new(
                        herald_core::infrastructure::points::PostgresPointsRepository::new(
                            Arc::new(sea_conn.clone()),
                            pool_with_schema.clone(),
                        ),
                    ),
                    // SubscriptionService::new() third parameter: the
                    // user-role repo backing the ImmediateCancel role revoke
                    // (BE-D05 / design §5.5).
                    user_role_repository.clone(),
                    // SubscriptionService::new() fourth parameter: the
                    // permission service backing the role-cache invalidation
                    // after a payment-driven grant (BE-T05 / design §5.3).
                    permission_checker.clone(),
                    None,
                ),
            ),
            idempotency_service: Arc::new(herald_core::domain::points::IdempotencyService::new(
                Arc::new(RedisIdempotencyStore::new(Arc::clone(&redis_manager))),
            )), // 测试环境配置幂等性服务
            webhook_service: Arc::new(WebhookService::new(Arc::new(WebhookEventRepository::new(
                pool_with_schema.clone(),
            )))),
            registration_service: Arc::new(
                herald_core::domain::points::services::RegistrationService::new(
                    Arc::new(
                        herald_core::infrastructure::points::PostgresPointsRepository::new(
                            Arc::new(sea_conn.clone()),
                            pool_with_schema.clone(),
                        ),
                    ),
                ),
            ),
            admin_user_service,
            role_assignment_service,
            user_permission_service,
            permission_management_service,
            fulfillment_service,
            payment_attempt_service,
            payment_attempt_repository,
            purchase_service,
            jwt_secret: TEST_JWT_SECRET.to_string(),
            user_role_repository,
            realm_config_repository: Arc::new(PostgresRealmConfigRepository::new(Arc::new(sea_conn.clone()))),
            legal_repository: Arc::new(
                herald_core::infrastructure::legal::PostgresLegalAgreementRepository::new(
                    sea_conn.clone(),
                ),
            ),
            user_consent_repository: Arc::new(
                herald_core::infrastructure::legal::PostgresUserConsentRepository::new(
                    sea_conn.clone(),
                ),
            ),
            legal_service: Arc::new(herald_core::domain::legal::LegalService::new(
                Arc::new(
                    herald_core::infrastructure::legal::PostgresLegalAgreementRepository::new(
                        sea_conn.clone(),
                    ),
                ),
                Arc::new(
                    herald_core::infrastructure::legal::PostgresUserConsentRepository::new(
                        sea_conn.clone(),
                    ),
                ),
                Arc::new(
                    herald_core::infrastructure::audit::PostgresAuditEventRepository::new(
                        sea_conn.clone(),
                    ),
                ),
            )),
            self_delete_service: Arc::new(
                herald_core::domain::user::services::SelfDeleteService::new(
                    Arc::new(PostgresUserRepository::new(sea_conn.clone().into())),
                    Arc::new(
                        herald_core::infrastructure::user_totp::PostgresUserTotpRepository::new(
                            Arc::new(sea_conn.clone()),
                        ),
                    ),
                    billing_repository.clone(),
                    Arc::new(
                        herald_core::infrastructure::authentication::RedisBrowserTokenService::new(
                            (*redis_manager).clone(),
                        ),
                    ),
                    Arc::new(
                        herald_core::infrastructure::audit::PostgresAuditEventRepository::new(
                            sea_conn.clone(),
                        ),
                    ),
                ),
            ),
            custom_domain_mapping_repo: Arc::new(PostgresCustomDomainMappingRepository::new(
                Arc::new(sea_conn.clone()),
            )),
            custom_domain_cname_target: String::new(),
            // Tests bypass build_app_state_with_migrations (which validates
            // non-empty); an empty key here keeps the authorize handler's
            // runtime mismatch path (any caller → 401) exercised without
            // forcing every fixture to configure a key.
            custom_domain_ask_key: String::new(),
            // Production default; scenarios that exercise the One Tap handler
            // override this via `create_unified_test_router_with_state`.
            google_jwks_url: herald_core::infrastructure::oauth::google::GoogleOAuthProvider::GOOGLE_JWKS_URL.to_string(),
            // Production default; scenarios that exercise the Apple native
            // login handler override this via
            // `create_unified_test_router_with_state`.
            apple_jwks_url:
                herald_core::infrastructure::oauth::apple::AppleOAuthProvider::JWKS_URL.to_string(),
            // Production adapter; LDAP scenarios replace it with a mock via
            // `create_unified_test_router_with_state`.
            ldap_authenticator: std::sync::Arc::new(
                herald_core::infrastructure::ldap::Ldap3Authenticator::default(),
            ),
        });

        // 13. 初始化 Redis Functions（只运行一次）
        RATE_LIMIT_INIT
            .get_or_init(|| async {
                init_rate_limit_function(&app_state)
                    .await
                    .expect("Failed to initialize Redis rate limiting functions");
            })
            .await;

        DEVICE_TOKEN_INIT
            .get_or_init(|| async {
                init_device_token_function(&app_state)
                    .await
                    .expect("Failed to initialize device token Redis Function");
            })
            .await;

        IDEMPOTENCY_INIT
            .get_or_init(|| async {
                init_idempotency_function(&app_state.redis_manager)
                    .await
                    .expect("Failed to initialize idempotency Redis Function");
            })
            .await;

        AUTHENTICATION_INIT
            .get_or_init(|| async {
                init_authentication_functions(&app_state.redis_manager)
                    .await
                    .expect("Failed to initialize authentication Redis Functions");
            })
            .await;

        tracing::debug!(
            schema_name = %schema_name,
            elapsed_ms = setup_started.elapsed().as_millis(),
            "SchemaTestContext setup completed"
        );

        Self {
            app_state: app_state.clone(),
            _app_state: app_state,
            schema_name,
            _realm_id: realm_id,
            _client_id: client_id,
            _client_app_id: client_app_id,
            cleanup_pool: shared.pool.clone(),
        }
    }

    async fn teardown(self) {
        let SchemaTestContext {
            app_state,
            _app_state,
            ref schema_name,
            cleanup_pool,
            ..
        } = self;

        // 先关闭 schema 绑定连接，避免 teardown 时持有活跃连接导致 DROP SCHEMA 超时。
        let schema_pool = app_state.pool.clone();
        let schema_db = app_state.db.clone();
        drop(_app_state);
        drop(app_state);

        // 等待连接真正关闭
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        schema_pool.close().await;
        let _ = schema_db.as_ref().clone().close().await;

        // 使用共享的 schema 清理逻辑
        if let Err(error) = cleanup_schema_if_needed(schema_name, &cleanup_pool).await {
            tracing::warn!(schema_name = %schema_name, %error, "Failed to drop test schema");
        }

        // Redis 清理
        // NEW: 测试使用 DB 1 隔离，不需要清理
        // Session keys 会在过期后自动清理
        tracing::debug!("🗑️  Redis DB 1 will be used for test isolation (no cleanup needed)");
    }
}

async fn cleanup_schema_if_needed(
    schema_name: &str,
    cleanup_pool: &PgPool,
) -> Result<(), sqlx::Error> {
    tracing::debug!("🗑️  Dropping test schema: {}", schema_name);
    sqlx::query(&format!(
        r#"DROP SCHEMA IF EXISTS "{}" CASCADE"#,
        schema_name
    ))
    .execute(cleanup_pool)
    .await?;
    Ok(())
}

/// 创建带 Schema 的连接池（sqlx 和 sea-orm）
impl SchemaTestContext {
    /// 创建统一测试路由（包含所有 API）
    ///
    /// 用于场景测试，完全复用 create_api_routes() 确保测试路由与生产路由保持一致
    pub fn create_unified_test_router(&self) -> axum::Router {
        // 所有 API 路由统一使用 AppState
        let state = (*self.app_state).clone();

        // 完全复用生产环境的 API 路由定义（OAuth, Realm Config, Auth, Permission, Client, Roles, User, Users, Realms, Billing）
        let api_routes =
            crate::application::http::server::create_api_routes(self.app_state.clone());

        api_routes.with_state(state)
    }

    /// Like [`create_unified_test_router`](Self::create_unified_test_router),
    /// but applies `override_state` to a private owned copy of the AppState
    /// that backs `with_state`.
    ///
    /// The shared `self.app_state: Arc<AppState>` is untouched, so other
    /// scenarios and other calls to `create_unified_test_router` are
    /// unaffected — no global/process-wide mutation, safe under parallel
    /// nextest runs. Intended for per-test config injection such as pointing
    /// the One Tap handler's JWKS fetch at a wiremock URL.
    pub fn create_unified_test_router_with_state(
        &self,
        override_state: impl FnOnce(&mut AppState),
    ) -> axum::Router {
        let mut state = (*self.app_state).clone();
        override_state(&mut state);
        let api_routes =
            crate::application::http::server::create_api_routes(self.app_state.clone());
        api_routes.with_state(state)
    }

    /// Insert test Creem configuration for a realm
    ///
    /// This helper method inserts test Creem configuration into the realm_config table.
    /// It's useful for testing billing functionality without relying on environment variables.
    ///
    /// # Arguments
    ///
    /// * `realm_id` - The realm ID to insert config for
    /// * `api_key` - The Creem API key (default: "test_api_key")
    /// * `webhook_secret` - The webhook secret (default: "test_webhook_secret")
    /// * `timeout` - HTTP timeout in seconds (default: 30)
    pub async fn with_creem_config(
        &self,
        realm_id: &str,
        api_key: Option<&str>,
        webhook_secret: Option<&str>,
        timeout: Option<u64>,
    ) {
        let api_key = api_key.unwrap_or("test_api_key");
        let webhook_secret = webhook_secret.unwrap_or("test_webhook_secret");
        let timeout = timeout.unwrap_or(30);

        // Insert API key config (with ON CONFLICT to handle duplicates)
        sqlx::query(
            "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, is_secret, enabled, metadata)
             VALUES ($1, 'creem', 'api_key', $2, true, true, null)
             ON CONFLICT (realm_id, config_type, config_key)
             DO UPDATE SET config_value = EXCLUDED.config_value, updated_at = CURRENT_TIMESTAMP"
        )
        .bind(realm_id)
        .bind(api_key)
        .execute(&self.app_state.pool)
        .await
        .expect("Failed to insert test Creem API key config");

        // Insert webhook secret config (with ON CONFLICT to handle duplicates)
        sqlx::query(
            "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, is_secret, enabled, metadata)
             VALUES ($1, 'creem', 'webhook_secret', $2, true, true, null)
             ON CONFLICT (realm_id, config_type, config_key)
             DO UPDATE SET config_value = EXCLUDED.config_value, updated_at = CURRENT_TIMESTAMP"
        )
        .bind(realm_id)
        .bind(webhook_secret)
        .execute(&self.app_state.pool)
        .await
        .expect("Failed to insert test Creem webhook secret config");

        // Insert timeout config (with ON CONFLICT to handle duplicates)
        sqlx::query(
            "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, is_secret, enabled, metadata)
             VALUES ($1, 'creem', 'timeout', $2, false, true, null)
             ON CONFLICT (realm_id, config_type, config_key)
             DO UPDATE SET config_value = EXCLUDED.config_value, updated_at = CURRENT_TIMESTAMP"
        )
        .bind(realm_id)
        .bind(timeout.to_string())
        .execute(&self.app_state.pool)
        .await
        .expect("Failed to insert test Creem timeout config");

        tracing::debug!(
            realm_id = %realm_id,
            "Inserted test Creem config for realm"
        );
    }
}
