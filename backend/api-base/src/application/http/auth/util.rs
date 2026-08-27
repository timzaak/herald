use axum::extract::FromRequestParts;
use serde::Deserialize;
use tokio::time::{Duration, timeout};

use crate::application::http::real_ip::RealIpConfig;
use crate::application::http::server::api_entities::ApiError;
use crate::application::http::state::AppState;
use herald_core::domain::authorization::permission_service::PermissionService;
use herald_core::domain::client::entities::ClientApp;
use herald_core::domain::ldap::{LdapDirectorySettings, LdapLoginConfig};

// Re-export rate limiting functions from the dedicated module
pub use crate::application::http::rate_limit::rate_limit_hit;

/// Extract the request `User-Agent` header as an owned string, if present.
///
/// Handlers that record `user_agent` in audit events or token-family metadata
/// should call this instead of re-inlining the header lookup.
pub fn user_agent_from_headers(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// Extract the client IP using a trusted-proxy-aware algorithm.
///
/// Reads `RealIpConfig` from request extensions (injected by `create_router`
/// via `axum::Extension`). Behaviour:
///
/// 1. **No `RealIpConfig`** (e.g. test routers that bypass `create_router`):
///    use the socket peer IP and ignore all forwarded headers — the safe
///    default so a misconfigured test harness cannot accidentally trust a
///    client-supplied header.
/// 2. **Socket peer NOT in `trusted_proxies`**: ignore all forwarded headers
///    (they may be client-forged) and use the socket peer IP.
/// 3. **Socket peer IS in `trusted_proxies`**: trust the configured header:
///    - `X-Forwarded-For` — walk the chain right-to-left, skip IPs inside
///      `trusted_proxies`, return the first non-trusted IP.
///    - otherwise (`CF-Connecting-IP`, `X-Real-IP`, ...) — return that
///      header's value verbatim.
/// 4. Fallback at any step: the socket peer IP, or empty string if it is
///    unavailable (e.g. oneshot test requests without `ConnectInfo`).
///
/// Usage in handlers: `client_ip: ClientIp` then `client_ip.0` for the IP string.
#[derive(Debug)]
pub struct ClientIp(pub String);

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let socket_ip = parts
            .extensions
            .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
            .map(|ci| ci.0.ip());
        let cfg = parts.extensions.get::<RealIpConfig>();
        Ok(ClientIp(resolve_client_ip(socket_ip, &parts.headers, cfg)))
    }
}

/// Pure trust-resolution core of the `ClientIp` extractor.
///
/// Extracted from `from_request_parts` so the security-critical trust logic is
/// unit-testable without constructing an axum router. See the `ClientIp` doc
/// comment for the behavioural spec.
fn resolve_client_ip(
    socket_ip: Option<std::net::IpAddr>,
    headers: &axum::http::HeaderMap,
    cfg: Option<&RealIpConfig>,
) -> String {
    // No trusted-proxy config injected → safe default: socket IP only
    // (empty if the socket peer is unknown, e.g. oneshot test requests).
    let Some(cfg) = cfg else {
        return socket_ip.map(|ip| ip.to_string()).unwrap_or_default();
    };
    // Socket peer unknown → cannot establish trust → must not read headers.
    let Some(socket_ip) = socket_ip else {
        return String::new();
    };
    // Socket peer NOT a trusted proxy → forwarded headers may be forged.
    if !cfg.trusts(socket_ip) {
        return socket_ip.to_string();
    }
    // Trusted proxy → resolve via the configured header.
    let resolved = if cfg.is_xff_chain() {
        headers
            .get("X-Forwarded-For")
            .and_then(|v| v.to_str().ok())
            .and_then(|xff| first_untrusted_in_xff(xff, cfg))
    } else {
        headers
            .get(&cfg.real_ip_header)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<std::net::IpAddr>().ok())
    };
    resolved
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| socket_ip.to_string())
}

