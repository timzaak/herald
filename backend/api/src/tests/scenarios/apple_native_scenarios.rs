// =============================================================================
// Scenario tests: Apple native login.
// =============================================================================
//
// Exercises the Apple native login endpoint end-to-end through the HTTP layer:
//   POST /api/oauth/{realmId}/apple/native-login
//
// Coverage focus:
//   - direct-session mode: new-user creation + token family
//   - account linkage by open_id and by email
//   - downstream mode: downstream authorization code + PKCE exchange
//   - rejections: invalid signature / expired token / audience mismatch
//   - provider config: realm without Apple provider → 404
//   - JWKS unreachable → 503 "Upstream service unavailable"
//   - DEC-005 email handling: placeholder on empty email + first-create,
//     open_id match on empty email + returning user, privaterelay as real email
//
// Framework alignment: mirrors `google_one_tap_scenarios.rs`:
//   - `use crate::tests::helpers::*;` family imports
//   - `SchemaTestContext as TestContext`
//   - `#[test_context(TestContext)]` + `#[tokio::test]`
//   - `ctx.create_unified_test_router_with_state(...)` + `tower::ServiceExt::oneshot`
//   - Function names use the `apple_native_` prefix — the runner locates the
//     module by its unique module name.
//
// User stories: docs/user-stories/auth/support-mobile-apple-login.md
//   US-AL-001 — in-app Apple login, auto-create, hidden-email, cancel
//   US-AL-002 — integration: verification, dual mode, rejections, not-configured
//   US-AL-003 — account linkage by Apple sub / email; empty-email first login
//
// =============================================================================
// JWKS INJECTION (dependency injection via AppState)
// -----------------------------------------------------------------------------
// `verify_apple_id_token` accepts a `jwks_url` parameter. The production Apple
// native handler reads it from `state.apple_jwks_url`, which is wired from the
// `[apple_oauth]` config section (default = the real Apple endpoint). Scenario
// tests override that one field on a private owned `AppState` copy via
// `ctx.create_unified_test_router_with_state(...)` — no process-wide env var,
// so the scenarios are safe under parallel nextest runs without
// `--test-threads=1`.
// =============================================================================

use crate::tests::helpers::apple_native_helpers::{
    MintAppleIdTokenOpts, full_apple_jwks_url, mint_test_apple_id_token, spawn_apple_default_jwks,
    spawn_apple_wiremock_jwks, test_kid, wrong_keypair,
};
use crate::tests::helpers::oauth_pkce_helpers::{
    compute_code_challenge, extract_auth_code_from_redirect, generate_code_verifier,
    oauth_token_exchange,
};
use crate::tests::helpers::oauth_test_helpers::assert_consent_required_and_recover;
use crate::tests::response_json;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use redis::AsyncCommands;
use serde_json::{Value, json};
use test_context::test_context;
use tower::ServiceExt;

// Mirrored from `herald_core::domain::security_constants::OAUTH_STATE_TTL_SECONDS`
// so the downstream-state seed below matches production TTL semantics.
const OAUTH_STATE_TTL_SECONDS: u64 = 300;

// ---------------------------------------------------------------------------
// Local setup helpers
// ---------------------------------------------------------------------------

/// Insert an enabled Apple provider config for the test Realm (mirrors the
/// `google_one_tap_scenarios.rs` direct-SQL seeding pattern). `client_id`
/// is `apple-test-client-id` so test identity tokens minted with the same
/// `aud` will validate.
async fn enable_apple_provider(ctx: &TestContext) {
    sqlx::query(
        "INSERT INTO oauth_provider_config (id, realm_id, provider_type, client_id, client_secret, scopes, enabled)
         VALUES ($1, $2, 'apple', 'apple-test-client-id', 'apple-test-client-secret',
                 ARRAY['email', 'name'], true)
         ON CONFLICT (realm_id, provider_type)
         DO UPDATE SET client_id = EXCLUDED.client_id,
                       client_secret = EXCLUDED.client_secret,
                       scopes = EXCLUDED.scopes,
                       enabled = true",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("failed to seed enabled Apple provider config");
}

/// Enable the realm registration policy so OAuth auto-provisioning of new
/// accounts is permitted (mirrors `email_otp_send_verify_scenarios.rs` /
/// `client_app_turnstile_scenarios.rs`). Without this, `find_or_create_user`
/// rejects new-account creation with HTTP 409.
async fn enable_registration(ctx: &TestContext) {
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
         VALUES ($1, 'registration', 'enabled', 'true', true)
         ON CONFLICT (realm_id, config_type, config_key)
         DO UPDATE SET config_value = 'true', enabled = true",
    )
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("failed to enable registration");
}

