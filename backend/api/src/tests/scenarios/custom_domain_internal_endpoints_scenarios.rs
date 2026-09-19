// =============================================================================
// Custom-Domain Internal Caddy Ask Endpoint Scenarios
// =============================================================================
//
// BE-T02-A — covers the Caddy On-Demand TLS ask endpoint
// `GET /api/internal/custom-domain/authorize` (design §4.2.2 ask, §4.5 security):
// shared-key `X-Herald-Ask-Key` gate; 200 / 404 / 401; no realm leak.
//
// NOTE: the INTERNAL host→realmId resolve endpoint (`GET /api/internal/custom-
// domain/resolve`) was removed when realm routing reverted to always relying on
// the `{realmId}` path segment. The PUBLIC resolve endpoint (`GET
// /api/public-config/custom-domain/resolve`) did survive; its scenarios are at
// the bottom of this file.
//
// ask_key handling (see task BE-T02-A execution note):
//   The shared test context (`SchemaTestContext`) hard-codes
//   `custom_domain_ask_key = String::new()` (empty). The ask handler rejects any
//   caller when the provided header fails to match the configured key, so the
//   default empty-key router exercises the 401 path (missing/mismatched key)
//   without any fixture. To exercise the 200 + no-leak paths we build a router
//   over a CLONED `AppState` whose `custom_domain_ask_key` is set to a known
//   non-empty value, then send that value in the `X-Herald-Ask-Key` header.
//   This keeps the production contract intact (header == configured key) and
//   avoids mutating the shared context for other tests.
//
// **运行方式**:
// ```bash
// cargo nextest run --workspace custom_domain_internal_endpoints_scenarios
// ```
//
// =============================================================================

use crate::application::http::server::create_api_routes;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::Value;
use std::sync::Arc;
use test_context::test_context;
use tower::ServiceExt;

const AUTHORIZE_PATH: &str = "/api/internal/custom-domain/authorize";
const RESOLVE_PATH: &str = "/api/public-config/custom-domain/resolve";
/// Shared ask key the 200-path tests configure on a cloned AppState and then
/// present via the `X-Herald-Ask-Key` header.
const TEST_ASK_KEY: &str = "test-ask-shared-secret";

/// Insert a published `custom_domain_mapping` row for an arbitrary realm.
///
/// `enabled` defaults true — the unified request-time effectiveness predicate
/// (design §5.1「生效判定」) is `enabled = true`; `cname_verified`/`tls_ready`
/// are display-only and default false. The ask endpoint filters on
/// `enabled = true` via `find_by_hostname`.
async fn insert_custom_domain_mapping(
    ctx: &TestContext,
    realm_id: &str,
    hostname: &str,
    enabled: bool,
) {
    sqlx::query(
        "INSERT INTO custom_domain_mapping (realm_id, hostname, enabled, cname_verified, tls_ready, created_at, updated_at)
         VALUES ($1, $2, $3, false, false, NOW(), NOW())
         ON CONFLICT (hostname)
         DO UPDATE SET realm_id = EXCLUDED.realm_id, enabled = EXCLUDED.enabled, updated_at = NOW()",
    )
    .bind(realm_id)
    .bind(hostname)
    .bind(enabled)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to upsert custom-domain mapping");
}

/// Build a router over a CLONED `AppState` whose `custom_domain_ask_key` is set
/// to [`TEST_ASK_KEY`], so the ask endpoint's shared-key gate can be satisfied.
///
/// Only the 200-path and no-leak tests need a non-empty configured key; the 401
/// tests use the default router (empty configured key → every header mismatches).
fn router_with_ask_key(ctx: &TestContext) -> axum::Router {
    let mut state = (*ctx._app_state).clone();
    state.custom_domain_ask_key = TEST_ASK_KEY.to_string();
    let api_routes = create_api_routes(Arc::new(state.clone()));
    api_routes.with_state(state)
}

