// =============================================================================
// Herald API Library - Public Interface
// =============================================================================
//
// Exports the API server functionality for use by the app crate
// and test support for integration testing
//
// =============================================================================

use anyhow::Result;
use clap::Parser;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

pub mod application;
pub mod config;
pub mod observability;

// Test support - only included when building tests
#[cfg(test)]
mod tests;

// Re-export the server module to expose create_api_routes for testing
pub use application::http::server::create_api_routes;

// Re-export auth util for test support

// Re-export AppState and WebhookEventProcessorImpl for assembly in main.rs
pub use application::http::state::AppState;
// Re-export RealIpConfig so callers that assemble the production router
// (the app crate, test-support's create_cors_test_router) can construct a
// default without depending on herald_api_base directly.
pub use herald_api_base::application::http::real_ip::RealIpConfig;
pub use herald_api_billing::WebhookEventProcessorImpl;

use application::http::oauth::device_token::init_device_token_function;
use application::http::rate_limit::init_rate_limit_function;
use application::http::server;
use config::ApiConfig;
use herald_core::admin::rbac::init_admin_realm_rbac;
use herald_core::admin::user::init_admin_user;
use herald_core::application::WebhookService;
use herald_core::domain::billing;
use herald_core::domain::legal::LegalService;
use herald_core::domain::payment_attempt;
use herald_core::domain::points;
use herald_core::domain::user::services::SelfDeleteService;
use herald_core::domain::user::services::admin::{
    AdminUserServiceImpl, PermissionManagementServiceImpl, RoleAssignmentServiceImpl,
    UserPermissionServiceImpl,
};
use herald_core::infrastructure::PostgresCustomDomainMappingRepository;
use herald_core::infrastructure::audit::PostgresAuditEventRepository;
use herald_core::infrastructure::authentication::{
    RedisBrowserTokenService, init_authentication_functions,
};
use herald_core::infrastructure::authorization::{
    RedisCache, RedisPermissionChecker,
    policies::{PermissionBasedBillingPolicy, PermissionBasedPointsPolicy},
};
use herald_core::infrastructure::billing::{
    ConfiguredProviderProductApi, PostgresBillingRepository, PostgresCreditNoteRepository,
    PostgresInvoiceRepository,
};
use herald_core::infrastructure::client_api_keys::{ApiKeyCache, ClientApiKeyRepository};
use herald_core::infrastructure::legal::{
    PostgresLegalAgreementRepository, PostgresUserConsentRepository,
};
use herald_core::infrastructure::payment_attempt::PostgresPaymentAttemptRepository;
use herald_core::infrastructure::points::PostgresPointsRepository;
use herald_core::infrastructure::points::init_idempotency_function;
use herald_core::infrastructure::purchase::{PostgresFulfillmentService, PurchaseService};
use herald_core::infrastructure::realm_config::PostgresRealmConfigRepository;
use herald_core::infrastructure::redis::{ManagerConfig, RedisConnectionManager};
use herald_core::infrastructure::user::repositories::PostgresUserRepository;
use herald_core::infrastructure::user::{
    PostgresAdminUserRepository, PostgresRolePolicyRepository, PostgresUserRoleRepository,
};
use herald_core::infrastructure::user_totp::PostgresUserTotpRepository;
use herald_core::infrastructure::webhook::WebhookEventRepository;

/// Herald API Server
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Export OpenAPI JSON to the specified file and exit
    #[arg(long)]
    export_openapi: Option<PathBuf>,
}

// =============================================================================
// Export main function for app crate
// =============================================================================

/// Run the Herald API server with default configuration
///
/// This function loads configuration from the HERALD_CONFIG environment variable
/// or defaults to "config/config.toml", then starts the API server.
///
/// For use by the app crate.
pub async fn run() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Export OpenAPI JSON if requested
    if let Some(output_path) = args.export_openapi {
        return export_openapi(&output_path);
    }

    // Load configuration from file
    let config_path = env::var("HERALD_CONFIG").unwrap_or("config/config.toml".to_owned());
    let config = config::ApiConfig::load(&config_path)?;

    run_with_config(config).await
}

/// Run the API server with the given configuration
///
/// This is the main entry point for starting the API server.
/// It initializes all services and starts the HTTP server.
pub async fn run_with_config(config: ApiConfig) -> Result<()> {
    // Initialize tracing with pretty text logging
    // Note: Tracing is already initialized by main.rs, so we skip initialization here
    // to avoid duplicate subscriber registration errors.

    tracing::info!("Starting Herald API Server");
    tracing::info!("Bind address: {}", config.server.bind_address);
    tracing::info!("Frontend URL: {}", config.frontend.url);
    if let Some(ref dir) = config.frontend.static_dir {
        tracing::info!("Static files directory: {}", dir);
    }

    let state = build_app_state(&config).await?;

    start_server(state, config).await
}

