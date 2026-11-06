// Scenario tests: SMS OTP login (send + verify + status).
// Coverage: US-PL-002 — bound-user login success, no-auto-register 409,
// anti-enumeration, writing-form normalization, code lifecycle (wrong-code
// attempts / one-time consumption), disabled-account and consent gates.
//
// Harness notes: the SMS channel fails loud when unconfigured, so send
// scenarios assert real wiremock dispatch. `RateLimitConfig.enforce_in_dev`
// defaults to false, so rate-limit branches assert their non-429 behaviour
// and MUST NOT be strengthened to assert 429.

use crate::tests::helpers::otp_helpers::{
    SMS_OTP_CODE_TTL_SECONDS, SMS_OTP_MAX_ATTEMPTS, enable_phone_login, inject_sms_otp_code,
    read_sms_otp_attempts, read_sms_otp_code, seed_phone_login_config, seed_sms_channel_aliyun,
    start_aliyun_wiremock, unique_cn_phone,
};
use crate::tests::helpers::test_setup_helpers::record_test_user_consent;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;

async fn send_otp(ctx: &TestContext, phone: &str) -> axum::response::Response {
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login/sms-otp/send", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "5.5.5.5")
        .body(Body::from(
            json!({"clientId": ctx._client_id, "phone": phone}).to_string(),
        ))
        .unwrap();
    ctx.create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap()
}

async fn verify_otp(ctx: &TestContext, phone: &str, code: &str) -> axum::response::Response {
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login/sms-otp/verify", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "5.5.5.5")
        .body(Body::from(
            json!({"clientId": ctx._client_id, "phone": phone, "code": code}).to_string(),
        ))
        .unwrap();
    ctx.create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap()
}

async fn phone_login_status(ctx: &TestContext) -> axum::response::Response {
    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/auth/{}/phone-login/status", ctx._realm_id))
        .body(Body::empty())
        .unwrap();
    ctx.create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap()
}

/// Recorded consent keeps the login-as-consent gate off the happy path.
async fn create_active_phone_user(
    ctx: &TestContext,
    email: &str,
    e164_phone: &str,
    with_consent: bool,
) -> uuid::Uuid {
    let user_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, phone, status)
         VALUES ($1, $2, $3, '$2a$12$dummy_password_hash', $4, 1)",
    )
    .bind(user_id)
    .bind(&ctx._realm_id)
    .bind(email)
    .bind(e164_phone)
    .execute(&ctx._app_state.pool)
    .await
    .expect("failed to create active phone test user");
    if with_consent {
        record_test_user_consent(&ctx._app_state.pool, user_id, &ctx._realm_id).await;
    }
    user_id
}

/// US-PL-002: known code → verify issues a Bearer session (custom-UI
/// direction).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_sms_otp_send_then_verify_login_success(ctx: &mut TestContext) {
    let wiremock = enable_phone_login(ctx).await;

    let e164 = format!("+86{}", unique_cn_phone("138"));
    create_active_phone_user(ctx, "pl002-a@test.com", &e164, true).await;

    let resp = send_otp(ctx, &e164).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["message"], "ok");
    assert_eq!(body["expiresInSeconds"], SMS_OTP_CODE_TTL_SECONDS);
    assert!(
        body["maskedPhone"]
            .as_str()
            .is_some_and(|m| m.ends_with("****") || m.contains("****")),
        "masked phone must be backend-derived: got {:?}",
        body["maskedPhone"]
    );
    assert_eq!(
        wiremock.received_requests().await.unwrap().len(),
        1,
        "the send must go through the configured channel"
    );

    let known_code = "123456";
    inject_sms_otp_code(
        ctx,
        &ctx._realm_id,
        &e164,
        known_code,
        0,
        SMS_OTP_MAX_ATTEMPTS,
        SMS_OTP_CODE_TTL_SECONDS,
    )
    .await;

    let verify_resp = verify_otp(ctx, &e164, known_code).await;
    assert_eq!(verify_resp.status(), StatusCode::OK);
    let (verify_resp, token) = crate::tests::extract_bearer_token(verify_resp).await;
    assert!(token.is_some(), "verify must issue a Bearer accessToken");
    let verify_body: serde_json::Value = crate::tests::response_json(verify_resp).await;
    assert_eq!(verify_body["tokenType"], "Bearer");
    assert!(verify_body["refreshToken"].as_str().is_some());
}

