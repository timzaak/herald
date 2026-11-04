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

    #[test]
    fn disposable_email_list_matches_flagship_disposable_providers() {
        // The defense IS the list's coverage: these are long-lived flagship
        // disposable providers, so a miss means the bundled file was replaced
        // or truncated and no longer covers the threat it ships for.
        for domain in ["mailinator.com", "10minutemail.com", "guerrillamail.com"] {
            assert!(
                is_disposable_email(&format!("user@{domain}")),
                "{domain} must be in the bundled list"
            );
        }
    }

    #[test]
    fn disposable_email_list_is_substantially_sized() {
        // Upstream currently ships ~9,900 domains. A floor of 3,000 pins that
        // an update did not quietly replace the file with a stub while still
        // passing the flagship-domain test above.
        assert!(DISPOSABLE_EMAIL_DOMAINS.len() > 3_000);
    }

    #[test]
    fn disposable_email_check_spares_ordinary_domains() {
        assert!(!is_disposable_email("user@gmail.com"));
        assert!(!is_disposable_email("user@example.com"));
    }

    #[test]
    fn disposable_email_check_normalizes_domain_case() {
        // Call sites hand in normalize_email output, but the check must not
        // depend on that to stay correct.
        assert!(is_disposable_email("USER@MAILINATOR.COM"));
    }

    #[test]
    fn disposable_email_check_requires_an_at() {
        // A bare domain is not an email address; upstream validators already
        // reject it, and this check must never fire on non-emails.
        assert!(!is_disposable_email("mailinator.com"));
    }
}

pub fn normalize_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