/// Build the application state from the given configuration.
///
/// Connects to database and Redis, initializes all services, runs migrations,
/// and returns an `Arc<AppState>` ready for use by both the API server and
/// background workers.
pub async fn build_app_state(config: &ApiConfig) -> Result<Arc<AppState>> {
    build_app_state_with_migrations(config, sqlx::migrate!("../app/migrations")).await
}

/// Build the application state with a specific migration source.
///
/// Used by `build_app_state` with the default migrations and by the app crate
/// which may use a different migration path.
pub async fn build_app_state_with_migrations(
    config: &ApiConfig,
    migrations: sqlx::migrate::Migrator,
) -> Result<Arc<AppState>> {
    // Fail-fast config validation that must hold before any service is wired.
    //
    // Custom-domain ask shared key: design §4.2.2 mandates "Herald 启动期校验
    // 非空". An empty `ask_key` would make the Caddy On-Demand TLS ask gate
    // authorize nothing (every call returns 401) or, worse, if the runtime
    // check were skipped, accept any caller — either is a misconfiguration that
    // is cheapest to catch here. This is the production-only server-build path;
    // unit/integration tests construct `AppState` directly and bypass this
    // check, so dev/test fixtures without a configured key are unaffected.
    if config.custom_domain.ask_key.trim().is_empty() {
        return Err(anyhow::anyhow!(
            "Configuration error: [custom_domain].ask_key must be set to a non-empty shared \
             secret. It gates the Caddy On-Demand TLS ask endpoint \
             (GET /api/internal/custom-domain/authorize via the X-Herald-Ask-Key header). \
             See design §4.2.2."
        ));
    }

    // Custom-domain CNAME target: PRD §5.1 mandates the system show Realm
    // Admins the Herald hostname they must CNAME to. An empty `cname_target`
    // would make the config endpoint return an empty CNAME guidance, leaving
    // admins unable to complete domain setup. Mirror the ask_key fail-fast.
    if config.custom_domain.cname_target.trim().is_empty() {
        return Err(anyhow::anyhow!(
            "Configuration error: [custom_domain].cname_target must be set to the Herald \
             hostname that Realm Admins should point their custom domain's CNAME record at. \
             See PRD §5.1 / design §4.2."
        ));
    }

    // Connect to database with pool tuning from config
    let mut connect_options = sea_orm::ConnectOptions::new(&config.database.url);
    connect_options
        .max_connections(config.database.max_connections)
        .acquire_timeout(std::time::Duration::from_secs(
            config.database.acquire_timeout_secs,
        ))
        .idle_timeout(std::time::Duration::from_secs(
            config.database.idle_timeout_secs,
        ))
        .max_lifetime(std::time::Duration::from_secs(
            config.database.max_lifetime_secs,
        ))
        .connect_timeout(std::time::Duration::from_secs(
            config.database.connect_timeout_secs,
        ))
        // Observability: enable sqlx statement logging + slow-statement
        // warning at the configured threshold. Slow-statement logging emits the
        // SQL statement via tracing; sensitive value governance is handled at the
        // tracing layer. With traces-off baseline, DB
        // spans produced here are not exported.
        .sqlx_logging(true)
        .sqlx_slow_statements_logging_settings(
            log::LevelFilter::Warn,
            std::time::Duration::from_millis(config.observability.sqlx_slow_statement_ms),
        );
    let db: sea_orm::DatabaseConnection = sea_orm::Database::connect(connect_options).await?;
    tracing::info!(
        "Connected to database (max_connections: {}, acquire_timeout: {}s)",
        config.database.max_connections,
        config.database.acquire_timeout_secs
    );

    // Run database migrations
    let pg_pool = db.get_postgres_connection_pool();
    migrations.run(pg_pool).await?;
    info!("Database migrations completed");

    // Clone pg_pool for use in repository
    let sqlx_pool = pg_pool.clone();

    // Connect to Redis and create ConnectionManager with DB isolation
    // Redis DB is specified in the configuration file (redis.url)
    let redis_config = ManagerConfig {
        url: config.redis.url.clone(),
        default_db: 0,
        test_mode: config.server.app_env == "test",
        test_db: 1,
    };

    let redis_manager = RedisConnectionManager::new(redis_config)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create Redis manager: {}", e))?;

    // Health check
    redis_manager
        .health_check()
        .await
        .map_err(|e| anyhow::anyhow!("Redis health check failed: {}", e))?;

    // Extract DB number from Redis URL for logging
    let redis_db = config
        .redis
        .url
        .split('/')
        .next_back()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);

    info!(
        "Connected to Redis with RedisConnectionManager (DB: {})",
        redis_db
    );

    // Initialize PermissionChecker with Redis cache
    let redis_cache = RedisCache::new(redis_manager.clone())
        .map_err(|e| anyhow::anyhow!("Failed to create Redis cache: {}", e))?;
    let permission_checker = Arc::new(RedisPermissionChecker::new(
        Arc::new(db.clone()),
        Arc::new(tokio::sync::RwLock::new(redis_cache)),
    ));
    info!("PermissionChecker initialized");

    // Initialize Redis Functions (rate limiting)
    // Create a temporary AppState for initialization
    let user_repository = Arc::new(PostgresUserRepository::new(db.clone().into()));
    let billing_repository = Arc::new(PostgresBillingRepository::new(db.clone()));
    let invoice_repository = Arc::new(PostgresInvoiceRepository::new(db.clone()));
    let credit_note_repository = Arc::new(PostgresCreditNoteRepository::new(db.clone()));
    let audit_event_repository = Arc::new(PostgresAuditEventRepository::new(db.clone()));
    let realm_config_repository =
        Arc::new(PostgresRealmConfigRepository::new(Arc::new(db.clone())));

    // Custom-domain host→realm mapping repository (BE-D03). Shared by the
    // lifecycle handlers (publish/restore side-effects) and BE-D04/D06/D07
    // (middleware / CORS / ask / resolve). Constructed from the same Sea-ORM
    // connection as the other repositories.
    let custom_domain_mapping_repo = Arc::new(PostgresCustomDomainMappingRepository::new(
        Arc::new(db.clone()),
    ));

    // Legal agreement + consent repositories + service
    let legal_repository = Arc::new(PostgresLegalAgreementRepository::new(db.clone()));
    let user_consent_repository = Arc::new(PostgresUserConsentRepository::new(db.clone()));
    let legal_service = Arc::new(LegalService::new(
        legal_repository.clone(),
        user_consent_repository.clone(),
        audit_event_repository.clone(),
    ));
    info!("Legal service initialized");

    let browser_token_service = Arc::new(RedisBrowserTokenService::new(redis_manager.clone()));
    let user_totp_repository = Arc::new(PostgresUserTotpRepository::new(db.clone().into()));
    let self_delete_service = Arc::new(SelfDeleteService::new(
        user_repository.clone(),
        user_totp_repository,
        billing_repository.clone(),
        browser_token_service.clone(),
        audit_event_repository.clone(),
    ));
    info!("Self-delete service initialized");

    // Create entitlement mapping service with permission-based policy
    let billing_policy = PermissionBasedBillingPolicy::new(permission_checker.clone());
    let entitlement_mapping_service = Arc::new(billing::EntitlementMappingService::new(
        billing_repository.clone(),
        Arc::new(billing_policy.clone()),
    ));
    info!("Entitlement mapping service initialized with PermissionBasedBillingPolicy");

    let provider_product_api = Arc::new(ConfiguredProviderProductApi::new(pg_pool.clone()));
    let provider_product_sync_service = Arc::new(billing::ProviderProductSyncService::new(
        billing_repository.clone(),
        Arc::new(billing_policy),
        provider_product_api,
    ));
    info!("Provider product sync service initialized");

    // Create points service with permission-based policy
    let points_repository = Arc::new(PostgresPointsRepository::new(
        Arc::new(db.clone()),
        sqlx_pool.clone(),
    ));
    let points_policy = PermissionBasedPointsPolicy::new(permission_checker.clone());
    let points_service = Arc::new(points::PointsService::new(
        points_repository.clone(),
        Arc::new(points_policy.clone()),
    ));
    info!("Points service initialized with PermissionBasedPointsPolicy");

    // Build the user-role repository ahead of the subscription service so the
    // subscription ImmediateCancel revoke (BE-D05 / design §5.5) can be wired
    // in at construction time.
    let user_role_repository = Arc::new(PostgresUserRoleRepository::new(pg_pool.clone()));

    // Create subscription service
    let subscription_service = Arc::new(points::SubscriptionService::new(
        points_service.clone(),
        points_repository.clone(),
        user_role_repository.clone(),
        permission_checker.clone(),
        None,
    ));
    info!("Subscription service initialized");

    // Create registration service
    let registration_service = Arc::new(points::services::RegistrationService::new(
        points_repository.clone(),
    ));
    info!("Registration service initialized");

    // Create admin user repositories
    let admin_user_repository = Arc::new(PostgresAdminUserRepository::new(pg_pool.clone()));
    let role_policy_repository = Arc::new(PostgresRolePolicyRepository::new(pg_pool.clone()));
    info!("Admin user repositories initialized");

    // Create admin user services
    let admin_user_service = Arc::new(AdminUserServiceImpl::new(
        admin_user_repository.clone(),
        user_role_repository.clone(),
        role_policy_repository.clone(),
        permission_checker.clone(),
        audit_event_repository.clone(),
        browser_token_service.clone(),
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
    info!("Admin user services initialized");

    // Create payment attempt service
    let payment_attempt_repository = Arc::new(PostgresPaymentAttemptRepository::new(
        Arc::new(db.clone()),
        pg_pool.clone(),
    ));
    let payment_attempt_service = Arc::new(payment_attempt::PaymentAttemptService::new(
        payment_attempt_repository.clone(),
    ));
    info!("Payment attempt service initialized");

    // Create fulfillment service
    let fulfillment_service = Arc::new(PostgresFulfillmentService::new(
        points_repository.clone(),
        billing_repository.clone(),
        payment_attempt_repository.clone(),
        user_role_repository.clone(),
        permission_checker.clone(),
    ));
    info!("Fulfillment service initialized");

    let purchase_service = Arc::new(PurchaseService::new(
        pg_pool.clone(),
        config.frontend.url.clone(),
        billing_repository.clone(),
        payment_attempt_service.clone(),
        payment_attempt_repository.clone(),
        user_role_repository.clone(),
        fulfillment_service.clone(),
    ));
    info!("Purchase service initialized");

    // Creem client is now loaded per-request from database (realm_config)
    // No global client needed

    // Extract JWT secret from config
    let jwt_secret = config
        .jwt
        .as_ref()
        .map(|j| j.secret.clone())
        .unwrap_or_default();
    if jwt_secret.is_empty() {
        tracing::warn!("JWT secret not configured - device code and OAuth flows will fail");
    }

    // Build application service once (no temp_state double-construction)
    let application_service = herald_core::application::ApplicationServiceBuilder::new()
        .with_database(Arc::new(db.clone()))
        .with_redis(redis_manager.clone())
        .with_permission_checker(permission_checker.clone())
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to build application service: {}", e))?;

    // Initialize application
    application_service
        .initialize_application()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to initialize application: {}", e))?;

    // Initialize admin realm RBAC
    // This ensures the admin realm has its default permissions
    let rbac_init_service = application_service.realm_service().get_rbac_init_service();

    init_admin_realm_rbac(pg_pool, rbac_init_service)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to initialize admin realm RBAC: {}", e))?;

    // Initialize admin user if database is empty
    init_admin_user(pg_pool, &config.server.app_env).await?;

    // Ensure the platform self-service signup toggle exists (fail-closed:
    // default disabled). Idempotent; an admin can flip it on later. A missing
    // row is already treated as disabled at read time, but seeding it keeps the
    // fail-closed state observable and consistent across deployments.
    ensure_platform_signup_default(pg_pool)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to seed platform signup toggle: {e}"))?;

    // Build final state once, then call init functions
    let startup_time = std::time::Instant::now();
    let api_key_cache = ApiKeyCache::new(redis_manager.clone().into());
    let api_key_repo = Arc::new(ClientApiKeyRepository::new(db.clone().into()));
    let idempotency_service = Arc::new(points::IdempotencyService::new(Arc::new(
        herald_core::infrastructure::points::RedisIdempotencyStore::new(
            redis_manager.clone().into(),
        ),
    )));
    let webhook_event_repository = Arc::new(WebhookEventRepository::new(pg_pool.clone()));
    let webhook_service = Arc::new(WebhookService::new(webhook_event_repository));

    // Shared HTTP client for outbound calls (Turnstile siteverify, etc.)
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to build HTTP client: {}", e))?;

    let state = Arc::new(AppState {
        service: application_service,
        pool: pg_pool.clone(),
        db: Arc::new(db),
        http_client,
        redis_manager: redis_manager.clone(),
        billing_repository,
        invoice_repository,
        credit_note_repository,
        audit_event_repository,
        entitlement_mapping_service,
        provider_product_sync_service,
        public_base_url: config.frontend.url.clone(),
        permission_checker,
        app_env: config.server.app_env.clone(),
        user_repository,
        api_key_cache,
        api_key_repo,
        idempotency_service,
        webhook_service,
        startup_time,
        points_repository,
        points_service,
        subscription_service,
        registration_service,
        admin_user_service,
        role_assignment_service,
        user_permission_service,
        permission_management_service,
        payment_attempt_service,
        payment_attempt_repository,
        fulfillment_service,
        purchase_service,
        jwt_secret,
        user_role_repository,
        role_policy_repository,
        realm_config_repository,
        legal_repository,
        user_consent_repository,
        legal_service,
        self_delete_service,
        custom_domain_mapping_repo,
        custom_domain_cname_target: config.custom_domain.cname_target.clone(),
        custom_domain_ask_key: config.custom_domain.ask_key.clone(),
        google_jwks_url: config.google_oauth.jwks_url.clone(),
        apple_jwks_url: config.apple_oauth.jwks_url.clone(),
        ldap_authenticator: std::sync::Arc::new(
            herald_core::infrastructure::ldap::Ldap3Authenticator::default(),
        ),
    });

    // Initialize Redis Functions using the final state's redis_manager
    init_rate_limit_function(&state)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to initialize Redis Functions: {:?}", e))?;
    info!("Redis Functions initialized");

    init_device_token_function(&state).await.map_err(|e| {
        anyhow::anyhow!("Failed to initialize device token Redis Function: {:?}", e)
    })?;
    info!("Device token Redis Function initialized");

    init_idempotency_function(&state.redis_manager)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to initialize idempotency Redis Function: {:?}", e))?;
    info!("Idempotency Redis Function initialized");

    init_authentication_functions(&state.redis_manager)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to initialize authentication Redis Functions: {e}"))?;
    info!("Authentication Redis Functions initialized");

    Ok(state)
}

/// Start the API HTTP server with a pre-built state.
///
/// This is the lower-level entry point used by `run_with_config` and by
/// the app crate when it needs to share `AppState` with background workers.
pub async fn start_server(state: Arc<AppState>, config: ApiConfig) -> Result<()> {
    // Validate frontend URL before creating router
    let _frontend_url_valid = config
        .frontend
        .url
        .parse::<axum::http::HeaderValue>()
        .map_err(|e| {
            tracing::error!("Invalid frontend URL '{}': {}", config.frontend.url, e);
            anyhow::anyhow!("Invalid frontend URL: must be a valid HTTP header value")
        })?;
    tracing::info!("Frontend URL validated: {}", config.frontend.url);

    // Parse trusted-proxy real-IP config. Fail loud on invalid CIDR/header: a
    // typo in `trusted_proxies` must abort startup rather than silently narrow
    // the trust set (which would degrade audit/rate-limit IP correctness).
    let real_ip_config = RealIpConfig::new(
        &config.server.trusted_proxies,
        &config.server.real_ip_header,
    )
    .map_err(|e| {
        tracing::error!("Invalid [server].trusted_proxies / real_ip_header: {e}");
        anyhow::anyhow!("Invalid real IP config: {e}")
    })?;

    // Create router
    let app = server::create_router(
        state,
        config.frontend.url,
        config.frontend.static_dir,
        real_ip_config,
    );

    // Start server
    let listener = tokio::net::TcpListener::bind(&config.server.bind_address).await?;
    tracing::info!("Server listening on {}", config.server.bind_address);

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    tracing::info!("Server shutdown complete");

    Ok(())
}

/// Export OpenAPI JSON schema to a file (public function for external use)
pub fn export_openapi_to_file(output_path: &PathBuf) -> Result<()> {
    export_openapi(output_path)
}

/// Export OpenAPI JSON schema to a file
fn export_openapi(output_path: &PathBuf) -> Result<()> {
    let openapi_schema = server::build_openapi_spec();
    let json = serde_json::to_string_pretty(&openapi_schema)?;

    // Create parent directory if it doesn't exist
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(output_path, json)?;
    println!("OpenAPI JSON exported to: {}", output_path.display());

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C, initiating graceful shutdown");
        }
        _ = terminate => {
            tracing::info!("Received SIGTERM, initiating graceful shutdown");
        }
    }
}

/// Seed the platform self-service signup toggle as disabled (fail-closed).
///
/// Idempotent: only inserts when no `(admin, platform_signup, enabled)` row
/// exists, so an admin's later opt-in is never overwritten. The row is marked
/// `enabled = true` (the config entry is active) with `config_value = 'false'`
/// (the feature is off) — these are independent axes in `realm_config`.
async fn ensure_platform_signup_default(pool: &sqlx::PgPool) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, is_secret, enabled, metadata)
         VALUES ('admin', 'platform_signup', 'enabled', 'false', false, true, '{}'::jsonb)
         ON CONFLICT (realm_id, config_type, config_key) DO NOTHING",
    )
    .execute(pool)
    .await?;
    Ok(())
}
