use crate::tests::helpers::auth_helpers::obtain_reauth_token;
use crate::tests::helpers::email_config_helpers::{
    delete_email_config_direct, insert_unreachable_smtp_email_config_direct,
};
use crate::tests::helpers::test_setup_helpers::*;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;

// User Story: US-RU-016（docs/user-stories/core/regular-user.md）
//
// Covers:
// - 场景 2：重复发起以最新确认链接为准（newest-wins）
// - 场景 3：确认链接无效或过期（TTL 拒绝；"已被更新链接取代"半边由场景 2 覆盖）
// - 场景 4：新邮箱与同 Realm 既有邮箱冲突（409）
// 场景 7（无邮件通道拒绝发起）与 confirm 的 same-user 校验见上方既有场景。

async fn change_email_code_count(pool: &sqlx::PgPool, realm_id: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM email_verification_code WHERE realm_id = $1 AND type = 'change_email'",
    )
    .bind(realm_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn account_email(pool: &sqlx::PgPool, user_id: uuid::Uuid) -> String {
    sqlx::query_scalar("SELECT email FROM account WHERE id = $1")
        .bind(user_id)
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

    let request_resp =
        post_change_email_request(&app, &realm_id, &token, "newemail@cas.com", &reauth_token).await;
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
    let probe_resp =
        post_change_email_request(&app, &realm_id, &token, "newemail@cas.com", &reauth_token).await;
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
    let request_resp = post_change_email_request(
        &app,
        &realm_id,
        &token_a,
        "owner-new@cas.com",
        &reauth_token,
    )
    .await;
    assert_eq!(
        request_resp.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "the unreachable relay makes the send fail after the code is persisted"
    );

    let change_code =
        latest_change_email_code(&ctx._app_state.pool, &realm_id, "owner-new@cas.com").await;
    assert!(
        change_code.contains(&user_a_id.to_string()),
        "the persisted code must embed the requesting user (got: {change_code})"
    );

    delete_email_config_direct(&ctx._app_state.pool, &realm_id).await;

    let attacker_confirm_resp =
        get_change_email_confirm(&app, &realm_id, &token_b, &change_code).await;
    assert_eq!(attacker_confirm_resp.status(), StatusCode::FORBIDDEN);

    let owner_confirm_resp =
        get_change_email_confirm(&app, &realm_id, &token_a, &change_code).await;
    assert_eq!(owner_confirm_resp.status(), StatusCode::OK);

    assert_eq!(
        account_email(&ctx._app_state.pool, user_a_id).await,
        "owner-new@cas.com"
    );
}

async fn post_change_email_request(
    app: &axum::Router,
    realm_id: &str,
    token: &str,
    new_email: &str,
    reauth_token: &str,
) -> Response {
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/change_email/request", realm_id))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", token))
        .body(Body::from(
            json!({ "newEmail": new_email, "reauthToken": reauth_token }).to_string(),
        ))
        .unwrap();
    app.clone().oneshot(req).await.unwrap()
}

async fn get_change_email_confirm(
    app: &axum::Router,
    realm_id: &str,
    token: &str,
    code: &str,
) -> Response {
    let req = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/auth/{}/change_email/confirm/{}",
            realm_id, code
        ))
        .header("authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    app.clone().oneshot(req).await.unwrap()
}

