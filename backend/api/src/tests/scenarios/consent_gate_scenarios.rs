// Exercises the consent gate injected into the existing auth endpoints by
// BE-D08. The gate runs after credentials (and after TOTP when enabled) and
// before session issuance:
//
//   credentials → TOTP → consent check → session
//
// The login path returns `requires_totp` before checking consent. The
// verify-totp issuance point then runs the same consent gate before creating
// an OAuth code or session.
//
// HTTP routes used (no new paths per BE-D08):
//   POST /api/auth/{realmId}/login
//   POST /api/auth/{realmId}/register
//   POST /api/user/consent   (existing consent endpoint, BE-D05)
//   PUT  /api/legal/admin/agreements/{type}   (admin publish)
//
// User stories: docs/user-stories/core/legal-consent-account-deletion.md
//   US-RU-011, US-RU-012, US-RU-015

use crate::tests::helpers::auth_helpers::{
    create_admin_session_with_user, generate_totp_code, grant_realm_admin_role,
};
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::Response as AxumResponse,
};
use herald_core::domain::legal::entities::AgreementType;
use herald_core::domain::user_totp::UserTotpService;
use serde_json::{Value, json};
use std::collections::HashMap;
use test_context::test_context;
use tower::ServiceExt;
use uuid::Uuid;

/// Enable registration and disable email verification so that a register call
/// immediately creates an active user that can log in.
async fn enable_instant_registration(ctx: &TestContext) {
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
         VALUES ($1, 'registration', 'enabled', 'true', true)",
    )
    .bind(&ctx._realm_id)
    .execute(&ctx.app_state.pool)
    .await
    .expect("failed to enable registration");

    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
         VALUES ($1, 'registration', 'require_email_verification', 'false', true)",
    )
    .bind(&ctx._realm_id)
    .execute(&ctx.app_state.pool)
    .await
    .expect("failed to disable email verification");
}

/// POST /api/auth/{realmId}/register with the given credentials.
///
/// The caller owns the returned response and must consume its body.
async fn register_user(
    ctx: &TestContext,
    realm_id: &str,
    email: &str,
    password: &str,
) -> AxumResponse {
    let payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{realm_id}/register"))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(payload.to_string()))
        .expect("failed to build register request");

    ctx.create_unified_test_router()
        .oneshot(request)
        .await
        .expect("register request must dispatch")
}

/// POST /api/auth/{realmId}/login with the given credentials.
///
/// Returns the response together with the issued browser access token, if any.
async fn login_user(
    ctx: &TestContext,
    realm_id: &str,
    email: &str,
    password: &str,
) -> (AxumResponse, Option<String>) {
    let payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{realm_id}/login"))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(payload.to_string()))
        .expect("failed to build login request");

    let response = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .expect("login request must dispatch");

    crate::tests::extract_bearer_token(response).await
}

/// POST /api/auth/{realmId}/login/verify-totp with the temporary token.
async fn verify_totp_login(
    ctx: &TestContext,
    realm_id: &str,
    temp_token: &str,
    code: &str,
) -> (AxumResponse, Option<String>) {
    let payload = json!({
        "tempToken": temp_token,
        "code": code
    });

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{realm_id}/login/verify-totp"))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(payload.to_string()))
        .expect("failed to build verify-totp request");

    let response = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .expect("verify-totp request must dispatch");

    crate::tests::extract_bearer_token(response).await
}

/// Insert an enabled TOTP config and return the plaintext secret for code generation.
async fn seed_enabled_totp(ctx: &TestContext, realm_id: &str, user_id: Uuid) -> String {
    let secret = UserTotpService::generate_secret();
    let secret_hash =
        UserTotpService::encrypt_secret(&secret).expect("test TOTP secret must encrypt");

    sqlx::query(
        "INSERT INTO user_totp_config
            (id, user_id, realm_id, secret_hash, key_version, enabled, verified_at, created_at, updated_at)
         VALUES ($1, $2, $3, $4, 1, true, NOW(), NOW(), NOW())",
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(realm_id)
    .bind(secret_hash)
    .execute(&ctx.app_state.pool)
    .await
    .expect("failed to seed enabled TOTP config");

    secret
}

/// POST /api/user/consent on behalf of an already-authenticated user.
async fn consent_to_current(
    ctx: &TestContext,
    _realm_id: &str,
    token: &str,
    items: &[(AgreementType, Uuid)],
) {
    let agreements: Vec<Value> = items
        .iter()
        .map(|(t, id)| {
            json!({
                "agreement_type": t.as_str(),
                "version_id": id
            })
        })
        .collect();

    let body = json!({ "agreements": agreements }).to_string();

    let request = Request::builder()
        .method("POST")
        .uri("/api/user/consent".to_string())
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(body))
        .expect("failed to build consent request");

    let response = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .expect("consent request must dispatch");

    assert_eq!(
        response.status(),
        StatusCode::NO_CONTENT,
        "consent to current effective version must return 204"
    );
}

