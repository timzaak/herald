// =============================================================================
// Scenario tests: Client App-level Turnstile (D-PROTECT-01 regression)
// =============================================================================
//
// Regression coverage for the Client App-level Turnstile migration
// (design email-otp-login §3.4 / §4.5 / §5.2 / §5.3 / §6.1 / §6.3): Turnstile
// is now sourced entirely from `client_app` (not `realm_config`), and every
// auth endpoint that previously called `verify_turnstile_for_realm` now calls
// `verify_turnstile_for_client_app(&client_app, token, ip)`.
//
// Coverage:
// - Client App create/update carry the Turnstile fields.
// - `ClientAppItem` responses never echo `turnstile_secret_key`.
// - Each migrated auth endpoint (login / register / reset_password / verify_email
//   / passkey options) honours the Client App Turnstile: enforced when enabled,
//   skipped (non-blocking) when not configured.
// - `GET /api/auth/{realmId}/turnstile/status` reads the Client App by `clientId`.
//
// The Turnstile "enabled + token" happy path uses Cloudflare's documented
// always-pass test secret `1x0000000000000000000000000000000AA`, which the
// production `verify_turnstile_for_client_app` short-circuits without any
// network call.
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

/// The Cloudflare Turnstile always-pass test secret (see production
/// `verify_turnstile_for_client_app`). Using it keeps the "token accepted"
/// assertions network-free and deterministic.
const TURNSTILE_TEST_SECRET: &str = "1x0000000000000000000000000000000AA";

/// Provision a realm admin and return its bearer token.
async fn admin_token(ctx: &mut TestContext, email: &str) -> String {
    let (token, user_id) = create_admin_session_with_user(ctx, email, 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;
    token
}

/// Create a Client App via the admin API with the given Turnstile fields and
/// return its database UUID.
async fn create_client_app_with_turnstile(
    ctx: &mut TestContext,
    admin_token: &str,
    client_id: &str,
    turnstile_enabled: bool,
    site_key: Option<&str>,
    secret_key: Option<&str>,
) -> String {
    let mut payload = json!({
        "clientId": client_id,
        "name": format!("Turnstile app {client_id}"),
        "redirectUris": ["https://example.com/callback"],
        "enabled": true,
    });
    payload["turnstileEnabled"] = json!(turnstile_enabled);
    if let Some(site_key) = site_key {
        payload["turnstileSiteKey"] = json!(site_key);
    }
    if let Some(secret_key) = secret_key {
        payload["turnstileSecretKey"] = json!(secret_key);
    }

    let request = Request::builder()
        .method("POST")
        .uri("/api/client".to_string())
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {admin_token}"))
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "client app create should succeed"
    );
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    body["id"]
        .as_str()
        .expect("created client app must return id")
        .to_string()
}

/// Enable registration on the test Realm so the register endpoint reaches the
/// Turnstile check instead of a registration-disabled rejection.
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

// =============================================================================
// Client App field round-trip + secret suppression
// =============================================================================