/// Read the latest change_email code row for (realm, new_email) straight from
/// the persisted row — exactly what the emailed confirmation link carries.
async fn latest_change_email_code(pool: &sqlx::PgPool, realm_id: &str, email: &str) -> String {
    sqlx::query_scalar(
        "SELECT verification_code FROM email_verification_code
         WHERE realm_id = $1 AND email = $2 AND type = 'change_email'
         ORDER BY id DESC LIMIT 1",
    )
    .bind(realm_id)
    .bind(email)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Issue a change-email request through the failing relay: a fresh reauth
/// ticket, a request that must reach the send step (500, code persisted),
/// then the latest persisted code for (realm, new_email). Every user in this
/// file is created with the fixture password "password123".
async fn issue_code_via_failing_send(
    ctx: &TestContext,
    app: &axum::Router,
    realm_id: &str,
    token: &str,
    new_email: &str,
) -> String {
    let reauth_token = obtain_reauth_token(ctx, token, "change_email", "password123").await;
    let resp = post_change_email_request(app, realm_id, token, new_email, &reauth_token).await;
    assert_eq!(
        resp.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "the unreachable relay makes the send fail after the code is persisted"
    );
    latest_change_email_code(&ctx._app_state.pool, realm_id, new_email).await
}

// =============================================================================
// US-RU-016 场景 2 —— 重复发起以最新确认链接为准（newest-wins）
// =============================================================================
//
// 重发同一新邮箱的变更请求后，此前未使用的旧确认链接必须失效。WHY：旧链接
// 若在完整 TTL 内继续可用，任何提前读到旧确认邮件的人（错发、转发、回收
// 邮箱复用）都能完成一次不属于最新请求的邮箱接管；"重发即作废"把可用窗口
// 收敛到最新一封邮件。实现侧是 request 的 delete-then-insert
// （api-auth/src/change_email.rs request_email_change_internal）。
#[test_context(TestContext)]
#[tokio::test]
async fn test_us_ru_016_scenario_2_resend_invalidates_previous_change_code(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();

    let (user_id, _pw_token) =
        create_user_and_login(ctx, "newest-owner@cas.com", "password123").await;
    let token = crate::tests::helpers::auth_helpers::mint_first_party_session(ctx, user_id).await;

    // 不可达 SMTP（127.0.0.1:9）：request 走到发送步骤即 500，但码已先行
    // 落库——码的签发/作废语义不依赖邮件真的发出去（与上方场景同一手法）。
    insert_unreachable_smtp_email_config_direct(&ctx._app_state.pool, &realm_id).await;

    let new_email = "newest-new@cas.com";

    let first_code = issue_code_via_failing_send(ctx, &app, &realm_id, &token, new_email).await;
    let second_code = issue_code_via_failing_send(ctx, &app, &realm_id, &token, new_email).await;
    assert_eq!(
        change_email_code_count(&ctx._app_state.pool, &realm_id).await,
        1,
        "resend must delete the previous code row (newest-wins)"
    );
    assert_ne!(
        first_code, second_code,
        "the surviving row must be the newly issued code"
    );

    delete_email_config_direct(&ctx._app_state.pool, &realm_id).await;

    let old_link_resp = get_change_email_confirm(&app, &realm_id, &token, &first_code).await;
    assert_eq!(
        old_link_resp.status(),
        StatusCode::BAD_REQUEST,
        "the previous confirmation link must be dead after a resend"
    );
    let body: serde_json::Value = crate::tests::response_json(old_link_resp).await;
    assert_eq!(body["message"], "change code not found");
    assert_eq!(
        account_email(&ctx._app_state.pool, user_id).await,
        "newest-owner@cas.com",
        "a dead link must not touch the account email"
    );

    let latest_link_resp = get_change_email_confirm(&app, &realm_id, &token, &second_code).await;
    assert_eq!(
        latest_link_resp.status(),
        StatusCode::OK,
        "the newest confirmation link must complete the change"
    );
    assert_eq!(
        account_email(&ctx._app_state.pool, user_id).await,
        new_email
    );
}

// =============================================================================
// US-RU-016 场景 3 —— 确认链接过期（TTL 1800s）拒绝
// =============================================================================
//
// 场景 3 的"已被更新链接取代"半边由场景 2 的旧码 400 覆盖；本场景钉住
// "已过有效期"半边： mailed link 不得永久有效。把码行 created_at 回拨到
// TTL（EMAIL_VERIFICATION_CODE_TTL_SECONDS=1800）之外后提交正确码 → 400
// "change code not found"，邮箱保持不变，且行不被删除（TTL 拒绝不是消费，
// 行仍在——区别于消费路径的 DELETE）。时间回拨直接 UPDATE 自有 fixture
// 行（确定性）。
#[test_context(TestContext)]
#[tokio::test]
async fn test_us_ru_016_scenario_3_expired_change_code_rejected(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();

    let (user_id, _pw_token) = create_user_and_login(ctx, "ttl-owner@cas.com", "password123").await;
    let token = crate::tests::helpers::auth_helpers::mint_first_party_session(ctx, user_id).await;

    insert_unreachable_smtp_email_config_direct(&ctx._app_state.pool, &realm_id).await;

    let new_email = "ttl-new@cas.com";
    let code = issue_code_via_failing_send(ctx, &app, &realm_id, &token, new_email).await;

    delete_email_config_direct(&ctx._app_state.pool, &realm_id).await;

    // 1801s = EMAIL_VERIFICATION_CODE_TTL_SECONDS (1800) + 1: the row falls
    // outside the confirm lookup's created_at cutoff.
    sqlx::query(
        "UPDATE email_verification_code SET created_at = NOW() - INTERVAL '1801 seconds'
         WHERE realm_id = $1 AND email = $2 AND type = 'change_email'",
    )
    .bind(&realm_id)
    .bind(new_email)
    .execute(&ctx._app_state.pool)
    .await
    .expect("failed to age the change-email code row");

    let confirm_resp = get_change_email_confirm(&app, &realm_id, &token, &code).await;
    assert_eq!(
        confirm_resp.status(),
        StatusCode::BAD_REQUEST,
        "an expired confirmation link must not complete the change"
    );
    let body: serde_json::Value = crate::tests::response_json(confirm_resp).await;
    assert_eq!(body["message"], "change code not found");

    assert_eq!(
        account_email(&ctx._app_state.pool, user_id).await,
        "ttl-owner@cas.com"
    );

    assert_eq!(
        change_email_code_count(&ctx._app_state.pool, &realm_id).await,
        1,
        "TTL rejection must not consume (delete) the code row"
    );
}

// =============================================================================
// US-RU-016 场景 4 —— 新邮箱与同 Realm 既有邮箱冲突（409）
// =============================================================================
//
// 同 Realm 已有其他账户占用目标邮箱时，确认步骤必须以 409 拒绝且两个账户
// 的邮箱都保持不变。WHY：confirm 的 UPDATE account 撞上
// account(realm_id, email) 唯一索引（23505）——放任其穿透会让同 Realm 出现
// 两个同邮箱账户，登录身份解析从此二义。码行不被消费（仍在库中），冲突
// 拒绝不销毁重试能力。
#[test_context(TestContext)]
#[tokio::test]
async fn test_us_ru_016_scenario_4_confirm_conflict_email_in_use_returns_409(
    ctx: &mut TestContext,
) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();

    let (user_b_id, _pw_token_b) =
        create_user_and_login(ctx, "conflict-taken@cas.com", "password123").await;
    let (user_a_id, _pw_token_a) =
        create_user_and_login(ctx, "conflict-owner@cas.com", "password123").await;
    let token_a =
        crate::tests::helpers::auth_helpers::mint_first_party_session(ctx, user_a_id).await;

    insert_unreachable_smtp_email_config_direct(&ctx._app_state.pool, &realm_id).await;

    let new_email = "conflict-taken@cas.com";
    let code = issue_code_via_failing_send(ctx, &app, &realm_id, &token_a, new_email).await;

    delete_email_config_direct(&ctx._app_state.pool, &realm_id).await;

    let confirm_resp = get_change_email_confirm(&app, &realm_id, &token_a, &code).await;
    assert_eq!(
        confirm_resp.status(),
        StatusCode::CONFLICT,
        "confirming onto a same-realm occupied mailbox must be a 409 conflict"
    );
    let body: serde_json::Value = crate::tests::response_json(confirm_resp).await;
    assert_eq!(body["message"], "email already in use");

    assert_eq!(
        account_email(&ctx._app_state.pool, user_a_id).await,
        "conflict-owner@cas.com",
        "the requester keeps their mailbox"
    );
    assert_eq!(
        account_email(&ctx._app_state.pool, user_b_id).await,
        "conflict-taken@cas.com",
        "the incumbent keeps their mailbox"
    );

    assert_eq!(
        change_email_code_count(&ctx._app_state.pool, &realm_id).await,
        1,
        "a rejected confirm must not consume the code row"
    );
}