/// POST /api/oauth/{realmId}/apple/native-login with the given body fields.
///
/// `jwks_url` overrides the Apple native handler's JWKS source on a private
/// owned `AppState` copy (via `create_unified_test_router_with_state`) so the
/// request drives signature verification against the scenario's wiremock JWKS
/// rather than the real Apple endpoint. The shared `ctx.app_state` is
/// untouched. Caller owns the response.
async fn post_apple_native(
    ctx: &TestContext,
    jwks_url: &str,
    identity_token: &str,
    client_id: &str,
    downstream_state: Option<&str>,
) -> axum::response::Response {
    let mut payload = json!({
        "identityToken": identity_token,
        "clientId": client_id,
    });
    if let Some(ds) = downstream_state {
        payload["downstreamState"] = json!(ds);
    }
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/oauth/{}/apple/native-login", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "5.5.5.5")
        .body(Body::from(payload.to_string()))
        .unwrap();
    ctx.create_unified_test_router_with_state(|s| {
        s.apple_jwks_url = jwks_url.to_string();
    })
    .oneshot(request)
    .await
    .unwrap()
}

/// Seed `oauth:state:{downstream_state}` in Redis with a valid
/// `DownstreamAuthorizationState` JSON shape (mirrors production
/// `backend/api-oauth/src/helper.rs` + the `issue_downstream_authorization_code`
/// reader). Returns the PKCE `code_verifier` that matches the stored
/// `code_challenge`, so the scenario can subsequently exchange the issued
/// `ac_*` code via `/token`.
async fn seed_downstream_state(
    ctx: &TestContext,
    downstream_state: &str,
    client_id: &str,
    redirect_uri: &str,
) -> String {
    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state_value = json!({
        "client_id": client_id,
        "realm_id": ctx._realm_id,
        "redirect_uri": redirect_uri,
        "code_challenge": code_challenge,
    })
    .to_string();

    let mut conn = ctx
        ._app_state
        .redis_manager
        .get()
        .await
        .expect("failed to get Redis connection for downstream state seed");
    let _: () = conn
        .set_ex(
            format!("oauth:state:{downstream_state}"),
            state_value,
            OAUTH_STATE_TTL_SECONDS,
        )
        .await
        .expect("failed to seed downstream oauth state");
    code_verifier
}

/// Return the number of `provider` rows linked to the given open_id (Apple
/// `sub`) for the test Realm. The production table is `provider` with column
/// `type` (not `provider_type`) and `open_id` — see migration 0001_core.sql:63.
async fn count_provider_links_by_open_id(ctx: &TestContext, provider_user_id: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM provider
         WHERE realm_id = $1 AND type = 'apple' AND open_id = $2",
    )
    .bind(&ctx._realm_id)
    .bind(provider_user_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap_or(0)
}

// =============================================================================
// Scenarios
// =============================================================================

