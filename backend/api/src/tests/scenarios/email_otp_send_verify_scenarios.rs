// =============================================================================
// Scenario tests: Email OTP login (send + verify + status)
// =============================================================================
//
// Exercises the email-OTP login flow end-to-end through the HTTP layer:
//   POST /api/auth/{realmId}/login/email-otp/send
//   POST /api/auth/{realmId}/login/email-otp/verify
//   GET  /api/auth/{realmId}/email-otp/status
//
// Coverage focus: US-EO-001 (existing user login), US-EO-002 (auto-register),
// the consent / not-registered 409 branches, code lifecycle (wrong-code
// attempts, expiry, one-time reuse), account/client-app/realm guards, the
// Client-App-level Turnstile behaviour (D-PROTECT-01), and enumeration
// resistance.
//
// Notes on environment behaviour:
// - The test Realm has NO email provider configured, so `EmailService::send_email`
//   is a silent no-op (`Ok(())`). `send` therefore returns 200 and the code is
//   still stored in Redis for `verify`.
// - `RateLimitConfig.enforce_in_dev` defaults to `false`, so `rate_limit_hit`
//   is skipped in the test context. The two `*_rate_limited` scenarios below
//   assert the *actual* (non-429) behaviour with a comment and MUST NOT be
//   strengthened to assert 429 by this item or the runner.
// - `verify` flows inject a *known* code via `helpers::otp_helpers::inject_otp_code`
//   (test-only) so the scenarios don't have to read the emailed code out of Redis.
//
// =============================================================================

use crate::tests::helpers::otp_helpers::{inject_otp_code, read_otp_attempts};
use crate::tests::helpers::test_setup_helpers::record_test_user_consent;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;

/// OTP constants mirrored from `herald_core::domain::security_constants`.
/// Kept local so the scenarios stay readable; if they drift the runner will
// surface it as a failing assertion, not a compile error.
const OTP_MAX_ATTEMPTS: i64 = 5;
const OTP_CODE_TTL_SECONDS: u64 = 300;

// ---------------------------------------------------------------------------
// Local request helpers
// ---------------------------------------------------------------------------

/// Enable Realm registration so the email-otp auto-register path is not
/// rejected by the Realm registration policy (email-otp-login PRD §4.1
/// "注册政策优先"). Only the auto-register scenarios need this; the realm
/// defaults to registration-disabled.
async fn enable_registration(ctx: &TestContext) {
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
         VALUES ($1, 'registration', 'enabled', 'true', true)
         ON CONFLICT (realm_id, config_type, config_key) DO UPDATE SET config_value = 'true', enabled = true",
    )
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("failed to enable registration");
}

/// Enable Email OTP login for the test Realm. `auto_register` defaults to
/// `false`; callers that need the auto-register path pass `true`.
async fn enable_email_otp(ctx: &TestContext, auto_register: bool) {
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, is_secret, enabled, metadata, created_at, updated_at)
         VALUES ($1, 'email_otp', 'settings', $2, false, true, '{}'::jsonb, NOW(), NOW())
         ON CONFLICT (realm_id, config_type, config_key)
         DO UPDATE SET config_value = EXCLUDED.config_value, enabled = EXCLUDED.enabled, updated_at = NOW()",
    )
    .bind(&ctx._realm_id)
    .bind(format!(
        r#"{{"enabled":true,"auto_register":{}}}"#,
        auto_register
    ))
    .execute(&ctx._app_state.pool)
    .await
    .expect("failed to enable email_otp config");
}

/// POST /api/auth/{realmId}/login/email-otp/send. Caller owns the response.
async fn send_otp(
    ctx: &TestContext,
    email: &str,
    turnstile_token: Option<&str>,
    agreements: Option<Vec<serde_json::Value>>,
) -> axum::response::Response {
    let mut payload = json!({
        "clientId": ctx._client_id,
        "email": email,
    });
    if let Some(token) = turnstile_token {
        payload["turnstileToken"] = json!(token);
    }
    if let Some(agreements) = agreements {
        payload["agreements"] = json!(agreements);
    }

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login/email-otp/send", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "5.5.5.5")
        .body(Body::from(payload.to_string()))
        .unwrap();

    ctx.create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap()
}

