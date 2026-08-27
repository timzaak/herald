// =============================================================================
// LDAP Test Helpers
// =============================================================================
//
// Test-only utilities for the LDAP login scenarios
// (`ldap_login_scenarios.rs` / `realm_ldap_config_scenarios.rs`).
//
// `MockLdapAuthenticator` implements the production
// `herald_core::domain::ldap::LdapAuthenticator` port and simulates the
// search-then-bind semantics (DEC-009) in-process: username lookup with a
// uniqueness requirement, then a password comparison against the matched
// entry. It is injected through
// `create_unified_test_router_with_state(|s| s.ldap_authenticator = ...)` —
// the same private-state override pattern as the JWKS URL fields. It does
// NOT touch production code.
//
// `enable_ldap`/`delete_ldap_settings` write the exact `realm_config` rows
// the production `load_ldap_config` reads (`ldap/settings` JSON +
// `ldap/bind_password` secret), mirroring `enable_email_otp`.
//
// The `one_mock_user`/`mock_dir`/`ldap_login*` helpers build the shared
// mock-directory + login-request scaffolding used by both scenario files.
//
// =============================================================================

use std::sync::Mutex;

use herald_core::domain::ldap::{
    LdapAuthError, LdapAuthenticatedUser, LdapAuthenticator, LdapLoginConfig,
};
use serde_json::json;
use tower::ServiceExt;

use crate::tests::schema_test_context::SchemaTestContext as TestContext;

/// One simulated directory entry. Multiple entries sharing a `username`
/// reproduce the "search returned more than one hit" directory shape (DEC-009).
#[derive(Debug, Clone)]
pub struct MockLdapUser {
    pub username: String,
    pub dn: String,
    pub email: Option<String>,
    pub password: String,
}

/// Programmable state of the mock directory. `fail_with`, when set, short-
/// circuits every authentication with that error (directory unreachable, etc).
#[derive(Debug, Default)]
pub struct MockLdapState {
    pub users: Vec<MockLdapUser>,
    pub fail_with: Option<LdapAuthError>,
}

/// In-process `LdapAuthenticator` test double.
pub struct MockLdapAuthenticator {
    state: Mutex<MockLdapState>,
}

impl MockLdapAuthenticator {
    pub fn new(state: MockLdapState) -> Self {
        Self {
            state: Mutex::new(state),
        }
    }

    /// Rewrite the whole simulated directory between calls of a scenario.
    pub fn set_state(&self, state: MockLdapState) {
        *self.state.lock().expect("mock ldap state poisoned") = state;
    }
}

impl LdapAuthenticator for MockLdapAuthenticator {
    fn authenticate<'a>(
        &'a self,
        _config: &'a LdapLoginConfig,
        username: &'a str,
        password: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<LdapAuthenticatedUser, LdapAuthError>>
                + Send
                + 'a,
        >,
    > {
        // Snapshot the state without holding the lock across the (immediate)
        // await below; the mock performs no I/O.
        let outcome = {
            let state = self.state.lock().expect("mock ldap state poisoned");
            if let Some(err) = &state.fail_with {
                Err(clone_auth_error(err))
            } else {
                let hits: Vec<&MockLdapUser> = state
                    .users
                    .iter()
                    .filter(|u| u.username == username)
                    .collect();
                match hits.as_slice() {
                    [only] if only.password == password => Ok(LdapAuthenticatedUser {
                        dn: only.dn.clone(),
                        email: only.email.clone(),
                    }),
                    // Zero hits, multiple hits, or wrong password — all
                    // InvalidCredentials, exactly like the real adapter.
                    _ => Err(LdapAuthError::InvalidCredentials),
                }
            }
        };
        Box::pin(async move { outcome })
    }
}

fn clone_auth_error(err: &LdapAuthError) -> LdapAuthError {
    match err {
        LdapAuthError::InvalidCredentials => LdapAuthError::InvalidCredentials,
        LdapAuthError::Unavailable(detail) => LdapAuthError::Unavailable(detail.clone()),
    }
}

// ---------------------------------------------------------------------------
// Shared fixture / request helpers
// ---------------------------------------------------------------------------

/// A one-entry mock directory.
pub fn one_mock_user(
    username: &str,
    dn: &str,
    email: Option<&str>,
    password: &str,
) -> MockLdapState {
    MockLdapState {
        users: vec![MockLdapUser {
            username: username.to_string(),
            dn: dn.to_string(),
            email: email.map(str::to_string),
            password: password.to_string(),
        }],
        fail_with: None,
    }
}

/// Build a shareable mock directory handle (Arc so `set_state` stays visible
/// to subsequent requests within one test).
pub fn mock_dir(state: MockLdapState) -> std::sync::Arc<MockLdapAuthenticator> {
    std::sync::Arc::new(MockLdapAuthenticator::new(state))
}

/// POST /api/auth/{realmId}/login/ldap against a router whose LDAP
/// authenticator is `mock` (shared state across calls of one test). Caller
/// owns the response.
pub async fn ldap_login(
    ctx: &TestContext,
    mock: &std::sync::Arc<MockLdapAuthenticator>,
    username: &str,
    password: &str,
    agreements: Option<Vec<serde_json::Value>>,
) -> axum::response::Response {
    ldap_login_ext(ctx, mock, username, password, agreements, json!({})).await
}

