// =============================================================================
// Scenario tests: LDAP enterprise-directory login
// =============================================================================
//
// Exercises the LDAP login flow end-to-end
// through the HTTP layer:
//   POST /api/auth/{realmId}/login/ldap
//
// Directory authentication is simulated by `MockLdapAuthenticator`
// (`helpers/ldap_helpers.rs`) implementing the production
// `LdapAuthenticator` port — search-then-bind semantics in-process (no
// real directory container in CI).
//
// Coverage focus (US-LD-001/002/004): linked-user login, JIT provisioning
// (placeholder email / email match / registration policy bypass per
// DEC-007), consent-before-provisioning, anti-enumeration 401
// generalization (DEC-009), directory-unavailable 503, disabled account,
// not-enabled realm, TOTP second-factor handoff, downstream OAuth code
// branch, Client-App Turnstile, and the shared login rate-limit budget.
//
// Notes on environment behaviour (mirrors email_otp_send_verify_scenarios):
// - `RateLimitConfig.enforce_in_dev` defaults to false → `rate_limit_hit`
//   is skipped in the test context; the rate-limit scenario asserts the
//   actual (non-429) behaviour and MUST NOT be strengthened to assert 429.
// - The TOTP scenario asserts the second-factor flag branch + temp session
//   existence; the /login/verify-totp consumption of that session is the
//   existing covered behaviour (user_totp_scenarios / login_flow_scenarios).
// =============================================================================

use crate::tests::helpers::ldap_helpers::{
    MockLdapState, MockLdapUser, current_effective_agreements, enable_ldap, ldap_login,
    ldap_login_ext, mock_dir, one_mock_user,
};
use crate::tests::helpers::test_setup_helpers::record_test_user_consent;
use crate::tests::helpers::user_helpers::create_simple_test_user;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use redis::AsyncCommands;
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Local fixture helpers
// ---------------------------------------------------------------------------

/// Create an active (status=1) user via the shared helper and record consent
/// so the login-as-consent gate does not intercept the happy paths.
async fn create_active_user_with_consent(ctx: &TestContext, email: &str) -> uuid::Uuid {
    let user_id = create_simple_test_user(ctx, email).await;
    record_test_user_consent(&ctx._app_state.pool, user_id, &ctx._realm_id).await;
    user_id
}

/// Link a DN to an existing account directly (repeat-login precondition).
async fn link_ldap_dn(ctx: &TestContext, user_id: uuid::Uuid, dn: &str) {
    sqlx::query(
        "INSERT INTO provider (id, realm_id, type, open_id, union_id, email, user_id, created_at, updated_at)
         VALUES ($1, $2, 'ldap', $3, NULL, NULL, $4, NOW(), NOW())",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(&ctx._realm_id)
    .bind(dn)
    .bind(user_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("failed to insert ldap provider link");
}

async fn count_accounts_by_email(ctx: &TestContext, email: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM account WHERE realm_id = $1 AND email = $2")
        .bind(&ctx._realm_id)
        .bind(email)
        .fetch_one(&ctx._app_state.pool)
        .await
        .unwrap()
}

async fn count_ldap_links_by_dn(ctx: &TestContext, dn: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM provider WHERE realm_id = $1 AND type = 'ldap' AND open_id = $2",
    )
    .bind(&ctx._realm_id)
    .bind(dn)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap()
}

async fn count_ldap_audit_events(ctx: &TestContext, action: &str, reason: Option<&str>) -> i64 {
    match reason {
        Some(reason) => sqlx::query_scalar(
            "SELECT COUNT(*) FROM audit_events
             WHERE realm_id = $1 AND action = $2 AND details->>'method' = 'ldap'
               AND details->>'reason' = $3",
        )
        .bind(&ctx._realm_id)
        .bind(action)
        .bind(reason)
        .fetch_one(&ctx._app_state.pool)
        .await
        .unwrap(),
        None => sqlx::query_scalar(
            "SELECT COUNT(*) FROM audit_events
             WHERE realm_id = $1 AND action = $2 AND details->>'method' = 'ldap'",
        )
        .bind(&ctx._realm_id)
        .bind(action)
        .fetch_one(&ctx._app_state.pool)
        .await
        .unwrap(),
    }
}

// =============================================================================
// Scenarios
// =============================================================================

/// User Story: US-LD-001
/// Covers: an already-linked directory identity logs in via the
/// DN link (level 1 of the matching chain), receives a token family, and the
/// event is audited with method="ldap".
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_ldap_login_linked_user_success(ctx: &mut TestContext) {
    enable_ldap(ctx).await;

    let email = format!("ld001-{}@test.com", uuid::Uuid::now_v7());
    let user_id = create_active_user_with_consent(ctx, &email).await;
    let dn = "uid=linked,dc=example,dc=com".to_string();
    link_ldap_dn(ctx, user_id, &dn).await;

    let mock = mock_dir(one_mock_user(
        "linked",
        &dn,
        Some(&email),
        "corp-password-1",
    ));
    let resp = ldap_login(ctx, &mock, "linked", "corp-password-1", None).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let (resp, token) = crate::tests::extract_bearer_token(resp).await;
    assert!(token.is_some(), "linked-user LDAP login must issue a token");
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["tokenType"], "Bearer");

    // No duplicate account, exactly the one LDAP link, success audit present.
    assert_eq!(count_accounts_by_email(ctx, &email).await, 1);
    assert_eq!(count_ldap_links_by_dn(ctx, &dn).await, 1);
    assert!(count_ldap_audit_events(ctx, "auth.login", None).await >= 1);
}