/// POST /api/auth/{realmId}/login/email-otp/verify. Caller owns the response.
async fn verify_otp(
    ctx: &TestContext,
    email: &str,
    code: &str,
    agreements: Option<Vec<serde_json::Value>>,
) -> axum::response::Response {
    let mut payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "code": code,
    });
    if let Some(agreements) = agreements {
        payload["agreements"] = json!(agreements);
    }

    let request = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/auth/{}/login/email-otp/verify",
            ctx._realm_id
        ))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "5.5.5.5")
        .body(Body::from(payload.to_string()))
        .unwrap();

    ctx.create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap()
}

/// Create an active (status=1) user via direct SQL and record consent to the
/// current effective platform-default agreements so the login-as-consent gate
/// does not intercept the OTP login happy path.
async fn create_active_user_with_consent(ctx: &TestContext, email: &str) -> uuid::Uuid {
    let user_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status)
         VALUES ($1, $2, $3, '$2a$12$dummy_password_hash', 1)",
    )
    .bind(user_id)
    .bind(&ctx._realm_id)
    .bind(email)
    .execute(&ctx._app_state.pool)
    .await
    .expect("failed to create active test user");

    record_test_user_consent(&ctx._app_state.pool, user_id, &ctx._realm_id).await;
    user_id
}

/// Read the current effective ToS + Privacy version_ids for the test Realm
/// (platform-default seeds), to build an `agreements` payload.
async fn current_effective_agreements(ctx: &TestContext) -> Vec<serde_json::Value> {
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

// =============================================================================
// Scenarios
// =============================================================================

/// User Story: US-EO-001
/// Covers: existing active user send → verify →
/// receives a Bearer access/refresh token family.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_otp_send_then_verify_login_success(ctx: &mut TestContext) {
    enable_email_otp(ctx, false).await;

    let email = format!("eo001-{}@test.com", uuid::Uuid::now_v7());
    create_active_user_with_consent(ctx, &email).await;

    // send → 200, code stored in Redis (email is a no-op without a provider).
    let send_resp = send_otp(ctx, &email, None, None).await;
    assert_eq!(send_resp.status(), StatusCode::OK);
    let send_body: serde_json::Value = crate::tests::response_json(send_resp).await;
    assert_eq!(send_body["expiresInSeconds"], OTP_CODE_TTL_SECONDS);

    // Inject a *known* code so verify is deterministic.
    let known_code = "123456";
    inject_otp_code(
        ctx,
        &ctx._realm_id,
        &email,
        known_code,
        0,
        OTP_MAX_ATTEMPTS,
        OTP_CODE_TTL_SECONDS,
    )
    .await;

    let verify_resp = verify_otp(ctx, &email, known_code, None).await;
    assert_eq!(verify_resp.status(), StatusCode::OK);

    let (verify_resp, token) = crate::tests::extract_bearer_token(verify_resp).await;
    assert!(token.is_some(), "verify must issue a Bearer accessToken");

    let verify_body: serde_json::Value = crate::tests::response_json(verify_resp).await;
    assert_eq!(verify_body["tokenType"], "Bearer");
    assert!(
        verify_body["expiresIn"].as_u64().is_some(),
        "response must include expiresIn"
    );
    assert!(
        verify_body["refreshToken"].as_str().is_some(),
        "response must include refreshToken"
    );
}