/// Walk an `X-Forwarded-For` chain right-to-left and return the first IP that
/// is NOT inside the trusted-proxy CIDRs. Returns `None` if every IP in the
/// chain is a trusted proxy (degenerate/empty chain) — callers fall back to the
/// socket peer IP.
fn first_untrusted_in_xff(xff: &str, cfg: &RealIpConfig) -> Option<std::net::IpAddr> {
    xff.split(',')
        .rev()
        .map(|s| s.trim())
        .filter_map(|s| s.parse::<std::net::IpAddr>().ok())
        .find(|ip| !cfg.trusts(*ip))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(trusted: &[&str], header: &str) -> RealIpConfig {
        RealIpConfig::new(
            &trusted.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            header,
        )
        .expect("test fixture CIDRs are valid")
    }

    fn headers(pairs: &[(&str, &str)]) -> axum::http::HeaderMap {
        let mut h = axum::http::HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        h
    }

    /// WHY: with no `RealIpConfig` injected, the extractor MUST ignore every
    /// forwarded header and return the socket peer. Otherwise a router that
    /// forgets the extension (a misconfigured test harness, or a new entry
    /// point that bypasses `create_router`) would silently start trusting
    /// client-supplied X-Real-IP, recreating the original spoofing bug. This
    /// safe default is what the whole change hinges on.
    #[test]
    fn no_config_ignores_headers_returns_socket_ip() {
        let h = headers(&[("X-Real-IP", "9.9.9.9"), ("X-Forwarded-For", "9.9.9.9")]);
        let ip = resolve_client_ip(Some("1.2.3.4".parse().unwrap()), &h, None);
        assert_eq!(ip, "1.2.3.4");
    }

    /// WHY: a client connecting directly (socket peer NOT in trusted_proxies)
    /// can set X-Real-IP / X-Forwarded-For / even CF-Connecting-IP to anything
    /// — the extractor MUST ignore those forged headers and return the real
    /// socket peer. This is the core anti-spoofing guarantee; a regression
    /// here lets anyone bypass rate-limiting and corrupt audit IPs by setting
    /// a header.
    #[test]
    fn untrusted_socket_ignores_forged_headers() {
        let cfg = cfg(&["10.0.0.0/8"], "CF-Connecting-IP");
        let h = headers(&[
            ("X-Real-IP", "9.9.9.9"),
            ("X-Forwarded-For", "9.9.9.9"),
            ("CF-Connecting-IP", "8.8.8.8"),
        ]);
        // Socket 5.5.5.5 is NOT in 10.0.0.0/8 → all forwarded headers ignored.
        let ip = resolve_client_ip(Some("5.5.5.5".parse().unwrap()), &h, Some(&cfg));
        assert_eq!(ip, "5.5.5.5");
    }

    /// WHY: behind Cloudflare the socket peer IS a CF IP and CF writes the real
    /// client IP into CF-Connecting-IP — the extractor MUST return that value.
    /// This is the production happy path behind the orange cloud.
    #[test]
    fn trusted_socket_reads_cf_connecting_ip() {
        let cfg = cfg(&["10.0.0.0/8"], "CF-Connecting-IP");
        let h = headers(&[("CF-Connecting-IP", "203.0.113.42")]);
        // Socket 10.0.0.1 (Caddy) is trusted.
        let ip = resolve_client_ip(Some("10.0.0.1".parse().unwrap()), &h, Some(&cfg));
        assert_eq!(ip, "203.0.113.42");
    }

    /// WHY: for the default X-Forwarded-For header, the extractor walks the
    /// chain right-to-left skipping trusted proxies and returns the first
    /// non-trusted IP. The leftmost entry (client-set, possibly forged) MUST
    /// never be returned — otherwise an attacker prepends a fake IP and
    /// defeats the trust check. This mirrors what nginx/Cloudflare themselves do.
    #[test]
    fn trusted_socket_xff_rightmost_untrusted() {
        let cfg = cfg(&["10.0.0.0/8"], "X-Forwarded-For");
        // Chain: 1.1.1.1(client-forged leftmost), 203.0.113.55(real), 10.0.0.2(trusted proxy).
        let h = headers(&[("X-Forwarded-For", "1.1.1.1, 203.0.113.55, 10.0.0.2")]);
        let ip = resolve_client_ip(Some("10.0.0.1".parse().unwrap()), &h, Some(&cfg));
        // Skip 10.0.0.2 (trusted), return 203.0.113.55 (first non-trusted from right).
        // 1.1.1.1 (client-forged leftmost) is never returned.
        assert_eq!(ip, "203.0.113.55");
    }

    /// WHY: if a trusted proxy forwards but the configured header is missing
    /// (misconfigured proxy, or a health check), the extractor MUST fall back
    /// to the socket peer rather than return empty — empty would corrupt
    /// audit records and create unkeyable rate-limit buckets.
    #[test]
    fn trusted_socket_missing_header_falls_back_to_socket() {
        let cfg = cfg(&["10.0.0.0/8"], "CF-Connecting-IP");
        let h = headers(&[]); // no CF-Connecting-IP present
        let ip = resolve_client_ip(Some("10.0.0.1".parse().unwrap()), &h, Some(&cfg));
        assert_eq!(ip, "10.0.0.1");
    }

    /// WHY: if the socket peer is unknown (e.g. a test harness using oneshot
    /// without ConnectInfo) but a config IS present, the extractor cannot
    /// decide trust — so it MUST return empty rather than read any header.
    /// Returning a header value here would let a test-only code path trust
    /// client-supplied data.
    #[test]
    fn unknown_socket_returns_empty_even_with_config() {
        let cfg = cfg(&["10.0.0.0/8"], "CF-Connecting-IP");
        let h = headers(&[("CF-Connecting-IP", "203.0.113.99")]);
        let ip = resolve_client_ip(None, &h, Some(&cfg));
        assert_eq!(
            ip, "",
            "unknown socket + present config must not trust any header"
        );
    }
}