/// Resolve the current effective version_id for both agreement types via the
/// public agreements endpoint. Returns at least ToS + Privacy when the seed
/// defaults are present.
async fn current_version_ids(ctx: &TestContext, realm_id: &str) -> Vec<(AgreementType, Uuid)> {
    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/legal/public/{realm_id}/agreements"))
        .body(Body::empty())
        .expect("failed to build agreements request");

    let response = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .expect("agreements request must dispatch");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "public agreements must resolve"
    );

    let body: Value = crate::tests::response_json(response).await;
    let agreements = body["agreements"]
        .as_array()
        .expect("agreements response must contain an array");

    agreements
        .iter()
        .map(|a| {
            let agreement_type = AgreementType::try_from(
                a["agreement_type"]
                    .as_str()
                    .expect("agreement_type must be a string"),
            )
            .expect("unknown agreement type");
            let version_id = Uuid::parse_str(
                a["version_id"]
                    .as_str()
                    .expect("version_id must be a string"),
            )
            .expect("version_id must be a UUID");
            (agreement_type, version_id)
        })
        .collect()
}

/// Admin publish a new custom version of the given agreement type and return
/// the newly minted `version_id`.
async fn publish_new_version_as_admin(
    ctx: &TestContext,
    _realm_id: &str,
    agreement_type: &str,
) -> Uuid {
    let admin_email = format!("admin-consent-gate-{}@test.com", Uuid::now_v7());
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, &admin_email, 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let body = json!({
        "content": { "en": format!("custom {agreement_type} body") }
    })
    .to_string();

    let request = Request::builder()
        .method("PUT")
        .uri(format!("/api/legal/admin/agreements/{agreement_type}"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {admin_token}"))
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(body))
        .expect("failed to build publish request");

    let response = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .expect("publish request must dispatch");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "admin publish must succeed"
    );

    let body: Value = crate::tests::response_json(response).await;
    Uuid::parse_str(
        body["version_id"]
            .as_str()
            .expect("published response must contain version_id"),
    )
    .expect("version_id must be a UUID")
}

// =============================================================================
// Scenario tests
// =============================================================================

/// User Story: US-RU-015 (login when already consented to the latest version)
/// Covers: Design §5.1 — when the user's recorded consent matches the current
/// effective versions, login falls through to the existing session issuance
/// without recording consent again.
///
/// WHY this matters: this is the happy path after the gate is injected. It must
/// remain indistinguishable from the pre-BE-D08 login flow: a valid password
/// still yields a session cookie and no frontend-visible consent flag.
#[test_context(TestContext)]
#[tokio::test]
async fn test_login_consented_latest_issues_session(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    enable_instant_registration(&*ctx).await;

    let email = "login-consented@cas.com";
    let password = "password123";

    let reg_resp = register_user(&*ctx, &realm_id, email, password).await;
    assert_eq!(
        reg_resp.status(),
        StatusCode::OK,
        "registration must succeed"
    );

    let user_id: Uuid =
        sqlx::query_scalar("SELECT id FROM account WHERE realm_id = $1 AND email = $2")
            .bind(&realm_id)
            .bind(email)
            .fetch_one(&ctx._app_state.pool)
            .await
            .expect("registered user must exist");
    let login_audit_before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE realm_id = $1
           AND actor_id = $2
           AND action = 'agreement.consent'
           AND details->>'source' = 'login'",
    )
    .bind(&realm_id)
    .bind(user_id.to_string())
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();

    let (login_resp, token) = login_user(&*ctx, &realm_id, email, password).await;
    assert_eq!(login_resp.status(), StatusCode::OK, "login must return 200");
    assert!(
        token.is_some(),
        "login must issue a session cookie when consent is current"
    );

    let body: Value = crate::tests::response_json(login_resp).await;
    assert!(
        body.get("consentRequired")
            .is_none_or(|v| v.as_bool() != Some(true)),
        "consentRequired must be absent or false when consent is current"
    );

    let login_audit_after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE realm_id = $1
           AND actor_id = $2
           AND action = 'agreement.consent'
           AND details->>'source' = 'login'",
    )
    .bind(&realm_id)
    .bind(user_id.to_string())
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(
        login_audit_after, login_audit_before,
        "normal login must not record consent again when current consent already exists"
    );
}

