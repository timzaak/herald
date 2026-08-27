// Audit log query scenarios
pub mod audit_scenarios;

// Audit event collection scenarios (verify events are recorded for core operations)
pub mod audit_collection_scenarios;

// Tests for self-implemented permission system
pub mod admin_init_scenarios;
pub mod builtin_protection_scenarios;
pub mod change_email_scenarios;
pub mod client_app_scenarios;
// Custom-Domain config lifecycle scenarios
pub mod custom_domain_config_scenarios;
// Custom-Domain internal Caddy ask endpoint scenarios.
// The public host→realmId resolve endpoint was removed when realm routing
// reverted to always relying on the {realmId} path segment; only the ask
// (Caddy On-Demand TLS authorization) scenarios remain.
pub mod consent_gate_scenarios;
pub mod custom_domain_internal_endpoints_scenarios;
// Delegated sub-admin escalation guards on the RBAC management endpoints
pub mod delegated_admin_escalation_scenarios;
pub mod login_flow_scenarios;
pub mod permission_security_scenarios;
pub mod realm_access_scenarios;
pub mod realm_admin_creation_scenarios;
pub mod realm_isolation_scenarios;
pub mod realm_passkey_config_scenarios;
pub mod realm_totp_config_scenarios;
pub mod realm_white_label_config_scenarios;
pub mod role_policies_scenarios;
pub mod signup_scenarios;
pub mod user_list_scenarios;
pub mod user_register_test;
pub mod user_roles_scenarios;

// Billing scenarios
pub mod billing;

// Client API scenarios
pub mod client_api_scenarios;

// Dashboard statistics scenarios
pub mod dashboard_stats_scenarios;

// TOTP scenarios
pub mod realm_totp_key_initialization_scenarios;
pub mod user_passkey_scenarios;
pub mod user_totp_disable_scenarios;
pub mod user_totp_scenarios;

// Public config scenarios
pub mod public_config_scenarios;

// Unified OAuth scenarios
pub mod unified_oauth_scenarios;

// Device code authorization scenarios
pub mod device_code_scenarios;

// OAuth PKCE (Authorization Code + PKCE) scenarios
pub mod oauth_pkce_scenarios;

// Unified permission hierarchy scenarios
pub mod unified_permission_hierarchy_scenarios;

// Dashboard & Audit permission enforcement scenarios
pub mod dashboard_audit_permission_scenarios;

// Realm manage permission scenarios (realm.manage CRUD + legacy removal)
pub mod realm_manage_permission_scenarios;

// API Keys view/manage permission split scenarios
pub mod api_keys_permission_scenarios;

// API Key roles CRUD + permission + cache scenarios
pub mod api_key_roles_scenarios;

// Billing & Points permission enforcement scenarios
pub mod billing_points_permission_scenarios;

// Points system scenarios
pub mod points;

// Email config scenarios
pub mod email_config_scenarios;

// Credit Bucket scenarios
pub mod credit_bucket;

pub mod legal;

pub mod account_self_delete_scenarios;

// Kickoff User (session management / forced logout) scenarios, covering
// docs/user-stories/core/realm-admin.md US-RA-020 and US-RA-021.
pub mod user_sessions_scenarios;

// Realm management scenarios
// (realm_config_update_scenarios and realm_delete_scenarios removed)

// Email-OTP login scenarios.
pub mod client_app_turnstile_scenarios;
pub mod email_otp_send_verify_scenarios;
pub mod realm_email_otp_config_scenarios;

// Google One Tap login scenarios.
pub mod google_one_tap_scenarios;

// Apple native login scenarios.
pub mod apple_native_scenarios;

// LDAP enterprise-directory login scenarios (login flow + admin config).
pub mod ldap_login_scenarios;
pub mod realm_ldap_config_scenarios;