pub fn normalize_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

pub fn epoch_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Deserialize)]
struct TurnstileSiteVerifyResp {
    success: bool,
}

/// Verify a Turnstile token against a Client App's Turnstile configuration.
///
/// Turnstile is fully delegated to the Client App (D-PROTECT-01). If the
/// client app has `turnstile_enabled == false` the token is optional and
/// verification is skipped (matching the legacy realm-level "not configured ->
/// skip" semantics). When enabled, a valid token is required.
///
/// This reads only `client_app` fields; it never reads `realm_config`.
pub async fn verify_turnstile_for_client_app(
    state: &AppState,
    client_app: &ClientApp,
    token: Option<&str>,
    ip: &str,
) -> Result<(), ApiError> {
    if !client_app.turnstile_enabled {
        return Ok(());
    }

    let secret = client_app
        .turnstile_secret_key
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            tracing::error!(
                client_id = %client_app.client_id,
                "Turnstile is enabled but secret key is not configured"
            );
            ApiError::internal("Turnstile secret key is not configured")
        })?;

    // Turnstile is enabled, token is required.
    let token = token
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request("turnstile token is required"))?;

    // Cloudflare Turnstile 官方测试 secret：开发/测试环境常用，避免测试依赖外网。
    // 生产环境若仍配置该测试 secret 则一律拒绝——否则人机校验会静默失效
    // （app_env 默认即 "production"，见 config::default_app_env）。
    if secret.trim() == "1x0000000000000000000000000000000AA" {
        if state.app_env.eq_ignore_ascii_case("production")
            || state.app_env.eq_ignore_ascii_case("prod")
        {
            tracing::error!(
                client_id = %client_app.client_id,
                "Turnstile test secret is configured in a production environment — rejecting"
            );
            return Err(ApiError::bad_request("Turnstile verification failed"));
        }
        return Ok(());
    }

    let mut form = vec![("secret", secret), ("response", token)];
    if !ip.trim().is_empty() {
        form.push(("remoteip", ip));
    }

    let resp = state
        .http_client
        .post("https://challenges.cloudflare.com/turnstile/v0/siteverify")
        .form(&form)
        .send()
        .await
        .map_err(|_| ApiError::internal("Internal server error"))?;

    let body: TurnstileSiteVerifyResp = resp
        .json()
        .await
        .map_err(|_| ApiError::internal("Internal server error"))?;

    if !body.success {
        return Err(ApiError::unauthorized("turnstile verification failed"));
    }

    Ok(())
}