/// direct-session mode, brand-new Apple `sub` → user auto-created → response
/// is `AppleNativeDirectResponse`-shaped with a non-empty `accessToken`; DB
/// gains exactly one user + one Apple provider link.
///
/// Covers: docs/user-stories/auth/support-mobile-apple-login.md US-AL-001
/// (in-app Apple login + auto-create).
#[test_context(TestContext)]
#[tokio::test]
async fn apple_native_creates_new_user_and_returns_token_family(ctx: &mut TestContext) {
    enable_apple_provider(ctx).await;
    enable_registration(ctx).await;
    // This scenario asserts user creation + token family, not the consent
    // gate. A brand-new OAuth user has no consent rows, so with the
    // platform-default agreements deployed the direct session is gated behind
    // consent (the consent-required variant of the shared direct-login branch
    // is covered by the One Tap consent scenarios). Drop the platform
    // defaults for this schema so the gate stays out of the way.
    sqlx::query("DELETE FROM legal_agreement_version WHERE realm_id IS NULL")
        .execute(&ctx._app_state.pool)
        .await
        .expect("failed to drop platform-default agreement rows");
    // Start the wiremock JWKS serving the default keypair under `test_kid()`,
    // and point the Apple native handler at it via the AppState override.
    let jwks = spawn_apple_default_jwks().await;
    let jwks_url = full_apple_jwks_url(&jwks.0.uri());

    let apple_sub = format!("apple-newuser-{}", uuid::Uuid::now_v7());
    let id_token = mint_test_apple_id_token(&MintAppleIdTokenOpts {
        sub: apple_sub.clone(),
        ..Default::default()
    });

    let resp = post_apple_native(ctx, &jwks_url, &id_token, &ctx._client_id, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "direct-session mode must return 200 for a fresh Apple sub"
    );
    let (resp, token) = crate::tests::extract_bearer_token(resp).await;
    assert!(
        token.is_some(),
        "Apple native direct-session must issue a non-empty Bearer accessToken"
    );

    let body: Value = response_json(resp).await;
    assert_eq!(body["tokenType"], "Bearer");
    assert!(
        body["refreshToken"].as_str().is_some(),
        "response must include refreshToken"
    );
    assert!(
        body["expiresIn"].as_u64().is_some(),
        "response must include expiresIn"
    );
    let user_id_str = body["userId"]
        .as_str()
        .expect("response must include userId");
    let user_id = uuid::Uuid::parse_str(user_id_str).expect("userId must be a valid UUID");

    // DB: a new active account exists under this realm with an Apple link.
    let user_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM account WHERE realm_id = $1 AND id = $2")
            .bind(&ctx._realm_id)
            .bind(user_id)
            .fetch_one(&ctx._app_state.pool)
            .await
            .unwrap();
    assert_eq!(
        user_count, 1,
        "Apple native must create exactly one account"
    );

    let link_count = count_provider_links_by_open_id(ctx, &apple_sub).await;
    assert_eq!(
        link_count, 1,
        "Apple native must create exactly one provider link to Apple"
    );
}

/// second login with the same Apple `sub` reuses the same Herald `user_id`;
/// no duplicate account or link.
///
/// Covers: docs/user-stories/auth/support-mobile-apple-login.md US-AL-003
/// (account linkage by Apple sub, scenario 1).
#[test_context(TestContext)]
#[tokio::test]
async fn apple_native_matches_existing_user_by_open_id(ctx: &mut TestContext) {
    enable_apple_provider(ctx).await;
    enable_registration(ctx).await;
    let jwks = spawn_apple_default_jwks().await;
    let jwks_url = full_apple_jwks_url(&jwks.0.uri());

    let apple_sub = format!("apple-returning-{}", uuid::Uuid::now_v7());
    let email = format!("apple-return-{}@test.com", uuid::Uuid::now_v7());

    // First Apple native login → creates the account.
    let first_token = mint_test_apple_id_token(&MintAppleIdTokenOpts {
        sub: apple_sub.clone(),
        email: Some(email.clone()),
        ..Default::default()
    });
    let first_resp = post_apple_native(ctx, &jwks_url, &first_token, &ctx._client_id, None).await;
    assert_eq!(first_resp.status(), StatusCode::OK);
    let first_body: Value = response_json(first_resp).await;
    let first_user_id = first_body["userId"]
        .as_str()
        .expect("first login must return userId")
        .to_string();

    // Second Apple native login with the same `sub` must reuse the same user_id.
    let second_token = mint_test_apple_id_token(&MintAppleIdTokenOpts {
        sub: apple_sub.clone(),
        email: Some(email.clone()),
        ..Default::default()
    });
    let second_resp = post_apple_native(ctx, &jwks_url, &second_token, &ctx._client_id, None).await;
    assert_eq!(
        second_resp.status(),
        StatusCode::OK,
        "second Apple native login with the same sub must succeed"
    );
    let second_body: Value = response_json(second_resp).await;
    let second_user_id = second_body["userId"]
        .as_str()
        .expect("second login must return userId")
        .to_string();
    assert_eq!(
        second_user_id, first_user_id,
        "Apple native must reuse the same user_id for the same Apple sub"
    );

    // No duplicate account or provider link.
    let user_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM account WHERE realm_id = $1 AND email = $2")
            .bind(&ctx._realm_id)
            .bind(&email)
            .fetch_one(&ctx._app_state.pool)
            .await
            .unwrap();
    assert_eq!(
        user_count, 1,
        "no duplicate account on second Apple native login"
    );

    let link_count = count_provider_links_by_open_id(ctx, &apple_sub).await;
    assert_eq!(
        link_count, 1,
        "no duplicate provider link on second Apple native login"
    );
}

