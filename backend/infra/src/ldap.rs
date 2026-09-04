// LDAP directory adapter (design support-ldap §9.3).
//
// Thin search-then-bind wrapper over ldap3 0.12 (rustls/ring backend, DEC-004):
// connect (LDAPS or StartTLS per settings) → service-account/anonymous bind →
// search for exactly one user entry → bind as the user entry DN → return the
// entry DN and mail attribute value. One connection per login; no pooling.
//
// Error contract (design §9.1): `InvalidCredentials` (zero hits / multiple
// hits / user bind rejected) is generalized to 401 by the caller;
// `Unavailable` covers connection/timeout/protocol errors and directory
// misconfiguration (service-account bind rejected, missing bind password) —
// those indicate the directory side is not usable, not a wrong user password
// (PRD §4.2 "directory configuration error").

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use herald_domain::ldap::{
    LdapAuthError, LdapAuthenticatedUser, LdapAuthenticator, LdapLoginConfig,
};
use herald_domain::security_constants::{LDAP_CONNECT_TIMEOUT_SECONDS, LDAP_TIMEOUT_SECONDS};
use ldap3::{LdapConnAsync, LdapConnSettings, Scope, SearchEntry, ldap_escape};
use rustls_pki_types::CertificateDer;
use rustls_pki_types::pem::PemObject;

pub struct Ldap3Authenticator {
    /// Built `ClientConfig`s keyed by the exact `caCertPem` value. Building
    /// one reads the whole OS trust store — an off-hot-path cost that must
    /// not repeat on every login. Keying by PEM content makes a changed
    /// config a natural cache miss.
    client_configs: std::sync::RwLock<HashMap<String, Arc<rustls::ClientConfig>>>,
}

impl Ldap3Authenticator {
    pub fn new() -> Self {
        Self {
            client_configs: std::sync::RwLock::new(HashMap::new()),
        }
    }

    /// Cached wrapper around [`extra_ca_client_config`] for the login path.
    fn cached_extra_ca_config(
        &self,
        settings: &herald_domain::ldap::LdapDirectorySettings,
    ) -> Result<Option<Arc<rustls::ClientConfig>>, LdapAuthError> {
        let Some(pem) = settings
            .ca_cert_pem
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
        else {
            return Ok(None);
        };

        if let Some(hit) = self
            .client_configs
            .read()
            .expect("LDAP client-config cache poisoned")
            .get(pem)
        {
            return Ok(Some(hit.clone()));
        }

        let Some(built) = extra_ca_client_config(settings)? else {
            return Ok(None);
        };
        self.client_configs
            .write()
            .expect("LDAP client-config cache poisoned")
            .insert(pem.to_string(), built.clone());
        Ok(Some(built))
    }
}

impl Default for Ldap3Authenticator {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a rustls `ClientConfig` from the system trust store plus the
/// optional `caCertPem` from the realm's LDAP settings (private-CA
/// directories). Trust is ADDITIVE; certificate verification itself stays
/// ON. A broken stored value must fail loud — a silent fallback would
/// surface as a confusing TLS handshake 503.
fn extra_ca_client_config(
    settings: &herald_domain::ldap::LdapDirectorySettings,
) -> Result<Option<Arc<rustls::ClientConfig>>, LdapAuthError> {
    let Some(pem) = settings
        .ca_cert_pem
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    else {
        return Ok(None);
    };

    // Mirror ldap3's default root store (system certs, lenient about errors)
    // and layer the configured CA(s) on top.
    let mut roots = rustls::RootCertStore::empty();
    let native = rustls_native_certs::load_native_certs();
    for cert in native.certs {
        let _ = roots.add(cert);
    }

    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(pem.as_bytes())
        .collect::<Result<_, _>>()
        .map_err(|e| LdapAuthError::Unavailable(format!("caCertPem: invalid PEM: {e}")))?;
    if certs.is_empty() {
        return Err(LdapAuthError::Unavailable(
            "caCertPem: no certificates found".to_string(),
        ));
    }
    for cert in certs {
        roots.add(cert).map_err(|e| {
            LdapAuthError::Unavailable(format!("caCertPem: unusable certificate: {e}"))
        })?;
    }

    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Some(Arc::new(config)))
}

fn render_filter(
    settings: &herald_domain::ldap::LdapDirectorySettings,
    username: &str,
) -> Result<String, LdapAuthError> {
    // The username is attacker-controlled and lands inside an RFC 4515 filter;
    // escaping the metacharacters first is the LDAP-injection defense
    // (design §6.3). Only the escaped form ever reaches the template.
    let escaped = ldap_escape(username);
    herald_domain::ldap::render_user_filter(&settings.user_filter, &escaped).map_err(|e| {
        tracing::warn!(error = %e, "LDAP user filter template rejected at render time");
        LdapAuthError::Unavailable("invalid user filter configuration".to_string())
    })
}

impl LdapAuthenticator for Ldap3Authenticator {
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
    > {
        Box::pin(async move {
            // Wall-clock budget for the whole sequence; ldap3 0.12 only offers
            // a connect timeout, so operations are bounded here (design §8,
            // D2-7). A slow directory fails the login as 503 instead of
            // pinning request workers.
            match tokio::time::timeout(
                Duration::from_secs(LDAP_TIMEOUT_SECONDS),
                self.authenticate_inner(config, username, password),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => Err(LdapAuthError::Unavailable(format!(
                    "LDAP operation exceeded {LDAP_TIMEOUT_SECONDS}s budget"
                ))),
            }
        })
    }
}