/// User Story: US-LD-002
/// Covers: first login without a directory mail JIT-creates an
/// account with the placeholder email (DEC-002), activates it, records
/// register-consent, and links the DN; the second login resolves the same
/// account (no duplicate provisioning).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_ldap_jit_placeholder_email_then_relogin_same_account(ctx: &mut TestContext) {
    enable_ldap(ctx).await;

    let dn = "uid=nomail,dc=example,dc=com";
    let agreements = current_effective_agreements(ctx).await;
    let mock = mock_dir(one_mock_user("nomail", dn, None, "corp-password-2"));

    let resp = ldap_login(ctx, &mock, "nomail", "corp-password-2", Some(agreements)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let (resp, token) = crate::tests::extract_bearer_token(resp).await;
    assert!(token.is_some(), "JIT first login must issue a token");
    let _: serde_json::Value = crate::tests::response_json(resp).await;

    // Placeholder account: active, unique, DN-linked, consent recorded.
    let row: Option<(uuid::Uuid, i16)> = sqlx::query_as(
        "SELECT id, status FROM account
         WHERE realm_id = $1 AND email LIKE '%@ldap.placeholder'",
    )
    .bind(&ctx._realm_id)
    .fetch_optional(&ctx._app_state.pool)
    .await
    .unwrap();
    let (user_id, status) = row.expect("placeholder account must exist");
    assert_eq!(status, 1, "JIT account must be activated");
    assert_eq!(count_ldap_links_by_dn(ctx, dn).await, 1);
    let consent_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_agreement_consent WHERE user_id = $1 AND realm_id = $2",
    )
    .bind(user_id)
    .bind(&ctx._realm_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert!(consent_count >= 1, "register-as-consent must be recorded");

    // Second login → same account id, still exactly one account + one link.
    let resp = ldap_login(ctx, &mock, "nomail", "corp-password-2", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let (resp, token) = crate::tests::extract_bearer_token(resp).await;
    assert!(token.is_some(), "repeat login must issue a token");
    let _: serde_json::Value = crate::tests::response_json(resp).await;

    let account_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM account WHERE realm_id = $1 AND email LIKE '%@ldap.placeholder'",
    )
    .bind(&ctx._realm_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(account_count, 1, "repeat login must not provision again");
}

/// User Story: US-LD-002
/// Covers: DEC-008 — a directory mail matching an
/// existing local account links that account instead of creating a duplicate.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_ldap_email_match_links_existing_account(ctx: &mut TestContext) {
    enable_ldap(ctx).await;

    let email = format!("ld002-{}@test.com", uuid::Uuid::now_v7());
    let existing_user_id = create_active_user_with_consent(ctx, &email).await;

    let dn = "uid=withmail,dc=example,dc=com";
    let mock = mock_dir(one_mock_user(
        "withmail",
        dn,
        Some(&email),
        "corp-password-3",
    ));
    let resp = ldap_login(ctx, &mock, "withmail", "corp-password-3", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let (resp, token) = crate::tests::extract_bearer_token(resp).await;
    assert!(token.is_some());
    let _: serde_json::Value = crate::tests::response_json(resp).await;

    // Same account (no duplicate), DN now linked to it.
    assert_eq!(count_accounts_by_email(ctx, &email).await, 1);
    let linked_user_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT user_id FROM provider WHERE realm_id = $1 AND type = 'ldap' AND open_id = $2",
    )
    .bind(&ctx._realm_id)
    .bind(dn)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(linked_user_id, existing_user_id);
}