/// Check if user registration is enabled for a realm
///
/// Returns true if registration is explicitly enabled, false otherwise.
/// By default, registration is disabled if no config is found.
pub async fn is_registration_enabled(state: &AppState, realm_id: &str) -> Result<bool, ApiError> {
    let row = sqlx::query_as::<_, (String,)>(
        "SELECT config_value FROM realm_config
         WHERE realm_id = $1 AND config_type = 'registration' AND config_key = 'enabled' AND enabled = true",
    )
    .bind(realm_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to query registration config: {e}");
        ApiError::internal("Failed to query registration config")
    })?;

    // Parse the config value as boolean
    let enabled = row
        .and_then(|(value,)| match value.to_lowercase().as_str() {
            "true" | "1" | "yes" => Some(true),
            "false" | "0" | "no" => Some(false),
            _ => None,
        })
        .unwrap_or(false); // Default to disabled if no config found

    Ok(enabled)
}

/// Check if email verification is required for user registration in a realm
///
/// Returns true if email verification is explicitly enabled, false otherwise.
/// By default, email verification is NOT required if no config is found.
pub async fn is_email_verification_required(
    state: &AppState,
    realm_id: &str,
) -> Result<bool, ApiError> {
    let row = sqlx::query_as::<_, (String,)>(
        "SELECT config_value FROM realm_config
         WHERE realm_id = $1
           AND config_type = 'registration'
           AND config_key = 'require_email_verification'
           AND enabled = true",
    )
    .bind(realm_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to query email verification config: {e}");
        ApiError::internal("Failed to query email verification config")
    })?;

    // Parse the config value as boolean
    let required = row
        .and_then(|(value,)| match value.to_lowercase().as_str() {
            "true" | "1" | "yes" => Some(true),
            "false" | "0" | "no" => Some(false),
            _ => None,
        })
        .unwrap_or(false); // Default to NOT required if no config found

    // If email verification is requested, verify email is actually configured
    if required {
        let email_ready = is_email_configured(state, realm_id).await?;
        if !email_ready {
            tracing::warn!(
                realm_id = %realm_id,
                "Email verification requested but email not configured, forcing false"
            );
            return Ok(false);
        }
    }

    Ok(required)
}

/// Check whether self-service realm signup is open on the platform.
///
/// Reads the admin realm's `platform_signup` / `enabled` config row. The
/// public signup entry is fail-closed: a missing or non-true row means the
/// endpoint must refuse to provision, so an unconfigured deployment never
/// accidentally opens self-service to the public internet.
pub async fn is_platform_signup_enabled(state: &AppState) -> Result<bool, ApiError> {
    let row = sqlx::query_as::<_, (String,)>(
        "SELECT config_value FROM realm_config
         WHERE realm_id = $1 AND config_type = 'platform_signup' AND config_key = 'enabled' AND enabled = true",
    )
    .bind(herald_core::domain::realm::ADMIN_REALM_ID)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to query platform signup config: {e}");
        ApiError::internal("Failed to query platform signup config")
    })?;

    let enabled = row
        .and_then(|(value,)| match value.to_lowercase().as_str() {
            "true" | "1" | "yes" => Some(true),
            "false" | "0" | "no" => Some(false),
            _ => None,
        })
        .unwrap_or(false); // fail-closed: missing row => disabled

    Ok(enabled)
}

/// Check if email is configured for a realm
///
/// Returns true if the realm has a complete email configuration, false otherwise.
pub async fn is_email_configured(state: &AppState, realm_id: &str) -> Result<bool, ApiError> {
    let status =
        herald_core::third::email::EmailService::is_email_configured(&state.pool, realm_id)
            .await
            .map_err(|e| {
                tracing::error!("Failed to query email config: {e}");
                ApiError::internal("Failed to query email config")
            })?;

    Ok(status.configured)
}