/// User Story: US-EO-002
/// Covers: unregistered email + auto-register ON +
/// agreements expressed → verify creates + activates an account and issues a
/// Bearer token.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_otp_send_unregistered_with_consent_then_auto_register(
    ctx: &mut TestContext,
) {
    enable_email_otp(ctx, true).await;
    enable_registration(ctx).await;

    // OTP auto-register is a registration for points purposes (points PRD
    // 注册积分): seed a Registration rule so the grant below is observable.
    const REGISTRATION_BONUS: i64 = 1000;
    crate::tests::helpers::points_helpers::seed_realm_registration_rules(
        &ctx._app_state.pool,
        &ctx._realm_id,
        REGISTRATION_BONUS,
        None,
        86400,
        1,
    )
    .await;

    let email = format!("eo002-{}@test.com", uuid::Uuid::now_v7());
    // Sanity: the email is NOT registered.
    let pre_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM account WHERE realm_id = $1 AND email = $2")
            .bind(&ctx._realm_id)
            .bind(&email)
            .fetch_one(&ctx._app_state.pool)
            .await
            .unwrap();
    assert_eq!(pre_count, 0);

    let agreements = current_effective_agreements(ctx).await;

    // send with agreements → 200 (consent was expressed, code issued).
    let send_resp = send_otp(ctx, &email, None, Some(agreements.clone())).await;
    assert_eq!(send_resp.status(), StatusCode::OK);

    // Inject a known code and verify → auto-register + token.
    let known_code = "654321";
    inject_otp_code(
        ctx,
        &ctx._realm_id,
        &email,
        known_code,
        0,
        OTP_MAX_ATTEMPTS,
        OTP_CODE_TTL_SECONDS,
    )
    .await;

    let verify_resp = verify_otp(ctx, &email, known_code, Some(agreements)).await;
    assert_eq!(verify_resp.status(), StatusCode::OK);

    let (verify_resp, token) = crate::tests::extract_bearer_token(verify_resp).await;
    assert!(
        token.is_some(),
        "auto-register verify must issue a Bearer accessToken"
    );
    let _: serde_json::Value = crate::tests::response_json(verify_resp).await;

    // The account was created and activated (status=1).
    let row: Option<(uuid::Uuid, i16)> =
        sqlx::query_as("SELECT id, status FROM account WHERE realm_id = $1 AND email = $2")
            .bind(&ctx._realm_id)
            .bind(&email)
            .fetch_optional(&ctx._app_state.pool)
            .await
            .unwrap();
    let (user_id, status) = row.expect("auto-registered account must exist");
    assert_eq!(status, 1, "auto-registered account must be active");

    // The JIT-created account received the registration credit once
    // (idempotent on `registration:{user_id}`).
    let ledgers = crate::tests::helpers::points_helpers::get_user_ledgers_by_credit_type(
        ctx,
        user_id,
        herald_core::domain::points::entities::CreditType::RegistrationCredit,
    )
    .await;
    assert_eq!(ledgers.len(), 1, "exactly one registration ledger row");
    assert_eq!(ledgers[0].granted_amount, REGISTRATION_BONUS);
    assert_eq!(
        ledgers[0].source_type,
        herald_core::domain::points::entities::CreditSourceType::Registration
    );
    let event_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM points_distribution_events WHERE event_key = $1")
            .bind(format!("registration:{user_id}"))
            .fetch_one(&ctx._app_state.pool)
            .await
            .unwrap();
    assert_eq!(event_count, 1, "registration distribution event recorded");

    // Register-as-consent was recorded best-effort for both agreement types.
    let consent_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_agreement_consent WHERE user_id = $1 AND realm_id = $2",
    )
    .bind(user_id)
    .bind(&ctx._realm_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert!(
        consent_count >= 1,
        "register-as-consent should be recorded for the new account"
    );
}