pub use herald_core::domain::user::phone::{
    is_supported_country_code, mask_phone, normalize_phone,
};

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
        if herald_core::config::is_production(&state.app_env) {
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

/// Check the optional registration email-domain allowlist. A missing or blank
/// `allowed_domains` row means unrestricted registration; otherwise the
/// normalized email domain must exactly match one comma-separated entry.
pub async fn is_registration_email_domain_allowed(
    state: &AppState,
    realm_id: &str,
    email: &str,
) -> Result<bool, ApiError> {
    let configured: Option<String> = sqlx::query_scalar(
        "SELECT config_value FROM realm_config
         WHERE realm_id = $1
           AND config_type = 'registration'
           AND config_key = 'allowed_domains'
           AND enabled = true",
    )
    .bind(realm_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| {
        tracing::error!(%error, %realm_id, "Failed to query registration domain allowlist");
        ApiError::internal("Failed to query registration config")
    })?;

    let Some(configured) = configured.filter(|value| !value.trim().is_empty()) else {
        return Ok(true);
    };
    let Some(email_domain) = normalized_email_domain(email) else {
        return Ok(false);
    };
    Ok(configured
        .split(',')
        .map(|domain| domain.trim().trim_start_matches('@').to_ascii_lowercase())
        .any(|domain| !domain.is_empty() && domain == email_domain))
}

/// Bundled disposable (temporary-mailbox) email domain list, compiled in via
/// `include_str!` from OUT_DIR. The CC0 upstream list is fetched at build
/// time by this crate's `build.rs` from a pinned upstream commit (sha256
/// verified) and is not stored in the repository — provenance and update
/// instructions live in `backend/api-base/build.rs`. Built once on first
/// use; lookups are pure in-memory O(1). Entries are ASCII-lowercased at
/// load so the lowercased query side keeps matching even if a future pin
/// bump pulls mixed-case entries from upstream.
static DISPOSABLE_EMAIL_DOMAINS: std::sync::LazyLock<std::collections::HashSet<String>> =
    std::sync::LazyLock::new(|| {
        include_str!(concat!(env!("OUT_DIR"), "/disposable_email_blocklist.conf"))
            .lines()
            .map(|line| line.trim().to_ascii_lowercase())
            .filter(|domain| !domain.is_empty() && !domain.starts_with('#'))
            .collect()
    });

/// The email's domain for exact-match comparisons: the part after the last
/// `@`, trimmed + ASCII-lowercased, no subdomain wildcarding. `None` when the
/// address has no `@`. Shared by the registration allowlist and the bundled
/// disposable-domain list — the realm-settings PRD pins both gates to the
/// same exact-match semantics, so they must not normalize independently.
fn normalized_email_domain(email: &str) -> Option<String> {
    email
        .rsplit_once('@')
        .map(|(_, domain)| domain.trim().to_ascii_lowercase())
}

/// Whether the email's domain is in the bundled disposable-domain list.
/// Same normalization as the registration allowlist: the part after the last
/// `@`, trimmed + ASCII-lowercased, exact match, no subdomain wildcarding.
/// An email without `@` can never match.
pub fn is_disposable_email(email: &str) -> bool {
    normalized_email_domain(email)
        .is_some_and(|domain| DISPOSABLE_EMAIL_DOMAINS.contains(domain.as_str()))
}

/// Check the optional registration `block_disposable_domains` toggle against
/// the bundled disposable-domain list. A missing or non-true row disables the
/// check — deployments that never set the toggle keep today's behavior, so
/// the default can never reject an address. The in-memory list lookup runs
/// first: ordinary domains (the overwhelming majority of traffic, including
/// every realm that never set the toggle) skip the config round-trip.
pub async fn is_registration_disposable_email_blocked(
    state: &AppState,
    realm_id: &str,
    email: &str,
) -> Result<bool, ApiError> {
    if !is_disposable_email(email) {
        return Ok(false);
    }
    let row = sqlx::query_scalar::<_, String>(
        "SELECT config_value FROM realm_config
         WHERE realm_id = $1
           AND config_type = 'registration'
           AND config_key = 'block_disposable_domains'
           AND enabled = true",
    )
    .bind(realm_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| {
        tracing::error!(%error, %realm_id, "Failed to query block_disposable_domains config");
        ApiError::internal("Failed to query registration config")
    })?;

    let enabled = row
        .map(|value| matches!(value.to_lowercase().as_str(), "true" | "1" | "yes"))
        .unwrap_or(false);
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

/// Deserialized shape of the `email_otp` / `settings` config_value JSON.
/// Both fields default to false when absent
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

/// Check if email OTP login is enabled for a realm.
///
/// Returns true if the `email_otp` / `settings` config row is active and its
/// JSON `enabled` field is `true`. Defaults to false when the config is
/// absent or malformed (opt-in per realm).
pub async fn is_email_otp_enabled(state: &AppState, realm_id: &str) -> Result<bool, ApiError> {
    let settings = load_email_otp_settings(state, realm_id).await?;
    Ok(settings.enabled)
}

/// `phone_login` / `settings` config_value JSON. `#[serde(default)]` is
/// fail-closed: missing `enabled` → disabled; a missing country code falls
/// back to `CN` (an empty region would 400 every `normalize_phone` in the
/// realm while the status endpoint still reports enabled).
#[derive(serde::Deserialize, Default, serde::Serialize)]
#[serde(default)]
pub struct PhoneLoginSettings {
    pub enabled: bool,
    #[serde(default = "phone_login_default_country_code")]
    pub default_country_code: String,
}

fn phone_login_default_country_code() -> String {
    "CN".to_string()
}

impl PhoneLoginSettings {
    pub fn disabled_default() -> Self {
        Self {
            enabled: false,
            default_country_code: phone_login_default_country_code(),
        }
    }
}

/// Fail-closed: a missing, disabled, or malformed row yields the disabled default.
pub async fn load_phone_login_settings(
    state: &AppState,
    realm_id: &str,
) -> Result<PhoneLoginSettings, ApiError> {
    let row = sqlx::query_as::<_, (String,)>(
        "SELECT config_value FROM realm_config
         WHERE realm_id = $1 AND config_type = 'phone_login' AND config_key = 'settings' AND enabled = true",
    )
    .bind(realm_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to query phone login config: {e}");
        ApiError::internal("Failed to query phone login config")
    })?;

    let Some((raw,)) = row else {
        return Ok(PhoneLoginSettings::disabled_default());
    };

    let settings: PhoneLoginSettings = serde_json::from_str(&raw).unwrap_or_else(|e| {
        tracing::error!("Failed to parse phone login settings JSON: {e}; using default");
        PhoneLoginSettings::disabled_default()
    });
    Ok(settings)
}

pub async fn is_phone_login_enabled(state: &AppState, realm_id: &str) -> Result<bool, ApiError> {
    let settings = load_phone_login_settings(state, realm_id).await?;
    Ok(settings.enabled)
}

pub async fn is_sms_configured(state: &AppState, realm_id: &str) -> Result<bool, ApiError> {
    herald_core::third::sms::SmsService::is_sms_configured(&state.pool, realm_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to query SMS config: {e}");
            ApiError::internal("Failed to query SMS config")
        })
}