/// Deserialized shape of the `email_otp` / `settings` config_value JSON
/// (design email-otp-login §5.1). Both fields default to false when absent
/// so a partial or legacy payload degrades to "disabled".
#[derive(serde::Deserialize, Default)]
#[serde(default)]
pub struct EmailOtpSettings {
    pub enabled: bool,
    pub auto_register: bool,
}

/// Load the `email_otp` settings JSON for a realm.
///
/// Reads `realm_config` where `config_type='email_otp'`,
/// `config_key='settings'`, and the row is `enabled=true`. Returns the
/// parsed [`EmailOtpSettings`] (or the disabled default) for the requested
/// realm; never returns an error for a missing or malformed payload — those
/// cases simply yield the all-false default. Callers that need both flags
/// should call this once rather than via the single-flag helpers below, to
/// avoid reading the same row twice.
pub async fn load_email_otp_settings(
    state: &AppState,
    realm_id: &str,
) -> Result<EmailOtpSettings, ApiError> {
    let row = sqlx::query_as::<_, (String,)>(
        "SELECT config_value FROM realm_config
         WHERE realm_id = $1 AND config_type = 'email_otp' AND config_key = 'settings' AND enabled = true",
    )
    .bind(realm_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to query email OTP config: {e}");
        ApiError::internal("Failed to query email OTP config")
    })?;

    let Some((raw,)) = row else {
        return Ok(EmailOtpSettings::default());
    };

    let settings: EmailOtpSettings = serde_json::from_str(&raw).unwrap_or_else(|e| {
        tracing::error!("Failed to parse email OTP settings JSON: {e}; using default");
        EmailOtpSettings::default()
    });

    Ok(settings)
}

/// Check if email OTP login is enabled for a realm (design §5.1).
///
/// Returns true if the `email_otp` / `settings` config row is active and its
/// JSON `enabled` field is `true`. Defaults to false when the config is
/// absent or malformed (opt-in per realm).
pub async fn is_email_otp_enabled(state: &AppState, realm_id: &str) -> Result<bool, ApiError> {
    let settings = load_email_otp_settings(state, realm_id).await?;
    Ok(settings.enabled)
}

/// Parse and gate the `ldap/settings` JSON: `None` when malformed, not
/// enabled, or the credential channel is insecure (legacy `ldap://` rows
/// without StartTLS fail closed). Shared by the full config load (login
/// path) and the enablement-only read (status path).
fn parse_enabled_ldap_settings(raw: &str) -> Option<LdapDirectorySettings> {
    let settings: LdapDirectorySettings = serde_json::from_str(raw).unwrap_or_else(|e| {
        tracing::error!("Failed to parse LDAP settings JSON: {e}; treating as disabled");
        LdapDirectorySettings::default()
    });
    (settings.enabled && settings.is_credential_channel_secure()).then_some(settings)
}

/// Load the LDAP directory login configuration for a realm (design
/// support-ldap §9.2): the `ldap/settings` row plus the separately-stored
/// `ldap/bind_password` row.
///
/// Returns `Ok(None)` — treated as "not enabled" by both `ldap_status`
/// (enabled:false) and the login gate (400) — when the settings row is
/// missing, malformed, not enabled, or carries an insecure credential
/// channel (`ldap://` without StartTLS legacy rows fail closed).
pub async fn load_ldap_config(
    state: &AppState,
    realm_id: &str,
) -> Result<Option<LdapLoginConfig>, ApiError> {
    // Both keys are known upfront; one round trip instead of two serial ones.
    // No row-level `enabled` filter: the JSON `enabled` field is the sole
    // enablement signal by design (§4.2.3); the row-level column is
    // display redundancy only.
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT config_key, config_value FROM realm_config
         WHERE realm_id = $1 AND config_type = 'ldap'
           AND config_key IN ('settings', 'bind_password')",
    )
    .bind(realm_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to query LDAP config: {e}");
        ApiError::internal("Failed to query LDAP config")
    })?;

    let mut raw_settings: Option<String> = None;
    let mut bind_password: Option<String> = None;
    for (key, value) in rows {
        match key.as_str() {
            "settings" => raw_settings = Some(value),
            "bind_password" => bind_password = Some(value),
            _ => {}
        }
    }

    let settings = raw_settings
        .as_deref()
        .and_then(parse_enabled_ldap_settings);

    let Some(settings) = settings else {
        return Ok(None);
    };

    // An absent bind_password row + configured bind_dn is a directory
    // misconfiguration the adapter reports as "unavailable" (the adapter
    // owns that classification).
    Ok(Some(LdapLoginConfig {
        settings,
        bind_password,
    }))
}