/// User Story: US-RU-012 / US-RU-015 (version mismatch blocks session issuance)
/// Covers: Design §4.1/§4.2 — when the effective version has changed since the
/// user last consented, login returns HTTP 200 + consent_required=true +
/// current effective summaries, and NO session is issued.
///
/// WHY this matters: the legal gate must not invent a new HTTP status code. By
/// mirroring the existing `requires_totp` 200-flag pattern, the frontend can
/// reuse the same flow (collect consent, re-submit login) without special-case
/// 4xx handling.
#[test_context(TestContext)]
#[tokio::test]
async fn test_login_version_mismatch_returns_consent_required(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    enable_instant_registration(&*ctx).await;

    let email = "login-mismatch@cas.com";
    let password = "password123";

    // Register records consent to the current (seed default) versions.
    let reg_resp = register_user(&*ctx, &realm_id, email, password).await;
    assert_eq!(
        reg_resp.status(),
        StatusCode::OK,
        "registration must succeed"
    );

    // Admin publishes a newer custom ToS → the user's ToS consent becomes stale.
    let new_tos_id = publish_new_version_as_admin(&*ctx, &realm_id, "terms_of_service").await;

    let (login_resp, token) = login_user(&*ctx, &realm_id, email, password).await;
    assert_eq!(
        login_resp.status(),
        StatusCode::OK,
        "stale consent must still return 200, not a 4xx"
    );
    assert!(
        token.is_none(),
        "no session cookie must be issued when consent is stale"
    );

    let body: Value = crate::tests::response_json(login_resp).await;
    assert_eq!(
        body["consentRequired"].as_bool(),
        Some(true),
        "consentRequired must be true when versions mismatch"
    );

    let agreements = body["agreements"]
        .as_array()
        .expect("agreements must be present when consentRequired=true");
    assert_eq!(
        agreements.len(),
        2,
        "agreements must contain both ToS and Privacy summaries"
    );

    let types: std::collections::HashSet<&str> = agreements
        .iter()
        .map(|a| {
            a["agreement_type"]
                .as_str()
                .expect("agreement_type must be a string")
        })
        .collect();
    assert!(types.contains("terms_of_service"));
    assert!(types.contains("privacy_policy"));

    let tos_summary = agreements
        .iter()
        .find(|a| a["agreement_type"] == "terms_of_service")
        .expect("ToS summary must be present");
    assert_eq!(
        tos_summary["version_id"].as_str(),
        Some(new_tos_id.to_string().as_str()),
        "returned ToS version must be the newly published one"
    );
}

