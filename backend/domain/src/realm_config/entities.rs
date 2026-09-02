use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::common::entities::app_errors::CoreError;

/// Realm generalized configuration entity
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq)]
pub struct RealmConfig {
    /// Configuration entry UUID
    pub id: Uuid,
    /// Realm this configuration belongs to
    #[serde(rename = "realmId")]
    pub realm_id: String,
    /// Configuration type (totp, turnstile, registration, white_label)
    #[serde(rename = "configType")]
    pub config_type: ConfigType,
    /// Configuration key (specific to each config_type)
    #[serde(rename = "configKey")]
    pub config_key: String,
    /// Configuration value (format depends on config_key)
    #[serde(rename = "configValue")]
    pub config_value: String,
    /// Whether this value contains sensitive data (e.g., API keys)
    #[serde(rename = "isSecret")]
    pub is_secret: bool,
    /// Whether this configuration is currently active
    pub enabled: bool,
    /// Additional metadata (JSON object)
    pub metadata: Option<serde_json::Value>,
    /// Creation timestamp
    #[serde(rename = "createdAt")]
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last update timestamp
    #[serde(rename = "updatedAt")]
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Configuration type enum
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ConfigType {
    /// TOTP two-factor authentication configuration
    ///
    /// Configuration is stored as a JSON object in config_value with the following structure:
    /// - config_key: `settings` (fixed key for TOTP configuration)
    /// - config_value: JSON string with `{"enabled": boolean, "force_enabled": boolean}`
    /// - enabled: boolean (whether the config entry itself is active)
    /// - metadata: null (not used for TOTP)
    ///
    /// Example configuration:
    /// ```json
    /// {
    ///   "config_type": "totp",
    ///   "config_key": "settings",
    ///   "config_value": "{\"enabled\":true,\"force_enabled\":false}",
    ///   "is_secret": false,
    ///   "enabled": true,
    ///   "metadata": null
    /// }
    /// ```
    Totp,

    /// Passkey authentication configuration
    ///
    /// Realm passkey settings reuse the realm_config table; no separate
    /// passkey configuration table is created. as_ref() returns `passkey`.
    ///
    /// Configuration is stored as a JSON object in config_value with the following structure:
    /// - config_key: `settings` (fixed key for Passkey configuration)
    /// - config_value: JSON string with `{"enabled": boolean, "force_enabled": boolean, "user_verification": "preferred", "cross_platform_authenticator": boolean}`
    /// - enabled: boolean (whether the config entry itself is active)
    /// - metadata: null (not used for Passkey)
    Passkey,

    /// Turnstile captcha configuration
    ///
    /// Valid config_key values:
    /// - `site_key`: Cloudflare Turnstile site key (public, non-secret)
    /// - `secret_key`: Cloudflare Turnstile secret key (secret, mark is_secret=true)
    ///
    /// Example site key configuration:
    /// ```json
    /// {
    ///   "config_type": "turnstile",
    ///   "config_key": "site_key",
    ///   "config_value": "0x4AAAAAAxxxxxxxxxxxxxxxxxx",
    ///   "is_secret": false,
    ///   "enabled": true
    /// }
    /// ```
    ///
    /// Example secret key configuration:
    /// ```json
    /// {
    ///   "config_type": "turnstile",
    ///   "config_key": "secret_key",
    ///   "config_value": "0x4AAAAAAxxxxxxxxxxxxxxxxxx",
    ///   "is_secret": true,
    ///   "enabled": true
    /// }
    /// ```
    Turnstile,

    /// User registration configuration
    ///
    /// Valid config_key values:
    /// - `enabled`: Enable user registration for the realm ("true" or "false")
    /// - `allowed_domains`: Comma-separated list of allowed email domains (e.g., "example.com,test.org")
    /// - `require_email_verification`: Require email verification for new accounts ("true" or "false")
    /// - `password_min_length`: Minimum password length
    /// - `require_uppercase`: Require at least one uppercase letter ("true" or "false")
    /// - `require_lowercase`: Require at least one lowercase letter ("true" or "false")
    /// - `require_numbers`: Require at least one number ("true" or "false")
    /// - `require_special_chars`: Require at least one special character ("true" or "false")
    ///
    /// Example allowed domains configuration:
    /// ```json
    /// {
    ///   "config_type": "registration",
    ///   "config_key": "allowed_domains",
    ///   "config_value": "example.com,test.org",
    ///   "is_secret": false,
    ///   "enabled": true
    /// }
    /// ```
    ///
    /// Example email verification configuration:
    /// ```json
    /// {
    ///   "config_type": "registration",
    ///   "config_key": "require_email_verification",
    ///   "config_value": "true",
    ///   "is_secret": false,
    ///   "enabled": true
    /// }
    /// ```
    Registration,

    /// White-label authentication UI configuration
    ///
    /// Configuration is stored as a JSON object in config_value using the public wire shape.
    ///
    /// Valid config_key values:
    /// - `settings`: Published configuration visible to auth pages
    /// - `draft`: Unpublished configuration edited by realm admins
    /// - `previous_settings`: Previous published configuration for one-step restore
    ///
    /// Example configuration:
    /// ```json
    /// {
    ///   "config_type": "white_label",
    ///   "config_key": "settings",
    ///   "config_value": "{\"logoUrl\":\"https://cdn.example.com/logo.svg\",\"accentColor\":\"#2563eb\"}",
    ///   "is_secret": false,
    ///   "enabled": true,
    ///   "metadata": null
    /// }
    /// ```
    WhiteLabel,

    /// Custom-domain configuration
    ///
    /// Stores the per-realm custom login hostname (precise match, e.g.
    /// `login.acme.com`), reusing the realm_config table and the same
    /// draft/publish/restore lifecycle as white-label.
    ///
    /// Valid config_key values:
    /// - `settings`: Published configuration visible to host→realm resolution
    /// - `draft`: Unpublished configuration edited by realm admins
    /// - `previous_settings`: Previous published configuration for one-step restore
    ///
    /// Example configuration:
    /// ```json
    /// {
    ///   "config_type": "custom_domain",
    ///   "config_key": "settings",
    ///   "config_value": "{\"hostname\":\"login.acme.com\"}",
    ///   "is_secret": false,
    ///   "enabled": true,
    ///   "metadata": null
    /// }
    /// ```
    CustomDomain,

    /// Realm TOTP encryption key configuration
    ///
    /// This config type stores the realm-level AES-256 key used to encrypt user TOTP secrets.
    ///
    /// Valid config_key values:
    /// - `version_1`: The realm's TOTP encryption key (version 1)
    ///
    /// Configuration details:
    /// - config_key: `version_1` (fixed key for version 1)
    /// - config_value: Base64-encoded 32-byte AES-256 key
    /// - is_secret: true (marked as sensitive)
    /// - enabled: true (key is active)
    /// - metadata: `{"version": 1}` (version metadata)
    ///
    /// Note: Key rotation is NOT implemented. The key_version field in user_totp_config
    /// is reserved for future extension. Currently all keys are version 1.
    ///
    /// Example configuration:
    /// ```json
    /// {
    ///   "config_type": "totp_key",
    ///   "config_key": "version_1",
    ///   "config_value": "SGVsbG8gV29ybGQ=", // Base64-encoded 32-byte key
    ///   "is_secret": true,
    ///   "enabled": true,
    ///   "metadata": {"version": 1}
    /// }
    /// ```
    TotpKey,

    /// Creem payment provider configuration
    ///
    /// Valid config_key values:
    /// - `api_key`: Creem API key (secret, mark is_secret=true)
    /// - `timeout`: HTTP request timeout in seconds (non-secret, e.g., "30")
    ///
    /// Example API key configuration:
    /// ```json
    /// {
    ///   "config_type": "creem",
    ///   "config_key": "api_key",
    ///   "config_value": "creem_your_api_key_here",
    ///   "is_secret": true,
    ///   "enabled": true
    /// }
    /// ```
    ///
    /// Example timeout configuration:
    /// ```json
    /// {
    ///   "config_type": "creem",
    ///   "config_key": "timeout",
    ///   "config_value": "30",
    ///   "is_secret": false,
    ///   "enabled": true
    /// }
    /// ```
    Creem,

    /// Stripe payment provider configuration
    ///
    /// Valid config_key values:
    /// - `api_key`: Stripe API key (secret, mark is_secret=true)
    /// - `webhook_secret`: Stripe webhook signing secret (secret, mark is_secret=true)
    /// - `publishable_key`: Stripe publishable key (non-secret)
    /// - `account_id`: Stripe account ID (non-secret, optional)
    /// - `timeout`: HTTP request timeout in seconds (non-secret, e.g., "30")
    /// - `webhook_endpoint_id`: Stripe webhook endpoint ID (non-secret, for verification)
    ///
    /// Example API key configuration:
    /// ```json
    /// {
    ///   "config_type": "stripe",
    ///   "config_key": "api_key",
    ///   "config_value": "sk_test_your_api_key_here",
    ///   "is_secret": true,
    ///   "enabled": true
    /// }
    /// ```
    ///
    /// Example webhook secret configuration:
    /// ```json
    /// {
    ///   "config_type": "stripe",
    ///   "config_key": "webhook_secret",
    ///   "config_value": "whsec_your_webhook_secret_here",
    ///   "is_secret": true,
    ///   "enabled": true
    /// }
    /// ```
    ///
    /// Example publishable key configuration:
    /// ```json
    /// {
    ///   "config_type": "stripe",
    ///   "config_key": "publishable_key",
    ///   "config_value": "pk_test_your_publishable_key_here",
    ///   "is_secret": false,
    ///   "enabled": true
    /// }
    /// ```
    ///
    /// Example timeout configuration:
    /// ```json
    /// {
    ///   "config_type": "stripe",
    ///   "config_key": "timeout",
    ///   "config_value": "30",
    ///   "is_secret": false,
    ///   "enabled": true
    /// }
    /// ```
    Stripe,

    /// App Store IAP (Apple) payment provider configuration
    ///
    /// Per design support-iap §4.3.2 / §5.4. Reuses the `realm_config` KV
    /// table; no separate IAP table is created.
    ///
    /// Valid config_key values:
    /// - `bundle_id`: App Bundle ID (non-secret)
    /// - `issuer_id`: App Store Connect Issuer ID (non-secret)
    /// - `key_id`: App Store Connect Key ID (non-secret)
    /// - `private_key_p8`: `.p8` private key in PEM form (secret, mark
    ///   is_secret=true; view masked, edit-leave-empty-keep)
    /// - `environment`: notification environment, `sandbox` / `production`
    ///   (non-secret)
    ///
    /// Example `.p8` configuration:
    /// ```json
    /// {
    ///   "config_type": "apple",
    ///   "config_key": "private_key_p8",
    ///   "config_value": "-----BEGIN PRIVATE KEY-----\n...\n-----END PRIVATE KEY-----",
    ///   "is_secret": true,
    ///   "enabled": true
    /// }
    /// ```
    ///
    /// `list_payment_providers` treats Apple as configured iff a config row
    /// with `config_key == "issuer_id"` exists.
    Apple,

    /// Google Play Billing (Google) payment provider configuration
    ///
    /// Per design support-iap §4.3.2 / §5.4. Reuses the `realm_config` KV
    /// table; no separate IAP table is created.
    ///
    /// Valid config_key values:
    /// - `package_name`: Play application package name (non-secret)
    /// - `service_account_json`: Service Account JSON (secret, mark
    ///   is_secret=true; view masked, edit-leave-empty-keep)
    ///
    /// Example service-account configuration:
    /// ```json
    /// {
    ///   "config_type": "google",
    ///   "config_key": "service_account_json",
    ///   "config_value": "{ \"type\": \"service_account\", ... }",
    ///   "is_secret": true,
    ///   "enabled": true
    /// }
    /// ```
    ///
    /// `list_payment_providers` treats Google as configured iff a config row
    /// with `config_key == "service_account_json"` exists.
    Google,

    /// WeChat Pay v3 merchant configuration (DEC-wechat-support-007).
    ///
    /// Valid config_key values (all stored in `realm_config` with
    /// `config_type = "wechat"`):
    /// - `app_id`: WeChat AppID (non-secret)
    /// - `mch_id`: Merchant ID (non-secret)
    /// - `private_key`: Merchant RSA private key PEM (secret, `is_secret=true`)
    /// - `serial_no`: Merchant certificate serial number (non-secret)
    /// - `v3_key`: APIv3 Key, 32 bytes (secret, `is_secret=true`)
    /// - `notify_url`: Public callback URL (non-secret)
    /// - `platform_public_key`: Optional manual platform public-key override
    ///   PEM for callback verification (non-secret)
    ///
    /// `list_payment_providers` treats WeChat as configured iff a config row
    /// with `config_key == "mch_id"` exists.
    Wechat,

    /// Email provider configuration
    ///
    /// Valid config_key values:
    /// - `provider`: Email provider type, either "resend" or "smtp" (non-secret)
    /// - `from_address`: Default sender email address (non-secret)
    /// - `resend_api_key`: Resend API key (secret, mark is_secret=true)
    /// - `smtp_host`: SMTP server hostname (non-secret)
    /// - `smtp_port`: SMTP server port number (non-secret, e.g., "587")
    /// - `smtp_username`: SMTP authentication username (non-secret)
    /// - `smtp_password`: SMTP authentication password (secret, mark is_secret=true)
    /// - `smtp_encryption`: Encryption mode, "starttls" or "ssl" (non-secret)
    ///
    /// Example provider configuration:
    /// ```json
    /// {
    ///   "config_type": "email",
    ///   "config_key": "provider",
    ///   "config_value": "resend",
    ///   "is_secret": false,
    ///   "enabled": true
    /// }
    /// ```
    ///
    /// Example Resend API key configuration:
    /// ```json
    /// {
    ///   "config_type": "email",
    ///   "config_key": "resend_api_key",
    ///   "config_value": "re_your_api_key_here",
    ///   "is_secret": true,
    ///   "enabled": true
    /// }
    /// ```
    ///
    /// Example SMTP configuration:
    /// ```json
    /// {
    ///   "config_type": "email",
    ///   "config_key": "smtp_host",
    ///   "config_value": "smtp.example.com",
    ///   "is_secret": false,
    ///   "enabled": true
    /// }
    /// ```
    Email,

    /// Invoice policy configuration
    ///
    /// Valid config_key values:
    /// - `policy`: Invoice policy settings as JSON string
    ///
    /// Example policy configuration:
    /// ```json
    /// {
    ///   "config_type": "invoice_policy",
    ///   "config_key": "policy",
    ///   "config_value": "{\"policy\":\"provider_first\",\"provider_capabilities\":{\"stripe\":{\"external_invoice_enabled\":true},\"creem\":{\"external_invoice_enabled\":true}}}",
    ///   "is_secret": false,
    ///   "enabled": true
    /// }
    /// ```
    InvoicePolicy,

    /// Email OTP login configuration
    ///
    /// Per-Realm switches for the email verification-code login flow
    /// (design email-otp-login §4.3.2 / §5.1). Storage reuses the
    /// `realm_config` KV table; no DDL is required.
    ///
    /// - config_key: `settings` (fixed key for Email OTP configuration)
    /// - config_value: JSON string with
    ///   `{"enabled": boolean, "auto_register": boolean}`
    /// - enabled: boolean (whether the config entry itself is active)
    /// - is_secret: false
    /// - metadata: null
    ///
    /// Example configuration:
    /// ```json
    /// {
    ///   "config_type": "email_otp",
    ///   "config_key": "settings",
    ///   "config_value": "{\"enabled\":true,\"auto_register\":false}",
    ///   "is_secret": false,
    ///   "enabled": true,
    ///   "metadata": null
    /// }
    /// ```
    EmailOtp,

    /// Enterprise LDAP directory login configuration
    ///
    /// Stored as two rows reusing the `realm_config` KV table (no DDL):
    /// - config_key: `settings` — non-sensitive JSON with
    ///   `{"enabled", "url", "starttls", "baseDn", "bindDn", "userFilter", "mailAttribute"}`
    ///   (validated by `domain::ldap::validate_ldap_settings_json` on the
    ///   configs-CRUD write path)
    /// - config_key: `bind_password` — service-account password, server-forced
    ///   `is_secret=true` (masked on read; empty submit preserves the old value)
    Ldap,

    /// Platform self-service realm signup toggle
    ///
    /// A platform-level switch owned by the admin realm that controls whether
    /// unauthenticated visitors can self-provision a new realm through the
    /// public signup endpoint. Reuses the `realm_config` KV table; no DDL.
    ///
    /// - config_key: `enabled` (fixed)
    /// - config_value: `"true"` | `"false"`
    /// - is_secret: false
    /// - enabled: true (the config row itself is active)
    /// - metadata: null
    ///
    /// Read as fail-closed: a missing row is treated as `false` so the public
    /// entry is never opened by accident.
    PlatformSignup,
}

impl From<String> for ConfigType {
    fn from(s: String) -> Self {
        ConfigType::try_from_str(&s).unwrap_or(ConfigType::Turnstile)
    }
}

impl ConfigType {
    pub fn try_from_str(s: &str) -> Result<Self, String> {
        let config_type = match s.to_lowercase().as_str() {
            "turnstile" => ConfigType::Turnstile,
            "registration" => ConfigType::Registration,
            "totp" => ConfigType::Totp,
            "passkey" => ConfigType::Passkey,
            "white_label" => ConfigType::WhiteLabel,
            "custom_domain" => ConfigType::CustomDomain,
            "totp_key" => ConfigType::TotpKey,
            "creem" => ConfigType::Creem,
            "stripe" => ConfigType::Stripe,
            "apple" => ConfigType::Apple,
            "google" => ConfigType::Google,
            "wechat" => ConfigType::Wechat,
            "email" => ConfigType::Email,
            "invoice_policy" => ConfigType::InvoicePolicy,
            "email_otp" => ConfigType::EmailOtp,
            "platform_signup" => ConfigType::PlatformSignup,
            "ldap" => ConfigType::Ldap,
            _ => return Err(format!("Invalid config type: {}", s)),
        };
        Ok(config_type)
    }