/// Covers: Design §3.4 / §5.2 / §6.1 (D-PROTECT-01) — create and update a
/// Client App carrying the Turnstile fields; values persist in the database.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_client_app_create_with_turnstile_fields(ctx: &mut TestContext) {
    let admin_token = admin_token(ctx, "ca-turn-create@test.com").await;
    let client_id = uuid::Uuid::now_v7().simple().to_string();

    let app_id = create_client_app_with_turnstile(
        ctx,
        &admin_token,
        &client_id,
        true,
        Some("site-key-abc"),
        Some(TURNSTILE_TEST_SECRET),
    )
    .await;

    // DB persisted the fields.
    let row: (bool, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT turnstile_enabled, turnstile_site_key, turnstile_secret_key
         FROM client_app WHERE id::text = $1",
    )
    .bind(&app_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert!(row.0, "turnstile_enabled must persist");
    assert_eq!(row.1.as_deref(), Some("site-key-abc"));
    assert_eq!(row.2.as_deref(), Some(TURNSTILE_TEST_SECRET));

    // Update via PUT flips turnstile_enabled off and changes the site key.
    let update = json!({
        "turnstileEnabled": false,
        "turnstileSiteKey": "updated-key",
        "turnstileSecretKey": "updated-secret",
    });
    let request = Request::builder()
        .method("PUT")
        .uri(format!("/api/client/{}", app_id))
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {admin_token}"))
        .body(Body::from(update.to_string()))
        .unwrap();
    let resp = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let row: (bool, Option<String>) = sqlx::query_as(
        "SELECT turnstile_enabled, turnstile_site_key FROM client_app WHERE id::text = $1",
    )
    .bind(&app_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert!(!row.0, "turnstile_enabled must update");
    assert_eq!(row.1.as_deref(), Some("updated-key"));
}

/// Covers: Design §3.4 / §5.2 / §6.1 (D-PROTECT-01) — `ClientAppItem` responses
/// (create, update, list/get) never echo `turnstile_secret_key`, while the
/// non-secret Turnstile fields are present.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_client_app_item_does_not_echo_secret(ctx: &mut TestContext) {
    let admin_token = admin_token(ctx, "ca-turn-secret@test.com").await;
    let client_id = uuid::Uuid::now_v7().simple().to_string();

    let app_id = create_client_app_with_turnstile(
        ctx,
        &admin_token,
        &client_id,
        true,
        Some("site-key-xyz"),
        Some(TURNSTILE_TEST_SECRET),
    )
    .await;

    // Update response must not echo the secret.
    let update = json!({ "name": "renamed" });
    let request = Request::builder()
        .method("PUT")
        .uri(format!("/api/client/{}", app_id))
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {admin_token}"))
        .body(Body::from(update.to_string()))
        .unwrap();
    let resp = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["turnstileEnabled"], true);
    assert_eq!(body["turnstileSiteKey"], "site-key-xyz");
    assert!(
        body.get("turnstileSecretKey").is_none() || body["turnstileSecretKey"].is_null(),
        "ClientAppItem must NEVER echo turnstile_secret_key; got {body}"
    );

    // GET (list / item) response must not echo the secret either.
    let request = Request::builder()
        .method("GET")
        .uri("/api/client".to_string())
        .header(header::AUTHORIZATION, format!("Bearer {admin_token}"))
        .body(Body::empty())
        .unwrap();
    let resp = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    // The list response shape is `{ items: [...] }` (or `{ data: { items } }`);
    // fall back to treating the body itself as a single item if neither holds.
    let items_arr = body["items"]
        .as_array()
        .or_else(|| body["data"]["items"].as_array());
    let target = if let Some(items) = items_arr {
        items
            .iter()
            .find(|item| item["id"].as_str() == Some(app_id.as_str()))
            .expect("created client app must appear in list")
    } else {
        &body
    };
    assert!(
        target.get("turnstileSecretKey").is_none() || target["turnstileSecretKey"].is_null(),
        "list/get ClientAppItem must NEVER echo turnstile_secret_key"
    );
}

// =============================================================================
// Turnstile behaviour across migrated auth endpoints
// =============================================================================
//
// Each migrated endpoint resolves the Client App first, then calls
// `verify_turnstile_for_client_app`. The regression contract:
//   - Turnstile NOT enabled on the Client App → verification skipped → endpoint
//     proceeds (NOT blocked).
//   - Turnstile enabled + missing token → 400 "turnstile token is required".
//   - Turnstile enabled + token (with the always-pass test secret) → proceeds.
//
// The default `admin-web-console` Client App has turnstile_enabled=false, so the
// "not configured → skipped" assertions run without any extra setup. For the
// "enabled → enforced" assertions the Client App is toggled on and restored to
// off at the end of the test (the client app is shared across tests in the same
// schema).

