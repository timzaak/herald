pub mod async_payment_helpers;
pub mod auth_helpers;
pub mod billing_helpers;
pub mod client_helpers;
#[cfg(test)]
pub mod credit_bucket_helpers;
#[cfg(test)]
pub mod creem_mocks;
// IAP test-only helpers (Apple JWS fixtures + Google Play Developer API
// wiremock + realm_config inserters). Not exported via `pub use` — imported
// explicitly by `scenarios/billing/iap_*_scenarios.rs`, mirroring the
// `creem_mocks` / `otp_helpers` pattern.
pub mod device_code_helpers;
pub mod email_config_helpers;
#[cfg(test)]
pub mod iap_mocks;
pub mod oauth_pkce_helpers;
pub mod oauth_test_helpers;
#[cfg(test)]
pub mod passkey_authenticator;
#[cfg(test)]
pub mod payment_assertions;
pub mod points_grant_helpers;
pub mod points_helpers;
pub mod rbac_helpers;
pub mod subscription_test_helpers;
#[cfg(test)]
pub mod test_setup_helpers;
pub mod user_helpers;
pub mod webhook_helpers;
pub mod wechat_mocks;

// Email OTP test-only helpers (Redis code injection for deterministic verify
// flows). Not exported via `pub use` — imported explicitly by the OTP
// scenario tests.
#[cfg(test)]
pub mod otp_helpers;

// Google One Tap test-only helpers (RSA keypair fixture + test ID Token mint
// + wiremock JWKS). Not exported via `pub use` — imported explicitly by the
// `google_one_tap_scenarios.rs`, mirroring the `otp_helpers` pattern.
#[cfg(test)]
pub mod google_one_tap_helpers;

// Apple native login test-only helpers (test identity token mint + wiremock
// JWKS at Apple's `/auth/keys` path). Reuses the provider-agnostic RSA keypair
// fixtures from `google_one_tap_helpers`. Not exported via `pub use` — imported
// explicitly by `apple_native_scenarios.rs`.
#[cfg(test)]
pub mod apple_native_helpers;

// LDAP login test-only helpers (mock directory implementing the
// LdapAuthenticator port + realm_config inserters). Not exported via
// `pub use` — imported explicitly by the LDAP scenario tests, mirroring the
// `otp_helpers` pattern.
#[cfg(test)]
pub mod ldap_helpers;

pub use auth_helpers::*;
pub use billing_helpers::*;
pub use client_helpers::*;
pub use rbac_helpers::*;
pub use user_helpers::*;
