// =============================================================================
// Scenario tests: Google One Tap login.
// =============================================================================
//
// Exercises the Google One Tap endpoint end-to-end through the HTTP layer:
//   POST /api/oauth/{realmId}/google/one-tap
//
// Coverage focus:
//   - direct-session mode: new-user creation + token family
//   - account linkage by open_id and by email
//   - downstream mode: downstream authorization code + PKCE exchange
//   - rejections: invalid signature / expired token / audience mismatch /
//     unverified email
//   - provider config: realm without Google provider → 404
//   - JWKS unreachable → 503 "Upstream service unavailable"
//   - legal consent gate: stale/absent consent withholds the token family,
//     restricted session + explicit consent recovers (direct-login entrances
//     are not exempt from the consent gate)
//
// Framework alignment: mirrors `email_otp_send_verify_scenarios.rs` /
// `realm_totp_config_scenarios.rs`:
//   - `use crate::tests::helpers::*;` family imports
//   - `SchemaTestContext as TestContext`
//   - `#[test_context(TestContext)]` + `#[tokio::test]`
//   - `ctx.create_unified_test_router()` + `tower::ServiceExt::oneshot`
//   - Function names use the `google_one_tap_*` prefix — the runner locates
//     the module by its unique module name.
//
// =============================================================================
// JWKS INJECTION (dependency injection via AppState)
// -----------------------------------------------------------------------------
// `verify_google_id_token` accepts a `jwks_url` parameter. The production
// `google_one_tap` handler reads it from `state.google_jwks_url`, which is
// wired from the `[google_oauth]` config section (default = the real Google
// endpoint). Scenario tests override that one field on a private owned
// `AppState` copy via `ctx.create_unified_test_router_with_state(...)` — no
// process-wide env var, so the scenarios are safe under parallel nextest runs
// without `--test-threads=1`.
// =============================================================================

use crate::tests::helpers::google_one_tap_helpers::{
    EmailVerifiedValue, MintIdTokenOpts, default_keypair, full_jwks_url, mint_test_google_id_token,
    spawn_default_jwks, spawn_wiremock_jwks, test_kid, wrong_keypair,
};
use crate::tests::helpers::oauth_pkce_helpers::{
    compute_code_challenge, extract_auth_code_from_redirect, generate_code_verifier,
    oauth_token_exchange,
};
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