/// `open_id → email → create` chain: a pre-existing email/password user is
/// re-used when the same email arrives via Apple native (linked under the
/// Apple provider, same account, no duplicate).
///
/// Covers: docs/user-stories/auth/support-mobile-apple-login.md US-AL-003
/// (account linkage by email, scenario 2).
#[test_context(TestContext)]
#[tokio::test]
async fn apple_native_matches_existing_user_by_email(ctx: &mut TestContext) {
    enable_apple_provider(ctx).await;
    let jwks = spawn_apple_default_jwks().await;
    let jwks_url = full_apple_jwks_url(&jwks.0.uri());

    // Pre-existing email/password account.
    let email = format!("apple-email-{}@test.com", uuid::Uuid::now_v7());
    let existing_user_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status)
         VALUES ($1, $2, $3, '$2a$12$dummy_password_hash', 1)",
    )
    .bind(existing_user_id)
    .bind(&ctx._realm_id)
    .bind(&email)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();

    let apple_sub = format!("apple-emailmatch-{}", uuid::Uuid::now_v7());
    let id_token = mint_test_apple_id_token(&MintAppleIdTokenOpts {
        sub: apple_sub.clone(),
        email: Some(email.clone()),
        ..Default::default()
    });

    let resp = post_apple_native(ctx, &jwks_url, &id_token, &ctx._client_id, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "Apple native with a matching email must link to the existing account"
    );
    let body: Value = response_json(resp).await;
    let returned_user_id = body["userId"]
        .as_str()
        .expect("Apple native email-match must return userId");

    assert_eq!(
        returned_user_id,
        existing_user_id.to_string(),
        "Apple native must bind to the pre-existing email/password account, not create a new one"
    );

    // Exactly one account, plus an Apple provider link on the existing account.
    let user_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM account WHERE realm_id = $1 AND email = $2")
            .bind(&ctx._realm_id)
            .bind(&email)
            .fetch_one(&ctx._app_state.pool)
            .await
            .unwrap();
    assert_eq!(user_count, 1, "no duplicate account for email match");

    let link_count = count_provider_links_by_open_id(ctx, &apple_sub).await;
    assert_eq!(
        link_count, 1,
        "an Apple provider link must be created on the existing account"
    );
}

