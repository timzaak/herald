// Scenario tests: Realm phone-login switch configuration endpoints.
// Coverage: US-PL-004 — fail-closed enable gate, unrestricted disable
// (smooth degradation), country-code validation, audit trail, deletion
// guard, cross-realm rejection, permission enforcement.

use crate::tests::helpers::auth_helpers::{create_admin_session_with_user, grant_realm_admin_role};
use crate::tests::helpers::otp_helpers::{seed_phone_login_config, seed_sms_channel_aliyun};
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;

fn put_config_request(realm_id: &str, token: &str, enabled: bool, cc: &str) -> Request<Body> {
    Request::builder()
        .method("PUT")
        .uri(format!("/api/realms/{realm_id}/config/phone-login"))
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(
            json!({"enabled": enabled, "defaultCountryCode": cc}).to_string(),
        ))
        .unwrap()
}

fn get_config_request(realm_id: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(format!("/api/realms/{realm_id}/config/phone-login"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

fn delete_sms_config_request(_realm_id: &str, token: &str, key: &str) -> Request<Body> {
    Request::builder()
        .method("DELETE")
        .uri(format!("/api/configs/sms/{key}"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

/// US-PL-004 (scenario 3): fail-closed — enabling without an SMS channel is a
/// 400 and writes no switch row.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_enable_phone_login_without_sms_config_rejected(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "pl004-no-channel@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    let resp = app
        .clone()
        .oneshot(put_config_request(&ctx._realm_id, &token, true, "CN"))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "enabling without an SMS channel must be rejected"
    );
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|m| m.contains("without SMS configuration")),
        "the rejection must name the fail-closed reason: {body}"
    );

    let resp = app
        .clone()
        .oneshot(get_config_request(&ctx._realm_id, &token))
        .await
        .unwrap();
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["enabled"], false);
    assert_eq!(body["smsConfigured"], false);
}

/// US-PL-004 (scenarios 4 + 5): enable requires a channel; disable is
/// unrestricted (smooth degradation); both directions are audited.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_phone_login_enable_after_channel_then_disable_round_trip(
    ctx: &mut TestContext,
) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "pl004-roundtrip@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    // The channel only needs to parse — a dummy address is fine.
    seed_sms_channel_aliyun(&ctx._app_state.pool, &ctx._realm_id, "http://127.0.0.1:9").await;

    let resp = app
        .clone()
        .oneshot(put_config_request(&ctx._realm_id, &token, true, "CN"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["enabled"], true);
    assert_eq!(body["defaultCountryCode"], "CN");
    assert_eq!(body["smsConfigured"], true);

    let resp = app
        .clone()
        .oneshot(get_config_request(&ctx._realm_id, &token))
        .await
        .unwrap();
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["enabled"], true);
    assert_eq!(body["defaultCountryCode"], "CN");

    let resp = app
        .clone()
        .oneshot(put_config_request(&ctx._realm_id, &token, false, "CN"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["enabled"], false);

    // Auth-policy changes are critical-config changes: both directions must
    // be audited.
    let enabled_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE realm_id = $1 AND action = 'phone_login_config.update'
           AND details->>'new_enabled' = 'true'",
    )
    .bind(&ctx._realm_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(enabled_count, 1, "the enable must be audited");
    let disabled_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE realm_id = $1 AND action = 'phone_login_config.update'
           AND details->>'new_enabled' = 'false'",
    )
    .bind(&ctx._realm_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(disabled_count, 1, "the disable must be audited");
}

#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_phone_login_invalid_country_code_rejected(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) = create_admin_session_with_user(ctx, "pl004-cc@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;
    seed_sms_channel_aliyun(&ctx._app_state.pool, &ctx._realm_id, "http://127.0.0.1:9").await;

    for bad_cc in ["cn", "CHN", "1A", ""] {
        let resp = app
            .clone()
            .oneshot(put_config_request(&ctx._realm_id, &token, false, bad_cc))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "country code {bad_cc:?} must be rejected"
        );
    }

    let resp = app
        .clone()
        .oneshot(put_config_request(&ctx._realm_id, &token, false, "US"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["defaultCountryCode"], "US");
}

/// No dangling enabled switch: `sms` rows cannot be deleted while the switch
/// is enabled.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_sms_channel_row_deletion_guarded_while_enabled(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) = create_admin_session_with_user(ctx, "pl004-guard@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    seed_sms_channel_aliyun(&ctx._app_state.pool, &ctx._realm_id, "http://127.0.0.1:9").await;
    seed_phone_login_config(&ctx._app_state.pool, &ctx._realm_id, true).await;

    let resp = app
        .clone()
        .oneshot(delete_sms_config_request(
            &ctx._realm_id,
            &token,
            "aliyun_sign_name",
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "deleting the channel under a live switch must be rejected"
    );
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|m| m.contains("phone login is enabled")),
        "the rejection must name the guard: {body}"
    );

    let resp = app
        .clone()
        .oneshot(put_config_request(&ctx._realm_id, &token, false, "CN"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = app
        .oneshot(delete_sms_config_request(
            &ctx._realm_id,
            &token,
            "aliyun_sign_name",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

/// US-PL-004 (scenario 6): cross-realm access is rejected for PUT and GET.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_phone_login_config_cross_realm_rejected(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) = create_admin_session_with_user(ctx, "pl004-cross@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    let other_realm_id = uuid::Uuid::now_v7().to_string();

    let resp = app
        .clone()
        .oneshot(put_config_request(&other_realm_id, &token, false, "CN"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let resp = app
        .oneshot(get_config_request(&other_realm_id, &token))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// PUT needs `settings.manage`, GET needs `settings.view`; the default test
/// user has neither.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_phone_login_config_permission_enforced(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, _user_id) =
        create_admin_session_with_user(ctx, "pl004-noperm@test.com", 1800).await;

    let resp = app
        .clone()
        .oneshot(put_config_request(&ctx._realm_id, &token, false, "CN"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let resp = app
        .oneshot(get_config_request(&ctx._realm_id, &token))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}