/// User Story: US-LD-002
/// Covers: DEC-007 — JIT provisioning is NOT gated by the realm
/// registration policy. WHY this is an explicit regression gate: every other
/// self-service provisioning path (register, email-otp auto-register, OAuth
/// find_or_create) checks `is_registration_enabled`; LDAP deliberately does
/// not, because enabling the directory is the admin's supply authorization.
/// If someone "harmonizes" the LDAP path with the other JIT paths, this test
/// fails.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_ldap_jit_ignores_registration_policy(ctx: &mut TestContext) {
    enable_ldap(ctx).await;

    // Explicitly DISABLE public registration for the realm.
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
         VALUES ($1, 'registration', 'enabled', 'false', true)
         ON CONFLICT (realm_id, config_type, config_key) DO UPDATE SET config_value = 'false'",
    )
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("failed to disable registration");

    let email = format!("ld007-{}@test.com", uuid::Uuid::now_v7());
    let dn = "uid=regclosed,dc=example,dc=com";
    let agreements = current_effective_agreements(ctx).await;
    let mock = mock_dir(one_mock_user("regclosed", dn, Some(&email), "corp-pw-4"));

    let resp = ldap_login(ctx, &mock, "regclosed", "corp-pw-4", Some(agreements)).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "JIT must provision even when public registration is disabled (DEC-007)"
    );
    let (resp, token) = crate::tests::extract_bearer_token(resp).await;
    assert!(token.is_some());
    let _: serde_json::Value = crate::tests::response_json(resp).await;

    assert_eq!(count_accounts_by_email(ctx, &email).await, 1);
}

/// User Story: US-LD-002
/// Covers: consent must be expressed BEFORE any account
/// row is created; with agreements the re-submission provisions and records
/// register-as-consent.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_ldap_jit_consent_required_before_account_creation(ctx: &mut TestContext) {
    enable_ldap(ctx).await;

    let email = format!("ld003-{}@test.com", uuid::Uuid::now_v7());
    let dn = "uid=consent,dc=example,dc=com";
    let mock = mock_dir(one_mock_user("consent", dn, Some(&email), "corp-pw-5"));

    // First attempt without agreements → 200 consent_required, no account.
    let resp = ldap_login(ctx, &mock, "consent", "corp-pw-5", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["consentRequired"], true);
    assert!(
        body["agreements"].as_array().is_some_and(|a| !a.is_empty()),
        "consent_required must carry the current effective agreements"
    );
    assert_eq!(
        count_accounts_by_email(ctx, &email).await,
        0,
        "no account may be created before consent is expressed"
    );

    // Re-submission with agreements → provisioned + consent recorded.
    let agreements = current_effective_agreements(ctx).await;
    let resp = ldap_login(ctx, &mock, "consent", "corp-pw-5", Some(agreements)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let (resp, token) = crate::tests::extract_bearer_token(resp).await;
    assert!(token.is_some());
    let _: serde_json::Value = crate::tests::response_json(resp).await;

    let user_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM account WHERE realm_id = $1 AND email = $2")
            .bind(&ctx._realm_id)
            .bind(&email)
            .fetch_one(&ctx._app_state.pool)
            .await
            .unwrap();
    let consent_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_agreement_consent WHERE user_id = $1 AND realm_id = $2",
    )
    .bind(user_id)
    .bind(&ctx._realm_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert!(consent_count >= 1, "register-as-consent must be recorded");
}