/// downstream / Code+PKCE: `downstreamState` present (valid Redis
/// `oauth:state:{ds}`) → the login consent gate runs at code issuance: a
/// fresh user with zero consent records gets 200 consentRequired + a
/// consent-restricted browser family (NO code); after recording consent via
/// the restricted family, re-triggering the same entrance + downstream_state
/// issues `AppleNativeCodeResponse` with `redirectUri` containing
/// `?code=ac_...&state=...`; the issued `ac_*` code then exchanges via
/// `/token` with the matching PKCE `verifier`.
///
/// Covers: docs/user-stories/auth/support-mobile-apple-login.md US-AL-002
/// (integration, downstream Code+PKCE mode, scenario 2).
#[test_context(TestContext)]
#[tokio::test]
async fn apple_native_downstream_mode_issues_authorization_code(ctx: &mut TestContext) {
    enable_apple_provider(ctx).await;
    enable_registration(ctx).await;
    let jwks = spawn_apple_default_jwks().await;
    let jwks_url = full_apple_jwks_url(&jwks.0.uri());

    let apple_sub = format!("apple-downstream-{}", uuid::Uuid::now_v7());
    let id_token = mint_test_apple_id_token(&MintAppleIdTokenOpts {
        sub: apple_sub.clone(),
        ..Default::default()
    });

    // Seed a valid downstream state. The downstream OAuth client_id must be a
    // registered, enabled Herald ClientApp — reusing the pre-seeded first-party
    // app (ctx._client_id). The redirect_uri must satisfy the first-party
    // redirect gate, which requires exactly
    // `<public_base_url>/callback` = http://localhost:8080/callback.
    let downstream_oauth_client_id = ctx._client_id.clone();
    let redirect_uri = "http://localhost:8080/callback";
    let downstream_state = format!("ds-{}", uuid::Uuid::now_v7());
    let code_verifier = seed_downstream_state(
        ctx,
        &downstream_state,
        &downstream_oauth_client_id,
        redirect_uri,
    )
    .await;

    let resp = post_apple_native(
        ctx,
        &jwks_url,
        &id_token,
        &downstream_oauth_client_id,
        Some(&downstream_state),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "downstream mode with a valid downstreamState must return 200"
    );
    let body: Value = response_json(resp).await;

    // Login consent gate (fresh user, zero consent records) + recovery: the
    // shared helper asserts the consentRequired shape and records consent via
    // the restricted family, so the re-trigger below can issue the code.
    assert_consent_required_and_recover(ctx, &body).await;

    // Re-trigger the same entrance with the same downstream_state.
    let resp = post_apple_native(
        ctx,
        &jwks_url,
        &id_token,
        &downstream_oauth_client_id,
        Some(&downstream_state),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "downstream mode after consent must return 200"
    );
    let body: Value = response_json(resp).await;
    let redirect_uri_resp = body["redirectUri"]
        .as_str()
        .expect("downstream mode must return redirectUri after consent");

    // Extract the `ac_*` authorization code from the redirect URI.
    let auth_code = extract_auth_code_from_redirect(redirect_uri_resp)
        .expect("redirectUri must carry an authorization code");
    assert!(
        auth_code.starts_with("ac_"),
        "downstream auth code must use the ac_ prefix; got {auth_code}"
    );

    // Exchange the code via /token with the matching PKCE verifier.
    let token_resp = oauth_token_exchange(
        ctx,
        &ctx._realm_id,
        "authorization_code",
        &auth_code,
        redirect_uri,
        &downstream_oauth_client_id,
        &code_verifier,
    )
    .await;
    assert_eq!(
        token_resp.status(),
        StatusCode::OK,
        "the downstream ac_ code must exchange for an access token via /token with the correct PKCE verifier"
    );
    let token_body: Value = response_json(token_resp).await;
    assert!(
        token_body["access_token"].as_str().is_some(),
        "/token exchange must return access_token"
    );
}

/// rejection: identity token signed by a different RSA private key (not
/// matching the JWKS served under `kid`) → 401.
///
/// Covers: docs/user-stories/auth/support-mobile-apple-login.md US-AL-002
/// (reject tampered/invalid-signature credential, scenario 3).
#[test_context(TestContext)]
#[tokio::test]
async fn apple_native_rejects_invalid_signature(ctx: &mut TestContext) {
    enable_apple_provider(ctx).await;
    let jwks = spawn_apple_default_jwks().await;
    let jwks_url = full_apple_jwks_url(&jwks.0.uri());

    // Sign with the *wrong* keypair, but keep the `kid` pointing at the JWKS
    // served from the *default* keypair — so `verify_apple_id_token` finds
    // the JWK but signature verification fails.
    let wrong_pem = wrong_keypair().private_key_pem.clone();
    let id_token = mint_test_apple_id_token(&MintAppleIdTokenOpts {
        sub: format!("apple-badsig-{}", uuid::Uuid::now_v7()),
        override_private_key_pem: Some(wrong_pem),
        ..Default::default()
    });

    let resp = post_apple_native(ctx, &jwks_url, &id_token, &ctx._client_id, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "an identity token whose signature does not match the JWK must be rejected with 401"
    );
    let _: Value = response_json(resp).await;
}