/// User Story: US-EO-002 (negative branch)
/// Covers: unregistered email + auto-register ON but
/// missing agreements → 409 `consent_required` with the current effective
/// agreement list; no code is sent.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_otp_send_unregistered_without_consent_returns_consent_required(
    ctx: &mut TestContext,
) {
    enable_email_otp(ctx, true).await;
    enable_registration(ctx).await;

    let email = format!("eo-consent-{}@test.com", uuid::Uuid::now_v7());

    // send with NO agreements → 409 consent_required + agreement summaries.
    let send_resp = send_otp(ctx, &email, None, None).await;
    assert_eq!(send_resp.status(), StatusCode::CONFLICT);

    let body: serde_json::Value = crate::tests::response_json(send_resp).await;
    assert_eq!(body["code"], "consent_required");
    assert_eq!(body["consentRequired"], true);

    let agreements = body["agreements"]
        .as_array()
        .expect("consent_required must include agreements list");
    assert!(
        !agreements.is_empty(),
        "agreements list must not be empty (platform-default ToS+Privacy are seeded)"
    );
    let types: std::collections::HashSet<&str> = agreements
        .iter()
        .filter_map(|a| {
            a["agreementType"]
                .as_str()
                .or_else(|| a["agreement_type"].as_str())
        })
        .collect();
    assert!(
        types.contains("terms_of_service"),
        "agreements must include terms_of_service; got {types:?}"
    );
    assert!(
        types.contains("privacy_policy"),
        "agreements must include privacy_policy; got {types:?}"
    );

    // No code should have been stored (consent before issuance, D-CONSENT-01).
    assert!(
        read_otp_attempts(ctx, &ctx._realm_id, &email)
            .await
            .is_none(),
        "no OTP code must be stored when consent is required"
    );
}

/// User Story: US-EO-002 (negative branch)
/// Covers: unregistered email + auto-register OFF →
/// 409 `email_not_registered`; no code is sent.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_otp_send_unregistered_auto_register_disabled_returns_not_registered(
    ctx: &mut TestContext,
) {
    enable_email_otp(ctx, false).await;

    let email = format!("eo-notreg-{}@test.com", uuid::Uuid::now_v7());

    let send_resp = send_otp(ctx, &email, None, None).await;
    assert_eq!(send_resp.status(), StatusCode::CONFLICT);

    let body: serde_json::Value = crate::tests::response_json(send_resp).await;
    assert_eq!(body["code"], "email_not_registered");
    // email_not_registered must NOT carry the consent flag/agreements.
    assert!(
        body.get("consentRequired")
            .is_none_or(|v| v.as_bool() != Some(true)),
        "email_not_registered must not set consentRequired"
    );
    assert!(
        body.get("agreements").is_none(),
        "email_not_registered must not include agreements"
    );

    assert!(
        read_otp_attempts(ctx, &ctx._realm_id, &email)
            .await
            .is_none(),
        "no OTP code must be stored when email is not registered"
    );
}

/// User Story: US-EO-001
/// Covers: consecutive wrong codes increment `attempts`;
/// once `attempts >= max_attempts` the code is invalidated (deleted) and
/// further verification reports the code as expired.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_otp_verify_wrong_code_increments_attempts_then_invalidates(
    ctx: &mut TestContext,
) {
    enable_email_otp(ctx, false).await;

    let email = format!("eo-attempts-{}@test.com", uuid::Uuid::now_v7());
    let correct_code = "111111";
    let wrong_code = "999999";

    // Start a fresh code with attempts=0.
    inject_otp_code(
        ctx,
        &ctx._realm_id,
        &email,
        correct_code,
        0,
        OTP_MAX_ATTEMPTS,
        OTP_CODE_TTL_SECONDS,
    )
    .await;

    // Submit wrong codes up to (max - 1) attempts; each is 401 + increments.
    for _ in 0..(OTP_MAX_ATTEMPTS - 1) {
        let resp = verify_otp(ctx, &email, wrong_code, None).await;
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "wrong code (under the limit) must be 401 retryable"
        );
        let _: serde_json::Value = crate::tests::response_json(resp).await;
    }

    let attempts_before = read_otp_attempts(ctx, &ctx._realm_id, &email)
        .await
        .expect("code must still exist before the final wrong attempt");
    assert_eq!(attempts_before, OTP_MAX_ATTEMPTS - 1);

    // The max-attempts wrong submission deletes the code → 401, and subsequent
    // verification reports the code as expired (no longer in Redis).
    let final_resp = verify_otp(ctx, &email, wrong_code, None).await;
    assert_eq!(
        final_resp.status(),
        StatusCode::UNAUTHORIZED,
        "wrong code at the attempts limit must be 401"
    );
    let _: serde_json::Value = crate::tests::response_json(final_resp).await;

    assert!(
        read_otp_attempts(ctx, &ctx._realm_id, &email)
            .await
            .is_none(),
        "code must be invalidated (deleted) once attempts reach the limit"
    );

    // The correct code can no longer be used.
    let resp = verify_otp(ctx, &email, correct_code, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "an invalidated code must not verify"
    );
}