/// User Story: US-RU-012 / US-RU-015 (TOTP users are gated after second factor)
/// Covers: Design §4.1 order — credentials -> TOTP -> consent -> session.
///
/// WHY this matters: TOTP-enabled users must not bypass a newly published
/// agreement version. The first password step may only return a temp token;
/// the verify-totp issuance point must block before creating a session.
#[test_context(TestContext)]
#[tokio::test]
async fn test_verify_totp_version_mismatch_returns_consent_required(ctx: &mut TestContext) {
    unsafe {
        std::env::set_var("TOTP_SECRET_KEY", "test_key_32_bytes_long_1234567890");
    }

    let realm_id = ctx._realm_id.clone();
    enable_instant_registration(&*ctx).await;

    let email = "login-totp-mismatch@cas.com";
    let password = "password123";

    let reg_resp = register_user(&*ctx, &realm_id, email, password).await;
    assert_eq!(
        reg_resp.status(),
        StatusCode::OK,
        "registration must succeed"
    );

    let user_id: Uuid =
        sqlx::query_scalar("SELECT id FROM account WHERE realm_id = $1 AND email = $2")
            .bind(&realm_id)
            .bind(email)
            .fetch_one(&ctx.app_state.pool)
            .await
            .expect("registered user must exist");
    let totp_secret = seed_enabled_totp(&*ctx, &realm_id, user_id).await;

    let new_tos_id = publish_new_version_as_admin(&*ctx, &realm_id, "terms_of_service").await;

    let (login_resp, login_token) = login_user(&*ctx, &realm_id, email, password).await;
    assert_eq!(login_resp.status(), StatusCode::OK);
    assert!(
        login_token.is_none(),
        "password step for TOTP must not issue a session"
    );

    let login_body: Value = crate::tests::response_json(login_resp).await;
    assert_eq!(login_body["requiresTotp"].as_bool(), Some(true));
    assert!(
        !login_body
            .as_object()
            .expect("login body must be an object")
            .contains_key("consentRequired"),
        "consent gate must wait until TOTP succeeds"
    );
    let temp_token = login_body["tempToken"]
        .as_str()
        .expect("TOTP login must return tempToken");

    let totp_code = generate_totp_code(&totp_secret);
    let (verify_resp, verify_token) =
        verify_totp_login(&*ctx, &realm_id, temp_token, &totp_code).await;
    assert_eq!(
        verify_resp.status(),
        StatusCode::OK,
        "stale consent after TOTP must still return 200"
    );
    assert!(
        verify_token.is_none(),
        "verify-totp must not issue a session when consent is stale"
    );

    let verify_body: Value = crate::tests::response_json(verify_resp).await;
    assert_eq!(verify_body["consentRequired"].as_bool(), Some(true));
    assert_eq!(
        verify_body["token"].as_str(),
        Some(""),
        "no session token must be returned on the consent-required branch"
    );

    let agreements = verify_body["agreements"]
        .as_array()
        .expect("agreements must be present when consentRequired=true");
    let tos_summary = agreements
        .iter()
        .find(|a| a["agreement_type"] == "terms_of_service")
        .expect("ToS summary must be present");
    assert_eq!(
        tos_summary["version_id"].as_str(),
        Some(new_tos_id.to_string().as_str()),
        "returned ToS version must be the newly published one"
    );
}

/// User Story: US-RU-012 / US-RU-015 (re-consent closes the gate)
/// Covers: Design §5.1 — after a version change, the user records consent to
/// the new current version, and the next login issues a session normally.
///
/// WHY this matters: the re-consent step must actually unblock the gate. If
/// recording consent did not reset the verdict, users would be trapped in a
/// consent loop even after explicitly agreeing.
#[test_context(TestContext)]
#[tokio::test]
async fn test_login_signs_session_after_consent_recorded(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    enable_instant_registration(&*ctx).await;

    let email = "login-reconsent@cas.com";
    let password = "password123";

    let reg_resp = register_user(&*ctx, &realm_id, email, password).await;
    assert_eq!(
        reg_resp.status(),
        StatusCode::OK,
        "registration must succeed"
    );

    // First login: consent is current, so a session is issued.
    let (first_login_resp, token) = login_user(&*ctx, &realm_id, email, password).await;
    assert_eq!(first_login_resp.status(), StatusCode::OK);
    let token = token.expect("first login must issue a session");

    // Publish a new ToS while the user still holds a valid Bearer access token.
    publish_new_version_as_admin(&*ctx, &realm_id, "terms_of_service").await;

    // The user's consent/status now flags ToS as needing re-consent.
    let status_request = Request::builder()
        .method("GET")
        .uri("/api/user/consent/status".to_string())
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .expect("failed to build status request");

    let status_resp = ctx
        .create_unified_test_router()
        .oneshot(status_request)
        .await
        .expect("status request must dispatch");
    assert_eq!(status_resp.status(), StatusCode::OK);

    let status_body: Value = crate::tests::response_json(status_resp).await;
    let tos_status = status_body["items"]
        .as_array()
        .expect("status must contain items")
        .iter()
        .find(|i| i["agreement_type"] == "terms_of_service")
        .expect("ToS status item must be present");
    assert_eq!(
        tos_status["needs_reconsent"].as_bool(),
        Some(true),
        "new publish must flip needs_reconsent=true"
    );

    // Record consent to the new current versions while logged in.
    let current = current_version_ids(&*ctx, &realm_id).await;
    consent_to_current(&*ctx, &realm_id, &token, &current).await;

    // Subsequent login must now issue a session again.
    let (login_resp, token2) = login_user(&*ctx, &realm_id, email, password).await;
    assert_eq!(login_resp.status(), StatusCode::OK);
    assert!(
        token2.is_some(),
        "login must issue a session after re-consent"
    );

    let body: Value = crate::tests::response_json(login_resp).await;
    assert!(
        !body
            .as_object()
            .expect("login body must be an object")
            .contains_key("consentRequired"),
        "consentRequired must be absent after re-consent"
    );
}