/// rejection: `exp` in the past → 401.
///
/// Covers: docs/user-stories/auth/support-mobile-apple-login.md US-AL-002
/// (reject expired credential, scenario 4).
#[test_context(TestContext)]
#[tokio::test]
async fn apple_native_rejects_expired_token(ctx: &mut TestContext) {
    enable_apple_provider(ctx).await;
    let jwks = spawn_apple_default_jwks().await;
    let jwks_url = full_apple_jwks_url(&jwks.0.uri());

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let id_token = mint_test_apple_id_token(&MintAppleIdTokenOpts {
        sub: format!("apple-expired-{}", uuid::Uuid::now_v7()),
        iat: now.saturating_sub(7200),
        exp: now.saturating_sub(3600),
        ..Default::default()
    });

    let resp = post_apple_native(ctx, &jwks_url, &id_token, &ctx._client_id, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "an expired identity token must be rejected with 401"
    );
    let _: Value = response_json(resp).await;
}

/// rejection: `aud` set to a different client_id → 401.
///
/// Covers: docs/user-stories/auth/support-mobile-apple-login.md US-AL-002
/// (reject audience-mismatch credential, scenario 5).
#[test_context(TestContext)]
#[tokio::test]
async fn apple_native_rejects_audience_mismatch(ctx: &mut TestContext) {
    enable_apple_provider(ctx).await;
    let jwks = spawn_apple_default_jwks().await;
    let jwks_url = full_apple_jwks_url(&jwks.0.uri());

    let id_token = mint_test_apple_id_token(&MintAppleIdTokenOpts {
        sub: format!("apple-badaud-{}", uuid::Uuid::now_v7()),
        // Token is for a different audience than the realm's Apple client_id.
        aud: "some-other-apple-client-id".to_string(),
        ..Default::default()
    });

    let resp = post_apple_native(ctx, &jwks_url, &id_token, &ctx._client_id, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "an identity token whose aud does not match the realm's Apple client_id must be rejected with 401"
    );
    let _: Value = response_json(resp).await;
}

/// rejection: realm with NO Apple provider configured/enabled → 404
/// `"Apple provider not configured or not enabled"`.
///
/// Covers: docs/user-stories/auth/support-mobile-apple-login.md US-AL-002
/// (reject when Apple provider not configured, scenario 6).
#[test_context(TestContext)]
#[tokio::test]
async fn apple_native_rejects_when_apple_provider_not_configured(ctx: &mut TestContext) {
    // Ensure NO enabled apple provider exists for this fresh realm.
    // (The schema clone seeds no oauth_provider_config rows; we delete
    // defensively in case a prior shared-realm row exists.)
    sqlx::query(
        "DELETE FROM oauth_provider_config
         WHERE realm_id = $1 AND provider_type = 'apple'",
    )
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();

    // Token content and JWKS URL are irrelevant — the handler returns 404
    // before identity token verification when no Apple provider is configured.
    // The JWKS mock is spawned only to satisfy `post_apple_native`'s signature.
    let jwks = spawn_apple_default_jwks().await;
    let jwks_url = full_apple_jwks_url(&jwks.0.uri());
    let id_token = mint_test_apple_id_token(&MintAppleIdTokenOpts::default());

    let resp = post_apple_native(ctx, &jwks_url, &id_token, &ctx._client_id, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "Apple native login on a realm without an enabled Apple provider must return 404"
    );
    let body: Value = response_json(resp).await;
    let message = body["message"].as_str().unwrap_or("");
    assert!(
        message.contains("Apple provider not configured or not enabled"),
        "404 message must be the exact production string; got {message:?}"
    );
}

