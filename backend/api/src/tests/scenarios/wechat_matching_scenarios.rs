// =============================================================================
// WeChat Identity Matching Scenarios
// =============================================================================
//
// **Purpose**: cover two WeChat login gaps:
// - the union_id → open_id → email four-level matching in
//   `find_or_create_user` had no union_id-branch coverage;
// - the `POST /api/oauth/{realmId}/wechat-miniprogram/login` endpoint
//   had no scenario coverage at all.
//
// **User Story Covered**: US-RU-003 (OAuth Third-Party Login, WeChat), see
// `docs/prd/auth/wechat-oauth.md` §4.1/§5.2.
//
// **Network boundary**: the matching tests call `find_or_create_user`
// directly with crafted `OAuthUserInfo` values against seeded `provider`
// rows — no upstream WeChat call. The miniprogram endpoint tests exercise
// only the local branches (validation, unconfigured provider); the happy
// path requires WeChat's real code2session API and is intentionally not
// driven from scenario tests (same boundary as the Beeceptor-based
// unified_oauth flows).

use crate::tests::helpers::user_helpers::{count_accounts_by_email, create_simple_test_user};
use crate::tests::response_json;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use herald_api_oauth::helper::find_or_create_user;
use herald_core::domain::oauth::entities::ProviderType;
use herald_core::domain::oauth::value_objects::OAuthUserInfo;
use test_context::test_context;
use tower::ServiceExt;

/// Seed a `provider` identity link exactly as a completed WeChat login
/// would leave it, so the matching lookups have a realistic base.
async fn seed_provider_identity(
    ctx: &TestContext,
    user_id: uuid::Uuid,
    provider_type: &str,
    open_id: &str,
    union_id: Option<&str>,
    email: &str,
) {
    sqlx::query(
        "INSERT INTO provider (id, realm_id, type, open_id, union_id, email, user_id, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), NOW())",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(&ctx._realm_id)
    .bind(provider_type)
    .bind(open_id)
    .bind(union_id)
    .bind(email)
    .bind(user_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("seed provider identity");
}

fn wechat_user_info(open_id: &str, union_id: Option<&str>, email: &str) -> OAuthUserInfo {
    OAuthUserInfo {
        provider_type: ProviderType::WeChat,
        provider_user_id: open_id.to_string(),
        email: email.to_string(),
        verified: true,
        avatar: None,
        name: None,
        union_id: union_id.map(str::to_string),
        open_id: Some(open_id.to_string()),
    }
}

/// Scenario 1: the same physical person logs in through a second
/// WeChat app — different open_id per app, same union_id — and must land on
/// their existing account instead of a fresh one. This cross-app union is
/// the entire reason union_id is match priority #1 (wechat-oauth.md §4.1);
/// if matching silently fell through to email/create here, every app-pair
/// would mint shadow accounts.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_union_id_matches_user_across_different_open_ids(ctx: &mut TestContext) {
    let email = "union-user@test.com";
    let user_id = create_simple_test_user(ctx, email).await;
    seed_provider_identity(
        ctx,
        user_id,
        "wechat",
        "openid-app-one",
        Some("union-shared-1"),
        email,
    )
    .await;

    let matched = find_or_create_user(
        &ctx._app_state,
        &ctx._realm_id,
        wechat_user_info("openid-app-two", Some("union-shared-1"), email),
    )
    .await
    .expect("union_id match should resolve");

    assert_eq!(
        matched, user_id,
        "same union_id must resolve to the existing account"
    );
    assert_eq!(
        count_accounts_by_email(ctx, email).await,
        1,
        "no shadow account may be created for a union_id match"
    );

    // The (user_id, type) unique index allows one provider row per user and
    // type: the cross-app login must reuse that row instead of failing on a
    // duplicate insert (the original bug — a 500 on every second-app login).
    let wechat_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM provider WHERE realm_id = $1 AND type = 'wechat' AND user_id = $2",
    )
    .bind(&ctx._realm_id)
    .bind(user_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(wechat_rows, 1, "one wechat provider row per user");

    // A repeat login through the second app must keep resolving (idempotent
    // across the already-linked conflict path).
    let repeat = find_or_create_user(
        &ctx._app_state,
        &ctx._realm_id,
        wechat_user_info("openid-app-two", Some("union-shared-1"), email),
    )
    .await
    .expect("repeat cross-app login should resolve");
    assert_eq!(repeat, user_id);
}

/// Scenario 2 (P3-3): providers that report no union_id must still match
/// through open_id — the second priority of the four-level strategy.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_open_id_matches_when_union_id_absent(ctx: &mut TestContext) {
    let email = "openid-user@test.com";
    let user_id = create_simple_test_user(ctx, email).await;
    seed_provider_identity(ctx, user_id, "wechat", "openid-plain", None, email).await;

    let matched = find_or_create_user(
        &ctx._app_state,
        &ctx._realm_id,
        wechat_user_info("openid-plain", None, email),
    )
    .await
    .expect("open_id match should resolve");

    assert_eq!(matched, user_id);
    assert_eq!(count_accounts_by_email(ctx, email).await, 1);
}