/// User Story: US-RU-011 (registration records consent with source=Register)
/// Covers: Design §5.1 — a successful registration best-effort records consent
/// to the current effective ToS and Privacy versions, attributed to the
/// Register source.
///
/// WHY this matters: "register = consent" is the legal baseline. The database
/// and audit trail must both reflect that the user agreed at account creation
/// time, using the Register source so downstream audit queries can distinguish
/// it from login-time auto-renewal or explicit re-consent.
#[test_context(TestContext)]
#[tokio::test]
async fn test_register_records_consent_register_source(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    enable_instant_registration(&*ctx).await;

    let email = "register-consent-source@cas.com";
    let password = "password123";

    let reg_resp = register_user(&*ctx, &realm_id, email, password).await;
    assert_eq!(
        reg_resp.status(),
        StatusCode::OK,
        "registration must succeed"
    );

    let reg_body: Value = crate::tests::response_json(reg_resp).await;
    assert_eq!(
        reg_body["verificationRequired"].as_bool(),
        Some(false),
        "registration must not require email verification in this test"
    );

    let user_id: Uuid =
        sqlx::query_scalar("SELECT id FROM account WHERE realm_id = $1 AND email = $2")
            .bind(&realm_id)
            .bind(email)
            .fetch_one(&ctx.app_state.pool)
            .await
            .expect("registered user must exist");

    let current = current_version_ids(&*ctx, &realm_id).await;
    let mut expected: HashMap<String, Uuid> = HashMap::new();
    for (t, id) in current {
        expected.insert(t.as_str().to_string(), id);
    }

    let rows: Vec<(String, Uuid)> = sqlx::query_as(
        "SELECT agreement_type, consented_version_id
         FROM user_agreement_consent
         WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_all(&ctx.app_state.pool)
    .await
    .expect("consent rows must be readable");

    assert_eq!(
        rows.len(),
        2,
        "registration must record one consent row per agreement type"
    );
    for (agreement_type, version_id) in rows {
        assert_eq!(
            expected.get(&agreement_type),
            Some(&version_id),
            "consented_version_id must match the current effective version for {agreement_type}"
        );
    }

    let audit_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM audit_events
         WHERE realm_id = $1
           AND category = 'compliance'
           AND action = 'agreement.consent'
           AND target_id = $2
           AND details->>'source' = 'register'",
    )
    .bind(&realm_id)
    .bind(user_id.to_string())
    .fetch_one(&ctx.app_state.pool)
    .await
    .expect("audit query must succeed");

    assert_eq!(
        audit_count, 2,
        "registration must emit one agreement.consent audit event per type with source=register"
    );
}

/// User Story: US-RU-011 best-effort fallback
/// Covers: Design §5.1 — consent recording at registration is best-effort. If
/// no effective agreement version is deployed (seed missing), registration must
/// still succeed and must not write any consent rows.
///
/// WHY this matters: making registration depend on legal seed data would be a
/// dangerous coupling. A realm without deployed agreements must still allow
/// account creation; the user will simply be prompted at first login.
#[test_context(TestContext)]
#[tokio::test]
async fn test_register_does_not_block_when_seed_missing(ctx: &mut TestContext) {
    // Remove the platform-default templates for this isolated schema.
    sqlx::query("DELETE FROM legal_agreement_version WHERE realm_id IS NULL")
        .execute(&ctx.app_state.pool)
        .await
        .expect("failed to delete platform-default agreement rows");

    let realm_id = ctx._realm_id.clone();
    enable_instant_registration(&*ctx).await;

    let email = "register-no-seed@cas.com";
    let password = "password123";

    let reg_resp = register_user(&*ctx, &realm_id, email, password).await;
    assert_eq!(
        reg_resp.status(),
        StatusCode::OK,
        "registration must succeed even when no agreement versions are deployed"
    );

    let reg_body: Value = crate::tests::response_json(reg_resp).await;
    assert_eq!(reg_body["verificationRequired"].as_bool(), Some(false));

    let user_id: Uuid =
        sqlx::query_scalar("SELECT id FROM account WHERE realm_id = $1 AND email = $2")
            .bind(&realm_id)
            .bind(email)
            .fetch_one(&ctx.app_state.pool)
            .await
            .expect("registered user must exist");

    let consent_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM user_agreement_consent WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .expect("consent count must be readable");

    assert_eq!(
        consent_count, 0,
        "no consent rows must be written when no effective versions exist"
    );
}

