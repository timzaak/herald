// Scenario tests: self-service phone bind/rebind/unbind + admin disposition.
// Coverage: US-PL-001 (bind/rebind/unbind with reauth, conflicts, verify-
// before-write contract), US-PL-005 (admin masked display, search, audited
// unbind/rebind).
// `PhoneManage` is a first-party-only scope: scenarios mint first-party
// sessions as the production path does.

use crate::tests::helpers::auth_helpers::{
    create_admin_session_with_user, grant_realm_admin_role, mint_first_party_session,
    obtain_reauth_token,
};
use crate::tests::helpers::otp_helpers::{
    SMS_OTP_CODE_TTL_SECONDS, SMS_OTP_MAX_ATTEMPTS, enable_phone_login, inject_sms_bind_code,
    read_sms_admin_rebind, seed_phone_login_config, seed_sms_channel_aliyun, start_aliyun_wiremock,
    unique_cn_phone,
};
use crate::tests::helpers::test_setup_helpers::{create_user_and_login, record_test_user_consent};
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
    response::Response,
};
use herald_core::domain::user::ports::UserRepository;
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;

async fn phone_status(ctx: &TestContext, token: &str) -> Response {
    ctx.create_unified_test_router()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/user/phone")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn bind_send(ctx: &TestContext, token: &str, phone: &str, reauth: &str) -> Response {
    ctx.create_unified_test_router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/user/phone/bind/send")
                .header("content-type", "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(
                    json!({"phone": phone, "reauthToken": reauth}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn bind_confirm(ctx: &TestContext, token: &str, phone: &str, code: &str) -> Response {
    ctx.create_unified_test_router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/user/phone/bind/confirm")
                .header("content-type", "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(
                    json!({"phone": phone, "code": code}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn unbind(ctx: &TestContext, token: &str, reauth: &str) -> Response {
    ctx.create_unified_test_router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/user/phone/unbind")
                .header("content-type", "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(json!({"reauthToken": reauth}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn admin_delete_phone(ctx: &TestContext, token: &str, user_id: uuid::Uuid) -> Response {
    ctx.create_unified_test_router()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/users/{user_id}/phone"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn admin_rebind_send(
    ctx: &TestContext,
    token: &str,
    user_id: uuid::Uuid,
    phone: &str,
) -> Response {
    ctx.create_unified_test_router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/users/{user_id}/phone/rebind/send"))
                .header("content-type", "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(json!({"phone": phone}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn admin_rebind_confirm(
    ctx: &TestContext,
    token: &str,
    user_id: uuid::Uuid,
    code: &str,
) -> Response {
    ctx.create_unified_test_router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/users/{user_id}/phone/rebind/confirm"))
                .header("content-type", "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(json!({"code": code}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn admin_get_user(ctx: &TestContext, token: &str, user_id: uuid::Uuid) -> Response {
    ctx.create_unified_test_router()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/users/{user_id}"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn account_phone(pool: &sqlx::PgPool, realm_id: &str, user_id: uuid::Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT phone FROM account WHERE realm_id = $1 AND id = $2")
        .bind(realm_id)
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// US-PL-001 (scenario 1): bind send → code proof → confirm; the DB stays
/// untouched until confirm.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_phone_bind_send_then_confirm_success(ctx: &mut TestContext) {
    let wiremock = enable_phone_login(ctx).await;

    let (user_id, _custom_token) =
        create_user_and_login(ctx, "pl001-bind@test.com", "password123").await;
    record_test_user_consent(&ctx._app_state.pool, user_id, &ctx._realm_id).await;
    let token = mint_first_party_session(ctx, user_id).await;

    let resp = phone_status(ctx, &token).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["bound"], false);
    assert_eq!(body["phoneLoginEnabled"], true);
    assert_eq!(body["smsConfigured"], true);

    let reauth = obtain_reauth_token(ctx, &token, "change_phone", "password123").await;
    let e164 = format!("+86{}", unique_cn_phone("138"));
    let resp = bind_send(ctx, &token, &e164, &reauth).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["expiresInSeconds"], SMS_OTP_CODE_TTL_SECONDS);
    assert!(body["maskedPhone"].as_str().is_some());
    assert_eq!(
        wiremock.received_requests().await.unwrap().len(),
        1,
        "the bind code must dispatch through the channel"
    );

    assert_eq!(
        account_phone(&ctx._app_state.pool, &ctx._realm_id, user_id).await,
        None
    );

    let known_code = "123456";
    inject_sms_bind_code(
        ctx,
        &ctx._realm_id,
        user_id,
        &e164,
        known_code,
        0,
        SMS_OTP_MAX_ATTEMPTS,
        SMS_OTP_CODE_TTL_SECONDS,
    )
    .await;
    let resp = bind_confirm(ctx, &token, &e164, known_code).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert!(body["maskedPhone"].as_str().is_some());

    assert_eq!(
        account_phone(&ctx._app_state.pool, &ctx._realm_id, user_id)
            .await
            .as_deref(),
        Some(e164.as_str()),
        "confirm must persist the normalized E.164 phone"
    );

    let resp = phone_status(ctx, &token).await;
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["bound"], true);
    assert!(
        body["maskedPhone"]
            .as_str()
            .is_some_and(|m| m.contains("****")),
        "status must show the masked number"
    );

    // Audit trail (masked only).
    let audited: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE realm_id = $1 AND action = 'phone.bind' AND target_id = $2",
    )
    .bind(&ctx._realm_id)
    .bind(user_id.to_string())
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(audited, 1, "the bind must be audited");
}

/// US-PL-001 (scenario 4): no reauth ticket → 401; a consumed ticket is not
/// replayable → 409.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_phone_bind_send_reauth_required_and_single_use(ctx: &mut TestContext) {
    enable_phone_login(ctx).await;

    let (user_id, _) = create_user_and_login(ctx, "pl001-reauth@test.com", "password123").await;
    let token = mint_first_party_session(ctx, user_id).await;

    let resp = bind_send(ctx, &token, "+8613800138000", "not-a-real-ticket").await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let reauth = obtain_reauth_token(ctx, &token, "change_phone", "password123").await;
    let e164 = format!("+86{}", unique_cn_phone("138"));
    let resp = bind_send(ctx, &token, &e164, &reauth).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = bind_send(ctx, &token, &e164, &reauth).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

/// US-PL-001 (scenario 4): an unconfigured-channel 400 must not burn the
/// single-use reauth ticket.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_phone_bind_send_unconfigured_channel_400_preserves_ticket(
    ctx: &mut TestContext,
) {
    seed_phone_login_config(&ctx._app_state.pool, &ctx._realm_id, true).await;

    let (user_id, _) = create_user_and_login(ctx, "pl001-nochannel@test.com", "password123").await;
    let token = mint_first_party_session(ctx, user_id).await;
    let reauth = obtain_reauth_token(ctx, &token, "change_phone", "password123").await;

    let resp = bind_send(ctx, &token, "+8613800138000", &reauth).await;
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "unconfigured channel must be an explicit rejection"
    );

    let wiremock = start_aliyun_wiremock().await;
    seed_sms_channel_aliyun(&ctx._app_state.pool, &ctx._realm_id, &wiremock.uri()).await;
    let e164 = format!("+86{}", unique_cn_phone("138"));
    let resp = bind_send(ctx, &token, &e164, &reauth).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "the reauth ticket must not be burned by a 400"
    );
}

/// US-PL-001 (scenario 2): uniqueness compares NORMALIZED numbers, so any
/// writing form of a bound number conflicts.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_phone_bind_conflict_with_other_account_any_writing_form(
    ctx: &mut TestContext,
) {
    enable_phone_login(ctx).await;

    let digits = "13877001122";
    let (other_id, _) = create_user_and_login(ctx, "pl001-other@test.com", "password123").await;
    sqlx::query("UPDATE account SET phone = $1 WHERE id = $2")
        .bind(format!("+86{digits}"))
        .bind(other_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    let (user_id, _) = create_user_and_login(ctx, "pl001-mine@test.com", "password123").await;
    let token = mint_first_party_session(ctx, user_id).await;
    let reauth = obtain_reauth_token(ctx, &token, "change_phone", "password123").await;

    for form in [digits, &format!("+86{digits}"), &format!("0086{digits}")] {
        let resp = bind_send(ctx, &token, form, &reauth).await;
        assert_eq!(
            resp.status(),
            StatusCode::CONFLICT,
            "writing form {form:?} of a bound number must conflict"
        );
        let body: serde_json::Value = crate::tests::response_json(resp).await;
        assert_eq!(body["code"], "phone_already_in_use");
    }

    assert_eq!(
        account_phone(&ctx._app_state.pool, &ctx._realm_id, user_id).await,
        None
    );
}

/// US-PL-001 (scenario 5): the old binding survives until confirm swaps it.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_phone_rebind_old_value_unchanged_until_confirm(ctx: &mut TestContext) {
    enable_phone_login(ctx).await;

    let (user_id, _) = create_user_and_login(ctx, "pl001-rebind@test.com", "password123").await;
    let old_e164 = format!("+86{}", unique_cn_phone("138"));
    sqlx::query("UPDATE account SET phone = $1 WHERE id = $2")
        .bind(&old_e164)
        .bind(user_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    let token = mint_first_party_session(ctx, user_id).await;
    let reauth = obtain_reauth_token(ctx, &token, "change_phone", "password123").await;

    let new_e164 = format!("+86{}", unique_cn_phone("138"));
    let resp = bind_send(ctx, &token, &new_e164, &reauth).await;
    assert_eq!(resp.status(), StatusCode::OK);

    assert_eq!(
        account_phone(&ctx._app_state.pool, &ctx._realm_id, user_id)
            .await
            .as_deref(),
        Some(old_e164.as_str()),
        "the old binding must survive until the code verifies"
    );

    let known_code = "654321";
    inject_sms_bind_code(
        ctx,
        &ctx._realm_id,
        user_id,
        &new_e164,
        known_code,
        0,
        SMS_OTP_MAX_ATTEMPTS,
        SMS_OTP_CODE_TTL_SECONDS,
    )
    .await;
    let resp = bind_confirm(ctx, &token, &new_e164, known_code).await;
    assert_eq!(resp.status(), StatusCode::OK);

    assert_eq!(
        account_phone(&ctx._app_state.pool, &ctx._realm_id, user_id)
            .await
            .as_deref(),
        Some(new_e164.as_str()),
        "confirm must swap the binding"
    );
}

/// US-PL-001 (scenario 6): unbind clears the phone; email + password must
/// keep working afterwards.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_phone_unbind_then_email_password_login_still_works(ctx: &mut TestContext) {
    enable_phone_login(ctx).await;

    let email = format!("pl001-unbind-{}@test.com", uuid::Uuid::now_v7().simple());
    let (user_id, _) = create_user_and_login(ctx, &email, "password123").await;
    let e164 = format!("+86{}", unique_cn_phone("138"));
    sqlx::query("UPDATE account SET phone = $1 WHERE id = $2")
        .bind(&e164)
        .bind(user_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    let token = mint_first_party_session(ctx, user_id).await;
    let reauth = obtain_reauth_token(ctx, &token, "remove_phone", "password123").await;

    let resp = unbind(ctx, &token, &reauth).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        account_phone(&ctx._app_state.pool, &ctx._realm_id, user_id).await,
        None
    );

    let audited: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE realm_id = $1 AND action = 'phone.unbind' AND target_id = $2",
    )
    .bind(&ctx._realm_id)
    .bind(user_id.to_string())
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(audited, 1);

    // Smooth degradation: the account keeps a working login path.
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", ctx._realm_id))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"clientId": ctx._client_id, "email": email, "password": "password123"})
                .to_string(),
        ))
        .unwrap();
    let resp = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