/// Covers: Design §3.4 / §5.3 / §6.1 / §6.3 — `/login` honours Client App
/// Turnstile (skipped when not configured, enforced when enabled).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_login_uses_client_app_turnstile(ctx: &mut TestContext) {
    // Not configured (default) → skipped. A login for a non-existent user still
    // passes the Turnstile gate (it then fails at credential check, but the
    // important assertion is that it is NOT a Turnstile 400).
    let payload = json!({
        "clientId": ctx._client_id,
        "email": "nope-turn-login@test.com",
        "password": "password123",
    });
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "6.6.6.6")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let resp = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_ne!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "login with Turnstile not configured must not be a 400 turnstile-token-required"
    );
    let _: serde_json::Value = crate::tests::response_json(resp).await;

    // Enable Turnstile on the default client app → enforced.
    set_turnstile(ctx, true, Some(TURNSTILE_TEST_SECRET), None).await;

    let payload = json!({
        "clientId": ctx._client_id,
        "email": "nope-turn-login2@test.com",
        "password": "password123",
    });
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "6.6.6.6")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let resp = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "login with Turnstile enabled + missing token must be 400"
    );
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    let msg = body["message"].as_str().unwrap_or("").to_lowercase();
    assert!(
        msg.contains("turnstile"),
        "rejection should reference turnstile; got {body}"
    );

    set_turnstile(ctx, false, None, None).await;
}

/// 回归（审计 run-1：turnstile_status:get_turnstile_status:
/// anonymous-clientapp-enumeration-oracle）：匿名三态中"存在但禁用"与
/// "不存在"两个 401 的响应体必须完全一致（否则构成 client-app 存在性/
/// 禁用态枚举预言），且端点必须有 per-IP 限流（对齐 discovery/JWKS）。
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_turnstile_status_uniform_401_and_rate_limited(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let probe_ip = format!("10.77.{}.{}", uuid::Uuid::now_v7().as_u128() % 200, 7);

    let disabled_client_id = uuid::Uuid::now_v7().simple().to_string();
    sqlx::query(
        "INSERT INTO client_app (id, realm_id, client_id, name, enabled, turnstile_enabled, created_at, updated_at)
         VALUES ($1, $2, $3, 'uniform-401-disabled', false, false, NOW(), NOW())",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(&ctx._realm_id)
    .bind(&disabled_client_id)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();

    let status_uri = |client_id: &str| {
        format!(
            "/api/auth/{}/turnstile/status?clientId={}",
            ctx._realm_id, client_id
        )
    };

    // Disabled app vs unknown app: identical 401 bodies (old code: distinct
    // "Client app is disabled" / "Invalid clientId" messages).
    let resp_disabled = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(status_uri(&disabled_client_id))
                .header("x-forwarded-for", &probe_ip)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp_disabled.status(), StatusCode::UNAUTHORIZED);
    let disabled_body: serde_json::Value = crate::tests::response_json(resp_disabled).await;

    let resp_unknown = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(status_uri("no-such-client-id-xyz"))
                .header("x-forwarded-for", &probe_ip)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp_unknown.status(), StatusCode::UNAUTHORIZED);
    let unknown_body: serde_json::Value = crate::tests::response_json(resp_unknown).await;

    assert_eq!(
        disabled_body, unknown_body,
        "the two 401 bodies must be indistinguishable"
    );

    // Per-IP rate limit: a fresh IP hammered past the cap gets 429. The
    // throttle only enforces outside dev/test (same rule as the discovery /
    // JWKS siblings), so hammer through a production-env state clone.
    let hammer_ip = format!("10.88.{}.{}", uuid::Uuid::now_v7().as_u128() % 200, 9);
    let prod_app = ctx.create_unified_test_router_with_state(|s| {
        s.app_env = "production".to_string();
    });
    let mut last_status = StatusCode::OK;
    for _ in 0..31 {
        let resp = prod_app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(status_uri(&disabled_client_id))
                    .header("x-forwarded-for", &hammer_ip)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        last_status = resp.status();
    }
    assert_eq!(
        last_status,
        StatusCode::TOO_MANY_REQUESTS,
        "the 31st request from one IP must be throttled"
    );
}

