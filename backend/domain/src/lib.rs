// Domain layer - business logic and ports
//
// This layer contains:
// - Entities: Core business objects
// - Value objects: Immutable values
// - Ports (traits): Repository and service interfaces
// - Business logic: Domain services
//
// IMPORTANT: This layer has ZERO external dependencies
// - No sea_orm, no redis, no http clients
// - Only pure Rust and domain types

pub mod audit;
pub mod authentication;
pub mod authorization;
pub mod billing;
pub mod client;
pub mod client_api_keys;
pub mod client_app;
pub mod common;
pub mod custom_domain;
pub mod dashboard;
pub mod ldap;
pub mod legal;
pub mod oauth;
pub mod payment_attempt;
pub mod points;
pub mod purchase;
pub mod rbac_init;
pub mod realm;
pub mod realm_config;
pub mod security_constants;
pub mod telemetry;
pub mod totp_key_management;
pub mod user;
pub mod user_passkey;
pub mod user_totp;

// Re-export commonly used types
pub use audit::{
    ActorType, AuditAction, AuditCategory, AuditContext, AuditEvent, AuditEventFilters,
    AuditEventRepository, AuditResult, AuditTargetType, NewAuditEvent, PaginatedAuditEvents,
};
pub use authentication::Identity;
pub use custom_domain::{CustomDomainMappingRepository, MappingRow};
pub use totp_key_management::{RealmTotpKeyRepository, RealmTotpKeyService};