    /// The canonical lowercase string for this config type — the single source
    /// of truth. `AsRef<str>` and `From<ConfigType> for String` both delegate
    /// here. Returns a `'static` literal so callers can feed it to APIs that
    /// require a `'static` provider/config name.
    pub fn as_static_str(&self) -> &'static str {
        match self {
            ConfigType::Turnstile => "turnstile",
            ConfigType::Registration => "registration",
            ConfigType::Totp => "totp",
            ConfigType::Passkey => "passkey",
            ConfigType::WhiteLabel => "white_label",
            ConfigType::CustomDomain => "custom_domain",
            ConfigType::TotpKey => "totp_key",
            ConfigType::Creem => "creem",
            ConfigType::Stripe => "stripe",
            ConfigType::Apple => "apple",
            ConfigType::Google => "google",
            ConfigType::Wechat => "wechat",
            ConfigType::Email => "email",
            ConfigType::InvoicePolicy => "invoice_policy",
            ConfigType::EmailOtp => "email_otp",
            ConfigType::PlatformSignup => "platform_signup",
            ConfigType::Ldap => "ldap",
        }
    }
}

impl From<ConfigType> for String {
    fn from(ct: ConfigType) -> Self {
        ct.as_static_str().to_owned()
    }
}

impl AsRef<str> for ConfigType {
    fn as_ref(&self) -> &str {
        self.as_static_str()
    }
}

