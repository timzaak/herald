// Integration tests for `Ldap3Authenticator` against a REAL OpenLDAP
// directory (cas-test-ldap container, started by scripts/test-start.py).
//
// The api-level scenario tests replace `LdapAuthenticator` with a mock, so
// everything in the thin ldap3 adapter itself — TLS negotiation, real slapd
// filter semantics, the DEC-009 unique-hit rule, and the 401/503 error
// classification — is only exercised here.
//
// Run via the unified entry (never bare cargo test):
//   uv run scripts/backend-test.py -- -E 'package(herald-infra) and test(test_ldap_it_)'
//
// Directory fixture (tests/ldap-directory-assets/seed.ldif):
//   uid=alice (mail=alice@example.com), uid=bob (no mail), and TWO uid=dup
//   entries under different OUs so `(uid=dup)` yields 2 hits.
// The service account is the suffix rootDN `cn=admin,dc=herald,dc=test`
// (fixed SSHA password baked into the container env) — the container's
// default ACL denies `entry` reads to non-rootDN identities, while the
// client-side code paths are identical for any service DN.

use herald_domain::ldap::{
    LdapAuthError, LdapAuthenticator, LdapDirectorySettings, LdapLoginConfig,
};
use herald_infra::ldap::Ldap3Authenticator;

/// The container directory's private CA, trusted via the realm setting
/// `caCertPem` (same trust path a real private-CA deployment would use).
const CA_PEM: &str = include_str!("ldap-directory-assets/certs/ca.crt");
const STARTTLS_URL: &str = "ldap://127.0.0.1:13890";
const LDAPS_URL: &str = "ldaps://127.0.0.1:13636";
const BASE_DN: &str = "dc=herald,dc=test";
const ADMIN_DN: &str = "cn=admin,dc=herald,dc=test";
const ADMIN_PW: &str = "svc-password";

fn config(url: &str, starttls: bool, ca_cert_pem: Option<&str>) -> LdapLoginConfig {
    LdapLoginConfig {
        settings: LdapDirectorySettings {
            enabled: true,
            url: url.to_string(),
            starttls,
            base_dn: BASE_DN.to_string(),
            bind_dn: Some(ADMIN_DN.to_string()),
            user_filter: "(uid={login})".to_string(),
            mail_attribute: "mail".to_string(),
            display_name_attribute: None,
            ca_cert_pem: ca_cert_pem.map(str::to_string),
        },
        bind_password: Some(ADMIN_PW.to_string()),
    }
}

/// Config that trusts the container's CA — the shape a realm admin would
/// save for a private-CA directory.
fn trusted_config(url: &str, starttls: bool) -> LdapLoginConfig {
    config(url, starttls, Some(CA_PEM))
}

async fn authenticate(
    config: &LdapLoginConfig,
    username: &str,
    password: &str,
) -> Result<herald_domain::ldap::LdapAuthenticatedUser, LdapAuthError> {
    Ldap3Authenticator::default()
        .authenticate(config, username, password)
        .await
}

#[tokio::test]
async fn test_ldap_it_starttls_success_returns_dn_and_mail() {
    let user = authenticate(&trusted_config(STARTTLS_URL, true), "alice", "alicepass")
        .await
        .expect("alice/alicepass must authenticate over StartTLS");
    assert_eq!(user.dn, "uid=alice,ou=people,dc=herald,dc=test");
    assert_eq!(user.email.as_deref(), Some("alice@example.com"));
}

#[tokio::test]
async fn test_ldap_it_ldaps_success_returns_dn_and_mail() {
    let user = authenticate(&trusted_config(LDAPS_URL, false), "alice", "alicepass")
        .await
        .expect("alice/alicepass must authenticate over LDAPS");
    assert_eq!(user.dn, "uid=alice,ou=people,dc=herald,dc=test");
    assert_eq!(user.email.as_deref(), Some("alice@example.com"));
}

#[tokio::test]
async fn test_ldap_it_user_without_mail_yields_none() {
    let user = authenticate(&trusted_config(STARTTLS_URL, true), "bob", "bobpass")
        .await
        .expect("bob/bobpass must authenticate");
    assert_eq!(user.dn, "uid=bob,ou=people,dc=herald,dc=test");
    assert!(
        user.email.is_none(),
        "bob has no mail attribute; email must be None, got {:?}",
        user.email
    );
}

