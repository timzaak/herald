// Scenario tests: password login with the phone identifier (three-way split).
// Coverage: US-PL-003 — phone + password success, numeric-username protection,
// switch-off regression.

use crate::tests::helpers::otp_helpers::{
    seed_phone_login_config, seed_sms_channel_aliyun, unique_cn_phone,
};
use crate::tests::helpers::test_setup_helpers::create_user_and_login;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;

async fn password_login(
    ctx: &TestContext,
    identifier: &str,
    password: &str,
) -> axum::response::Response {
    ctx.create_unified_test_router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/auth/{}/login", ctx._realm_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "clientId": ctx._client_id,
                        "username": identifier,
                        "password": password,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap()
}

/// Channel rows only need to exist for the switch; password login never
/// dispatches SMS (dummy base_url is fine).
async fn enable_phone_login(ctx: &TestContext) {
    seed_sms_channel_aliyun(&ctx._app_state.pool, &ctx._realm_id, "http://127.0.0.1:9").await;
    seed_phone_login_config(&ctx._app_state.pool, &ctx._realm_id, true).await;
}

/// US-PL-003 (scenario 1): the bare 11-digit form and the +86 form must
/// resolve to the SAME account.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_password_login_with_phone_identifier_success_all_forms(
    ctx: &mut TestContext,
) {
    enable_phone_login(ctx).await;

    let digits = unique_cn_phone("138");
    let email = format!("pl003-{}@test.com", uuid::Uuid::now_v7().simple());
    let (user_id, _) = create_user_and_login(ctx, &email, "password123").await;
    sqlx::query("UPDATE account SET phone = $1 WHERE id = $2")
        .bind(format!("+86{digits}"))
        .bind(user_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    for form in [digits.as_str(), &format!("+86{digits}")] {
        let resp = password_login(ctx, form, "password123").await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "phone form {form:?} + password must log in"
        );
        // Plain success returns the BrowserTokenResponse shape (userId only
        // appears on the second-factor / consent branches).
        let (resp, token) = crate::tests::extract_bearer_token(resp).await;
        assert!(
            token.is_some(),
            "phone form {form:?} must issue a Bearer session"
        );
        let _: serde_json::Value = crate::tests::response_json(resp).await;
    }

    // Wrong password keeps the EXISTING error shape (US-PL-003 scenario 2).
    let resp = password_login(ctx, &digits, "wrong-password").await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// US-PL-003 (scenario 3): unknown phone reuses the unknown-identifier error
/// (no account-existence oracle); pure-numeric usernames still match.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_password_login_unknown_phone_and_numeric_username_fallback(
    ctx: &mut TestContext,
) {
    enable_phone_login(ctx).await;

    let resp = password_login(ctx, "13899999999", "password123").await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|m| m.eq_ignore_ascii_case("invalid credentials")),
        "unknown phone must reuse the existing unknown-identifier error: {body}"
    );

    // Phone parse fails → the raw username falls through to exact match.
    let email = format!("pl003-num-{}@test.com", uuid::Uuid::now_v7().simple());
    let (user_id, _) = create_user_and_login(ctx, &email, "password123").await;
    sqlx::query("UPDATE account SET username = '7304115' WHERE id = $1")
        .bind(user_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    let resp = password_login(ctx, "7304115", "password123").await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "pure-numeric usernames must survive the phone split"
    );
    let (resp, token) = crate::tests::extract_bearer_token(resp).await;
    assert!(
        token.is_some(),
        "the numeric-username login must issue a session"
    );
    let _: serde_json::Value = crate::tests::response_json(resp).await;
}

/// US-PL-002 (scenario 7): switch OFF — phone-shaped input stays on the
/// legacy username path.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_switch_off_password_login_behaviour_unchanged(ctx: &mut TestContext) {
    let digits = unique_cn_phone("138");
    let email = format!("pl003-off-{}@test.com", uuid::Uuid::now_v7().simple());
    let (user_id, _) = create_user_and_login(ctx, &email, "password123").await;
    sqlx::query("UPDATE account SET phone = $1 WHERE id = $2")
        .bind(format!("+86{digits}"))
        .bind(user_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    let resp = password_login(ctx, &digits, "password123").await;
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "switch off must leave phone input on the legacy username path"
    );

    let resp = ctx
        .create_unified_test_router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/auth/{}/login", ctx._realm_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "clientId": ctx._client_id,
                        "email": email,
                        "password": "password123",
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