/// Create or update configuration request
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct UpsertRealmConfigRequest {
    /// Configuration type (totp, turnstile, registration, white_label)
    ///
    /// See ConfigType documentation for details on each type
    #[schema(example = "totp")]
    #[serde(rename = "configType")]
    pub config_type: ConfigType,

    /// Configuration key (specific to each config_type)
    ///
    /// **TOTP**: `settings` (fixed key, stores JSON object with enabled/force_enabled)
    /// **WhiteLabel**: `settings`, `draft`, `previous_settings` (stores camelCase JSON object)
    /// **Turnstile**: `site_key`, `secret_key`
    /// **Registration**: `allowed_domains`, `require_email_verification`
    #[schema(example = "settings")]
    #[serde(rename = "configKey")]
    pub config_key: String,

    /// Configuration value (format depends on config_key)
    ///
    /// For TOTP `settings`: JSON string like `{"enabled":true,"force_enabled":false}`
    /// For Turnstile keys: The actual key string
    /// For Registration `allowed_domains`: Comma-separated domains (e.g., "example.com,test.org")
    /// For Registration `require_email_verification`: "true" or "false"
    #[schema(example = r#"{"enabled":true,"force_enabled":false}"#)]
    #[serde(rename = "configValue")]
    pub config_value: String,

    /// Whether this value contains sensitive data (e.g., API keys)
    ///
    /// Set to true for secret keys, passwords, etc. Secret values may be masked in logs
    #[schema(example = "false")]
    #[serde(rename = "isSecret")]
    pub is_secret: Option<bool>,

    /// Whether this configuration is currently active
    ///
    /// Only applies to certain config types (e.g., TOTP uses this as the main enabled flag)
    #[schema(example = "true")]
    pub enabled: Option<bool>,

    /// Additional metadata (JSON object)
    ///
    /// Used for type-specific metadata:
    /// - TOTP: `{"force_enabled": boolean}` - whether TOTP is force-required
    /// - Other types: Can be used for future extensions
    #[schema(example = json!({"force_enabled": false}))]
    pub metadata: Option<serde_json::Value>,
}