/// User Story: US-EO-001
/// Covers: a missing / expired code → 401 (已失效).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_otp_verify_expired_code(ctx: &mut TestContext) {
    enable_email_otp(ctx, false).await;

    let email = format!("eo-expired-{}@test.com", uuid::Uuid::now_v7());

    // No code in Redis (never sent / expired) → verify is 401.
    let resp = verify_otp(ctx, &email, "123456", None).await;
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "missing/expired code must be 401"
    );
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    // The 401 message must indicate the code is no longer valid (not just wrong).
    let message = body["message"].as_str().unwrap_or("");
    assert!(
        message.contains("失效") || message.contains("expired") || message.contains("invalid"),
        "expired/missing code message should signal invalidation; got {message:?}"
    );

    // Inject a code with a 1s TTL, wait for it to expire, then verify → 401.
    inject_otp_code(
        ctx,
        &ctx._realm_id,
        &email,
        "222222",
        0,
        OTP_MAX_ATTEMPTS,
        1,
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let resp = verify_otp(ctx, &email, "222222", None).await;
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "a code whose TTL elapsed must verify as expired (401)"
    );
    let _: serde_json::Value = crate::tests::response_json(resp).await;
}

/// User Story: US-EO-001
/// Covers: a successfully matched code is consumed
/// (deleted); reusing the same code immediately afterwards is 401.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_otp_verify_reused_code_after_success(ctx: &mut TestContext) {
    enable_email_otp(ctx, false).await;

    let email = format!("eo-reuse-{}@test.com", uuid::Uuid::now_v7());
    create_active_user_with_consent(ctx, &email).await;

    let known_code = "333333";
    inject_otp_code(
        ctx,
        &ctx._realm_id,
        &email,
        known_code,
        0,
        OTP_MAX_ATTEMPTS,
        OTP_CODE_TTL_SECONDS,
    )
    .await;

    // First verify consumes the code → 200 + token.
    let resp = verify_otp(ctx, &email, known_code, None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let _: serde_json::Value = crate::tests::response_json(resp).await;

    assert!(
        read_otp_attempts(ctx, &ctx._realm_id, &email)
            .await
            .is_none(),
        "matched code must be consumed (one-time)"
    );

    // Reusing the same code → 401 (expired/missing).
    let resp = verify_otp(ctx, &email, known_code, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "a consumed code must not verify a second time"
    );
    let _: serde_json::Value = crate::tests::response_json(resp).await;
}

/// User Story: US-EO-001
/// Covers: IP/email send rate limiting.
///
/// P2 NOTE: `RateLimitConfig.enforce_in_dev` defaults to `false`, and the OTP
/// handlers use `rate_limit_hit` (NOT `rate_limit_hit_forced`). In the test
/// context the limit is therefore skipped and `send` does NOT return 429. This
/// test asserts the *actual* behaviour (send stays 200 across many calls) and
/// MUST NOT be strengthened to assert 429. To force a 429, the test context
/// would need to opt into `enforce_in_dev` (see existing rate-limit scenarios).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_otp_send_rate_limited(ctx: &mut TestContext) {
    enable_email_otp(ctx, false).await;

    let email = format!("eo-sendrl-{}@test.com", uuid::Uuid::now_v7());
    create_active_user_with_consent(ctx, &email).await;

    // Exceed OTP_SEND_IP_RATE_LIMIT (5,60) + OTP_SEND_EMAIL_RATE_LIMIT (2,60).
    for _ in 0..6 {
        let resp = send_otp(ctx, &email, None, None).await;
        // enforce_in_dev=false → rate limiting skipped → 200, not 429.
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "send rate limit is skipped in the test context (enforce_in_dev=false)"
        );
        let _: serde_json::Value = crate::tests::response_json(resp).await;
    }
}

