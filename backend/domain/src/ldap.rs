// LDAP enterprise-directory login (design support-ldap).
//
// The realm_config `ldap/settings` row shape, the write-path validation, the
// search-filter rendering, and the `LdapAuthenticator` port implemented by
// the infra ldap3 adapter and replaced by a mock in scenario tests.

use crate::common::entities::app_errors::CoreError;

/// realm_config `ldap/settings` config_value JSON shape, shared by the login
/// path and the configs-CRUD write validation.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct LdapDirectorySettings {
    /// Sole enablement signal for both `ldap_status` and the login gate.
    pub enabled: bool,
    /// `ldap://` (requires `starttls=true`) or `ldaps://`.
    pub url: String,
    pub starttls: bool,
    pub base_dn: String,
    /// `None` = anonymous search.
    pub bind_dn: Option<String>,
    /// Filter template with exactly one `{login}` placeholder.
    pub user_filter: String,
    /// Directory attribute carrying the mail value. Defaults to `mail`.
    pub mail_attribute: String,
    /// Optional directory attribute carrying the display name (e.g.
    /// `displayName` / `cn`). When set, its value seeds the JIT-created
    /// account's nickname; entries without it simply get no nickname.
    /// `None` = no display-name mapping configured.
    pub display_name_attribute: Option<String>,
    /// Optional PEM certificate bundle (the directory's private CA) to trust
    /// for this realm's LDAP TLS connections, ADDITIVE on top of the system
    /// trust store. A CA certificate is public material — no secret handling.
    pub ca_cert_pem: Option<String>,
}

impl LdapDirectorySettings {
    /// Fail-closed runtime check for legacy rows written before the
    /// write-path validation existed: a plaintext `ldap://` URL with
    /// `starttls=false` must be treated as not enabled (design §7).
    pub fn is_credential_channel_secure(&self) -> bool {
        self.url.starts_with("ldaps://") || self.starttls
    }
}

/// Full authentication input assembled per request: the settings row plus the
/// separately-stored service-account password (never part of the settings
/// JSON).
#[derive(Debug, Clone)]
pub struct LdapLoginConfig {
    pub settings: LdapDirectorySettings,
    pub bind_password: Option<String>,
}

/// Successful directory authentication result.
#[derive(Debug, Clone)]
pub struct LdapAuthenticatedUser {
    pub dn: String,
    /// Value of the configured mail attribute; raw (not normalized).
    pub email: Option<String>,
    /// Value of the configured display-name attribute (when
    /// `display_name_attribute` is set and the entry carries it).
    pub display_name: Option<String>,
}

/// Error classification for the LDAP authentication port.
#[derive(Debug)]
pub enum LdapAuthError {
    /// Zero search hits, multiple hits, or the user bind was rejected —
    /// always generalized to 401 invalid credentials (anti-enumeration).
    InvalidCredentials,
    /// Connection failure / timeout / protocol error — 503; the payload
    /// carries the adapter error detail for tracing only, never the response.
    Unavailable(String),
}

/// Port for directory authentication (search-then-bind). Object-safe via
/// boxed futures (same pattern as `billing::WebhookEventProcessor`) so
/// `Arc<dyn LdapAuthenticator>` can be injected into AppState and swapped
/// for a mock in scenario tests.
pub trait LdapAuthenticator: Send + Sync {
    fn authenticate<'a>(
        &'a self,
        config: &'a LdapLoginConfig,
        username: &'a str,
        password: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<LdapAuthenticatedUser, LdapAuthError>>
                + Send
                + 'a,
        >,
    >;
}