/// API response type for realm configuration (with sensitive data hidden)
///
/// This type is used to return realm configuration data via API endpoints.
/// Sensitive data (where `is_secret=true`) is masked by setting `config_value` to `None`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RealmConfigResponse {
    /// Configuration entry UUID
    pub id: Uuid,
    /// Realm this configuration belongs to
    #[serde(rename = "realmId")]
    pub realm_id: String,
    /// Configuration type (totp, turnstile, registration, white_label, totp_key)
    #[serde(rename = "configType")]
    pub config_type: ConfigType,
    /// Configuration key (specific to each config_type)
    #[serde(rename = "configKey")]
    pub config_key: String,
    /// Configuration value (hidden if is_secret=true)
    ///
    /// This field is `None` when `is_secret=true` to prevent leaking sensitive data.
    #[serde(rename = "configValue")]
    pub config_value: Option<String>,
    /// Whether this value contains sensitive data (e.g., API keys)
    #[serde(rename = "isSecret")]
    pub is_secret: bool,
    /// Whether this configuration is currently active
    pub enabled: bool,
    /// Additional metadata (JSON object)
    pub metadata: Option<serde_json::Value>,
    /// Creation timestamp
    #[serde(rename = "createdAt")]
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last update timestamp
    #[serde(rename = "updatedAt")]
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl RealmConfig {
    /// Create a safe API response from RealmConfig
    ///
    /// This method creates a `RealmConfigResponse` that hides sensitive data.
    /// If `is_secret` is true, the `config_value` field is set to `None`.
    ///
    /// # Returns
    /// A `RealmConfigResponse` with sensitive data masked
    pub fn to_safe_response(self) -> RealmConfigResponse {
        RealmConfigResponse {
            id: self.id,
            realm_id: self.realm_id,
            config_type: self.config_type,
            config_key: self.config_key,
            config_value: if self.is_secret {
                None
            } else {
                Some(self.config_value)
            },
            is_secret: self.is_secret,
            enabled: self.enabled,
            metadata: self.metadata,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// Batch update configuration request
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct BatchUpsertRealmConfigRequest {
    /// List of configuration entries to create or update
    ///
    /// Each entry in the list follows the UpsertRealmConfigRequest structure
    #[schema(example = json!([
        {
            "config_type": "totp",
            "config_key": "settings",
            "config_value": "{\"enabled\":true,\"force_enabled\":false}",
            "is_secret": false,
            "enabled": true,
            "metadata": null
        },
        {
            "config_type": "turnstile",
            "config_key": "site_key",
            "config_value": "0x4AAAAAAxxxxxxxxxxxxxxxxxx",
            "is_secret": false,
            "enabled": true,
            "metadata": null
        }
    ]))]
    pub configs: Vec<UpsertRealmConfigRequest>,
}