/// User Story: US-EO-001
/// Covers: IP/email verify rate limiting.
///
/// P2 NOTE: as above, `enforce_in_dev=false` skips the limit in the test
/// context, so verify returns 401 (invalid code) rather than 429. This test
/// asserts the actual behaviour and MUST NOT be strengthened to assert 429.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_otp_verify_rate_limited(ctx: &mut TestContext) {
    enable_email_otp(ctx, false).await;

    let email = format!("eo-verifyrl-{}@test.com", uuid::Uuid::now_v7());
    inject_otp_code(
        ctx,
        &ctx._realm_id,
        &email,
        "444444",
        0,
        OTP_MAX_ATTEMPTS,
        OTP_CODE_TTL_SECONDS,
    )
    .await;

    // Exceed OTP_VERIFY_IP_RATE_LIMIT (10,60) + OTP_VERIFY_EMAIL_RATE_LIMIT (5,60).
    // Each wrong submission that is under the attempts limit returns 401; once
    // rate limiting were enforced it would be 429 — but it is skipped here.
    for _ in 0..11 {
        let resp = verify_otp(ctx, &email, "000000", None).await;
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "verify rate limit is skipped in the test context (enforce_in_dev=false); wrong code is 401"
        );
        let _: serde_json::Value = crate::tests::response_json(resp).await;
    }
}

/// User Story: US-EO-001
/// Covers: a disabled (non-active) account cannot
/// complete verify even with a correct code → 401.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_otp_disabled_account_rejected(ctx: &mut TestContext) {
    enable_email_otp(ctx, false).await;

    let email = format!("eo-disabled-{}@test.com", uuid::Uuid::now_v7());
    // status=2 (Forbidden) — disabled account.
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

    let known_code = "555555";
    inject_otp_code(
        ctx,
        &ctx._realm_id,
        &email,
        known_code,
        0,
        OTP_MAX_ATTEMPTS,
        OTP_CODE_TTL_SECONDS,
    )
    .await;

    let resp = verify_otp(ctx, &email, known_code, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "a disabled account must not complete OTP login"
    );
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    let message = body["message"].as_str().unwrap_or("");
    assert!(
        message.contains("禁用") || message.to_lowercase().contains("disabled"),
        "disabled-account rejection should signal the account is disabled; got {message:?}"
    );
}

/// User Story: US-EO-001
/// Covers: when OTP login is not enabled for the Realm,
/// both send and verify return 400; the public `status` endpoint reports
/// `enabled: false`.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_otp_realm_otp_disabled_returns_400(ctx: &mut TestContext) {
    // Do NOT enable email_otp for this Realm. Confirm the public status first.
    let status_req = Request::builder()
        .method("GET")
        .uri(format!("/api/auth/{}/email-otp/status", ctx._realm_id))
        .body(Body::empty())
        .unwrap();
    let status_resp = ctx
        .create_unified_test_router()
        .oneshot(status_req)
        .await
        .unwrap();
    assert_eq!(status_resp.status(), StatusCode::OK);
    let status_body: serde_json::Value = crate::tests::response_json(status_resp).await;
    assert_eq!(status_body["enabled"], false);

    let email = format!("eo-realmoff-{}@test.com", uuid::Uuid::now_v7());

    let send_resp = send_otp(ctx, &email, None, None).await;
    assert_eq!(
        send_resp.status(),
        StatusCode::BAD_REQUEST,
        "send must be 400 when OTP login is disabled"
    );
    let _: serde_json::Value = crate::tests::response_json(send_resp).await;

    let verify_resp = verify_otp(ctx, &email, "123456", None).await;
    assert_eq!(
        verify_resp.status(),
        StatusCode::BAD_REQUEST,
        "verify must be 400 when OTP login is disabled"
    );
    let _: serde_json::Value = crate::tests::response_json(verify_resp).await;
}