/// US-PL-002 (scenario 4): unregistered phone → 409 `phone_not_registered`
/// with no agreements field (no auto-register), no code stored, no dispatch.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_sms_otp_send_unregistered_rejected_without_account_creation(
    ctx: &mut TestContext,
) {
    let wiremock = enable_phone_login(ctx).await;

    let unknown = format!("+86{}", unique_cn_phone("137"));
    let resp = send_otp(ctx, &unknown).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["code"], "phone_not_registered");
    assert!(
        body.get("agreements").is_none() || body["agreements"].is_null(),
        "the SMS 409 must NOT carry an agreements payload (no auto-register)"
    );

    let created: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM account WHERE realm_id = $1 AND phone = $2")
            .bind(&ctx._realm_id)
            .bind(&unknown)
            .fetch_one(&ctx._app_state.pool)
            .await
            .unwrap();
    assert_eq!(created, 0, "unregistered phone must not create an account");
    assert!(
        read_sms_otp_code(ctx, &ctx._realm_id, &unknown)
            .await
            .is_none(),
        "no code may be stored for an unregistered phone"
    );
    assert_eq!(
        wiremock.received_requests().await.unwrap().len(),
        0,
        "no SMS may be dispatched for an unregistered phone"
    );
}

/// US-PL-002 (scenario 5): existing-but-forbidden account → byte-identical
/// 200 with no code stored and no dispatch (anti-enumeration).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_sms_otp_send_disabled_account_fake_success_no_code(ctx: &mut TestContext) {
    let wiremock = enable_phone_login(ctx).await;

    let e164 = format!("+86{}", unique_cn_phone("138"));
    let user_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, phone, status)
         VALUES ($1, $2, 'pl002-disabled@test.com', '$2a$12$dummy_password_hash', $3, 2)",
    )
    .bind(user_id)
    .bind(&ctx._realm_id)
    .bind(&e164)
    .execute(&ctx._app_state.pool)
    .await
    .expect("failed to create forbidden test user");

    let resp = send_otp(ctx, &e164).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "existing-but-disabled must look identical to a real send"
    );
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["message"], "ok");
    assert!(
        read_sms_otp_code(ctx, &ctx._realm_id, &e164)
            .await
            .is_none(),
        "no code may be stored for a non-active account"
    );
    assert_eq!(
        wiremock.received_requests().await.unwrap().len(),
        0,
        "no SMS may be dispatched for a non-active account"
    );
}

/// Every writing form of one number normalizes to the same account and the
/// same Redis code key.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_sms_otp_writing_forms_normalize_to_same_account(ctx: &mut TestContext) {
    enable_phone_login(ctx).await;

    let digits = unique_cn_phone("138");
    let e164 = format!("+86{digits}");
    create_active_phone_user(ctx, "pl002-forms@test.com", &e164, true).await;

    let dashed = format!("+86 {}-{}", &digits[..3], &digits[3..]);
    for form in [
        digits.as_str(),
        &format!("+86{digits}"),
        &format!("0086 {digits}"),
        dashed.as_str(),
    ] {
        let resp = send_otp(ctx, form).await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "writing form {form:?} must resolve to the bound account"
        );
        assert!(
            read_sms_otp_code(ctx, &ctx._realm_id, &e164)
                .await
                .is_some(),
            "form {form:?} must store its code under the normalized key"
        );
    }
}

#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_sms_otp_switch_off_rejects_and_status_is_false(ctx: &mut TestContext) {
    let wiremock = start_aliyun_wiremock().await;
    seed_sms_channel_aliyun(&ctx._app_state.pool, &ctx._realm_id, &wiremock.uri()).await;

    let status_resp = phone_login_status(ctx).await;
    assert_eq!(status_resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(status_resp).await;
    assert_eq!(
        body["enabled"], false,
        "absent config must read as disabled"
    );

    let resp = send_otp(ctx, "+8613800138000").await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        wiremock.received_requests().await.unwrap().len(),
        0,
        "switch off must not dispatch"
    );
}

/// All invalid phone inputs collapse to one uniform 400 (anti-probing).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_sms_otp_send_invalid_phone_uniform_400(ctx: &mut TestContext) {
    enable_phone_login(ctx).await;

    for bad in [
        "12345678901",
        "not-a-phone",
        "1380013800000000000000000000000000",
    ] {
        let resp = send_otp(ctx, bad).await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "invalid input {bad:?} must be a uniform 400"
        );
    }
}

/// Defensive: an enabled switch with the channel deleted is still an
/// explicit 400 at send.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_sms_otp_dangling_channel_state_rejected(ctx: &mut TestContext) {
    seed_phone_login_config(&ctx._app_state.pool, &ctx._realm_id, true).await;

    // The not-registered 409 fires before the channel gate, so the
    // dangling-channel 400 is only observable for a registered active phone.
    let e164 = format!("+86{}", unique_cn_phone("138"));
    create_active_phone_user(ctx, "pl002-dangling@test.com", &e164, true).await;

    let resp = send_otp(ctx, &e164).await;
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "enabled switch with missing channel must be an explicit 400"
    );
}