/// Custom-domain configuration
///
/// Stores the precise custom login hostname assigned to a realm.
/// `hostname` is normalized server-side (IDNA-lowercase, trailing dot stripped)
/// before persistence, so consumers can rely on a canonical form.
#[derive(Debug, Clone, Default, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CustomDomainConfig {
    /// Precise custom login hostname (e.g. `login.acme.com`), normalized to
    /// lowercase with any trailing dot stripped. `None` clears the draft.
    pub hostname: Option<String>,
}

/// Live status of a published custom-domain hostname
///
/// Surface-only fields (CNAME/TLS readiness) shown on the realm admin config
/// page. These are **not** part of request-time host→realm resolution, which
/// keys solely on `custom_domain_mapping.enabled`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CustomDomainStatus {
    /// Whether the hostname's CNAME currently resolves to Herald's cname target
    pub cname_verified: bool,
    /// Whether Caddy has successfully issued (On-Demand) TLS for the hostname
    pub tls_ready: bool,
    /// Last time the CNAME/TLS status was probed
    pub checked_at: Option<chrono::DateTime<chrono::Utc>>,
}

// ---------------------------------------------------------------------------
// Hostname normalization + validation
// ---------------------------------------------------------------------------

const MAX_HOSTNAME_LEN: usize = 253;
const MAX_LABEL_LEN: usize = 63;