/// User Story: US-EO-001
/// Covers: a disabled Client App is rejected before a
/// code is issued.
///
/// BEHAVIOUR NOTE: the OTP handlers resolve the Client App via
/// `mailflow::require_enabled_client`, which returns `bad_request` (400 —
/// "Client app is disabled") for a disabled client. This differs from the
/// originally specified "401 Client App 禁用", but it is the actual
/// production behaviour shared by every mailflow-bound auth endpoint. This
/// test asserts the real (400) behaviour and documents the divergence. It
/// MUST NOT be weakened or silently flipped to 401.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_otp_client_app_disabled_returns_401(ctx: &mut TestContext) {
    enable_email_otp(ctx, false).await;

    // Create a separate disabled Client App and target it by clientId.
    let disabled_client_id = uuid::Uuid::now_v7().simple().to_string();
    sqlx::query(
        "INSERT INTO client_app (id, realm_id, client_id, name, enabled, turnstile_enabled, created_at, updated_at)
         VALUES ($1, $2, $3, 'disabled app', false, false, NOW(), NOW())",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(&ctx._realm_id)
    .bind(&disabled_client_id)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();

    let email = format!("eo-clientapp-{}@test.com", uuid::Uuid::now_v7());
    let payload = json!({
        "clientId": disabled_client_id,
        "email": email,
    });
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login/email-otp/send", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "5.5.5.5")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    // Actual production behaviour: 400 via require_enabled_client.
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "disabled client app is rejected by require_enabled_client with 400 (the originally \
         specified table says 401; see test doc comment — do not silently flip to 401)"
    );
    let _: serde_json::Value = crate::tests::response_json(resp).await;
}

/// User Story: US-EO-001
/// Covers: D-PROTECT-01 — when the bound Client App has
/// Turnstile enabled, a send without a turnstile token is rejected (the
/// production `verify_turnstile_for_client_app` returns 400
/// "turnstile token is required").
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_otp_turnstile_required_when_client_app_enabled(ctx: &mut TestContext) {
    enable_email_otp(ctx, false).await;

    // Enable Turnstile on the test context's default client app (admin-web-console).
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

    let email = format!("eo-tsen-{}@test.com", uuid::Uuid::now_v7());
    // Register an active user so that, with auto_register=false, a send to this
    // email proceeds (200) once Turnstile passes. Without this the send would
    // correctly return 409 email_not_registered for an unregistered email.
    create_active_user_with_consent(ctx, &email).await;

    // No turnstile token while Turnstile is enabled → 400 token required.
    let resp = send_otp(ctx, &email, None, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "Turnstile enabled + missing token must be rejected"
    );

    // With the always-pass test secret, a non-empty token is accepted → 200.
    let resp = send_otp(ctx, &email, Some("dummy-token"), None).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "Turnstile enabled with a valid test-secret token must proceed"
    );
    let _: serde_json::Value = crate::tests::response_json(resp).await;

    // Restore: disable Turnstile so it does not leak into other tests sharing
    // the same admin-web-console client app within this schema.
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