/// Extended variant carrying the downstream-OAuth fields.
pub async fn ldap_login_ext(
    ctx: &TestContext,
    mock: &std::sync::Arc<MockLdapAuthenticator>,
    username: &str,
    password: &str,
    agreements: Option<Vec<serde_json::Value>>,
    oauth_fields: serde_json::Value,
) -> axum::response::Response {
    let mut payload = json!({
        "clientId": ctx._client_id,
        "username": username,
        "password": password,
    });
    if let Some(agreements) = agreements {
        payload["agreements"] = json!(agreements);
    }
    for (key, value) in oauth_fields.as_object().into_iter().flatten() {
        payload[key] = value.clone();
    }
    let request = axum::http::Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login/ldap", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "6.6.6.6")
        .body(axum::body::Body::from(payload.to_string()))
        .unwrap();
    // Arc<MockLdapAuthenticator> coerces to Arc<dyn LdapAuthenticator>; the
    // shared Arc keeps set_state() visible to subsequent requests.
    ctx.create_unified_test_router_with_state(move |s| {
        s.ldap_authenticator = mock.clone();
    })
    .oneshot(request)
    .await
    .unwrap()
}

/// Read the current effective ToS + Privacy version_ids for the test Realm
/// (platform-default seeds), to build an `agreements` payload.
pub async fn current_effective_agreements(ctx: &TestContext) -> Vec<serde_json::Value> {
    let mut items = Vec::new();
    for agreement_type in ["terms_of_service", "privacy_policy"] {
        let version_id: uuid::Uuid = sqlx::query_scalar(
            "SELECT id FROM legal_agreement_version
             WHERE agreement_type = $1
               AND (realm_id = $2 OR realm_id IS NULL)
             ORDER BY CASE WHEN realm_id = $2 THEN 0 ELSE 1 END, version_no DESC
             LIMIT 1",
        )
        .bind(agreement_type)
        .bind(&ctx._realm_id)
        .fetch_one(&ctx._app_state.pool)
        .await
        .expect("expected a seeded platform-default agreement version");
        items.push(json!({
            "agreementType": agreement_type,
            "versionId": version_id.to_string(),
        }));
    }
    items
}

/// Enable LDAP login for the test Realm by writing the exact realm_config
/// rows the production `load_ldap_config` reads.
pub async fn enable_ldap(ctx: &TestContext) {
    insert_ldap_settings(
        ctx,
        &json!({
            "enabled": true,
            "url": "ldaps://ldap.example.com:636",
            "starttls": false,
            "baseDn": "dc=example,dc=com",
            "bindDn": "cn=admin,dc=example,dc=com",
            "userFilter": "(&(objectClass=user)(sAMAccountName={login}))",
            "mailAttribute": "mail",
        }),
    )
    .await;
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, is_secret, enabled, metadata, created_at, updated_at)
         VALUES ($1, 'ldap', 'bind_password', $2, true, true, '{}'::jsonb, NOW(), NOW())
         ON CONFLICT (realm_id, config_type, config_key)
         DO UPDATE SET config_value = EXCLUDED.config_value, updated_at = NOW()",
    )
    .bind(&ctx._realm_id)
    .bind("svc-password")
    .execute(&ctx._app_state.pool)
    .await
    .expect("failed to insert ldap bind_password config");
}

/// Write a raw `ldap/settings` JSON row (used to seed malformed/legacy rows
/// or custom filters). Row-level `enabled` column mirrors the JSON `enabled`
/// value the way the admin UI would submit it.
pub async fn insert_ldap_settings(ctx: &TestContext, settings: &serde_json::Value) {
    let row_enabled = settings
        .get("enabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, is_secret, enabled, metadata, created_at, updated_at)
         VALUES ($1, 'ldap', 'settings', $2, false, $3, '{}'::jsonb, NOW(), NOW())
         ON CONFLICT (realm_id, config_type, config_key)
         DO UPDATE SET config_value = EXCLUDED.config_value, enabled = EXCLUDED.enabled, updated_at = NOW()",
    )
    .bind(&ctx._realm_id)
    .bind(settings.to_string())
    .bind(row_enabled)
    .execute(&ctx._app_state.pool)
    .await
    .expect("failed to insert ldap settings config");
}

/// Disable LDAP (enabled=false in the settings JSON — the sole signal).
pub async fn disable_ldap(ctx: &TestContext) {
    insert_ldap_settings(
        ctx,
        &json!({
            "enabled": false,
            "url": "ldaps://ldap.example.com:636",
            "starttls": false,
            "baseDn": "dc=example,dc=com",
            "userFilter": "(&(objectClass=user)(sAMAccountName={login}))",
        }),
    )
    .await;
}

/// Remove the settings row entirely (admin deleted the configuration).
pub async fn delete_ldap_settings(ctx: &TestContext) {
    sqlx::query("DELETE FROM realm_config WHERE realm_id = $1 AND config_type = 'ldap'")
        .bind(&ctx._realm_id)
        .execute(&ctx._app_state.pool)
        .await
        .expect("failed to delete ldap config rows");
}
