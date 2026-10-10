use crate::tests::helpers::auth_helpers::obtain_reauth_token;
use crate::tests::helpers::email_config_helpers::{
    delete_email_config_direct, insert_unreachable_smtp_email_config_direct,
};
use crate::tests::helpers::test_setup_helpers::*;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;

async fn change_email_code_count(pool: &sqlx::PgPool, realm_id: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM email_verification_code WHERE realm_id = $1 AND type = 'change_email'",
    )
    .bind(realm_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_change_email_request_rejected_without_email_config(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();

    // Step 1: Create and login user
    let email = "changeme@cas.com";
    let password = "password123";
    let (user_id, _token) = create_user_and_login(ctx, email, password).await;

    // 双轨凭证类：密码登录签 CustomUserUi family（不含 ChangeEmail scope），
    // 改邮箱需要 FirstParty 会话（生产路径为直登/换客户端入口）。
    let token = crate::tests::helpers::auth_helpers::mint_first_party_session(ctx, user_id).await;

    // 未配置邮件通道：发起必须被明确拒绝，而非静默跳过发送返回假成功
    // 让用户死等一封永不到达的确认邮件。
    delete_email_config_direct(&ctx._app_state.pool, &realm_id).await;

    // Step 2: Request email change (requires a fresh reauth ticket)
    let reauth_token = obtain_reauth_token(ctx, &token, "change_email", password).await;
    let request_payload = json!({
        "newEmail": "newemail@cas.com",
        "reauthToken": reauth_token
    });

    let build_request = || {
        Request::builder()
            .method("POST")
            .uri(format!("/api/auth/{}/change_email/request", realm_id))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", token))
            .body(Body::from(request_payload.to_string()))
            .unwrap()
    };

    let request_resp = app.clone().oneshot(build_request()).await.unwrap();
    assert_eq!(
        request_resp.status(),
        StatusCode::BAD_REQUEST,
        "request must be rejected when the realm has no email channel"
    );
    let body: serde_json::Value = crate::tests::response_json(request_resp).await;
    assert!(
        body["message"]
            .as_str()
            .unwrap_or("")
            .contains("Email is not configured for this realm"),
        "error message should mention the missing email channel, got: {:?}",
        body
    );

    assert_eq!(
        change_email_code_count(&ctx._app_state.pool, &realm_id).await,
        0,
        "no change_email code may be issued on rejection"
    );

    // 票据未烧探针：门控必须在消耗一次性 reauth 票据之前拒绝。若 400 已烧票，
    // 同一票据的重发会是 401（Invalid or expired reauthentication token）而到不了
    // 发送步骤。配置通道后重发：不可达的本地 SMTP（127.0.0.1:9 拒绝连接）让发送
    // 快速、离线、确定性地失败 → 如实 500（wire 上 internal 错误统一脱敏为
    // "Internal server error"，具体原因只在日志），且码已先行落库。
    insert_unreachable_smtp_email_config_direct(&ctx._app_state.pool, &realm_id).await;
    let probe_resp = app.clone().oneshot(build_request()).await.unwrap();
    assert_eq!(
        probe_resp.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "probe must reach the send step: 401 would mean the gated 400 burned the \
         reauth ticket, 409 a ticket-consumption conflict; the unreachable local \
         SMTP relay makes the send itself fail honestly and offline"
    );

    assert_eq!(
        change_email_code_count(&ctx._app_state.pool, &realm_id).await,
        1,
        "the code must be persisted before the send attempt"
    );

    delete_email_config_direct(&ctx._app_state.pool, &realm_id).await;
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

    // 码必须来自真实的 request 路径而非硬编码格式播种：request 走到发送步骤即
    // 500（不可达 SMTP，见上一场景），但码已先行落库——从库里取出真实生成的码，
    // 使 ChangeEmailCode::generate → parse 的耦合始终被本场景执行。取码后删除
    // 邮件配置，同时钉住"无通道时旧邮箱通知静默跳过、不回滚、不 5xx"的尽力
    // 送达语义。
    insert_unreachable_smtp_email_config_direct(&ctx._app_state.pool, &realm_id).await;
    let reauth_token = obtain_reauth_token(ctx, &token_a, "change_email", "password123").await;
    let request_req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/change_email/request", realm_id))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", token_a))
        .body(Body::from(
            json!({ "newEmail": "owner-new@cas.com", "reauthToken": reauth_token }).to_string(),
        ))
        .unwrap();
    let request_resp = app.clone().oneshot(request_req).await.unwrap();
    assert_eq!(
        request_resp.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "the unreachable relay makes the send fail after the code is persisted"
    );

    let change_code: String = sqlx::query_scalar(
        "SELECT verification_code FROM email_verification_code
         WHERE realm_id = $1 AND email = $2 AND type = 'change_email'
         ORDER BY id DESC LIMIT 1",
    )
    .bind(&realm_id)
    .bind("owner-new@cas.com")
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert!(
        change_code.contains(&user_a_id.to_string()),
        "the persisted code must embed the requesting user (got: {change_code})"
    );

    delete_email_config_direct(&ctx._app_state.pool, &realm_id).await;

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