impl Ldap3Authenticator {
    async fn authenticate_inner(
        &self,
        config: &LdapLoginConfig,
        username: &str,
        password: &str,
    ) -> Result<LdapAuthenticatedUser, LdapAuthError> {
        let settings = &config.settings;

        let mut conn_settings = LdapConnSettings::new()
            .set_conn_timeout(Duration::from_secs(LDAP_CONNECT_TIMEOUT_SECONDS));
        if let Some(config) = self.cached_extra_ca_config(settings)? {
            conn_settings = conn_settings.set_config(config);
        }
        // `ldaps://` gets TLS from the scheme; `ldap://` must negotiate StartTLS
        // (write-path validation enforces this pairing, `set_starttls` executes
        // the extended op during connect). Certificate verification stays ON —
        // there is deliberately no option to disable it (design §7).
        if settings.starttls {
            conn_settings = conn_settings.set_starttls(true);
        }

        let (conn, mut ldap) = LdapConnAsync::with_settings(conn_settings, &settings.url)
            .await
            .map_err(|e| unavailable("connect", &settings.url, e))?;
        ldap3::drive!(conn);

        // Service-account (or anonymous) bind for the search step. A rejected
        // service account means the directory configuration is broken — the user
        // did nothing wrong — so it classifies as Unavailable, not 401.
        let search_bind_dn = settings.bind_dn.as_deref().unwrap_or("");
        let search_bind_pw = match settings.bind_dn.as_deref() {
            Some(_) => config.bind_password.as_deref().ok_or_else(|| {
                LdapAuthError::Unavailable(
                    "bind_dn configured but bind_password config row is missing".to_string(),
                )
            })?,
            // Anonymous bind: empty DN with empty password (RFC 4513 §5.1).
            None => "",
        };
        ldap.simple_bind(search_bind_dn, search_bind_pw)
        .await
        .map_err(|e| unavailable("service bind", &settings.url, e))?
        .success()
        .map_err(|e| {
            tracing::warn!(error = %e, "LDAP service-account bind rejected (credentials invalid?)");
            LdapAuthError::Unavailable("LDAP service-account bind failed".to_string())
        })?;

        // Search for exactly one matching entry (DEC-009: zero or multiple hits
        // are a generalized authentication failure — no guess-binding).
        let filter = render_filter(settings, username)?;
        let mut requested_attrs = vec![settings.mail_attribute.as_str()];
        if let Some(display_attr) = settings.display_name_attribute.as_deref() {
            requested_attrs.push(display_attr);
        }
        let (entries, _result) = ldap
            .search(&settings.base_dn, Scope::Subtree, &filter, requested_attrs)
            .await
            .map_err(|e| unavailable("search", &settings.url, e))?
            .success()
            .map_err(|e| unavailable("search result", &settings.url, e))?;

        if entries.len() != 1 {
            tracing::debug!(
                hit_count = entries.len(),
                "LDAP user search did not return exactly one entry",
            );
            return Err(LdapAuthError::InvalidCredentials);
        }
        let entry = SearchEntry::construct(entries.into_iter().next().expect("len checked == 1"));
        let dn = entry.dn;
        let email = entry
            .attrs
            .get(&settings.mail_attribute)
            .and_then(|values| values.first())
            .filter(|mail| !mail.trim().is_empty())
            .cloned();
        let display_name = settings
            .display_name_attribute
            .as_deref()
            .and_then(|attr| entry.attrs.get(attr))
            .and_then(|values| values.first())
            .filter(|name| !name.trim().is_empty())
            .cloned();

        // User bind = the actual credential check for THIS user.
        let user_bind = ldap.simple_bind(&dn, password).await;
        // Best-effort teardown on every path; the connection is per-login anyway.
        if let Err(e) = ldap.unbind().await {
            tracing::debug!(error = %e, "LDAP unbind failed (ignored)");
        }
        match user_bind {
            Ok(result) if result.rc == 0 => Ok(LdapAuthenticatedUser {
                dn,
                email,
                display_name,
            }),
            Ok(result) => {
                tracing::debug!(rc = result.rc, "LDAP user bind rejected");
                Err(LdapAuthError::InvalidCredentials)
            }
            Err(e) => Err(unavailable("user bind", &settings.url, e)),
        }
    }
}

/// Wrap a transport/protocol error as `Unavailable` with server-side detail.
/// The detail goes to tracing only — the API response stays generic (503).
fn unavailable(step: &str, url: &str, error: ldap3::LdapError) -> LdapAuthError {
    // Log the URL for operators; it never leaves the server log.
    tracing::warn!(step = step, url = %url, error = %error, "LDAP directory operation failed");
    LdapAuthError::Unavailable(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use herald_domain::ldap::LdapDirectorySettings;

    fn settings_with_ca(pem: Option<&str>) -> LdapDirectorySettings {
        LdapDirectorySettings {
            ca_cert_pem: pem.map(str::to_string),
            ..LdapDirectorySettings::default()
        }
    }

    #[test]
    fn extra_ca_absent_means_default_trust_only() {
        assert!(matches!(
            extra_ca_client_config(&settings_with_ca(None)),
            Ok(None)
        ));
    }

    #[test]
    fn extra_ca_non_pem_value_fails_loud() {
        // The write path rejects this shape, but the adapter must still fail
        // loud on legacy/directly-written rows instead of silently skipping
        // the configured trust.
        let err = extra_ca_client_config(&settings_with_ca(Some("not a pem")))
            .expect_err("non-PEM value must be an error");
        let LdapAuthError::Unavailable(detail) = err else {
            panic!("expected Unavailable, got {err:?}");
        };
        assert!(detail.contains("caCertPem"), "detail should name the field");
    }

    #[test]
    fn extra_ca_fixture_pem_loads() {
        let pem = include_str!("../tests/ldap-directory-assets/certs/ca.crt");
        assert!(
            extra_ca_client_config(&settings_with_ca(Some(pem)))
                .expect("fixture PEM should load")
                .is_some()
        );
    }
}