/// Normalize and validate a raw custom-domain hostname.
///
/// The normalization is pure (no I/O) so it can be reused by both the infra
/// and api layers. It applies the following rules (design §4.5):
/// - IDNA-lowercase (ASCII lowercasing; IDNA-punycode encoding is the caller's
///   deployment concern, not validated here).
/// - Strip a single trailing dot (`login.acme.com.` → `login.acme.com`).
/// - Reject empty input, wildcard hostnames (`*.` or any multilevel wildcard),
///   embedded port (`:`), embedded path or fragment (`/`, `#`, `?`),
///   and scheme prefixes (`http://`, `https://`, `//`).
/// - Reject hostnames longer than 253 characters or with a label longer than 63.
///
/// Returns the normalized hostname.
pub fn normalize_and_validate_hostname(raw: &str) -> Result<String, CoreError> {
    // Strip surrounding whitespace first so accidental leading/trailing spaces
    // don't trip the empty check or leave stray characters.
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(CoreError::bad_request(
            "custom domain hostname",
            "hostname must not be empty",
        ));
    }

    // Reject scheme prefixes and protocol-relative URLs up front — these indicate
    // the caller pasted a full URL rather than a hostname.
    let lowered = trimmed.to_lowercase();
    if lowered.starts_with("http://")
        || lowered.starts_with("https://")
        || lowered.starts_with("//")
    {
        return Err(CoreError::bad_request(
            "custom domain hostname",
            "hostname must not contain a scheme (http://, https://, //)",
        ));
    }

    // After lowercasing, reject any disallowed character. Per RFC 1035 a hostname
    // label is `[a-z0-9-]` separated by dots; we reject `:` (port), `/` (path),
    // `#` (fragment), `?` (query), `@` (userinfo) and whitespace by exclusion.
    // Non-ASCII letters are allowed for IDNA domains (canonical punycode
    // encoding is a deployment-time concern, not validated here).
    for ch in lowered.chars() {
        let ok = ch.is_ascii_lowercase()
            || ch.is_ascii_digit()
            || ch == '.'
            || ch == '-'
            || ch.is_alphabetic();
        if !ok {
            return Err(CoreError::bad_request(
                "custom domain hostname",
                &format!("hostname contains disallowed character {:?}", ch),
            ));
        }
    }

    // Strip a single trailing dot (FQDN form) but keep the inner structure.
    let without_trailing_dot = lowered.strip_suffix('.').unwrap_or(&lowered);
    if without_trailing_dot.is_empty() {
        return Err(CoreError::bad_request(
            "custom domain hostname",
            "hostname must not be empty after normalization",
        ));
    }

    // Reject wildcards: leading `*.` or any label equal to `*` (multilevel).
    if without_trailing_dot.starts_with("*.")
        || without_trailing_dot == "*"
        || without_trailing_dot.split('.').any(|label| label == "*")
    {
        return Err(CoreError::bad_request(
            "custom domain hostname",
            "wildcard hostnames are not allowed",
        ));
    }

    // Length bounds: total ≤ 253, each label ≤ 63 and non-empty.
    if without_trailing_dot.len() > MAX_HOSTNAME_LEN {
        return Err(CoreError::bad_request(
            "custom domain hostname",
            &format!(
                "hostname exceeds maximum length of {} characters",
                MAX_HOSTNAME_LEN
            ),
        ));
    }
    for label in without_trailing_dot.split('.') {
        if label.is_empty() {
            return Err(CoreError::bad_request(
                "custom domain hostname",
                "hostname contains an empty label (consecutive dots)",
            ));
        }
        if label.len() > MAX_LABEL_LEN {
            return Err(CoreError::bad_request(
                "custom domain hostname",
                &format!(
                    "hostname label {:?} exceeds maximum length of {} characters",
                    label, MAX_LABEL_LEN
                ),
            ));
        }
        // Labels must not start or end with a hyphen.
        if label.starts_with('-') || label.ends_with('-') {
            return Err(CoreError::bad_request(
                "custom domain hostname",
                &format!(
                    "hostname label {:?} must not start or end with a hyphen",
                    label
                ),
            ));
        }
    }

    Ok(without_trailing_dot.to_string())
}