/// Build a plain `GET {path}?host={host}` request, optionally attaching the
/// `X-Herald-Ask-Key` header (the ask endpoint's shared secret).
fn host_get_request(path: &str, host: &str, ask_key: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method("GET")
        .uri(format!("{path}?host={host}"));
    if let Some(key) = ask_key {
        builder = builder.header("x-herald-ask-key", key);
    }
    builder.body(Body::empty()).unwrap()
}

fn resolve_request(host: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(RESOLVE_PATH)
        .header("host", host)
        .header("x-forwarded-proto", "https")
        .body(Body::empty())
        .unwrap()
}

/// ============================================================================
/// Caddy ask endpoint — 200 authorized for a published + enabled host
/// ============================================================================
//
/// User Story: US-CD-005 — Caddy may only issue TLS for a host a Realm has
/// registered and published (design §4.2.2 ask, §4.5 certificate-abuse gate).
/// Covers: design §5.1 effectiveness predicate (`enabled = true`).
#[test_context(TestContext)]
#[tokio::test]
async fn custom_domain_authorize_returns_200_when_published_and_enabled(ctx: &mut TestContext) {
    let hostname = "login.authorize-200-example.com";
    insert_custom_domain_mapping(ctx, &ctx._realm_id, hostname, true).await;

    // Use a router with a non-empty configured ask key so the shared-secret
    // gate passes when the matching header is presented.
    let app = router_with_ask_key(ctx);
    let request = host_get_request(AUTHORIZE_PATH, hostname, Some(TEST_ASK_KEY));

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body: Value = crate::tests::response_json(response).await;
    assert_eq!(body["authorized"], true);

    let status: (bool, bool, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        "SELECT cname_verified, tls_ready, status_checked_at
         FROM custom_domain_mapping WHERE hostname = $1",
    )
    .bind(hostname)
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("Failed to read custom-domain status");
    assert!(
        status.0,
        "a successful Caddy ask proves DNS routing reached Herald"
    );
    assert!(
        !status.1,
        "TLS is not ready until an HTTPS request reaches the app"
    );
    assert!(status.2.is_some());
}

/// ============================================================================
/// Caddy ask endpoint — host is normalized (case-insensitive, trailing-dot tolerant)
/// ============================================================================
//
/// User Story: US-CD-005 — Caddy's `host`/SNI for a published domain may arrive
/// with differing case or a trailing dot (FQDN form). The mapping column is
/// written normalized (lowercase, trailing dot stripped) by the publish path
/// (`normalize_and_validate_hostname`), so the authorize READ path must apply
/// the same normalization — otherwise a legitimately published domain misses
/// and returns 404, declining TLS issuance.
///
/// This test distinguishes the fixed read path from the original `host.trim()`-
/// only path: under the fix both requests authorize; before it they mismatched
/// the lowercased, dot-stripped column and returned 404.
/// Covers: authorize read-path normalization (symmetry with publish write path).
#[test_context(TestContext)]
#[tokio::test]
async fn custom_domain_authorize_normalizes_host_case_and_trailing_dot(ctx: &mut TestContext) {
    // Mapping row stores the canonical (lowercase, no trailing dot) hostname —
    // exactly what the publish path writes after `normalize_and_validate_hostname`.
    let canonical = "login.authorize-normalize-example.com";
    insert_custom_domain_mapping(ctx, &ctx._realm_id, canonical, true).await;

    let app = router_with_ask_key(ctx);

    // Mixed-case host → must still authorize (normalized to the stored form).
    // The query string MUST mirror the stored canonical label exactly (modulo
    // case/trailing dot); `login.authorize-normalize-example.com` is what the
    // publish path writes, so the mixed-case variant is
    // `Login.Authorize-Normalize-Example.COM`.
    let upper_req = host_get_request(
        AUTHORIZE_PATH,
        "Login.Authorize-Normalize-Example.COM",
        Some(TEST_ASK_KEY),
    );
    let upper_resp = app.clone().oneshot(upper_req).await.unwrap();
    assert_eq!(upper_resp.status(), StatusCode::OK);
    let upper_body: Value = crate::tests::response_json(upper_resp).await;
    assert_eq!(
        upper_body["authorized"], true,
        "mixed-case host must authorize after read-path normalization"
    );

    // FQDN form (trailing dot) → must still authorize.
    let fqdn_req = host_get_request(
        AUTHORIZE_PATH,
        "login.authorize-normalize-example.com.",
        Some(TEST_ASK_KEY),
    );
    let fqdn_resp = app.oneshot(fqdn_req).await.unwrap();
    assert_eq!(fqdn_resp.status(), StatusCode::OK);
    let fqdn_body: Value = crate::tests::response_json(fqdn_resp).await;
    assert_eq!(
        fqdn_body["authorized"], true,
        "trailing-dot host must authorize after read-path normalization"
    );
}

