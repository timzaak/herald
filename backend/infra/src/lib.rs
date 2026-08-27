// Infrastructure layer - external adapters implementation

pub mod audit;
pub mod authentication;
pub mod authorization;
pub mod billing;
pub mod client;
pub mod client_api_keys;
pub mod creem;
pub mod custom_domain;
pub mod dashboard;
pub mod ldap;
pub mod legal;
pub mod oauth;
pub mod payment_attempt;
pub mod points;
pub mod purchase;
pub mod realm;
pub mod realm_config;
pub mod redis;
pub mod stripe;
pub mod totp_key_management;
pub mod user;
pub mod user_passkey;
pub mod user_totp;
pub mod wechatpay;

pub mod webhook;

// Re-export commonly used types
pub use audit::PostgresAuditEventRepository;
pub use custom_domain::PostgresCustomDomainMappingRepository;
pub use dashboard::PostgresDashboardRepository;
pub use realm_config::PostgresRealmConfigRepository;
pub use user::{
    PostgresAdminUserRepository, PostgresRolePolicyRepository, PostgresUserRepository,
    PostgresUserRoleRepository, PostgresVerificationRepository,
};