/// Public enablement signal for `GET /api/auth/{realmId}/ldap/status` —
/// true only when a well-formed, enabled, encrypted-channel `ldap/settings`
/// row exists (fail-closed, mirrors `is_email_otp_enabled`).
///
/// Reads only the settings row: the unauthenticated status endpoint must not
/// load the `bind_password` secret.
pub async fn is_ldap_enabled(state: &AppState, realm_id: &str) -> Result<bool, ApiError> {
    let row = sqlx::query_as::<_, (String,)>(
        "SELECT config_value FROM realm_config
         WHERE realm_id = $1 AND config_type = 'ldap' AND config_key = 'settings'",
    )
    .bind(realm_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to query LDAP settings config: {e}");
        ApiError::internal("Failed to query LDAP config")
    })?;

    Ok(row
        .and_then(|(raw,)| parse_enabled_ldap_settings(&raw))
        .is_some())
}

/// Check if a user has a specific permission with timeout
///
/// This is a centralized helper for permission checking across the application.
/// It provides consistent timeout handling and error conversion.
///
/// # Arguments
/// * `state` - Application state containing the permission checker
/// * `realm_id` - Realm ID for the permission check
/// * `user_id` - User ID to check permissions for
/// # Arguments
/// * `state` - Application state containing the permission checker
/// * `realm_id` - Realm ID for the permission check
/// * `user_id` - User ID to check permissions for
/// * `resource` - Resource identifier (e.g., "users", "roles")
/// * `action` - Action identifier (e.g., "view", "manage")
///
/// # Returns
/// * `Ok(true)` if permission is granted
/// * `Ok(false)` if permission is denied
/// * `Err(ApiError)` if an error occurs
pub async fn check_permission_with_timeout(
    state: &AppState,
    realm_id: &str,
    user_id: &str,
    resource: &str,
    action: &str,
) -> Result<bool, ApiError> {
    let permission_checker = &state.permission_checker;

    let result = timeout(
        Duration::from_secs(5),
        permission_checker.check_permission(realm_id, user_id, resource, action),
    )
    .await
    .map_err(|e| ApiError::internal(format!("Permission check timeout: {}", e)))?;

    result.map_err(|e| ApiError::internal(format!("Permission check failed: {}", e)))
}

/// Require a specific permission, returning Forbidden error if not granted
///
/// This is a convenience wrapper around `check_permission_with_timeout` that
/// automatically returns a Forbidden error when permission is denied.
///
/// # Arguments
/// * `state` - Application state
/// * `realm_id` - Realm ID for the permission check
/// * `user_id` - User ID to check permissions for
/// * `resource` - Resource identifier
/// * `action` - Action identifier
/// * `permission_name` - Human-readable permission name for error messages
///   (e.g., "permissions.manage")
///
/// # Returns
/// * `Ok(())` if permission is granted
/// * `Err(ApiError::Forbidden)` if permission is denied
/// * `Err(ApiError)` if an error occurs
pub async fn require_permission(
    state: &AppState,
    realm_id: &str,
    user_id: &str,
    resource: &str,
    action: &str,
    permission_name: &str,
) -> Result<(), ApiError> {
    let has_permission =
        check_permission_with_timeout(state, realm_id, user_id, resource, action).await?;

    if !has_permission {
        return Err(ApiError::forbidden(format!(
            "Missing {permission_name} permission"
        )));
    }

    Ok(())
}