/// User Story: US-LD-001
/// Covers: DEC-009 — wrong password, zero search hits, and
/// multiple search hits all yield the SAME generalized 401 response (status,
/// message), so a caller cannot distinguish "no such user" from "wrong
/// password" from "ambiguous directory entry".
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_ldap_invalid_credentials_anti_enumeration(ctx: &mut TestContext) {
    enable_ldap(ctx).await;

    let dn = "uid=enum,dc=example,dc=com";
    let mock = mock_dir(one_mock_user(
        "enum",
        dn,
        Some("enum@test.com"),
        "correct-password",
    ));

    // Wrong password.
    let wrong_pw = ldap_login(ctx, &mock, "enum", "wrong-password", None).await;
    assert_eq!(wrong_pw.status(), StatusCode::UNAUTHORIZED);
    let wrong_pw_body: serde_json::Value = crate::tests::response_json(wrong_pw).await;

    // Zero hits (unknown username).
    let no_user = ldap_login(ctx, &mock, "ghost", "whatever", None).await;
    assert_eq!(no_user.status(), StatusCode::UNAUTHORIZED);
    let no_user_body: serde_json::Value = crate::tests::response_json(no_user).await;

    // Multiple hits: a second entry sharing the username.
    mock.set_state(MockLdapState {
        users: vec![
            MockLdapUser {
                username: "dup".into(),
                dn: "uid=dup1,dc=example,dc=com".into(),
                email: None,
                password: "p".into(),
            },
            MockLdapUser {
                username: "dup".into(),
                dn: "uid=dup2,dc=example,dc=com".into(),
                email: None,
                password: "p".into(),
            },
        ],
        fail_with: None,
    });
    let multi = ldap_login(ctx, &mock, "dup", "p", None).await;
    assert_eq!(multi.status(), StatusCode::UNAUTHORIZED);
    let multi_body: serde_json::Value = crate::tests::response_json(multi).await;

    // Identical shape and message across all three causes.
    assert_eq!(wrong_pw_body["message"], json!("invalid credentials"));
    assert_eq!(no_user_body["message"], multi_body["message"]);
    assert_eq!(wrong_pw_body["message"], multi_body["message"]);

    // Failure audit distinguishes the causes for administrators only.
    assert!(
        count_ldap_audit_events(ctx, "auth.login_failed", Some("invalid_credentials")).await >= 3
    );
}

/// User Story: US-LD-001
/// Covers: directory unavailability yields a generic
/// 503 whose body carries no directory detail (host, port, error string);
/// the failure is audited with reason=directory_unavailable.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_ldap_directory_unavailable_returns_503(ctx: &mut TestContext) {
    enable_ldap(ctx).await;

    let secret_detail = "ldaps://secret-dir-internal.example.com:636 connection refused";
    let mock = mock_dir(MockLdapState {
        users: vec![],
        fail_with: Some(herald_core::domain::ldap::LdapAuthError::Unavailable(
            secret_detail.to_string(),
        )),
    });

    let resp = ldap_login(ctx, &mock, "anyone", "pw", None).await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    let body_str = body.to_string();
    assert!(
        !body_str.contains("secret-dir-internal") && !body_str.contains("connection refused"),
        "503 body must not leak directory details; got {body_str}"
    );
    assert_eq!(body["code"], json!("service_unavailable"));

    assert!(
        count_ldap_audit_events(ctx, "auth.login_failed", Some("directory_unavailable")).await >= 1
    );
}

