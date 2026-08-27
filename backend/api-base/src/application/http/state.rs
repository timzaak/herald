use sqlx::PgPool;
use std::sync::Arc;
use std::time::Instant;

use herald_core::application::{ApplicationService, WebhookService};
use herald_core::domain::legal::LegalService;
use herald_core::domain::payment_attempt::PaymentAttemptService;
use herald_core::domain::user::services::SelfDeleteService;
use herald_core::domain::user::services::admin::{
    AdminUserServiceImpl, PermissionManagementServiceImpl, RoleAssignmentServiceImpl,
    UserPermissionServiceImpl,
};
use herald_core::infrastructure::PostgresCustomDomainMappingRepository;
use herald_core::infrastructure::audit::PostgresAuditEventRepository;
use herald_core::infrastructure::authentication::RedisBrowserTokenService;
use herald_core::infrastructure::authorization::RedisPermissionChecker;
use herald_core::infrastructure::authorization::policies::{
    PermissionBasedBillingPolicy, PermissionBasedPointsPolicy,
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
use herald_core::infrastructure::purchase::PostgresFulfillmentService;
use herald_core::infrastructure::purchase::PurchaseService;
use herald_core::infrastructure::realm_config::PostgresRealmConfigRepository;
use herald_core::infrastructure::redis::RedisConnectionManager;
use herald_core::infrastructure::user::{
    PostgresAdminUserRepository, PostgresRolePolicyRepository, PostgresUserRoleRepository,
    repositories::PostgresUserRepository,
};
use herald_core::infrastructure::user_totp::PostgresUserTotpRepository;
use sea_orm::DatabaseConnection;

/// Type alias for the PurchaseService to reduce complexity in AppState
type PurchaseServiceImpl = PurchaseService<
    PostgresBillingRepository,
    PostgresPaymentAttemptRepository,
    PostgresFulfillmentService<
        PostgresPointsRepository,
        PostgresBillingRepository,
        PostgresPaymentAttemptRepository,
        PostgresUserRoleRepository,
        RedisPermissionChecker,
    >,
    PostgresUserRoleRepository,
>;

type ProviderProductSyncServiceImpl = herald_core::domain::billing::ProviderProductSyncService<
    PostgresBillingRepository,
    PermissionBasedBillingPolicy,
    ConfiguredProviderProductApi,
>;

/// AppState for API handlers
/// Contains database connections and configuration for HTTP endpoints
#[derive(Clone)]
pub struct AppState {
    /// Core application service (所有领域服务的聚合)
    pub service: ApplicationService,

    /// Database connection pool (sqlx)
    pub pool: PgPool,

    /// Database connection (Sea-ORM) for entity operations
    pub db: Arc<DatabaseConnection>,

    /// Shared HTTP client (pooled connections) for outbound calls
    /// (e.g. Cloudflare Turnstile siteverify).
    pub http_client: reqwest::Client,

    /// Redis connection manager with DB isolation
    /// - Production: uses DB 0 (default_db)
    /// - Test: uses DB 1 (test_db) for automatic isolation
    pub redis_manager: RedisConnectionManager,

    /// Billing repository
    pub billing_repository: Arc<PostgresBillingRepository>,

    /// Invoice repository
    pub invoice_repository: Arc<PostgresInvoiceRepository>,

    /// Credit note repository
    pub credit_note_repository: Arc<PostgresCreditNoteRepository>,

    /// Audit event repository
    pub audit_event_repository: Arc<PostgresAuditEventRepository>,

    /// Entitlement mapping service
    pub entitlement_mapping_service: Arc<
        herald_core::domain::billing::EntitlementMappingService<
            PostgresBillingRepository,
            PermissionBasedBillingPolicy,
        >,
    >,

    /// Provider product sync service
    pub provider_product_sync_service: Arc<ProviderProductSyncServiceImpl>,

    /// Points repository
    pub points_repository: Arc<PostgresPointsRepository>,

    /// Points service (with policy)
    pub points_service: Arc<
        herald_core::domain::points::PointsService<
            PostgresPointsRepository,
            PermissionBasedPointsPolicy,
        >,
    >,

    /// Subscription service (for subscription lifecycle events)
    pub subscription_service: Arc<
        herald_core::domain::points::SubscriptionService<
            PostgresPointsRepository,
            PermissionBasedPointsPolicy,
            PostgresUserRoleRepository,
            RedisPermissionChecker,
        >,
    >,

    /// Registration service (for free user points on registration)
    pub registration_service:
        Arc<herald_core::domain::points::services::RegistrationService<PostgresPointsRepository>>,

    /// Public base URL for the API
    pub public_base_url: String,

    /// Permission checker using custom RBAC implementation
    pub permission_checker: Arc<RedisPermissionChecker>,

    /// Application environment (dev/prod/test)
    pub app_env: String,

    /// User repository (used by identity middleware to load user from database)
    /// Note: HTTP handlers should use Extension<Identity> instead of user_repository directly
    pub user_repository: Arc<PostgresUserRepository>,

    /// API Key cache (Redis)
    pub api_key_cache: ApiKeyCache,

    /// API Key repository (PostgreSQL)
    pub api_key_repo: Arc<ClientApiKeyRepository>,

    /// Idempotency service
    pub idempotency_service: Arc<
        herald_core::domain::points::IdempotencyService<
            herald_core::infrastructure::points::RedisIdempotencyStore,
        >,
    >,

    /// Webhook service (for webhook event processing with idempotency)
    pub webhook_service: Arc<WebhookService>,

    /// Server startup time for uptime calculation
    pub startup_time: Instant,

    // ============================================================================
    // Admin User Services
    // ============================================================================
    /// Admin user service
    pub admin_user_service: Arc<
        AdminUserServiceImpl<
            PostgresAdminUserRepository,
            PostgresUserRoleRepository,
            PostgresRolePolicyRepository,
            RedisPermissionChecker,
            PostgresAuditEventRepository,
            RedisBrowserTokenService,
        >,
    >,

    /// Role assignment service
    pub role_assignment_service: Arc<
        RoleAssignmentServiceImpl<
            PostgresUserRoleRepository,
            PostgresRolePolicyRepository,
            RedisPermissionChecker,
        >,
    >,

    /// User permission service
    pub user_permission_service: Arc<
        UserPermissionServiceImpl<
            PostgresUserRoleRepository,
            PostgresRolePolicyRepository,
            RedisPermissionChecker,
        >,
    >,

    /// Permission management service
    pub permission_management_service: Arc<
        PermissionManagementServiceImpl<
            PostgresUserRoleRepository,
            PostgresRolePolicyRepository,
            RedisPermissionChecker,
            herald_core::infrastructure::audit::PostgresAuditEventRepository,
        >,
    >,

    // ============================================================================
    // Payment Services
    // ============================================================================
    /// Payment attempt service
    pub payment_attempt_service: Arc<PaymentAttemptService<PostgresPaymentAttemptRepository>>,

    /// Payment attempt repository (for direct repository access)
    pub payment_attempt_repository: Arc<PostgresPaymentAttemptRepository>,

    /// Fulfillment service (for unified purchase handling)
    pub fulfillment_service: Arc<
        PostgresFulfillmentService<
            PostgresPointsRepository,
            PostgresBillingRepository,
            PostgresPaymentAttemptRepository,
            PostgresUserRoleRepository,
            RedisPermissionChecker,
        >,
    >,

    /// Purchase service (routes attempts into fulfillment)
    pub purchase_service: Arc<PurchaseServiceImpl>,

    /// JWT secret key for token generation (device code, OAuth)
    pub jwt_secret: String,

    /// User role repository for batch role queries (e.g. API key role summaries)
    pub user_role_repository: Arc<PostgresUserRoleRepository>,

    /// Role policy repository for direct role reads (e.g. validating that
    /// `granted_role_ids` on an entitlement mapping belong to the realm —
    /// design §5.2 / BE-D02). Direct AppState field, same pattern as
    /// `billing_repository`; NOT the `state.service.<svc>()` registry.
    pub role_policy_repository: Arc<PostgresRolePolicyRepository>,

    /// Realm config repository (for direct SQL access to realm_config table)
    pub realm_config_repository: Arc<PostgresRealmConfigRepository>,

    /// Legal agreement repository (legal_agreement_version CRUD / resolution)
    pub legal_repository: Arc<PostgresLegalAgreementRepository>,

    /// User consent repository (user_agreement_consent upsert / read)
    pub user_consent_repository: Arc<PostgresUserConsentRepository>,

    /// Legal use-case service (public agreements + self consent gate).
    /// Direct AppState field — same pattern as `billing_repository`, NOT the
    /// `state.service.<svc>()` registry. BE-D08 reuses this from api-auth.
    pub legal_service: Arc<
        LegalService<
            PostgresLegalAgreementRepository,
            PostgresUserConsentRepository,
            PostgresAuditEventRepository,
        >,
    >,

    /// Self-service account deletion (soft-delete) pipeline — BE-D07.
    /// Orchestrates user/profile anonymization + TOTP wipe (in-tx) with
    /// subscription cancellation, session revocation, and `user.delete`
    /// audit (post-tx). Direct AppState field, same pattern as `legal_service`.
    pub self_delete_service: Arc<
        SelfDeleteService<
            PostgresUserRepository,
            PostgresUserTotpRepository,
            PostgresBillingRepository,
            RedisBrowserTokenService,
            PostgresAuditEventRepository,
        >,
    >,

    /// Custom-domain host→realm mapping repository (design §4.3.2 / §5.1).
    ///
    /// Request-time query surface for the `custom_domain_mapping` table, shared
    /// by the custom-domain lifecycle handlers (BE-D03 publish/restore
    /// side-effects) and the host→realm middleware / CORS / ask / resolve
    /// endpoints (BE-D04/D06/D07). Held as the concrete infra type — same
    /// pattern as `realm_config_repository` — because `api-base` depends on
    /// `herald-core`, which re-exports `herald-infra`.
    pub custom_domain_mapping_repo: Arc<PostgresCustomDomainMappingRepository>,

    /// Herald-owned CNAME target hostname tenants point their custom login
    /// domain at (design §4.2.2 `cnameTarget`). Surfaced to realm admins in the
    /// custom-domain GET response. Empty when unset.
    pub custom_domain_cname_target: String,

    /// Shared secret for the Caddy On-Demand TLS ask authorization endpoint
    /// (design §4.2.2 ask). The `GET /api/internal/custom-domain/authorize`
    /// handler compares the request's `X-Herald-Ask-Key` header against this
    /// value; mismatch/missing → 401. Validated non-empty at startup
    /// (`build_app_state_with_migrations`, design §4.2.2 "Herald 启动期校验
    /// 非空"). Held as a direct AppState field alongside
    /// `custom_domain_cname_target` since the ask handler reads it per request
    /// without a service-layer indirection.
    pub custom_domain_ask_key: String,

    /// Google JWKS endpoint used to validate Google One Tap ID Token
    /// signatures. Read from AppState (not an env var) so scenario tests can
    /// override it on a private AppState copy to point at a wiremock JWKS
    /// without process-wide mutation.
    pub google_jwks_url: String,

    /// Apple JWKS endpoint used to validate Apple native login identity token
    /// signatures. Same injection pattern as `google_jwks_url`: read from
    /// AppState so scenario tests override it on a private copy to point at
    /// a wiremock JWKS without process-wide mutation.
    pub apple_jwks_url: String,

    /// LDAP directory authenticator (enterprise login). Injected as a trait
    /// object so scenario tests replace it with a mock via
    /// `create_unified_test_router_with_state` (same override pattern as the
    /// JWKS URL fields above).
    pub ldap_authenticator: std::sync::Arc<dyn herald_core::domain::ldap::LdapAuthenticator>,
}