/// Apple JWKS endpoint returns HTTP 500 → 503 `"Upstream service unavailable"`.
/// The handler must NOT silently downgrade this to 401 or skip signature
/// verification.
///
/// Covers: PRD docs/prd/auth/support-mobile-apple-login.md §4.2 / §6 (JWKS
/// unreachable must surface as 503, not silently skip verification).
#[test_context(TestContext)]
#[tokio::test]
async fn apple_native_returns_503_when_jwks_unreachable(ctx: &mut TestContext) {
    enable_apple_provider(ctx).await;

    // Spawn a JWKS mock that returns HTTP 500. We use the default keypair's
    // public components (irrelevant for an unreachable response, but keeps
    // the helper signature uniform).
    let jwks = spawn_apple_wiremock_jwks(test_kid(), "", "", 500).await;
    // Point the Apple native handler at the unreachable (HTTP 500) wiremock so
    // the 503 branch is exercised.
    let jwks_url = full_apple_jwks_url(&jwks.uri());

    let id_token = mint_test_apple_id_token(&MintAppleIdTokenOpts::default());

    let resp = post_apple_native(ctx, &jwks_url, &id_token, &ctx._client_id, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "JWKS unreachable must surface as 503, not silently downgrade to 401 or skip verification"
    );
    let body: Value = response_json(resp).await;
    let message = body["message"].as_str().unwrap_or("");
    assert!(
        message.contains("Upstream service unavailable"),
        "503 message must be the exact upstream-unavailable string; got {message:?}"
    );
}

/// DEC-005 core: identity token with no email + open_id not previously seen →
/// account created with placeholder email `{sub}@apple.placeholder` and
/// `verified=false`. This is what lets existing Apple users succeed on their
/// first native login (Apple omits email after the first authorization).
///
/// Covers: docs/user-stories/auth/support-mobile-apple-login.md US-AL-003
/// (empty-email first login, scenario 3) + DEC-005.
#[test_context(TestContext)]
#[tokio::test]
async fn apple_native_empty_email_creates_with_placeholder(ctx: &mut TestContext) {
    enable_apple_provider(ctx).await;
    enable_registration(ctx).await;
    let jwks = spawn_apple_default_jwks().await;
    let jwks_url = full_apple_jwks_url(&jwks.0.uri());

    let apple_sub = format!("apple-noemail-{}", uuid::Uuid::now_v7());
    // No email claim, no email_verified — mirrors a non-first Apple authorization.
    let id_token = mint_test_apple_id_token(&MintAppleIdTokenOpts {
        sub: apple_sub.clone(),
        email: None,
        email_verified: None,
        ..Default::default()
    });

    let resp = post_apple_native(ctx, &jwks_url, &id_token, &ctx._client_id, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "empty email + first-create must succeed via placeholder email (DEC-005)"
    );
    let body: Value = response_json(resp).await;
    let user_id_str = body["userId"]
        .as_str()
        .expect("placeholder-email creation must return userId");
    let user_id = uuid::Uuid::parse_str(user_id_str).expect("userId must be a valid UUID");

    // The account is stored with the DEC-005 placeholder email. (The handler
    // builds `verified=false` in the `OAuthUserInfo`, but `find_or_create_user`
    // does not persist that flag onto the account — OAuth-created accounts
    // always start at `status=0` (wait-verified), so there is no distinct
    // `verified` column to assert here. The business guarantee under test is
    // the placeholder email + successful creation.)
    let stored_email: String =
        sqlx::query_scalar("SELECT email FROM account WHERE realm_id = $1 AND id = $2")
            .bind(&ctx._realm_id)
            .bind(user_id)
            .fetch_one(&ctx._app_state.pool)
            .await
            .unwrap();
    assert_eq!(
        stored_email,
        format!("{apple_sub}@apple.placeholder"),
        "account.email must be the DEC-005 placeholder; got {stored_email}"
    );

    let link_count = count_provider_links_by_open_id(ctx, &apple_sub).await;
    assert_eq!(link_count, 1, "an Apple provider link must be created");
}