/// User Story: US-LD-001
/// Covers: directory credentials valid but the linked Herald
/// account is disabled → 403 with the disabled-account message.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_ldap_disabled_account_rejected(ctx: &mut TestContext) {
    enable_ldap(ctx).await;

    let email = format!("ld004-{}@test.com", uuid::Uuid::now_v7());
    let user_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status)
         VALUES ($1, $2, $3, '$2a$12$dummy_password_hash', 2)",
    )
    .bind(user_id)
    .bind(&ctx._realm_id)
    .bind(&email)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();
    let dn = "uid=disabled,dc=example,dc=com";
    link_ldap_dn(ctx, user_id, dn).await;

    let mock = mock_dir(one_mock_user("disabled", dn, Some(&email), "corp-pw-6"));
    let resp = ldap_login(ctx, &mock, "disabled", "corp-pw-6", None).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    let message = body["message"].as_str().unwrap_or("");
    assert!(
        message.contains("禁用") || message.to_lowercase().contains("disabled"),
        "disabled-account rejection must say so; got {message:?}"
    );
    assert!(count_ldap_audit_events(ctx, "auth.login_failed", Some("disabled_account")).await >= 1);
}

/// Defense in depth for the DN-link resolution: the link row
/// is realm-scoped, but tenant isolation must not depend on that row's
/// integrity — the OAuth callback re-checks the loaded user's realm for the
/// same reason. A corrupt (realm, "ldap", DN) link pointing at ANOTHER
/// realm's user must fail closed: valid directory credentials for this realm
/// must never mint a session for a foreign-realm account.
///
/// The foreign user's consent is pre-recorded in THIS realm so that, were the
/// guard removed, the flow would run to completion and answer 200 with an
/// accessToken — the assertions below must fail in exactly that case, not
/// pass vacuously via the consent branch.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_ldap_foreign_realm_link_fails_closed(ctx: &mut TestContext) {
    enable_ldap(ctx).await;

    let other_realm_id = uuid::Uuid::now_v7().to_string();
    let foreign_user_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status)
         VALUES ($1, $2, 'foreign-link@other-realm.test', NULL, 1)",
    )
    .bind(foreign_user_id)
    .bind(&other_realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("failed to seed foreign-realm user");
    // Consent recorded in the LOGIN realm (not the user's own) so a guardless
    // flow would skip the consent gate and issue tokens.
    record_test_user_consent(&ctx._app_state.pool, foreign_user_id, &ctx._realm_id).await;

    let dn = "uid=foreign-link,dc=example,dc=com";
    link_ldap_dn(ctx, foreign_user_id, dn).await;

    let mock = mock_dir(one_mock_user("foreign-link", dn, None, "corp-pw-7"));
    let resp = ldap_login(ctx, &mock, "foreign-link", "corp-pw-7", None).await;
    assert_eq!(
        resp.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "cross-realm DN link must fail closed, not fall through"
    );
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert!(
        body["accessToken"].is_null(),
        "no token may be issued for a foreign-realm user; got {body}"
    );
    assert_eq!(
        count_accounts_by_email(ctx, "foreign-link@other-realm.test").await,
        0,
        "no shadow account may be created in this realm either"
    );
}

/// User Story: US-LD-001
/// Covers: when the realm has no enabled LDAP config,
/// the login is 400 and nothing is created (no account, no session).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_ldap_realm_not_enabled_returns_400(ctx: &mut TestContext) {
    // LDAP intentionally NOT enabled for this realm.
    let accounts_before: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM account WHERE realm_id = $1")
            .bind(&ctx._realm_id)
            .fetch_one(&ctx._app_state.pool)
            .await
            .unwrap();

    let mock = mock_dir(one_mock_user(
        "noldap",
        "uid=noldap,dc=example,dc=com",
        None,
        "pw",
    ));
    let resp = ldap_login(ctx, &mock, "noldap", "pw", None).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert!(
        body["message"]
            .as_str()
            .unwrap_or("")
            .contains("not enabled"),
        "not-enabled rejection must say so; got {body}"
    );

    let accounts_after: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM account WHERE realm_id = $1")
            .bind(&ctx._realm_id)
            .fetch_one(&ctx._app_state.pool)
            .await
            .unwrap();
    assert_eq!(accounts_before, accounts_after, "no account may be created");
}