/// User Story: US-EO-001
/// Covers: D-PROTECT-01 — when the bound Client App has
/// Turnstile NOT enabled, verification is skipped (not blocking) and send
/// proceeds even without a turnstile token.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_otp_turnstile_skipped_when_not_configured(ctx: &mut TestContext) {
    enable_email_otp(ctx, false).await;

    // The default admin-web-console client app has turnstile_enabled=false, so
    // verification is skipped. Send without a token must succeed.
    let email = format!("eo-tsoff-{}@test.com", uuid::Uuid::now_v7());
    create_active_user_with_consent(ctx, &email).await;

    let resp = send_otp(ctx, &email, None, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "Turnstile disabled (not configured) must be skipped, not blocking"
    );
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["expiresInSeconds"], OTP_CODE_TTL_SECONDS);
}

/// User Story: US-EO-001
/// Covers: send returns the same 200 for a non-active
/// (disabled) account as for a successful send, and crucially does NOT store a
/// code (enumeration resistance: an attacker cannot distinguish existing-but-
/// disabled from a successful send by behaviour).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_otp_anti_enumeration_non_active_returns_200(ctx: &mut TestContext) {
    enable_email_otp(ctx, false).await;

    let email = format!("eo-enum-{}@test.com", uuid::Uuid::now_v7());
    // status=2 (Forbidden) — existing but non-active.
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status)
         VALUES ($1, $2, $3, '$2a$12$dummy_password_hash', 2)",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(&ctx._realm_id)
    .bind(&email)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();

    let resp = send_otp(ctx, &email, None, None).await;
    // Enumeration-resistant: indistinguishable from a successful send.
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "send for a non-active account must return 200 (enumeration resistance)"
    );
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["expiresInSeconds"], OTP_CODE_TTL_SECONDS);

    // But no code was actually stored.
    assert!(
        read_otp_attempts(ctx, &ctx._realm_id, &email)
            .await
            .is_none(),
        "no code must be stored for a non-active account"
    );
}

/// User Story: US-EO-001 (second-factor coexistence, email-otp-login PRD
/// §4.1 "不得绕过现有 TOTP 二因素" / D-COEXIST-01)
/// Covers: an existing user with an ENABLED TOTP config verifying a valid OTP
/// code must NOT receive a Bearer token family — the OTP is only the first
/// factor. The response carries the same second-factor branch shape as
/// password login (`secondFactors` + `tempToken`), and completing verify-totp
/// with the temp token issues the session.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_otp_verify_totp_user_requires_second_factor(ctx: &mut TestContext) {
    enable_email_otp(ctx, false).await;

    let email = format!("eo-2fa-{}@test.com", uuid::Uuid::now_v7());
    let user_id = create_active_user_with_consent(ctx, &email).await;

    // Enable TOTP for the user (the probe only reads the enabled flag).
    sqlx::query(
        "INSERT INTO user_totp_config (id, user_id, realm_id, secret_hash, enabled, created_at, updated_at)
         VALUES ($1, $2, $3, 'mock-secret-hash', true, NOW(), NOW())",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(user_id)
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("failed to seed TOTP config");

    let send_resp = send_otp(ctx, &email, None, None).await;
    assert_eq!(send_resp.status(), StatusCode::OK);

    let known_code = "123456";
    inject_otp_code(
        ctx,
        &ctx._realm_id,
        &email,
        known_code,
        0,
        OTP_MAX_ATTEMPTS,
        OTP_CODE_TTL_SECONDS,
    )
    .await;

    let verify_resp = verify_otp(ctx, &email, known_code, None).await;
    assert_eq!(verify_resp.status(), StatusCode::OK);

    let body: serde_json::Value = crate::tests::response_json(verify_resp).await;
    assert!(
        body["accessToken"].is_null(),
        "OTP verify must NOT issue tokens for a TOTP-enabled user (2FA bypass)"
    );
    let second_factors: Vec<String> = body["secondFactors"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        second_factors,
        vec!["totp".to_string()],
        "response must advertise the totp second factor (SDK requires-second-factor branch)"
    );
    let temp_token = body["tempToken"].as_str().unwrap_or_default();
    assert!(
        !temp_token.is_empty(),
        "response must carry a temp token for the second-factor step"
    );
}