/// DEC-005: a returning user (existing Apple provider record) whose identity
/// token carries no email is still matched by `open_id` — empty email does not
/// block them and produces no duplicate account. This is the steady-state
/// Apple native login path.
///
/// Covers: docs/user-stories/auth/support-mobile-apple-login.md US-AL-003
/// (empty-email returning user) + DEC-005.
#[test_context(TestContext)]
#[tokio::test]
async fn apple_native_empty_email_existing_user_matches_by_open_id(ctx: &mut TestContext) {
    enable_apple_provider(ctx).await;
    enable_registration(ctx).await;
    let jwks = spawn_apple_default_jwks().await;
    let jwks_url = full_apple_jwks_url(&jwks.0.uri());

    let apple_sub = format!("apple-relapse-{}", uuid::Uuid::now_v7());
    // First login: with email, creates the account + provider record.
    let first_token = mint_test_apple_id_token(&MintAppleIdTokenOpts {
        sub: apple_sub.clone(),
        email: Some(format!("apple-relapse-{}@test.com", uuid::Uuid::now_v7())),
        ..Default::default()
    });
    let first_resp = post_apple_native(ctx, &jwks_url, &first_token, &ctx._client_id, None).await;
    assert_eq!(first_resp.status(), StatusCode::OK);
    let first_body: Value = response_json(first_resp).await;
    let first_user_id = first_body["userId"]
        .as_str()
        .expect("first login must return userId")
        .to_string();

    // Second login: no email at all — must still match by open_id, reusing the
    // same account, no duplicate.
    let second_token = mint_test_apple_id_token(&MintAppleIdTokenOpts {
        sub: apple_sub.clone(),
        email: None,
        email_verified: None,
        ..Default::default()
    });
    let second_resp = post_apple_native(ctx, &jwks_url, &second_token, &ctx._client_id, None).await;
    assert_eq!(
        second_resp.status(),
        StatusCode::OK,
        "returning user with empty email must succeed by open_id match (DEC-005)"
    );
    let second_body: Value = response_json(second_resp).await;
    let second_user_id = second_body["userId"]
        .as_str()
        .expect("second login must return userId");
    assert_eq!(
        second_user_id, first_user_id,
        "empty-email returning user must reuse the same user_id"
    );

    let link_count = count_provider_links_by_open_id(ctx, &apple_sub).await;
    assert_eq!(
        link_count, 1,
        "no duplicate provider link on empty-email returning login"
    );
}

/// DEC-005: Apple's private relay address
/// (`@privaterelay.appleid.apple.com`) is a real, deliverable mailbox and must
/// be stored as the user's real email (not the placeholder). This preserves
/// email-based account linkage for users who chose Apple's hide-my-email.
///
/// Covers: docs/user-stories/auth/support-mobile-apple-login.md US-AL-001
/// (hidden/relay email still creates the account) + DEC-005.
#[test_context(TestContext)]
#[tokio::test]
async fn apple_native_privaterelay_email_treated_as_real(ctx: &mut TestContext) {
    enable_apple_provider(ctx).await;
    enable_registration(ctx).await;
    let jwks = spawn_apple_default_jwks().await;
    let jwks_url = full_apple_jwks_url(&jwks.0.uri());

    let apple_sub = format!("apple-relay-{}", uuid::Uuid::now_v7());
    let relay_email = format!(
        "relay-{}@privaterelay.appleid.apple.com",
        uuid::Uuid::now_v7()
    );
    let id_token = mint_test_apple_id_token(&MintAppleIdTokenOpts {
        sub: apple_sub.clone(),
        email: Some(relay_email.clone()),
        ..Default::default()
    });

    let resp = post_apple_native(ctx, &jwks_url, &id_token, &ctx._client_id, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "privaterelay email must create the account successfully"
    );
    let body: Value = response_json(resp).await;
    let user_id_str = body["userId"]
        .as_str()
        .expect("relay-email creation must return userId");
    let user_id = uuid::Uuid::parse_str(user_id_str).expect("userId must be a valid UUID");

    // The relay address is stored verbatim as the real email — NOT replaced by
    // a placeholder.
    let stored_email: String =
        sqlx::query_scalar("SELECT email FROM account WHERE realm_id = $1 AND id = $2")
            .bind(&ctx._realm_id)
            .bind(user_id)
            .fetch_one(&ctx._app_state.pool)
            .await
            .unwrap();
    assert_eq!(
        stored_email, relay_email,
        "privaterelay address must be stored as the real email, not a placeholder (DEC-005)"
    );
}