/// US-PL-002 (scenario 2): exhausting the wrong-code budget invalidates the
/// code — a later correct code is rejected.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_sms_otp_verify_wrong_code_attempts_then_exhaustion(ctx: &mut TestContext) {
    enable_phone_login(ctx).await;

    let e164 = format!("+86{}", unique_cn_phone("138"));
    create_active_phone_user(ctx, "pl002-attempts@test.com", &e164, true).await;

    inject_sms_otp_code(
        ctx,
        &ctx._realm_id,
        &e164,
        "999999",
        0,
        SMS_OTP_MAX_ATTEMPTS,
        SMS_OTP_CODE_TTL_SECONDS,
    )
    .await;

    // max_attempts - 1 wrong submissions: still 400, counter climbing.
    for i in 1..SMS_OTP_MAX_ATTEMPTS {
        let resp = verify_otp(ctx, &e164, "000000").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            read_sms_otp_attempts(ctx, &ctx._realm_id, &e164).await,
            Some(i),
            "attempt counter must climb one per wrong submission"
        );
    }

    let resp = verify_otp(ctx, &e164, "000000").await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert!(
        read_sms_otp_code(ctx, &ctx._realm_id, &e164)
            .await
            .is_none(),
        "exhaustion must delete the code"
    );

    let resp = verify_otp(ctx, &e164, "999999").await;
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "an exhausted code must not verify even with the right digits"
    );
}

/// US-PL-002 (scenario 6): one-time consumption — a used code cannot be
/// replayed.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_sms_otp_verify_one_time_consumption_no_replay(ctx: &mut TestContext) {
    enable_phone_login(ctx).await;

    let e164 = format!("+86{}", unique_cn_phone("138"));
    create_active_phone_user(ctx, "pl002-replay@test.com", &e164, true).await;

    inject_sms_otp_code(
        ctx,
        &ctx._realm_id,
        &e164,
        "111222",
        0,
        SMS_OTP_MAX_ATTEMPTS,
        SMS_OTP_CODE_TTL_SECONDS,
    )
    .await;

    let first = verify_otp(ctx, &e164, "111222").await;
    assert_eq!(first.status(), StatusCode::OK);

    let replay = verify_otp(ctx, &e164, "111222").await;
    assert_eq!(
        replay.status(),
        StatusCode::BAD_REQUEST,
        "a consumed code must not verify twice"
    );
}

/// A disabled account is rejected at verify even with a correct code (403,
/// distinct from the wrong-code 400).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_sms_otp_verify_disabled_account_rejected(ctx: &mut TestContext) {
    enable_phone_login(ctx).await;

    let e164 = format!("+86{}", unique_cn_phone("138"));
    let user_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, phone, status)
         VALUES ($1, $2, 'pl002-verify-disabled@test.com', '$2a$12$dummy_password_hash', $3, 2)",
    )
    .bind(user_id)
    .bind(&ctx._realm_id)
    .bind(&e164)
    .execute(&ctx._app_state.pool)
    .await
    .expect("failed to create forbidden test user");

    // The send side refuses to store codes for non-active accounts; inject
    // one directly to prove the verify-side status gate holds regardless.
    inject_sms_otp_code(
        ctx,
        &ctx._realm_id,
        &e164,
        "333444",
        0,
        SMS_OTP_MAX_ATTEMPTS,
        SMS_OTP_CODE_TTL_SECONDS,
    )
    .await;

    let resp = verify_otp(ctx, &e164, "333444").await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// Stale consent gates the token — the SMS entrance reuses the shared login
/// completion chain.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_sms_otp_verify_consent_gate_intercepts_token(ctx: &mut TestContext) {
    enable_phone_login(ctx).await;

    let e164 = format!("+86{}", unique_cn_phone("138"));
    create_active_phone_user(ctx, "pl002-consent@test.com", &e164, false).await;

    inject_sms_otp_code(
        ctx,
        &ctx._realm_id,
        &e164,
        "555666",
        0,
        SMS_OTP_MAX_ATTEMPTS,
        SMS_OTP_CODE_TTL_SECONDS,
    )
    .await;

    let resp = verify_otp(ctx, &e164, "555666").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(
        body["consentRequired"], true,
        "stale consent must gate the token"
    );
    assert!(
        body["accessToken"].is_null() || body.get("accessToken").is_none(),
        "no access token may be issued on the consent branch"
    );
    assert!(
        body["restrictedSession"].is_object(),
        "the gate must mint a restricted session"
    );
}