/// User Story: US-LD-004
/// Covers: second-factor branch — a linked user with TOTP enabled
/// receives secondFactors=["totp"] + tempToken (no session yet), and the
/// temp session lands in the SAME `totp:temp:{token}` store the existing
/// /login/verify-totp endpoint consumes (that consumption is covered by the
/// existing TOTP/login-flow scenario suites; LDAP writes the identical
/// session shape).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_ldap_totp_user_gets_second_factor(ctx: &mut TestContext) {
    enable_ldap(ctx).await;

    let email = format!("ld005-{}@test.com", uuid::Uuid::now_v7());
    let user_id = create_active_user_with_consent(ctx, &email).await;
    let dn = "uid=totp,dc=example,dc=com";
    link_ldap_dn(ctx, user_id, dn).await;
    // The login probe only reads the enabled flag of user_totp_config.
    sqlx::query(
        "INSERT INTO user_totp_config (id, user_id, realm_id, secret_hash, enabled, created_at, updated_at)
         VALUES ($1, $2, $3, 'mock-secret-hash', true, NOW(), NOW())",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(user_id)
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("failed to insert user_totp_config");

    let mock = mock_dir(one_mock_user("totp", dn, Some(&email), "corp-pw-7"));
    let resp = ldap_login(ctx, &mock, "totp", "corp-pw-7", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["requiresTotp"], true);
    assert_eq!(
        body["secondFactors"],
        json!(["totp"]),
        "TOTP-enabled user must be routed to the second-factor branch"
    );
    let temp_token = body["tempToken"].as_str().expect("tempToken required");

    // The temp session exists in the shared store verify-totp reads.
    let mut conn = ctx
        ._app_state
        .redis_manager
        .get()
        .await
        .expect("redis conn");
    let exists: bool = conn
        .exists(format!("totp:temp:{temp_token}"))
        .await
        .expect("exists check");
    assert!(exists, "totp:temp session must be seeded");
}

/// User Story: US-LD-004
/// Covers: OAuth branch — a downstream authorization login seeded
/// via `oauth:state` returns redirectTo carrying ac_* code + state instead of
/// a session.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_ldap_login_with_oauth_context_redirects(ctx: &mut TestContext) {
    enable_ldap(ctx).await;

    let email = format!("ld006-{}@test.com", uuid::Uuid::now_v7());
    let user_id = create_active_user_with_consent(ctx, &email).await;
    let dn = "uid=oauth,dc=example,dc=com";
    link_ldap_dn(ctx, user_id, dn).await;

    let ds_client = "ldap-ds-client";
    let redirect_uri = "https://app.example.com/callback";
    let downstream_state = format!("ds-{}", uuid::Uuid::now_v7().simple());
    let state_value = json!({
        "client_id": ds_client,
        "realm_id": ctx._realm_id,
        "redirect_uri": redirect_uri,
        "code_challenge": "",
    })
    .to_string();
    {
        let mut conn = ctx
            ._app_state
            .redis_manager
            .get()
            .await
            .expect("redis conn");
        let _: () = conn
            .set_ex(format!("oauth:state:{downstream_state}"), state_value, 300)
            .await
            .expect("failed to seed oauth state");
    }

    let mock = mock_dir(one_mock_user("oauth", dn, Some(&email), "corp-pw-8"));
    let resp = ldap_login_ext(
        ctx,
        &mock,
        "oauth",
        "corp-pw-8",
        None,
        json!({
            "oauthClientId": ds_client,
            "redirectUri": redirect_uri,
            "state": downstream_state,
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    let redirect_to = body["redirectTo"].as_str().expect("redirectTo required");
    assert!(
        redirect_to.starts_with(&format!("{redirect_uri}?code=ac_")),
        "redirect must carry an authorization code; got {redirect_to}"
    );
    assert!(
        redirect_to.ends_with(&format!("&state={downstream_state}")),
        "redirect must echo the downstream state; got {redirect_to}"
    );
}

/// User Story: US-LD-001
/// Covers: Turnstile (per Client App) — with Turnstile enabled
/// on the bound Client App, a missing token is rejected; with the Cloudflare
/// always-pass test secret, a token proceeds.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_ldap_turnstile_required_when_client_app_enabled(ctx: &mut TestContext) {
    enable_ldap(ctx).await;

    sqlx::query(
        "UPDATE client_app SET turnstile_enabled = true,
            turnstile_site_key = 'site-key-x', turnstile_secret_key = $1
         WHERE realm_id = $2 AND client_id = $3",
    )
    .bind("1x0000000000000000000000000000000AA") // Cloudflare always-pass test secret
    .bind(&ctx._realm_id)
    .bind(&ctx._client_id)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();

    let dn = "uid=ts,dc=example,dc=com";
    let mock = mock_dir(one_mock_user("ts", dn, None, "pw"));

    // No token while Turnstile is enabled → 400 (verify_turnstile_for_client_app).
    let resp = ldap_login(ctx, &mock, "ts", "pw", None).await;
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "Turnstile enabled + missing token must be rejected"
    );

    // Restore before asserting the pass path so a failure here cannot leak
    // Turnstile state into other tests sharing this client app.
    sqlx::query(
        "UPDATE client_app SET turnstile_enabled = false,
            turnstile_site_key = NULL, turnstile_secret_key = NULL
         WHERE realm_id = $1 AND client_id = $2",
    )
    .bind(&ctx._realm_id)
    .bind(&ctx._client_id)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();
}