/// US-PL-005 (scenario 1): admin sees the masked number and can search by
/// the full one.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_admin_masked_display_and_phone_search(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (admin_token, admin_id) =
        create_admin_session_with_user(ctx, "pl005-admin-view@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_id).await;

    let (user_id, _) =
        create_user_and_login(ctx, "pl005-target-view@test.com", "password123").await;
    let e164 = format!("+86{}", unique_cn_phone("138"));
    sqlx::query("UPDATE account SET phone = $1 WHERE id = $2")
        .bind(&e164)
        .bind(user_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/users/{user_id}"))
                .header(header::AUTHORIZATION, format!("Bearer {admin_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    let masked = body["phoneMasked"]
        .as_str()
        .expect("phoneMasked must be present");
    assert!(
        masked.contains("****"),
        "masked form expected, got {masked}"
    );
    assert!(!masked.contains(&e164), "the raw number must never appear");

    let bare = &e164[3..];
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/users?phone={bare}"))
                .header(header::AUTHORIZATION, format!("Bearer {admin_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    let items = body["items"].as_array().expect("paged items");
    assert_eq!(
        items.len(),
        1,
        "the phone search must locate exactly the bound account"
    );
    assert_eq!(items[0]["id"].as_str().unwrap(), user_id.to_string());
    assert!(
        items[0]["phoneMasked"]
            .as_str()
            .is_some_and(|m| m.contains("****"))
    );
}

/// US-PL-005 (scenario 2): admin unbind is audited with the masked old value.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_admin_unbind_audited(ctx: &mut TestContext) {
    let (admin_token, admin_id) =
        create_admin_session_with_user(ctx, "pl005-admin-unbind@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_id).await;

    let (user_id, _) =
        create_user_and_login(ctx, "pl005-target-unbind@test.com", "password123").await;
    let e164 = format!("+86{}", unique_cn_phone("138"));
    sqlx::query("UPDATE account SET phone = $1 WHERE id = $2")
        .bind(&e164)
        .bind(user_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    let resp = admin_delete_phone(ctx, &admin_token, user_id).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        account_phone(&ctx._app_state.pool, &ctx._realm_id, user_id).await,
        None
    );

    let audited: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE realm_id = $1 AND action = 'phone.admin_unbind' AND target_id = $2
           AND details->>'masked_phone' LIKE '%****%'",
    )
    .bind(&ctx._realm_id)
    .bind(user_id.to_string())
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(
        audited, 1,
        "admin unbind must be audited with the masked old number"
    );
}

/// US-PL-005 (scenarios 3 + 4): admin rebind — pending record until confirm;
/// already-bound numbers conflict (409).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_admin_rebind_pending_then_confirm(ctx: &mut TestContext) {
    let wiremock = enable_phone_login(ctx).await;

    let (admin_token, admin_id) =
        create_admin_session_with_user(ctx, "pl005-admin-rebind@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_id).await;

    let (user_id, _) =
        create_user_and_login(ctx, "pl005-target-rebind@test.com", "password123").await;
    let old_e164 = format!("+86{}", unique_cn_phone("138"));
    sqlx::query("UPDATE account SET phone = $1 WHERE id = $2")
        .bind(&old_e164)
        .bind(user_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    let new_e164 = format!("+86{}", unique_cn_phone("138"));

    // Rebinding to the CURRENT binding is a conflict (no-op rebind).
    let resp = admin_rebind_send(ctx, &admin_token, user_id, &old_e164).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    let resp = admin_rebind_send(ctx, &admin_token, user_id, &new_e164).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert!(
        body["maskedNewPhone"]
            .as_str()
            .is_some_and(|m| m.contains("****"))
    );
    assert_eq!(wiremock.received_requests().await.unwrap().len(), 1);

    let pending = read_sms_admin_rebind(ctx, &ctx._realm_id, user_id).await;
    assert!(pending.is_some(), "the pending record must be registered");
    assert_eq!(
        account_phone(&ctx._app_state.pool, &ctx._realm_id, user_id)
            .await
            .as_deref(),
        Some(old_e164.as_str()),
        "the old binding must survive while the rebind is pending"
    );

    let resp = admin_get_user(ctx, &admin_token, user_id).await;
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert!(
        body["pendingPhoneMasked"]
            .as_str()
            .is_some_and(|m| m.contains("****")),
        "the detail view must surface the pending rebind"
    );

    let resp = admin_rebind_confirm(ctx, &admin_token, user_id, "000000").await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // The pending record carries a production-generated random code; read it
    // back from Redis instead of injecting.
    let pending_json = read_sms_admin_rebind(ctx, &ctx._realm_id, user_id)
        .await
        .expect("pending record must survive the wrong-code attempt");
    let code = serde_json::from_str::<serde_json::Value>(&pending_json).unwrap()["code"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = admin_rebind_confirm(ctx, &admin_token, user_id, &code).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert!(
        body["maskedPhone"]
            .as_str()
            .is_some_and(|m| m.contains("****"))
    );

    assert_eq!(
        account_phone(&ctx._app_state.pool, &ctx._realm_id, user_id)
            .await
            .as_deref(),
        Some(new_e164.as_str())
    );
    assert_eq!(
        read_sms_admin_rebind(ctx, &ctx._realm_id, user_id).await,
        None
    );

    let audited: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE realm_id = $1 AND action = 'phone.admin_rebind' AND target_id = $2",
    )
    .bind(&ctx._realm_id)
    .bind(user_id.to_string())
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(audited, 1);
}

#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_admin_rebind_confirm_without_pending_400(ctx: &mut TestContext) {
    let (admin_token, admin_id) =
        create_admin_session_with_user(ctx, "pl005-admin-nopend@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_id).await;

    let (user_id, _) =
        create_user_and_login(ctx, "pl005-target-nopend@test.com", "password123").await;

    let resp = admin_rebind_confirm(ctx, &admin_token, user_id, "123456").await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// Anonymization must clear the phone: the login entrance treats the number
/// as unregistered again and never dispatches SMS to it.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_anonymization_clears_phone_and_login_reports_unregistered(
    ctx: &mut TestContext,
) {
    let wiremock = enable_phone_login(ctx).await;

    let e164 = format!("+86{}", unique_cn_phone("138"));
    let (user_id, _) = create_user_and_login(ctx, "pl-anonymize@test.com", "password123").await;
    sqlx::query("UPDATE account SET phone = $1 WHERE id = $2")
        .bind(&e164)
        .bind(user_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    ctx._app_state
        .user_repository
        .anonymize_user_for_deletion(&ctx._realm_id, user_id)
        .await
        .expect("anonymization failed");

    assert_eq!(
        account_phone(&ctx._app_state.pool, &ctx._realm_id, user_id).await,
        None,
        "anonymization must clear the phone"
    );

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login/sms-otp/send", ctx._realm_id))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"clientId": ctx._client_id, "phone": e164}).to_string(),
        ))
        .unwrap();
    let resp = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["code"], "phone_not_registered");
    assert_eq!(
        wiremock.received_requests().await.unwrap().len(),
        0,
        "anonymized numbers must not receive SMS"
    );
}