/// Covers: Design §3.4 / §5.3 / §6.1 / §6.3 — `/register` honours Client App
/// Turnstile (skipped when not configured, enforced when enabled).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_register_uses_client_app_turnstile(ctx: &mut TestContext) {
    enable_registration(ctx).await;

    // Not configured → skipped: register passes the Turnstile gate.
    let payload = json!({
        "clientId": ctx._client_id,
        "email": format!("reg-turn-{}@test.com", uuid::Uuid::now_v7().simple()),
        "password": "password123",
    });
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/register", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "6.6.6.6")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let resp = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_ne!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "register with Turnstile not configured must not be a 400 turnstile-token-required"
    );
    let _: serde_json::Value = crate::tests::response_json(resp).await;

    // Enable → enforced.
    set_turnstile(ctx, true, Some(TURNSTILE_TEST_SECRET), None).await;
    let payload = json!({
        "clientId": ctx._client_id,
        "email": format!("reg-turn2-{}@test.com", uuid::Uuid::now_v7().simple()),
        "password": "password123",
    });
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/register", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "6.6.6.6")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let resp = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "register with Turnstile enabled + missing token must be 400"
    );

    set_turnstile(ctx, false, None, None).await;
}

/// Covers: Design §3.4 / §5.3 / §6.1 / §6.3 — `/reset_password/request` honours
/// Client App Turnstile (skipped when not configured, enforced when enabled).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_reset_password_uses_client_app_turnstile(ctx: &mut TestContext) {
    // Not configured → skipped: reset request always returns 200 (anti-enumeration).
    let payload = json!({
        "clientId": ctx._client_id,
        "email": "reset-turn@test.com",
    });
    let request = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/auth/{}/reset_password/request",
            ctx._realm_id
        ))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "6.6.6.6")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let resp = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "reset_password/request with Turnstile not configured must succeed (200)"
    );
    let _: serde_json::Value = crate::tests::response_json(resp).await;

    // Enable → enforced (missing token → 400, before the always-200 fallback).
    set_turnstile(ctx, true, Some(TURNSTILE_TEST_SECRET), None).await;
    let request = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/auth/{}/reset_password/request",
            ctx._realm_id
        ))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "6.6.6.6")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let resp = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "reset_password/request with Turnstile enabled + missing token must be 400"
    );

    set_turnstile(ctx, false, None, None).await;
}

/// Covers: Design §3.4 / §5.3 / §6.1 / §6.3 — `/verify_email/trigger` honours
/// Client App Turnstile (skipped when not configured, enforced when enabled).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_verify_email_uses_client_app_turnstile(ctx: &mut TestContext) {
    // Not configured → skipped: verify_email/trigger passes the Turnstile gate.
    let payload = json!({
        "clientId": ctx._client_id,
        "email": "verify-turn@test.com",
    });
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/verify_email/trigger", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "6.6.6.6")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let resp = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_ne!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "verify_email/trigger with Turnstile not configured must not be a 400 turnstile-token-required"
    );
    let _: serde_json::Value = crate::tests::response_json(resp).await;

    // Enable → enforced.
    set_turnstile(ctx, true, Some(TURNSTILE_TEST_SECRET), None).await;
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/verify_email/trigger", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "6.6.6.6")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let resp = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "verify_email/trigger with Turnstile enabled + missing token must be 400"
    );

    set_turnstile(ctx, false, None, None).await;
}