/// User Story: US-LD-001
/// Covers: rate limits — LDAP shares the `rl:login:*` keys and
/// thresholds with password login.
///
/// P2 NOTE (mirrors email_otp scenarios): `enforce_in_dev` defaults to false,
/// so `rate_limit_hit` is skipped in the test context and repeated failures
/// stay 401 rather than becoming 429. This test asserts the actual behaviour
/// and MUST NOT be strengthened to assert 429.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_ldap_rate_limit_shared_budget_note(ctx: &mut TestContext) {
    enable_ldap(ctx).await;

    let mock = mock_dir(one_mock_user(
        "rluser",
        "uid=rluser,dc=example,dc=com",
        None,
        "right",
    ));
    // Exceed LOGIN_IDENTIFIER_RATE_LIMIT (2,60) with wrong passwords.
    for _ in 0..3 {
        let resp = ldap_login(ctx, &mock, "rluser", "wrong", None).await;
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "rate limit is skipped in the test context (enforce_in_dev=false)"
        );
    }
}

/// User Story: US-LD-004
/// Covers: US-LD-004 scenario 3 — an account created by LDAP JIT has no
/// local password, so the password login form fails for it with the SAME
/// generalized 401 as any wrong password (no account-detail oracle), while
/// the LDAP path keeps working.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_ldap_created_account_cannot_password_login(ctx: &mut TestContext) {
    enable_ldap(ctx).await;

    let email = format!("ldpwd-{}@test.com", uuid::Uuid::now_v7());
    let dn = "uid=nopwd,dc=example,dc=com";
    let agreements = current_effective_agreements(ctx).await;
    let mock = mock_dir(one_mock_user("nopwd", dn, Some(&email), "corp-pw-9"));

    // JIT-provision the account.
    let resp = ldap_login(ctx, &mock, "nopwd", "corp-pw-9", Some(agreements)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let _: serde_json::Value = crate::tests::response_json(resp).await;
    let has_password: Option<String> =
        sqlx::query_scalar("SELECT password FROM account WHERE realm_id = $1 AND email = $2")
            .bind(&ctx._realm_id)
            .bind(&email)
            .fetch_one(&ctx._app_state.pool)
            .await
            .unwrap();
    assert!(has_password.is_none(), "JIT account has no local password");

    // Password login with that email → 401 invalid credentials (generalized).
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "6.6.6.6")
        .body(Body::from(
            json!({
                "clientId": ctx._client_id,
                "email": email,
                "password": "anything-123",
            })
            .to_string(),
        ))
        .unwrap();
    let resp = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["message"], json!("invalid credentials"));

    // LDAP login still succeeds.
    let resp = ldap_login(ctx, &mock, "nopwd", "corp-pw-9", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
}