/// Insert an enabled Google provider config for the test Realm (mirrors the
/// `public_config_scenarios.rs` direct-SQL seeding pattern). `client_id`
/// defaults to `OAuthProviderTestConfig::google().client_id` so test ID
/// Tokens minted with the same `aud` will validate.
async fn enable_google_provider(ctx: &TestContext) {
    sqlx::query(
        "INSERT INTO oauth_provider_config (id, realm_id, provider_type, client_id, client_secret, scopes, enabled)
         VALUES ($1, $2, 'google', 'google-test-client-id', 'google-test-client-secret',
                 ARRAY['openid', 'email', 'profile'], true)
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
    .expect("failed to seed enabled Google provider config");
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

/// POST /api/oauth/{realmId}/google/one-tap with the given body fields.
///
/// `jwks_url` overrides the One Tap handler's JWKS source on a private owned
/// `AppState` copy (via `create_unified_test_router_with_state`) so the
/// request drives signature verification against the scenario's wiremock
/// JWKS rather than the real Google endpoint. The shared `ctx.app_state` is
/// untouched. Caller owns the response.
async fn post_one_tap(
    ctx: &TestContext,
    jwks_url: &str,
    credential: &str,
    client_id: &str,
    downstream_state: Option<&str>,
) -> axum::response::Response {
    let mut payload = json!({
        "credential": credential,
        "clientId": client_id,
    });
    if let Some(ds) = downstream_state {
        payload["downstreamState"] = json!(ds);
    }
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/oauth/{}/google/one-tap", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "5.5.5.5")
        .body(Body::from(payload.to_string()))
        .unwrap();
    ctx.create_unified_test_router_with_state(|s| {
        s.google_jwks_url = jwks_url.to_string();
    })
    .oneshot(request)
    .await
    .unwrap()
}

/// Seed `oauth:state:{downstream_state}` in Redis with a valid
/// `DownstreamAuthorizationState` JSON shape (mirrors production
/// `backend/api-oauth/src/helper.rs:38-43` + the `issue_downstream_authorization_code`
/// reader at `:722`). Returns the PKCE `code_verifier` that matches the
/// stored `code_challenge`, so the scenario can subsequently exchange the
/// issued `ac_*` code via `/token`.
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

/// Return the number of `provider` rows linked to the given open_id (Google
/// `sub`) for the test Realm. The production table is `provider` (not
/// `oauth_provider_link`), with column `type` (not `provider_type`) and
/// `open_id` (not `provider_user_id`) — see migration 0001_core.sql:61.
async fn count_provider_links_by_open_id(ctx: &TestContext, provider_user_id: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM provider
         WHERE realm_id = $1 AND type = 'google' AND open_id = $2",
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

/// direct-session mode, brand-new Google `sub` → user auto-created → response
/// is `OneTapDirectResponse`-shaped with a non-empty `accessToken`; DB gains
/// exactly one user + one `oauth_provider_link` to Google.
#[test_context(TestContext)]
#[tokio::test]
async fn google_one_tap_creates_new_user_and_returns_token_family(ctx: &mut TestContext) {
    enable_google_provider(ctx).await;
    enable_registration(ctx).await;
    // This scenario asserts user creation + token family + registration
    // credit, not the consent gate. A brand-new OAuth user has no consent
    // rows, so with the platform-default agreements deployed the direct
    // session is gated behind consent (consent-required variant — covered by
    // `google_one_tap_consent_gate_withholds_tokens_until_consent`). Drop the
    // platform defaults for this schema so the gate stays out of the way
    // (same pattern as `consent_gate_scenarios`'s missing-seed test).
    sqlx::query("DELETE FROM legal_agreement_version WHERE realm_id IS NULL")
        .execute(&ctx._app_state.pool)
        .await
        .expect("failed to drop platform-default agreement rows");
    // OAuth first-login creation is a registration for points purposes
    // (points PRD 注册积分): seed a Registration rule so the grant is
    // observable.
    const REGISTRATION_BONUS: i64 = 1000;
    crate::tests::helpers::points_helpers::seed_realm_registration_rules(
        &ctx._app_state.pool,
        &ctx._realm_id,
        REGISTRATION_BONUS,
        None,
        86400,
        1,
    )
    .await;
    // Start the wiremock JWKS serving the default keypair under `test_kid()`,
    // and point the One Tap handler at it via the AppState override.
    let jwks = spawn_default_jwks().await;
    let jwks_url = full_jwks_url(&jwks.0.uri());

    let google_sub = format!("ot-newuser-{}", uuid::Uuid::now_v7());
    let id_token = mint_test_google_id_token(&MintIdTokenOpts {
        sub: google_sub.clone(),
        email: format!("ot-{}@test.com", uuid::Uuid::now_v7()),
        ..Default::default()
    });

    let resp = post_one_tap(ctx, &jwks_url, &id_token, &ctx._client_id, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "direct-session mode must return 200 for a fresh verified Google sub"
    );
    let (resp, token) = crate::tests::extract_bearer_token(resp).await;
    assert!(
        token.is_some(),
        "One Tap direct-session must issue a non-empty Bearer accessToken"
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

    // DB: a new active account exists under this realm with a Google link.
    let user_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM account WHERE realm_id = $1 AND id = $2")
            .bind(&ctx._realm_id)
            .bind(user_id)
            .fetch_one(&ctx._app_state.pool)
            .await
            .unwrap();
    assert_eq!(user_count, 1, "One Tap must create exactly one account");

    let link_count = count_provider_links_by_open_id(ctx, &google_sub).await;
    assert_eq!(
        link_count, 1,
        "One Tap must create exactly one oauth_provider_link to Google"
    );

    // The created account received the registration credit once.
    let ledgers = crate::tests::helpers::points_helpers::get_user_ledgers_by_credit_type(
        ctx,
        user_id,
        herald_core::domain::points::entities::CreditType::RegistrationCredit,
    )
    .await;
    assert_eq!(ledgers.len(), 1, "exactly one registration ledger row");
    assert_eq!(ledgers[0].granted_amount, REGISTRATION_BONUS);
    assert_eq!(
        ledgers[0].source_type,
        herald_core::domain::points::entities::CreditSourceType::Registration
    );
}

/// second login with the same Google `sub` reuses the same Herald `user_id`;
/// no duplicate account or link.
#[test_context(TestContext)]
#[tokio::test]
async fn google_one_tap_matches_existing_user_by_open_id(ctx: &mut TestContext) {
    enable_google_provider(ctx).await;
    enable_registration(ctx).await;
    let jwks = spawn_default_jwks().await;
    let jwks_url = full_jwks_url(&jwks.0.uri());

    let google_sub = format!("ot-returning-{}", uuid::Uuid::now_v7());
    let email = format!("ot-return-{}@test.com", uuid::Uuid::now_v7());

    // First One Tap login → creates the account.
    let first_token = mint_test_google_id_token(&MintIdTokenOpts {
        sub: google_sub.clone(),
        email: email.clone(),
        ..Default::default()
    });
    let first_resp = post_one_tap(ctx, &jwks_url, &first_token, &ctx._client_id, None).await;
    assert_eq!(first_resp.status(), StatusCode::OK);
    let first_body: Value = response_json(first_resp).await;
    let first_user_id = first_body["userId"]
        .as_str()
        .expect("first login must return userId")
        .to_string();

    // Second One Tap login with the same `sub` must reuse the same user_id.
    let second_token = mint_test_google_id_token(&MintIdTokenOpts {
        sub: google_sub.clone(),
        email: email.clone(),
        ..Default::default()
    });
    let second_resp = post_one_tap(ctx, &jwks_url, &second_token, &ctx._client_id, None).await;
    assert_eq!(
        second_resp.status(),
        StatusCode::OK,
        "second One Tap login with the same sub must succeed"
    );
    let second_body: Value = response_json(second_resp).await;
    let second_user_id = second_body["userId"]
        .as_str()
        .expect("second login must return userId")
        .to_string();
    assert_eq!(
        second_user_id, first_user_id,
        "One Tap must reuse the same user_id for the same Google sub"
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
        "no duplicate account on second One Tap login"
    );

    let link_count = count_provider_links_by_open_id(ctx, &google_sub).await;
    assert_eq!(
        link_count, 1,
        "no duplicate oauth_provider_link on second One Tap login"
    );
}

/// `open_id → email → create` chain: a pre-existing email/password user is
/// re-used when the same email arrives via One Tap (linked under the Google
/// provider, same account, no duplicate).
#[test_context(TestContext)]
#[tokio::test]
async fn google_one_tap_matches_existing_user_by_email(ctx: &mut TestContext) {
    enable_google_provider(ctx).await;
    let jwks = spawn_default_jwks().await;
    let jwks_url = full_jwks_url(&jwks.0.uri());

    // Pre-existing email/password account.
    let email = format!("ot-email-{}@test.com", uuid::Uuid::now_v7());
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

    let google_sub = format!("ot-emailmatch-{}", uuid::Uuid::now_v7());
    let id_token = mint_test_google_id_token(&MintIdTokenOpts {
        sub: google_sub.clone(),
        email: email.clone(),
        ..Default::default()
    });

    let resp = post_one_tap(ctx, &jwks_url, &id_token, &ctx._client_id, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "One Tap with a matching email must link to the existing account"
    );
    let body: Value = response_json(resp).await;
    let returned_user_id = body["userId"]
        .as_str()
        .expect("One Tap email-match must return userId");

    assert_eq!(
        returned_user_id,
        existing_user_id.to_string(),
        "One Tap must bind to the pre-existing email/password account, not create a new one"
    );

    // Exactly one account, plus a Google provider link on the existing account.
    let user_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM account WHERE realm_id = $1 AND email = $2")
            .bind(&ctx._realm_id)
            .bind(&email)
            .fetch_one(&ctx._app_state.pool)
            .await
            .unwrap();
    assert_eq!(user_count, 1, "no duplicate account for email match");

    let link_count = count_provider_links_by_open_id(ctx, &google_sub).await;
    assert_eq!(
        link_count, 1,
        "a Google oauth_provider_link must be created on the existing account"
    );
}

/// downstream / Code+PKCE: `downstreamState` present (valid Redis
/// `oauth:state:{ds}`) → `OneTapCodeResponse` with `redirectUri` containing
/// `?code=ac_...&state=...`; the issued `ac_*` code then exchanges via
/// `/token` with the matching PKCE `verifier`.
#[test_context(TestContext)]
#[tokio::test]
async fn google_one_tap_downstream_mode_issues_authorization_code(ctx: &mut TestContext) {
    enable_google_provider(ctx).await;
    enable_registration(ctx).await;
    let jwks = spawn_default_jwks().await;
    let jwks_url = full_jwks_url(&jwks.0.uri());

    let google_sub = format!("ot-downstream-{}", uuid::Uuid::now_v7());
    let id_token = mint_test_google_id_token(&MintIdTokenOpts {
        sub: google_sub.clone(),
        email: format!("ot-down-{}@test.com", uuid::Uuid::now_v7()),
        ..Default::default()
    });

    // Seed a valid downstream state. The downstream OAuth client_id must be a
    // registered, enabled Herald ClientApp — reusing the pre-seeded first-party
    // `admin-web-console` app (ctx._client_id). The redirect_uri must satisfy
    // the first-party redirect gate (`validate_first_party_redirect`), which
    // requires exactly `<public_base_url>/callback` = http://localhost:8080/callback
    // (the test AppState's public_base_url).
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

    let resp = post_one_tap(
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
    let redirect_uri_resp = body["redirectUri"]
        .as_str()
        .expect("downstream mode must return redirectUri");

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

/// rejection: ID Token signed by a different RSA private key (not matching
/// the JWKS served under `kid`) → 401.
#[test_context(TestContext)]
#[tokio::test]
async fn google_one_tap_rejects_invalid_signature(ctx: &mut TestContext) {
    enable_google_provider(ctx).await;
    let jwks = spawn_default_jwks().await;
    let jwks_url = full_jwks_url(&jwks.0.uri());

    // Sign with the *wrong* keypair, but keep the `kid` pointing at the JWKS
    // served from the *default* keypair — so `verify_google_id_token` finds
    // the JWK but signature verification fails.
    let wrong_pem = wrong_keypair().private_key_pem.clone();
    let id_token = mint_test_google_id_token(&MintIdTokenOpts {
        sub: format!("ot-badsig-{}", uuid::Uuid::now_v7()),
        override_private_key_pem: Some(wrong_pem),
        ..Default::default()
    });

    let resp = post_one_tap(ctx, &jwks_url, &id_token, &ctx._client_id, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "an ID Token whose signature does not match the JWK must be rejected with 401"
    );
    let _: Value = response_json(resp).await;
}

/// rejection: `exp` in the past → 401.
#[test_context(TestContext)]
#[tokio::test]
async fn google_one_tap_rejects_expired_token(ctx: &mut TestContext) {
    enable_google_provider(ctx).await;
    let jwks = spawn_default_jwks().await;
    let jwks_url = full_jwks_url(&jwks.0.uri());

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let id_token = mint_test_google_id_token(&MintIdTokenOpts {
        sub: format!("ot-expired-{}", uuid::Uuid::now_v7()),
        iat: now.saturating_sub(7200),
        exp: now.saturating_sub(3600),
        ..Default::default()
    });

    let resp = post_one_tap(ctx, &jwks_url, &id_token, &ctx._client_id, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "an expired ID Token must be rejected with 401"
    );
    let _: Value = response_json(resp).await;
}

/// rejection: `aud` set to a different client_id → 401.
#[test_context(TestContext)]
#[tokio::test]
async fn google_one_tap_rejects_audience_mismatch(ctx: &mut TestContext) {
    enable_google_provider(ctx).await;
    let jwks = spawn_default_jwks().await;
    let jwks_url = full_jwks_url(&jwks.0.uri());

    let id_token = mint_test_google_id_token(&MintIdTokenOpts {
        sub: format!("ot-badaud-{}", uuid::Uuid::now_v7()),
        // Token is for a different audience than the realm's Google client_id.
        aud: "some-other-google-client-id".to_string(),
        ..Default::default()
    });

    let resp = post_one_tap(ctx, &jwks_url, &id_token, &ctx._client_id, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "an ID Token whose aud does not match the realm's Google client_id must be rejected with 401"
    );
    let _: Value = response_json(resp).await;
}

/// rejection: `email_verified = false` (bool) → 401.
#[test_context(TestContext)]
#[tokio::test]
async fn google_one_tap_rejects_unverified_email(ctx: &mut TestContext) {
    enable_google_provider(ctx).await;
    let jwks = spawn_default_jwks().await;
    let jwks_url = full_jwks_url(&jwks.0.uri());

    let id_token = mint_test_google_id_token(&MintIdTokenOpts {
        sub: format!("ot-unverified-{}", uuid::Uuid::now_v7()),
        email_verified: EmailVerifiedValue::Bool(false),
        ..Default::default()
    });

    let resp = post_one_tap(ctx, &jwks_url, &id_token, &ctx._client_id, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "an ID Token whose email_verified is not true must be rejected with 401"
    );
    let body: Value = response_json(resp).await;
    // Handler emits the explicit "Email not verified by Google" message.
    let message = body["message"].as_str().unwrap_or("");
    assert!(
        message.to_lowercase().contains("verified"),
        "unverified-email rejection should mention verification; got {message:?}"
    );

    // String-form `email_verified: "false"` must also be rejected.
    let id_token_str_false = mint_test_google_id_token(&MintIdTokenOpts {
        sub: format!("ot-unverified-str-{}", uuid::Uuid::now_v7()),
        email_verified: EmailVerifiedValue::Str("false".to_string()),
        ..Default::default()
    });
    let resp_str = post_one_tap(ctx, &jwks_url, &id_token_str_false, &ctx._client_id, None).await;
    assert_eq!(
        resp_str.status(),
        StatusCode::UNAUTHORIZED,
        "an ID Token whose email_verified is the string \"false\" must also be rejected with 401"
    );
    let _: Value = response_json(resp_str).await;
}

/// rejection: realm with NO Google provider configured/enabled → 404
/// `"Google provider not configured or not enabled"`.
#[test_context(TestContext)]
#[tokio::test]
async fn google_one_tap_rejects_when_google_provider_not_configured(ctx: &mut TestContext) {
    // Ensure NO enabled google provider exists for this fresh realm.
    // (The schema clone seeds no oauth_provider_config rows; we delete
    // defensively in case a prior shared-realm row exists.)
    sqlx::query(
        "DELETE FROM oauth_provider_config
         WHERE realm_id = $1 AND provider_type = 'google'",
    )
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();

    // Token content and JWKS URL are irrelevant — the handler returns 404
    // before ID Token verification when no Google provider is configured. The
    // JWKS mock is spawned only to satisfy `post_one_tap`'s signature.
    let jwks = spawn_default_jwks().await;
    let jwks_url = full_jwks_url(&jwks.0.uri());
    let id_token = mint_test_google_id_token(&MintIdTokenOpts::default());

    let resp = post_one_tap(ctx, &jwks_url, &id_token, &ctx._client_id, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "One Tap on a realm without an enabled Google provider must return 404"
    );
    let body: Value = response_json(resp).await;
    let message = body["message"].as_str().unwrap_or("");
    assert!(
        message.contains("Google provider not configured or not enabled"),
        "404 message must be the exact production string; got {message:?}"
    );
}

/// Google JWKS endpoint returns HTTP 500 → 503 `"Upstream service
/// unavailable"`. The handler must NOT silently downgrade this to 401 or skip
/// signature verification.
#[test_context(TestContext)]
#[tokio::test]
async fn google_one_tap_returns_503_when_jwks_unreachable(ctx: &mut TestContext) {
    enable_google_provider(ctx).await;

    // Spawn a JWKS mock that returns HTTP 500. We use the default keypair's
    // public components (irrelevant for an unreachable response, but keeps
    // the helper signature uniform).
    let kp = default_keypair();
    let jwks = spawn_wiremock_jwks(test_kid(), &kp.n_b64, &kp.e_b64, 500).await;
    // Point the One Tap handler at the unreachable (HTTP 500) wiremock so the
    // 503 branch is exercised.
    let jwks_url = full_jwks_url(&jwks.uri());

    let id_token = mint_test_google_id_token(&MintIdTokenOpts::default());

    let resp = post_one_tap(ctx, &jwks_url, &id_token, &ctx._client_id, None).await;
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

/// rejection: a registration-disabled Realm must NOT auto-provision an account
/// via OAuth. A brand-new Google `sub` with a verified email is otherwise valid,
/// but because no `registration/enabled` row exists the realm defaults to
/// registration-disabled, so `find_or_create_user` must refuse account creation
/// with HTTP 409 and leave the DB untouched. This guards the policy-bypass fix
/// (OAuth auto-register must respect realm registration policy, PRD: 注册政策优先).
#[test_context(TestContext)]
#[tokio::test]
async fn google_one_tap_blocked_when_registration_disabled(ctx: &mut TestContext) {
    enable_google_provider(ctx).await;
    // Intentionally do NOT call enable_registration(ctx) — realm defaults to
    // registration-disabled, which is the condition under test.
    let jwks = spawn_default_jwks().await;
    let jwks_url = full_jwks_url(&jwks.0.uri());

    let google_sub = format!("ot-noreg-{}", uuid::Uuid::now_v7());
    let email = format!("ot-noreg-{}@test.com", uuid::Uuid::now_v7());
    let id_token = mint_test_google_id_token(&MintIdTokenOpts {
        sub: google_sub.clone(),
        email: email.clone(),
        ..Default::default()
    });

    let resp = post_one_tap(ctx, &jwks_url, &id_token, &ctx._client_id, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "a registration-disabled realm must refuse OAuth auto-provisioning with 409"
    );
    let body: Value = response_json(resp).await;
    assert_eq!(
        body["code"].as_str().unwrap_or(""),
        "conflict",
        "rejection must carry the standard conflict code; got {:?}",
        body["code"]
    );

    // No account must have been created for this email — the policy gate must
    // short-circuit before any INSERT into `account`.
    let user_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM account WHERE realm_id = $1 AND email = $2")
            .bind(&ctx._realm_id)
            .bind(&email)
            .fetch_one(&ctx._app_state.pool)
            .await
            .unwrap();
    assert_eq!(
        user_count, 0,
        "registration-disabled realm must not create an account via OAuth"
    );
}

/// OAuth direct-login consent gate: a user without current legal consent must
/// NOT receive a token family from One Tap — the response mirrors the
/// password-login consent shape (`consentRequired: true` + `agreements` +
/// `restrictedSession`, no token fields). The gate lives in
/// `issue_callback_token_response`, so this One Tap scenario exercises the
/// shared branch used by the OAuth callback / One Tap / Apple native / WeChat
/// continuation direct-session entrances.
///
/// Recovery: the provider credential is single-use, so the login cannot be
/// replayed with agreements inline — instead the restricted browser family
/// (profile-read/delete-account/logout scopes only) posts an explicit consent
/// to /api/legal/{realmId}/consent (the endpoint intentionally has no scope
/// check: it is the consent recovery path), and the user re-triggers One Tap,
/// which then issues the full token family.
///
/// WHY this matters: OAuth direct login must not become a consent bypass. If
/// the gate only ran on password login, publishing a new ToS would strand
/// password users behind re-consent while Google/Apple users sailed through
/// on stale (or absent) consent records.
#[test_context(TestContext)]
#[tokio::test]
async fn google_one_tap_consent_gate_withholds_tokens_until_consent(ctx: &mut TestContext) {
    enable_google_provider(ctx).await;
    enable_registration(ctx).await;
    let jwks = spawn_default_jwks().await;
    let jwks_url = full_jwks_url(&jwks.0.uri());

    let google_sub = format!("ot-consent-{}", uuid::Uuid::now_v7());
    let email = format!("ot-consent-{}@test.com", uuid::Uuid::now_v7());

    // First One Tap: the account is auto-provisioned with no consent rows,
    // so the gate must fire before any session is issued.
    let first_token = mint_test_google_id_token(&MintIdTokenOpts {
        sub: google_sub.clone(),
        email: email.clone(),
        ..Default::default()
    });
    let resp = post_one_tap(ctx, &jwks_url, &first_token, &ctx._client_id, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "consent gate must stay a 200-flag on OAuth direct login, not a 4xx"
    );
    let body: Value = response_json(resp).await;
    assert_eq!(
        body["consentRequired"].as_bool(),
        Some(true),
        "One Tap must surface consentRequired when consent is absent"
    );
    assert!(
        !body
            .as_object()
            .expect("one-tap body must be an object")
            .contains_key("accessToken"),
        "the consent-required branch must not carry any token fields"
    );
    let agreements = body["agreements"]
        .as_array()
        .expect("agreements must be present when consentRequired=true");
    assert_eq!(
        agreements.len(),
        2,
        "a never-consented user must see both ToS and Privacy summaries"
    );
    let restricted_access_token = body["restrictedSession"]["accessToken"]
        .as_str()
        .expect("the gate must mint a consent-restricted browser family")
        .to_string();

    // The account itself is already provisioned — creation happens before the
    // gate; only session issuance is withheld.
    let user_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM account WHERE realm_id = $1 AND email = $2")
            .bind(&ctx._realm_id)
            .bind(&email)
            .fetch_one(&ctx._app_state.pool)
            .await
            .unwrap();
    assert_eq!(
        user_count, 1,
        "auto-provisioning must still happen; the gate withholds only the session"
    );

    // Recovery step 1: record explicit consent with the restricted family.
    let consent_items: Vec<Value> = agreements
        .iter()
        .map(|a| {
            json!({
                "agreement_type": a["agreement_type"],
                "version_id": a["version_id"],
            })
        })
        .collect();
    let consent_request = Request::builder()
        .method("POST")
        .uri(format!("/api/legal/{}/consent", ctx._realm_id))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {restricted_access_token}"))
        .header("x-forwarded-for", "5.5.5.5")
        .body(Body::from(
            json!({ "agreements": consent_items }).to_string(),
        ))
        .unwrap();
    let consent_resp = ctx
        .create_unified_test_router()
        .oneshot(consent_request)
        .await
        .unwrap();
    assert_eq!(
        consent_resp.status(),
        StatusCode::NO_CONTENT,
        "the restricted family must be able to record consent (recovery path)"
    );

    // Recovery step 2: re-trigger One Tap (fresh single-use credential) —
    // consent is now current, so the full token family is issued.
    let second_token = mint_test_google_id_token(&MintIdTokenOpts {
        sub: google_sub.clone(),
        email: email.clone(),
        ..Default::default()
    });
    let resp = post_one_tap(ctx, &jwks_url, &second_token, &ctx._client_id, None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let (resp, token) = crate::tests::extract_bearer_token(resp).await;
    assert!(
        token.is_some(),
        "after explicit consent, One Tap must issue the full token family"
    );
    let body: Value = response_json(resp).await;
    assert!(
        !body
            .as_object()
            .expect("second one-tap body must be an object")
            .contains_key("consentRequired"),
        "consentRequired must be absent once consent is current"
    );
}