#[tokio::test]
async fn test_ldap_it_wrong_password_is_invalid_credentials() {
    let err = authenticate(
        &trusted_config(STARTTLS_URL, true),
        "alice",
        "not-alices-password",
    )
    .await
    .expect_err("wrong password must fail");
    assert!(
        matches!(err, LdapAuthError::InvalidCredentials),
        "user bind rejection must classify as 401 InvalidCredentials, got {err:?}"
    );
}

#[tokio::test]
async fn test_ldap_it_zero_hits_is_invalid_credentials() {
    let err = authenticate(&trusted_config(STARTTLS_URL, true), "ghost", "whatever")
        .await
        .expect_err("unknown user must fail");
    assert!(
        matches!(err, LdapAuthError::InvalidCredentials),
        "zero search hits must classify as 401 InvalidCredentials, got {err:?}"
    );
}

// DEC-009: two entries match `(uid=dup)` — authentication must fail without
// ever attempting to bind as either entry (no guess-binding). This branch is
// unreachable for the mock in scenario tests, which always returns ≤1 user.
#[tokio::test]
async fn test_ldap_it_multiple_hits_is_invalid_credentials() {
    let err = authenticate(&trusted_config(STARTTLS_URL, true), "dup", "duponepass")
        .await
        .expect_err("uid=dup resolves to two entries; must fail");
    assert!(
        matches!(err, LdapAuthError::InvalidCredentials),
        "multiple hits must classify as 401 InvalidCredentials, got {err:?}"
    );
}

// Injection defense against a REAL slapd filter evaluator: the raw username
// must reach the directory escaped, so these payloads match nobody. If the
// escaping were dropped, `(uid=*)(objectClass=*)` / `(uid=*)` would match
// every entry and the login would (wrongly) hit the unique-hit/401 path via
// a different route — or worse, a crafted payload could select a victim.
#[tokio::test]
async fn test_ldap_it_injection_payloads_match_nobody() {
    for payload in ["*", "*)(objectClass=*", "alice)(objectClass=*"] {
        let err = authenticate(&trusted_config(STARTTLS_URL, true), payload, "x")
            .await
            .expect_err("injection payload must not authenticate");
        assert!(
            matches!(err, LdapAuthError::InvalidCredentials),
            "payload {payload:?} must classify as 401, got {err:?}"
        );
    }
}

// A rejected service-account bind is a DIRECTORY misconfiguration (PRD §4.2):
// the user did nothing wrong, so it must classify as 503 Unavailable — never
// as 401, which would leak "directory broken" as "bad credentials".
#[tokio::test]
async fn test_ldap_it_service_bind_rejected_is_unavailable() {
    let mut cfg = trusted_config(STARTTLS_URL, true);
    cfg.bind_password = Some("wrong-service-password".to_string());
    let err = authenticate(&cfg, "alice", "alicepass")
        .await
        .expect_err("broken service credentials must fail");
    assert!(
        matches!(err, LdapAuthError::Unavailable(_)),
        "service bind rejection must classify as 503 Unavailable, got {err:?}"
    );
}

// The container CA is self-signed: with no caCertPem configured the rustls
// handshake fails against the system trust store, and a TLS failure is a
// 503 Unavailable — not a 401 that would blame the user's credentials.
#[tokio::test]
async fn test_ldap_it_untrusted_certificate_is_unavailable() {
    // No caCertPem configured: only the system store is trusted, and the
    // container's self-signed CA is not in it.
    let err = authenticate(&config(STARTTLS_URL, true, None), "alice", "alicepass")
        .await
        .expect_err("handshake against an untrusted CA must fail");
    assert!(
        matches!(err, LdapAuthError::Unavailable(_)),
        "TLS failure must classify as 503 Unavailable, got {err:?}"
    );
}

// Unreachable directory (nothing listens on this port) is a 503 Unavailable,
// bounded by the connect timeout rather than hanging the request worker.
#[tokio::test]
async fn test_ldap_it_dead_directory_is_unavailable() {
    let err = authenticate(
        &trusted_config("ldap://127.0.0.1:1", true),
        "alice",
        "alicepass",
    )
    .await
    .expect_err("connection refused must fail");
    assert!(
        matches!(err, LdapAuthError::Unavailable(_)),
        "dead directory must classify as 503 Unavailable, got {err:?}"
    );
}