/// User Story: US-RU-015 / Design §6.3 (no-version-change regression)
/// Covers: Design §6.3 — when nothing has changed, injecting the consent gate
/// must not alter the normal login/register behavior. A registered user with
// current consent still gets a session and sees no consent flag.
///
/// WHY this matters: BE-D08 changes a hot authentication path. This test guards
/// against the gate accidentally blocking or changing the response shape for
/// the vast majority of users who are already up to date.
#[test_context(TestContext)]
#[tokio::test]
async fn test_login_no_version_change_regression(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    enable_instant_registration(&*ctx).await;

    let email = "login-regression@cas.com";
    let password = "password123";

    let reg_resp = register_user(&*ctx, &realm_id, email, password).await;
    assert_eq!(
        reg_resp.status(),
        StatusCode::OK,
        "registration must succeed"
    );

    let (login_resp, token) = login_user(&*ctx, &realm_id, email, password).await;
    assert_eq!(login_resp.status(), StatusCode::OK);
    assert!(
        token.is_some(),
        "normal login must still issue a session when the gate is injected"
    );

    let body: Value = crate::tests::response_json(login_resp).await;
    assert!(
        !body
            .as_object()
            .expect("login body must be an object")
            .contains_key("consentRequired"),
        "consentRequired must be absent when there is no version change"
    );
}

/// User Story: US-RU-012 / US-RU-015
/// Covers: Design §4.2 serialization constraint — `consent_required` and
/// `agreements` use `skip_serializing_if = Option::is_none`. They must appear
/// only on the stale-consent branch and be absent on the normal branch, so
/// existing callers/frontends are unaffected.
///
/// WHY this matters: adding optional fields to a response can break clients that
/// treat unknown keys as errors or that branch on key presence. The
/// `skip_serializing_if` guarantee is part of the contract and must hold on the
/// wire.
#[test_context(TestContext)]
#[tokio::test]
async fn test_consent_required_appears_only_when_flag_true(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    enable_instant_registration(&*ctx).await;

    // Branch A: consent current → no consent-related keys.
    let email_current = "login-current-flag@cas.com";
    let password_current = "password123";
    let reg_resp_a = register_user(&*ctx, &realm_id, email_current, password_current).await;
    assert_eq!(reg_resp_a.status(), StatusCode::OK);

    let (login_resp_a, token_a) =
        login_user(&*ctx, &realm_id, email_current, password_current).await;
    assert_eq!(login_resp_a.status(), StatusCode::OK);
    assert!(token_a.is_some());

    let body_a: Value = crate::tests::response_json(login_resp_a).await;
    let obj_a = body_a.as_object().expect("login body must be an object");
    assert!(
        !obj_a.contains_key("consentRequired"),
        "consentRequired must be absent on the current-consent branch"
    );
    assert!(
        !obj_a.contains_key("agreements"),
        "agreements must be absent on the current-consent branch"
    );

    // Branch B: version mismatch → keys present.
    let email_stale = "login-stale-flag@cas.com";
    let password_stale = "password123";
    let reg_resp_b = register_user(&*ctx, &realm_id, email_stale, password_stale).await;
    assert_eq!(reg_resp_b.status(), StatusCode::OK);

    publish_new_version_as_admin(&*ctx, &realm_id, "terms_of_service").await;

    let (login_resp_b, token_b) = login_user(&*ctx, &realm_id, email_stale, password_stale).await;
    assert_eq!(login_resp_b.status(), StatusCode::OK);
    assert!(token_b.is_none(), "stale login must not issue a session");

    let body_b: Value = crate::tests::response_json(login_resp_b).await;
    let obj_b = body_b.as_object().expect("login body must be an object");
    assert!(
        obj_b.contains_key("consentRequired"),
        "consentRequired must be present on the stale-consent branch"
    );
    assert_eq!(body_b["consentRequired"].as_bool(), Some(true));
    assert!(
        obj_b.contains_key("agreements"),
        "agreements must be present on the stale-consent branch"
    );
}
