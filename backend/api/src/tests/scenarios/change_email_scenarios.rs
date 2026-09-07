use crate::tests::helpers::auth_helpers::obtain_reauth_token;
use crate::tests::helpers::test_setup_helpers::*;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;

#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_change_email_flow(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();

    // Step 1: Create and login user
    let email = "changeme@cas.com";
    let password = "password123";
    let (user_id, _token) = create_user_and_login(ctx, email, password).await;

    // 双轨凭证类：密码登录签 CustomUserUi family（不含 ChangeEmail scope），
    // 改邮箱需要 FirstParty 会话（生产路径为直登/换客户端入口）。
    let token = crate::tests::helpers::auth_helpers::mint_first_party_session(ctx, user_id).await;

    // Step 2: Request email change (requires a fresh reauth ticket)
    let reauth_token = obtain_reauth_token(ctx, &token, "change_email", password).await;
    let request_payload = json!({
        "newEmail": "newemail@cas.com",
        "reauthToken": reauth_token
    });

    let request_req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/change_email/request", realm_id))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", token))
        .body(Body::from(request_payload.to_string()))
        .unwrap();

    let request_resp = app.clone().oneshot(request_req).await.unwrap();
    assert_eq!(request_resp.status(), 200, "Request should return 200 OK");

    let request_body = axum::body::to_bytes(request_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let request_json: serde_json::Value = serde_json::from_slice(&request_body).unwrap();
    // New response structure: just check response is valid JSON
    assert!(request_json.is_object(), "Response should be a JSON object");

    // Step 3: Confirm email change (would require verification code from DB)
    // For now, just verify the request endpoint works
}

#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_change_email_confirm_requires_same_authenticated_user(
    ctx: &mut TestContext,
) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();

    let (user_a_id, _pw_token_a) =
        create_user_and_login(ctx, "change-owner@cas.com", "password123").await;
    let (_user_b_id, _pw_token_b) =
        create_user_and_login(ctx, "change-attacker@cas.com", "password123").await;

    // 双轨凭证类：改邮箱的 request/confirm 都要求 FirstParty 会话；
    // 密码登录的 CustomUserUi family 不含 ChangeEmail scope。
    let token_a =
        crate::tests::helpers::auth_helpers::mint_first_party_session(ctx, user_a_id).await;
    // 攻击者也持 FirstParty 会话：断言的 403 必须来自 confirm 的
    // same-authenticated-user 校验，而不是 token scope 拒绝。
    let token_b =
        crate::tests::helpers::auth_helpers::mint_first_party_session(ctx, _user_b_id).await;

    let reauth_token = obtain_reauth_token(ctx, &token_a, "change_email", "password123").await;
    let request_payload = json!({
        "newEmail": "owner-new@cas.com",
        "reauthToken": reauth_token
    });

    let request_req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/change_email/request", realm_id))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", token_a))
        .body(Body::from(request_payload.to_string()))
        .unwrap();

    let request_resp = app.clone().oneshot(request_req).await.unwrap();
    assert_eq!(request_resp.status(), StatusCode::OK);

    let change_code: String = sqlx::query_scalar(
        "SELECT verification_code FROM email_verification_code
         WHERE email = $1 AND type = 'change_email'
         ORDER BY id DESC LIMIT 1",
    )
    .bind("owner-new@cas.com")
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();

    let attacker_confirm_req = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/auth/{}/change_email/confirm/{}",
            realm_id, change_code
        ))
        .header("authorization", format!("Bearer {}", token_b))
        .body(Body::empty())
        .unwrap();

    let attacker_confirm_resp = app.clone().oneshot(attacker_confirm_req).await.unwrap();
    assert_eq!(attacker_confirm_resp.status(), StatusCode::FORBIDDEN);

    let owner_confirm_req = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/auth/{}/change_email/confirm/{}",
            realm_id, change_code
        ))
        .header("authorization", format!("Bearer {}", token_a))
        .body(Body::empty())
        .unwrap();

    let owner_confirm_resp = app.clone().oneshot(owner_confirm_req).await.unwrap();
    assert_eq!(owner_confirm_resp.status(), StatusCode::OK);

    let updated_email: String = sqlx::query_scalar("SELECT email FROM account WHERE id = $1")
        .bind(user_a_id)
        .fetch_one(&ctx._app_state.pool)
        .await
        .unwrap();
    assert_eq!(updated_email, "owner-new@cas.com");
}