// Shared by the login, bind and admin-rebind OTP families: the claim
// protocol and constant-time comparison must exist exactly once.

/// Stored SMS OTP payload (Redis JSON): plaintext code in the same trust
/// zone as session tokens; attempts live in a separate INCR key.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct StoredOtp {
    pub code: String,
    pub max_attempts: i64,
    /// Absolute (epoch ms) so a mismatch-restore re-applies the original TTL.
    pub expires_at_ms: u64,
}

impl OtpRecord for StoredOtp {
    fn code(&self) -> &str {
        &self.code
    }
    fn max_attempts(&self) -> i64 {
        self.max_attempts
    }
    fn expires_at_ms(&self) -> u64 {
        self.expires_at_ms
    }
}

/// Lets OTP families with extra payload fields (admin-rebind's `new_phone`)
/// share the same claim/store primitives.
pub trait OtpRecord: serde::Serialize + serde::de::DeserializeOwned {
    fn code(&self) -> &str;
    fn max_attempts(&self) -> i64;
    fn expires_at_ms(&self) -> u64;
}

pub fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Constant-time comparison; every SMS OTP code check must go through this,
/// never `==`.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a_bytes, b_bytes) = (a.as_bytes(), b.as_bytes());
    if a_bytes.len() != b_bytes.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a_bytes.iter().zip(b_bytes.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Why a code attempt failed. All variants map to 400 with distinguishing
/// message text; callers with family-specific wording map the variants
/// themselves instead of using [`sms_otp_failure_error`].
pub enum SmsOtpFailure {
    Expired,
    Invalid,
    Exhausted,
}

pub fn sms_otp_failure_error(failure: SmsOtpFailure) -> ApiError {
    let message = match failure {
        SmsOtpFailure::Expired => "Verification code expired, please request a new one",
        SmsOtpFailure::Invalid => "Invalid or expired verification code",
        SmsOtpFailure::Exhausted => "Too many attempts, please request a new code",
    };
    ApiError::bad_request(message)
}

/// Atomic GET+DEL claim: two concurrent requests presenting the same code
/// cannot both consume it. On mismatch the code is restored (unless the
/// attempt budget ran out); attempts count in a separate INCR key so
/// concurrent guesses cannot undercount.
pub async fn claim_sms_otp_code<T: OtpRecord>(
    conn: &mut redis::aio::ConnectionManager,
    key: &str,
    attempts_key: &str,
    code: &str,
) -> Result<Result<T, SmsOtpFailure>, ApiError> {
    use redis::AsyncCommands;
    let redis_err = |e: redis::RedisError| {
        tracing::error!(error = %e, "Redis operation error in SMS OTP claim");
        ApiError::internal("Redis operation error")
    };
    let (stored_raw, _deleted): (Option<String>, i64) = redis::pipe()
        .atomic()
        .cmd("GET")
        .arg(key)
        .cmd("DEL")
        .arg(key)
        .query_async(conn)
        .await
        .map_err(redis_err)?;
    let Some(stored_json) = stored_raw else {
        return Ok(Err(SmsOtpFailure::Expired));
    };
    let stored: T = serde_json::from_str(&stored_json).map_err(|e| {
        tracing::error!(error = %e, "Failed to parse stored SMS OTP JSON");
        ApiError::internal("Redis operation error")
    })?;

    if constant_time_eq(code, stored.code()) {
        let _: () = conn.del(attempts_key).await.map_err(redis_err)?;
        return Ok(Ok(stored));
    }

    let attempts: i64 = conn.incr(attempts_key, 1).await.map_err(redis_err)?;
    if attempts == 1 {
        // The counter must not outlive the code: recompute the TTL from the
        // stored absolute expiry (post-claim TTL is negative); `.max(1)`
        // avoids leaving the counter with no TTL at all.
        let remaining_secs = (stored.expires_at_ms().saturating_sub(now_epoch_ms()) / 1000).max(1);
        let _: () = conn
            .expire(attempts_key, remaining_secs as i64)
            .await
            .map_err(redis_err)?;
    }
    if attempts >= stored.max_attempts() {
        // Burned: drop the counter too so the user must request a new code.
        let _: () = conn.del(attempts_key).await.map_err(redis_err)?;
        return Ok(Err(SmsOtpFailure::Exhausted));
    }
    // NX so a newer code is never clobbered; a failed restore burns the code.
    let remaining_ms = stored.expires_at_ms().saturating_sub(now_epoch_ms());
    if remaining_ms > 0 {
        let restore: Result<(), _> = redis::pipe()
            .atomic()
            .cmd("SET")
            .arg(key)
            .arg(&stored_json)
            .arg("NX")
            .arg("PX")
            .arg(remaining_ms)
            .query_async(conn)
            .await;
        if let Err(e) = restore {
            tracing::warn!(error = %e, "Failed to restore claimed SMS OTP code");
        }
    }
    Ok(Err(SmsOtpFailure::Invalid))
}

/// Newest-wins SET EX plus a cleared attempt counter: a leftover counter
/// from the previous code must not count against this one.
pub async fn store_otp_record<T: OtpRecord>(
    conn: &mut redis::aio::ConnectionManager,
    key: &str,
    attempts_key: &str,
    record: &T,
    ttl_secs: u64,
) -> Result<(), ApiError> {
    use redis::AsyncCommands;
    let stored_json = serde_json::to_string(record)
        .map_err(|_| ApiError::internal("Failed to serialize OTP state"))?;
    let _: () = conn.set_ex(key, stored_json, ttl_secs).await.map_err(|e| {
        tracing::error!(error = %e, "Failed to store SMS OTP code in Redis");
        ApiError::internal("Redis operation error")
    })?;
    let _: () = conn.del(attempts_key).await.map_err(|e| {
        tracing::error!(error = %e, "Failed to reset SMS OTP attempt counter");
        ApiError::internal("Redis operation error")
    })?;
    Ok(())
}

pub async fn store_sms_otp_code(
    conn: &mut redis::aio::ConnectionManager,
    key: &str,
    attempts_key: &str,
    code: &str,
) -> Result<(), ApiError> {
    let record = StoredOtp {
        code: code.to_string(),
        max_attempts: herald_core::domain::security_constants::SMS_OTP_MAX_ATTEMPTS,
        expires_at_ms: now_epoch_ms()
            + herald_core::domain::security_constants::SMS_OTP_CODE_TTL_SECONDS * 1000,
    };
    store_otp_record(
        conn,
        key,
        attempts_key,
        &record,
        herald_core::domain::security_constants::SMS_OTP_CODE_TTL_SECONDS,
    )
    .await
}

/// Unconfigured channel → 400 explicit rejection (no fake success);
/// upstream failure → 502. All send paths map through this so status codes
/// and wording cannot drift between entrances.
pub fn map_sms_send_error(e: herald_core::third::sms::SmsError) -> ApiError {
    match e {
        herald_core::third::sms::SmsError::NotConfigured => {
            ApiError::bad_request("SMS channel is not configured")
        }
        herald_core::third::sms::SmsError::UnsupportedDestination => {
            ApiError::bad_request("Destination is not supported by the configured SMS provider")
        }
        herald_core::third::sms::SmsError::Upstream(_) => ApiError::new(
            axum::http::StatusCode::BAD_GATEWAY,
            "Upstream service unavailable",
        ),
    }
}

/// 409 body for `phone_already_in_use`, shared by the self-service bind and
/// admin bind/rebind paths; the frontend and tests key on this code string.
pub fn phone_in_use_conflict() -> ApiError {
    ApiError::with_json(
        axum::http::StatusCode::CONFLICT,
        serde_json::json!({
            "code": "phone_already_in_use",
            "message": "Phone number is already in use"
        }),
    )
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

/// Load the LDAP directory login configuration for a realm:
/// the `ldap/settings` row plus the separately-stored
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
    // enablement signal; the row-level column is
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
        // Audit PRD §4.2: denied (403) operations must also produce an audit
        // event marked as failed. Best-effort — the 403 is already decided.
        crate::application::http::common::auth_utils::record_permission_denied_audit(
            state, realm_id, user_id, None, resource, action,
        )
        .await;
        return Err(ApiError::forbidden(format!(
            "Missing {permission_name} permission"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod sms_otp_tests {
    use super::*;

    // WHY: the stored JSON is a live wire format — a serde rename or a switch
    // to a hashed code would invalidate every in-flight stored code.
    #[test]
    fn stored_otp_round_trips_plaintext_code() {
        let stored = StoredOtp {
            code: "123456".to_string(),
            max_attempts: 5,
            expires_at_ms: 1_700_000_000_000,
        };
        let json = serde_json::to_string(&stored).unwrap();
        assert!(
            json.contains("\"code\":\"123456\""),
            "stored JSON must carry the plaintext code; got {json}"
        );
        let back: StoredOtp = serde_json::from_str(&json).unwrap();
        assert_eq!(back.code, "123456");
        assert_eq!(back.max_attempts, 5);
    }

    // WHY: must agree with plain equality while never short-circuiting
    // (timing side-channel).
    #[test]
    fn constant_time_eq_matches_equality() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(constant_time_eq("123456", "123456"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
        assert!(!constant_time_eq("123456", "654321"));
    }

    // WHY: the 400/502 mapping is shared by every send path in three crates;
    // drift here would change the status code of one entrance only.
    #[test]
    fn sms_send_error_mapping_keeps_the_contract() {
        assert_eq!(
            map_sms_send_error(herald_core::third::sms::SmsError::NotConfigured).status(),
            axum::http::StatusCode::BAD_REQUEST
        );
        assert_eq!(
            map_sms_send_error(herald_core::third::sms::SmsError::UnsupportedDestination).status(),
            axum::http::StatusCode::BAD_REQUEST
        );
        assert_eq!(
            map_sms_send_error(herald_core::third::sms::SmsError::Upstream(
                "signature=s3cret".into()
            ))
            .status(),
            axum::http::StatusCode::BAD_GATEWAY
        );
    }

    // WHY: a missing country code must fall back to CN, not "" — an empty
    // region 400s every phone in the realm while status reports enabled.
    #[test]
    fn phone_login_settings_missing_country_falls_back_to_cn() {
        let settings: PhoneLoginSettings = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
        assert!(settings.enabled);
        assert_eq!(settings.default_country_code, "CN");
        assert_eq!(
            PhoneLoginSettings::disabled_default().default_country_code,
            "CN"
        );
    }
}