/// Scenario 3: an unknown union_id must not abort the login — the
/// strategy falls through to open_id matching (wechat-oauth.md §4.1).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_unknown_union_id_falls_through_to_open_id(ctx: &mut TestContext) {
    let email = "fallthrough-user@test.com";
    let user_id = create_simple_test_user(ctx, email).await;
    seed_provider_identity(
        ctx,
        user_id,
        "wechat",
        "openid-fallthrough",
        Some("union-stored-1"),
        email,
    )
    .await;

    let matched = find_or_create_user(
        &ctx._app_state,
        &ctx._realm_id,
        wechat_user_info("openid-fallthrough", Some("union-never-seen"), email),
    )
    .await
    .expect("open_id fallback should resolve after a union_id miss");

    assert_eq!(matched, user_id);
}

/// POST the miniprogram login endpoint with a raw JSON body.
async fn post_miniprogram_login(ctx: &mut TestContext, body: &str) -> axum::response::Response {
    let app = ctx.create_unified_test_router();
    let request = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/api/oauth/{}/wechat-miniprogram/login",
            ctx._realm_id
        ))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    app.oneshot(request).await.unwrap()
}

/// Seed an enabled `wechat_miniprogram` OAuth provider configuration (the
/// api-crate test context is a distinct type from test-support's, so the
/// shared helper is mirrored here as a plain insert).
async fn seed_miniprogram_provider_config(ctx: &TestContext) {
    sqlx::query(
        "INSERT INTO oauth_provider_config (id, realm_id, provider_type, client_id, client_secret, scopes, enabled)
         VALUES ($1, $2, 'wechat_miniprogram', 'wx-test-app-id', 'wx-test-app-secret', '{}', true)",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("seed wechat_miniprogram provider config");
}

/// Scenario 4: a realm without a `wechat_miniprogram` provider
/// configuration must answer 404, not attempt an upstream code2session call
/// (wechat-oauth.md §5.2 — the endpoint is only meaningful for realms that
/// configured the mini program app).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_miniprogram_login_without_provider_config_is_404(ctx: &mut TestContext) {
    let response = post_miniprogram_login(ctx, r#"{"code":"js-code-any"}"#).await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value = response_json(response).await;
    assert!(
        body.to_string().contains("not configured"),
        "404 should name the missing provider configuration, got: {body}"
    );
}

/// Scenario 5: an empty `code` fails validation before any upstream
/// call — the mini program client always receives a non-empty js_code from
/// wx.login, so an empty one is a malformed request.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_miniprogram_login_rejects_empty_code(ctx: &mut TestContext) {
    // Configure the provider first so the request reaches the validation
    // layer rather than the 404 branch.
    seed_miniprogram_provider_config(ctx).await;

    let response = post_miniprogram_login(ctx, r#"{"code":""}"#).await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