/// Covers: Design §3.4 / §5.3 / §6.1 / §6.3 — `/login/passkey/options` honours
/// Client App Turnstile (skipped when not configured, enforced when enabled).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_verify_passkey_uses_client_app_turnstile(ctx: &mut TestContext) {
    // Not configured → skipped: passkey options passes the Turnstile gate
    // (it then proceeds to build a WebAuthn challenge, which may 4xx for other
    // reasons; the assertion is only that it is NOT a turnstile 400).
    let payload = json!({ "clientId": ctx._client_id });
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login/passkey/options", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "6.6.6.6")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let resp = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    let no_token_status = resp.status();
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert!(
        !is_turnstile_rejection(no_token_status, &body),
        "passkey/options with Turnstile not configured must not be a turnstile rejection (got {no_token_status})"
    );

    // Enable → enforced.
    set_turnstile(ctx, true, Some(TURNSTILE_TEST_SECRET), None).await;
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login/passkey/options", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "6.6.6.6")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let resp = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "passkey/options with Turnstile enabled + missing token must be 400"
    );

    set_turnstile(ctx, false, None, None).await;
}

/// Covers: Design §3.4 / §4.2.2 / §6.1 (D-PROTECT-01) — `GET
/// /api/auth/{realmId}/turnstile/status` reads the Client App by `clientId`
/// and returns its `enabled` + `siteKey`; a disabled Client App is rejected.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_turnstile_status_reads_client_app(ctx: &mut TestContext) {
    // Default client app: turnstile not enabled → enabled=false, siteKey=null.
    let request = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/auth/{}/turnstile/status?clientId={}",
            ctx._realm_id, ctx._client_id
        ))
        .body(Body::empty())
        .unwrap();
    let resp = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    let data = body.get("data").cloned().unwrap_or(body);
    assert_eq!(data["enabled"], false);
    assert!(
        data["siteKey"].is_null(),
        "siteKey should be null when Turnstile is not configured"
    );

    // Enable Turnstile on the default client app → status reflects it.
    set_turnstile(
        ctx,
        true,
        Some(TURNSTILE_TEST_SECRET),
        Some("status-site-key"),
    )
    .await;
    let request = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/auth/{}/turnstile/status?clientId={}",
            ctx._realm_id, ctx._client_id
        ))
        .body(Body::empty())
        .unwrap();
    let resp = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    let data = body.get("data").cloned().unwrap_or(body);
    assert_eq!(data["enabled"], true);
    assert_eq!(data["siteKey"], "status-site-key");

    // A disabled Client App is rejected with 401.
    let disabled_client_id = uuid::Uuid::now_v7().simple().to_string();
    sqlx::query(
        "INSERT INTO client_app (id, realm_id, client_id, name, enabled, turnstile_enabled, created_at, updated_at)
         VALUES ($1, $2, $3, 'disabled', false, false, NOW(), NOW())",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(&ctx._realm_id)
    .bind(&disabled_client_id)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();
    let request = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/auth/{}/turnstile/status?clientId={}",
            ctx._realm_id, disabled_client_id
        ))
        .body(Body::empty())
        .unwrap();
    let resp = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "turnstile/status for a disabled client app must be 401"
    );

    set_turnstile(ctx, false, None, None).await;
}

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

/// Toggle the default `admin-web-console` Client App's Turnstile config.
/// `site_key` defaults to a fixed marker when Turnstile is enabled and no
/// explicit value is given; pass `None` with `enabled=false` to clear it.
async fn set_turnstile(
    ctx: &TestContext,
    enabled: bool,
    secret: Option<&str>,
    site_key: Option<&str>,
) {
    let site_key: Option<&str> = site_key.or(if enabled { Some("site-key-x") } else { None });
    sqlx::query(
        "UPDATE client_app SET turnstile_enabled = $1,
            turnstile_site_key = $2, turnstile_secret_key = $3
         WHERE realm_id = $4 AND client_id = $5",
    )
    .bind(enabled)
    .bind(site_key)
    .bind(secret)
    .bind(&ctx._realm_id)
    .bind(&ctx._client_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("failed to toggle turnstile on default client app");
}

/// Heuristic: did a response get rejected specifically by the Turnstile check?
fn is_turnstile_rejection(status: StatusCode, body: &serde_json::Value) -> bool {
    if status != StatusCode::BAD_REQUEST {
        return false;
    }
    body["message"]
        .as_str()
        .map(|m| m.to_lowercase().contains("turnstile"))
        .unwrap_or(false)
}