/// ============================================================================
/// Caddy ask endpoint — 404 for an unregistered host
/// ============================================================================
//
/// User Story: US-CD-005 — a host not registered in any Realm's published
/// mapping must not be authorized for TLS issuance (design §4.2.2 ask 404,
/// §4.5 certificate-abuse gate). Caddy declines issuance on a miss.
#[test_context(TestContext)]
#[tokio::test]
async fn custom_domain_authorize_returns_404_for_unregistered_host(ctx: &mut TestContext) {
    // A host that has no mapping row at all.
    let hostname = "unregistered.authorize-404-example.com";

    let app = router_with_ask_key(ctx);
    let request = host_get_request(AUTHORIZE_PATH, hostname, Some(TEST_ASK_KEY));

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// ============================================================================
/// Caddy ask endpoint — 401 without or with a wrong shared key
/// ============================================================================
//
/// User Story: US-CD-005 / §4.5 — the ask endpoint is an internal Caddy gate
/// guarded by a shared secret. A missing OR mismatched `X-Herald-Ask-Key`
/// header must yield 401 regardless of whether the host is registered
/// (design §4.2.2 ask 401, §4.5 shared-key gate). Uses the default test
/// context router whose configured ask key is empty (every header mismatches).
#[test_context(TestContext)]
#[tokio::test]
async fn custom_domain_authorize_returns_401_without_or_with_wrong_shared_key(
    ctx: &mut TestContext,
) {
    // The host IS registered + enabled — the key gate must still reject before
    // the mapping lookup is reached (proves 401 is key-driven, not miss-driven).
    let hostname = "login.authorize-401-example.com";
    insert_custom_domain_mapping(ctx, &ctx._realm_id, hostname, true).await;

    // Default test router: configured ask key is empty → no header matches.
    let app = ctx.create_unified_test_router();

    // Missing key entirely → 401.
    let missing_req = host_get_request(AUTHORIZE_PATH, hostname, None);
    let missing_resp = app.clone().oneshot(missing_req).await.unwrap();
    assert_eq!(missing_resp.status(), StatusCode::UNAUTHORIZED);

    // Wrong key value → 401.
    let wrong_req = host_get_request(AUTHORIZE_PATH, hostname, Some("definitely-wrong-key"));
    let wrong_resp = app.oneshot(wrong_req).await.unwrap();
    assert_eq!(wrong_resp.status(), StatusCode::UNAUTHORIZED);
}

/// ============================================================================
/// Caddy ask endpoint — 200 body never leaks realm identity
/// ============================================================================
//
/// User Story: US-CD-005 / §4.5 — the ask endpoint is a certificate-abuse gate;
/// leaking the realmId would let an attacker map a host to a Realm without
/// owning it. The 200 body must contain ONLY `{"authorized": true}` — no realm
/// id, no realm metadata (design §4.2.2 ask, certificate-abuse gate).
#[test_context(TestContext)]
#[tokio::test]
async fn custom_domain_authorize_does_not_leak_realm(ctx: &mut TestContext) {
    let hostname = "login.authorize-noleak-example.com";
    insert_custom_domain_mapping(ctx, &ctx._realm_id, hostname, true).await;

    let app = router_with_ask_key(ctx);
    let request = host_get_request(AUTHORIZE_PATH, hostname, Some(TEST_ASK_KEY));

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body: Value = crate::tests::response_json(response).await;
    // The body must contain exactly the `authorized` boolean.
    assert_eq!(body["authorized"], true);
    assert!(
        body.get("realmId").is_none(),
        "ask 200 body must not leak realmId; got: {body}"
    );
    assert!(
        body.get("realm_id").is_none(),
        "ask 200 body must not leak realm_id; got: {body}"
    );
    // No other realm-shaped fields.
    let leaked_keys: Vec<&str> = body
        .as_object()
        .map(|o| o.keys().map(String::as_str).collect())
        .unwrap_or_default();
    assert_eq!(
        leaked_keys,
        ["authorized"],
        "ask 200 body must contain only the authorized field; got: {leaked_keys:?}"
    );
}

/// ============================================================================
/// Public custom-domain resolve endpoint — host maps to realm + public config
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn custom_domain_resolve_returns_realm_and_public_config_for_published_host(
    ctx: &mut TestContext,
) {
    let hostname = "login.resolve-200-example.com";
    insert_custom_domain_mapping(ctx, &ctx._realm_id, hostname, true).await;

    let app = ctx.create_unified_test_router();
    let response = app
        .oneshot(resolve_request("Login.Resolve-200-Example.COM."))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body: Value = crate::tests::response_json(response).await;
    assert_eq!(body["realmId"], ctx._realm_id);
    assert!(
        body.get("publicConfig").is_some(),
        "resolve endpoint should include publicConfig; got: {body}"
    );

    let status: (bool, bool, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        "SELECT cname_verified, tls_ready, status_checked_at
         FROM custom_domain_mapping WHERE hostname = $1",
    )
    .bind(hostname)
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("Failed to read custom-domain status");
    assert!(
        status.0 && status.1,
        "a real HTTPS host request proves TLS readiness"
    );
    assert!(status.2.is_some());
}

/// ============================================================================
/// Public custom-domain resolve endpoint — unregistered host returns 404
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn custom_domain_resolve_returns_404_for_unregistered_host(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let response = app
        .oneshot(resolve_request("unregistered.resolve-404-example.com"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// ============================================================================
/// Public resolve endpoint — `?host=` override is gone (audit run-1:
/// public_config.rs:resolve_custom_domain:unauthenticated-host-to-realm-oracle)
/// ============================================================================
//
// The resolve endpoint exists so a SPA already served on a published custom
// domain can discover its realm id; it must resolve ONLY the host the request
// actually arrived on. An arbitrary `?host=` query override turned it into an
// unauthenticated host-to-realm enumeration oracle — the exact disclosure the
// shared-secret ask endpoint exists to prevent. Regression: a request whose
// real Host header is unregistered must 404 even when `?host=` names a
// registered hostname (old code: 200 + realm disclosure).
#[test_context(TestContext)]
#[tokio::test]
async fn custom_domain_resolve_ignores_host_query_override(ctx: &mut TestContext) {
    let registered = "login.registered-override-example.com";
    insert_custom_domain_mapping(ctx, &ctx._realm_id, registered, true).await;

    let app = ctx.create_unified_test_router();
    let request = Request::builder()
        .method("GET")
        .uri(format!("{RESOLVE_PATH}?host={registered}"))
        // The request's actual host is unregistered — the only host the
        // endpoint may resolve.
        .header("host", "unregistered.resolve-override-example.com")
        .header("x-forwarded-proto", "https")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "?host= must not override the request Host: no arbitrary-host oracle"
    );

    // Positive control: the same registered host resolved via its own Host
    // header still succeeds (the SPA bootstrap path is intact).
    let response = app.oneshot(resolve_request(registered)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = crate::tests::response_json(response).await;
    assert_eq!(body["realmId"], ctx._realm_id);
}