#[cfg(test)]
mod config_type_tests {
    use super::ConfigType;

    /// Guards the `From<String> -> ConfigType::Turnstile` fallback quirk:
    /// `"email_otp"` must resolve to `EmailOtp`, not fall through to the
    /// default `Turnstile` branch. See design email-otp-login §7 risk table.
    #[test]
    fn email_otp_from_string_does_not_fall_back_to_turnstile() {
        let ct: ConfigType = "email_otp".to_string().into();
        assert_eq!(ct, ConfigType::EmailOtp);
        assert_ne!(ct, ConfigType::Turnstile);
    }

    #[test]
    fn email_otp_is_case_insensitive() {
        assert_eq!(
            ConfigType::try_from_str("EMAIL_OTP").unwrap(),
            ConfigType::EmailOtp
        );
    }

    /// Platform self-service signup toggle (design realm-create §4.3.2):
    /// the canonical string is `platform_signup` and must round-trip.
    #[test]
    fn platform_signup_round_trip() {
        assert_eq!(ConfigType::PlatformSignup.as_ref(), "platform_signup");
        assert_eq!(String::from(ConfigType::PlatformSignup), "platform_signup");
        assert_eq!(
            ConfigType::try_from_str("platform_signup").unwrap(),
            ConfigType::PlatformSignup
        );
        assert_eq!(
            ConfigType::try_from_str("PLATFORM_SIGNUP").unwrap(),
            ConfigType::PlatformSignup
        );
        // The From<String> fallback quirk must NOT silently turn the toggle
        // into Turnstile — it must resolve to PlatformSignup.
        let ct: ConfigType = "platform_signup".to_string().into();
        assert_eq!(ct, ConfigType::PlatformSignup);
        assert_ne!(ct, ConfigType::Turnstile);
    }

