// =============================================================================
// Scenario tests: Realm admin Email-OTP configuration endpoints
// =============================================================================
//
// Admin-only configuration for the per-Realm Email-OTP login + auto-register
// switches. Mirrors the TOTP/Passkey
// config scenario shape and the template `realm_totp_config_scenarios.rs`:
//   PUT  /api/realms/{realmId}/config/email-otp   (settings.manage)
//   GET  /api/realms/{realmId}/config/email-otp   (settings.view)
//
// Coverage: US-EO-003 (admin enable/disable + read-back), cross-realm write
// rejection, the `autoRegister` toggle round-trip, and permission enforcement
// for both settings.manage (PUT) and settings.view (GET).
//
// =============================================================================

use crate::tests::helpers::*;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;

/// Build a PUT config request with the given admin bearer token.
fn put_config_request(
    realm_id: &str,
    token: &str,
    enabled: bool,
    auto_register: bool,
) -> Request<Body> {
    Request::builder()
        .method("PUT")
        .uri(format!("/api/realms/{realm_id}/config/email-otp"))
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(
            json!({
                "enabled": enabled,
                "autoRegister": auto_register
            })
            .to_string(),
        ))
        .unwrap()
}

/// Build a GET config request with the given admin bearer token.
fn get_config_request(realm_id: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(format!("/api/realms/{realm_id}/config/email-otp"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

// =============================================================================
// Scenarios
// =============================================================================

/// User Story: US-EO-003
/// Covers: admin enables then disables Email OTP
/// and reads each state back consistently via GET.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_admin_enable_disable_email_otp(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "email-otp-config-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    // Enable.
    let resp = app
        .clone()
        .oneshot(put_config_request(&ctx._realm_id, &token, true, false))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["enabled"], true);
    assert_eq!(body["autoRegister"], false);
    assert_eq!(body["message"], "Realm Email OTP configuration updated");
    assert!(
        body["updatedAt"].as_str().is_some(),
        "PUT response must include updatedAt"
    );

    // GET reads it back enabled.
    let resp = app
        .clone()
        .oneshot(get_config_request(&ctx._realm_id, &token))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["enabled"], true);
    assert_eq!(body["autoRegister"], false);

    // Disable.
    let resp = app
        .clone()
        .oneshot(put_config_request(&ctx._realm_id, &token, false, false))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["enabled"], false);

    // GET reads it back disabled.
    let resp = app
        .clone()
        .oneshot(get_config_request(&ctx._realm_id, &token))
        .await
        .unwrap();
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["enabled"], false);
    assert_eq!(body["autoRegister"], false);

    // Auth-policy changes are "关键配置变更" (audit PRD): each PUT (enable +
    // disable) must leave an audit trail naming the new values.
    let enabled_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE realm_id = $1 AND action = 'email_otp_config.update'
           AND details->>'enabled' = 'true' AND details->>'auto_register' = 'false'",
    )
    .bind(&ctx._realm_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(
        enabled_count, 1,
        "Email OTP enable must be audited with the new policy values"
    );
    let disabled_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE realm_id = $1 AND action = 'email_otp_config.update'
           AND details->>'enabled' = 'false'",
    )
    .bind(&ctx._realm_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(
        disabled_count, 1,
        "Email OTP disable must be audited with the new policy values"
    );
}

/// User Story: US-EO-003
/// Covers: 权限边界 — a realm admin cannot write/read
/// another realm's Email-OTP config (cross-realm access is rejected).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_admin_email_otp_config_cross_realm_rejected(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "email-otp-cross-realm@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    let other_realm_id = uuid::Uuid::now_v7().to_string();

    // PUT to a different realm → rejected.
    let resp = app
        .clone()
        .oneshot(put_config_request(&other_realm_id, &token, true, false))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "PUT config/email-otp for a different realm should be 403"
    );
    let _: serde_json::Value = crate::tests::response_json(resp).await;

    // GET from a different realm → rejected.
    let resp = app
        .oneshot(get_config_request(&other_realm_id, &token))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "GET config/email-otp for a different realm should be 403"
    );
    let _: serde_json::Value = crate::tests::response_json(resp).await;
}

/// User Story: US-EO-003
/// Covers: the `autoRegister` flag round-trips through
/// PUT → GET independently of `enabled`.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_admin_email_otp_auto_register_toggle(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "email-otp-autoreg@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    // Enable OTP with auto-register ON.
    let resp = app
        .clone()
        .oneshot(put_config_request(&ctx._realm_id, &token, true, true))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["enabled"], true);
    assert_eq!(body["autoRegister"], true);

    // GET reads auto-register ON.
    let resp = app
        .clone()
        .oneshot(get_config_request(&ctx._realm_id, &token))
        .await
        .unwrap();
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["enabled"], true);
    assert_eq!(body["autoRegister"], true);

    // Flip auto-register OFF while keeping OTP enabled.
    let resp = app
        .clone()
        .oneshot(put_config_request(&ctx._realm_id, &token, true, false))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app
        .oneshot(get_config_request(&ctx._realm_id, &token))
        .await
        .unwrap();
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["enabled"], true);
    assert_eq!(body["autoRegister"], false);
}

/// User Story: US-EO-003
/// Covers: 权限边界 — without `settings.manage` a PUT is
/// rejected (403); without `settings.view` a GET is rejected (403). The
/// default test user (no realm-admin role) has neither permission.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_admin_email_otp_config_permission_enforced(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    // Plain user with NO realm-admin role → no settings.manage / settings.view.
    let (token, _user_id) =
        create_admin_session_with_user(ctx, "email-otp-plain-user@test.com", 1800).await;

    // PUT without settings.manage → 403.
    let resp = app
        .clone()
        .oneshot(put_config_request(&ctx._realm_id, &token, true, false))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "PUT config/email-otp without settings.manage should be 403"
    );
    let _: serde_json::Value = crate::tests::response_json(resp).await;

    // GET without settings.view → 403.
    let resp = app
        .oneshot(get_config_request(&ctx._realm_id, &token))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "GET config/email-otp without settings.view should be 403"
    );
    let _: serde_json::Value = crate::tests::response_json(resp).await;
}