/// Validate an admin-submitted `ldap/settings` JSON payload (design §4.2.3).
///
/// Err carries a field-specific message — this is the admin configuration
/// surface, so no anti-enumeration generalization applies.
pub fn validate_ldap_settings_json(raw: &str) -> Result<LdapDirectorySettings, CoreError> {
    let mut settings: LdapDirectorySettings = serde_json::from_str(raw)
        .map_err(|e| CoreError::BadRequest(format!("invalid LDAP settings JSON: {e}")))?;

    if settings.url.len() > 512 {
        return Err(CoreError::BadRequest(
            "url must be at most 512 characters".to_string(),
        ));
    }
    let parsed = url::Url::parse(&settings.url)
        .map_err(|_| CoreError::BadRequest(format!("invalid LDAP url: {}", settings.url)))?;
    match parsed.scheme() {
        "ldap" | "ldaps" => {}
        other => {
            return Err(CoreError::BadRequest(format!(
                "LDAP url scheme must be ldap or ldaps, got: {other}"
            )));
        }
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return Err(CoreError::BadRequest(
            "LDAP url must include a host".to_string(),
        ));
    }
    // Userinfo, query, and fragment are rejected: they smuggle credentials or
    // extra parameters past the dedicated fields (bind_dn / bind_password).
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(CoreError::BadRequest(
            "LDAP url must not contain userinfo, query, or fragment".to_string(),
        ));
    }

    // Credential-channel hard rule: the enterprise password only travels on
    // an encrypted channel (US-LD-003 scenario 5).
    match (parsed.scheme(), settings.starttls) {
        ("ldaps", false) => {}
        ("ldaps", true) => {
            return Err(CoreError::BadRequest(
                "starttls must be false for ldaps:// URLs (TLS is provided by the scheme)"
                    .to_string(),
            ));
        }
        ("ldap", true) => {}
        _ => {
            return Err(CoreError::BadRequest(
                "plaintext LDAP is not allowed; use ldaps:// or enable StartTLS".to_string(),
            ));
        }
    }

    if settings.base_dn.trim().is_empty() {
        return Err(CoreError::BadRequest("baseDn is required".to_string()));
    }
    if settings.base_dn.len() > 512 {
        return Err(CoreError::BadRequest(
            "baseDn must be at most 512 characters".to_string(),
        ));
    }
    if let Some(bind_dn) = settings.bind_dn.as_deref() {
        if bind_dn.trim().is_empty() {
            return Err(CoreError::BadRequest(
                "bindDn must not be empty when present; omit it for anonymous search".to_string(),
            ));
        }
        if bind_dn.len() > 512 {
            return Err(CoreError::BadRequest(
                "bindDn must be at most 512 characters".to_string(),
            ));
        }
    }

    if settings.user_filter.trim().is_empty() {
        return Err(CoreError::BadRequest("userFilter is required".to_string()));
    }
    if settings.user_filter.len() > 512 {
        return Err(CoreError::BadRequest(
            "userFilter must be at most 512 characters".to_string(),
        ));
    }
    let placeholder_count = settings.user_filter.matches("{login}").count();
    if placeholder_count != 1 {
        return Err(CoreError::BadRequest(format!(
            "userFilter must contain exactly one {{login}} placeholder, found {placeholder_count}"
        )));
    }
    let open = settings.user_filter.matches('(').count();
    let close = settings.user_filter.matches(')').count();
    if open != close || open == 0 {
        return Err(CoreError::BadRequest(
            "userFilter parentheses are unbalanced".to_string(),
        ));
    }
    if settings.mail_attribute.is_empty() {
        // Deserialize with `default` yields "" when the field is absent; the
        // canonical default is `mail`.
        settings.mail_attribute = "mail".to_string();
    }
    if settings.mail_attribute.len() > 64
        || !settings
            .mail_attribute
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(CoreError::BadRequest(
            "mailAttribute must be at most 64 characters of [A-Za-z0-9-]".to_string(),
        ));
    }

    // Optional display-name mapping (seeds the JIT account nickname).
    // Whitespace-only input is normalized to None; same attribute-name shape
    // as mailAttribute (an attribute name is never an injection vector, it
    // only selects which directory value to read).
    settings.display_name_attribute = settings
        .display_name_attribute
        .take()
        .map(|attr| attr.trim().to_string())
        .filter(|attr| !attr.is_empty());
    if let Some(attr) = &settings.display_name_attribute
        && (attr.len() > 64 || !attr.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
    {
        return Err(CoreError::BadRequest(
            "displayNameAttribute must be at most 64 characters of [A-Za-z0-9-]".to_string(),
        ));
    }

    // Optional private-CA trust. Whitespace-only input is normalized to None;
    // anything else must at least look like a PEM certificate bundle. Full
    // parsing happens in the infra adapter when the trust store is built.
    let ca_pem = settings
        .ca_cert_pem
        .take()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());
    if let Some(pem) = &ca_pem {
        if pem.len() > 32_768 {
            return Err(CoreError::BadRequest(
                "caCertPem must be at most 32768 characters".to_string(),
            ));
        }
        if !pem.contains("-----BEGIN CERTIFICATE-----") {
            return Err(CoreError::BadRequest(
                "caCertPem must be a PEM certificate bundle".to_string(),
            ));
        }
    }
    settings.ca_cert_pem = ca_pem;

    Ok(settings)
}