    /// Guards the `From<String> -> ConfigType::Turnstile` fallback quirk
    /// (design support-iap §6.3): an unknown config_type string must still
    /// resolve to the default `Turnstile` branch, NOT silently become a new
    /// IAP variant.
    #[test]
    fn unknown_config_type_falls_back_to_turnstile() {
        let ct: ConfigType = "definitely_not_a_real_config_type".to_string().into();
        assert_eq!(ct, ConfigType::Turnstile);
        assert!(ConfigType::try_from_str("definitely_not_a_real_config_type").is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_and_validate_hostname;

    #[test]
    fn valid_hostname_is_lowercased() {
        assert_eq!(
            normalize_and_validate_hostname("login.acme.com").unwrap(),
            "login.acme.com"
        );
    }

    #[test]
    fn uppercase_is_lowered() {
        assert_eq!(
            normalize_and_validate_hostname("Login.AcMe.COM").unwrap(),
            "login.acme.com"
        );
    }

    #[test]
    fn trailing_dot_is_stripped() {
        assert_eq!(
            normalize_and_validate_hostname("login.acme.com.").unwrap(),
            "login.acme.com"
        );
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        assert_eq!(
            normalize_and_validate_hostname("  login.acme.com  ").unwrap(),
            "login.acme.com"
        );
    }

    #[test]
    fn rejects_empty() {
        assert!(normalize_and_validate_hostname("").is_err());
        assert!(normalize_and_validate_hostname("   ").is_err());
    }

    #[test]
    fn rejects_wildcard_prefix() {
        assert!(normalize_and_validate_hostname("*.acme.com").is_err());
    }

    #[test]
    fn rejects_multilevel_wildcard() {
        assert!(normalize_and_validate_hostname("login.*.com").is_err());
    }

    #[test]
    fn rejects_bare_wildcard() {
        assert!(normalize_and_validate_hostname("*").is_err());
    }

    #[test]
    fn rejects_embedded_port() {
        assert!(normalize_and_validate_hostname("login.acme.com:443").is_err());
    }

    #[test]
    fn rejects_embedded_path() {
        assert!(normalize_and_validate_hostname("login.acme.com/path").is_err());
    }

    #[test]
    fn rejects_scheme() {
        assert!(normalize_and_validate_hostname("https://login.acme.com").is_err());
        assert!(normalize_and_validate_hostname("http://login.acme.com").is_err());
        assert!(normalize_and_validate_hostname("//login.acme.com").is_err());
    }

    #[test]
    fn rejects_fragment_and_query() {
        assert!(normalize_and_validate_hostname("login.acme.com#top").is_err());
        assert!(normalize_and_validate_hostname("login.acme.com?q=1").is_err());
    }

    #[test]
    fn rejects_overlong_hostname() {
        // 4 labels of 63 chars + dots + tld = 63*4 + 3 + 3 = 258 > 253 limit.
        let label = "a".repeat(63);
        let host = format!("{label}.{label}.{label}.{label}.com");
        assert!(host.len() > 253, "test fixture must exceed limit");
        assert!(normalize_and_validate_hostname(&host).is_err());
    }

    #[test]
    fn rejects_overlong_label() {
        let label = "a".repeat(64); // 64 > 63 limit
        let host = format!("{label}.com");
        assert!(normalize_and_validate_hostname(&host).is_err());
    }

    #[test]
    fn rejects_consecutive_dots() {
        assert!(normalize_and_validate_hostname("login..acme.com").is_err());
    }

    #[test]
    fn rejects_leading_hyphen_label() {
        assert!(normalize_and_validate_hostname("-login.acme.com").is_err());
        assert!(normalize_and_validate_hostname("login.-acme.com").is_err());
    }

    #[test]
    fn rejects_trailing_hyphen_label() {
        assert!(normalize_and_validate_hostname("login-.acme.com").is_err());
        assert!(normalize_and_validate_hostname("login.acme-.com").is_err());
    }

    #[test]
    fn accepts_hostname_at_length_boundary() {
        // Exactly 253 chars should pass (length is the boundary).
        let label = "a".repeat(63);
        // 63 + 1 + 63 + 1 + 63 + 1 + 57 = 249; pad tld to reach 253.
        let mut exact = format!("{label}.{label}.{label}.", label = label);
        exact.push_str(&"a".repeat(253 - exact.len()));
        assert_eq!(exact.len(), 253, "fixture must be exactly 253 chars");
        assert_eq!(normalize_and_validate_hostname(&exact).unwrap(), exact);
    }
}