/// Render the user search filter by replacing the single `{login}` placeholder
/// with the already-escaped username. This crate does not depend on ldap3;
/// escaping is performed by the caller (infra adapter) via `ldap3::ldap_escape`.
pub fn render_user_filter(template: &str, escaped_username: &str) -> Result<String, CoreError> {
    if template.matches("{login}").count() != 1 {
        return Err(CoreError::BadRequest(
            "user filter template must contain exactly one {login} placeholder".to_string(),
        ));
    }
    // ldap3::ldap_escape covers the filter metacharacters but NUL cannot
    // appear in a valid filter literal at all; refuse it here so a bypass of
    // the adapter-side escaping cannot smuggle one in.
    if escaped_username.contains('\0') {
        return Err(CoreError::BadRequest(
            "username contains a NUL character".to_string(),
        ));
    }
    Ok(template.replacen("{login}", escaped_username, 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_settings_json() -> String {
        serde_json::json!({
            "enabled": true,
            "url": "ldaps://ldap.example.com:636",
            "baseDn": "dc=example,dc=com",
            "userFilter": "(&(objectClass=user)(sAMAccountName={login}))",
        })
        .to_string()
    }

    #[test]
    fn accepts_valid_settings_and_fills_defaults() {
        let settings = validate_ldap_settings_json(&valid_settings_json()).unwrap();
        assert!(settings.enabled);
        assert!(!settings.starttls);
        assert_eq!(settings.mail_attribute, "mail");
        assert_eq!(settings.bind_dn, None);
        assert_eq!(settings.ca_cert_pem, None);
    }

    #[test]
    fn accepts_ca_cert_pem_and_normalizes_blank_to_none() {
        let pem = "-----BEGIN CERTIFICATE-----\nMIIFazCCA1OgAwIBAgIRAIIQz7DSQONZRGPgu2OCiwAwDQYJKoZIhvcNAQEL\n-----END CERTIFICATE-----";
        let raw = serde_json::json!({
            "enabled": true,
            "url": "ldaps://ldap.example.com:636",
            "baseDn": "dc=example,dc=com",
            "userFilter": "(uid={login})",
            "caCertPem": format!("  {pem}\n"),
        })
        .to_string();
        let settings = validate_ldap_settings_json(&raw).unwrap();
        assert_eq!(settings.ca_cert_pem.as_deref(), Some(pem));

        let raw = serde_json::json!({
            "enabled": true,
            "url": "ldaps://ldap.example.com:636",
            "baseDn": "dc=example,dc=com",
            "userFilter": "(uid={login})",
            "caCertPem": "   ",
        })
        .to_string();
        let settings = validate_ldap_settings_json(&raw).unwrap();
        assert_eq!(settings.ca_cert_pem, None, "blank must normalize to None");
    }

    #[test]
    fn rejects_ca_cert_pem_without_pem_marker() {
        let raw = serde_json::json!({
            "enabled": true,
            "url": "ldaps://ldap.example.com:636",
            "baseDn": "dc=example,dc=com",
            "userFilter": "(uid={login})",
            "caCertPem": "definitely not a certificate",
        })
        .to_string();
        let err = validate_ldap_settings_json(&raw).unwrap_err();
        assert!(err.to_string().contains("caCertPem"), "got: {err}");
    }

    #[test]
    fn rejects_oversized_ca_cert_pem() {
        let oversized = format!(
            "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----",
            "A".repeat(33_000)
        );
        let raw = serde_json::json!({
            "enabled": true,
            "url": "ldaps://ldap.example.com:636",
            "baseDn": "dc=example,dc=com",
            "userFilter": "(uid={login})",
            "caCertPem": oversized,
        })
        .to_string();
        let err = validate_ldap_settings_json(&raw).unwrap_err();
        assert!(err.to_string().contains("32768"), "got: {err}");
    }

    #[test]
    fn rejects_ldaps_with_starttls_enabled() {
        let raw = serde_json::json!({
            "enabled": true,
            "url": "ldaps://ldap.example.com",
            "starttls": true,
            "baseDn": "dc=example,dc=com",
            "userFilter": "(uid={login})",
        })
        .to_string();
        let err = validate_ldap_settings_json(&raw).unwrap_err();
        assert!(err.to_string().contains("starttls"), "got: {err}");
    }

    #[test]
    fn rejects_plaintext_ldap_without_starttls() {
        // WHY: the enterprise password must never travel on a plaintext
        // channel; this is the rule-level assertion for US-LD-003 scenario 5.
        let raw = serde_json::json!({
            "enabled": true,
            "url": "ldap://ldap.example.com",
            "starttls": false,
            "baseDn": "dc=example,dc=com",
            "userFilter": "(uid={login})",
        })
        .to_string();
        let err = validate_ldap_settings_json(&raw).unwrap_err();
        assert!(err.to_string().contains("plaintext"), "got: {err}");
    }

    #[test]
    fn accepts_ldap_url_with_starttls() {
        let raw = serde_json::json!({
            "enabled": true,
            "url": "ldap://ldap.example.com:389",
            "starttls": true,
            "baseDn": "dc=example,dc=com",
            "userFilter": "(uid={login})",
        })
        .to_string();
        assert!(validate_ldap_settings_json(&raw).is_ok());
    }

    #[test]
    fn rejects_bad_placeholder_counts() {
        for filter in ["(uid=*)", "(uid={login})(cn={login})"] {
            let raw = serde_json::json!({
                "enabled": true,
                "url": "ldaps://ldap.example.com",
                "baseDn": "dc=example,dc=com",
                "userFilter": filter,
            })
            .to_string();
            let err = validate_ldap_settings_json(&raw).unwrap_err();
            assert!(err.to_string().contains("{login}"), "got: {err}");
        }
    }

    #[test]
    fn rejects_filter_without_parentheses() {
        // A single placeholder outside any parentheses is not a valid filter.
        let raw = serde_json::json!({
            "enabled": true,
            "url": "ldaps://ldap.example.com",
            "baseDn": "dc=example,dc=com",
            "userFilter": "uid={login}",
        })
        .to_string();
        let err = validate_ldap_settings_json(&raw).unwrap_err();
        assert!(err.to_string().contains("parentheses"), "got: {err}");
    }

    #[test]
    fn rejects_unbalanced_parentheses() {
        let raw = serde_json::json!({
            "enabled": true,
            "url": "ldaps://ldap.example.com",
            "baseDn": "dc=example,dc=com",
            "userFilter": "(&(objectClass=user)(uid={login})",
        })
        .to_string();
        let err = validate_ldap_settings_json(&raw).unwrap_err();
        assert!(err.to_string().contains("parentheses"), "got: {err}");
    }

    #[test]
    fn rejects_url_with_userinfo_or_query() {
        for url in [
            "ldaps://user:pass@ldap.example.com",
            "ldaps://ldap.example.com?debug=1",
            "ldaps://ldap.example.com#frag",
        ] {
            let raw = serde_json::json!({
                "enabled": true,
                "url": url,
                "baseDn": "dc=example,dc=com",
                "userFilter": "(uid={login})",
            })
            .to_string();
            let err = validate_ldap_settings_json(&raw).unwrap_err();
            assert!(err.to_string().contains("userinfo"), "url {url}: {err}");
        }
    }

    #[test]
    fn rejects_wrong_scheme_and_missing_host() {
        for url in ["https://ldap.example.com", "ldaps:///"] {
            let raw = serde_json::json!({
                "enabled": true,
                "url": url,
                "baseDn": "dc=example,dc=com",
                "userFilter": "(uid={login})",
            })
            .to_string();
            assert!(validate_ldap_settings_json(&raw).is_err(), "url {url}");
        }
    }

    #[test]
    fn rejects_malformed_json_and_missing_required_fields() {
        assert!(validate_ldap_settings_json("not json").is_err());
        let raw = r#"{"enabled":true}"#;
        let err = validate_ldap_settings_json(raw).unwrap_err();
        assert!(err.to_string().contains("url"), "got: {err}");
    }

    #[test]
    fn rejects_invalid_mail_attribute() {
        for mail_attribute in ["", "mail;drop", "a".repeat(65).as_str()] {
            let raw = serde_json::json!({
                "enabled": true,
                "url": "ldaps://ldap.example.com",
                "baseDn": "dc=example,dc=com",
                "userFilter": "(uid={login})",
                "mailAttribute": mail_attribute,
            })
            .to_string();
            // "" is normalized to the default and accepted.
            if mail_attribute.is_empty() {
                let settings = validate_ldap_settings_json(&raw).unwrap();
                assert_eq!(settings.mail_attribute, "mail");
            } else {
                assert!(
                    validate_ldap_settings_json(&raw).is_err(),
                    "mailAttribute {mail_attribute}"
                );
            }
        }
    }

    #[test]
    fn runtime_secure_channel_check_is_fail_closed() {
        // WHY: rows written before write-path validation existed (or by a
        // direct DB write) must not silently re-enable a plaintext channel.
        let mut settings = validate_ldap_settings_json(&valid_settings_json()).unwrap();
        assert!(settings.is_credential_channel_secure());
        settings.url = "ldap://ldap.example.com".to_string();
        settings.starttls = false;
        assert!(!settings.is_credential_channel_secure());
    }

    #[test]
    fn render_user_filter_replaces_single_placeholder() {
        let rendered =
            render_user_filter("(&(objectClass=user)(sAMAccountName={login}))", "jdoe\\2a")
                .unwrap();
        assert_eq!(rendered, "(&(objectClass=user)(sAMAccountName=jdoe\\2a))");
    }

    #[test]
    fn render_user_filter_keeps_filter_structure_under_escaped_input() {
        // WHY: LDAP injection defense. ldap3::ldap_escape (called by the
        // infra adapter before this function) turns every filter metacharacter
        // into a backslash escape, so even an attacker-controlled username
        // cannot open/close the surrounding filter structure. This pins the
        // passthrough contract: escaped input must not shift the template's
        // parenthesis balance.
        let escaped_evil = r"\2a\29\28|\28uid=\2a\29"; // ldap_escape("*)(|(uid=*)")
        let rendered = render_user_filter("(uid={login})", escaped_evil).unwrap();
        assert_eq!(rendered, r"(uid=\2a\29\28|\28uid=\2a\29)");
        let open = rendered.matches('(').count();
        let close = rendered.matches(')').count();
        assert_eq!(open, close);
    }

    #[test]
    fn render_user_filter_rejects_nul_and_bad_template() {
        assert!(render_user_filter("(uid={login})", "a\0b").is_err());
        assert!(render_user_filter("(uid={login})(cn={login})", "a").is_err());
        assert!(render_user_filter("(uid=*)", "a").is_err());
    }
}
