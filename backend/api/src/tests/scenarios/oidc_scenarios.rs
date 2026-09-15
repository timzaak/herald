// =============================================================================
// OIDC Scenario Tests
// =============================================================================
//
// Scenario tests for the OpenID Connect identity layer stacked on the existing
// OAuth 2.1 Authorization Code + PKCE flow: discovery, JWKS, RS256 id_token,
// userinfo, signing-key rotation, and the zero-regression guarantee for flows
// without the `openid` scope.
//
// id_tokens are verified locally against the live JWKS endpoint — the exact
// verification a standard OIDC client (Grafana-style) performs, which is the
// acceptance bar for "configure one issuer URL and log in".
//
// User Stories covered (docs/user-stories/auth/third-party-app.md):
// - US-TP-001: Authorization Code + PKCE login (now with optional `openid`)
// - US-TP-004: Get user information (userinfo endpoint)
// - US-TP-006: Exception handling
// - US-TP-015: SPA SSO initiation (discovery-driven client setup)
// - US-TP-016: Third-party backend token exchange (id_token in the response)
//
// =============================================================================

use crate::tests::helpers::auth_helpers::{
    create_admin_session_with_user, generate_totp_code, grant_realm_admin_role,
};
use crate::tests::helpers::oauth_pkce_helpers::*;
use crate::tests::helpers::oauth_test_helpers::assert_consent_required_and_recover;
use crate::tests::helpers::passkey_flow_helpers::{
    RP_ORIGIN, register_one_passkey, setup_realm_passkey_config,
};
use crate::tests::helpers::test_setup_helpers::create_test_user;
use crate::tests::response_json;
use crate::tests::schema_test_context::SchemaTestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use herald_core::domain::user_totp::UserTotpService;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde_json::Value;
use test_context::test_context;
use tower::ServiceExt;
use uuid::Uuid;

/// Shared ask key the rotation 200-path tests configure on a cloned AppState
/// and then present via the `X-Herald-Ask-Key` header.
const TEST_ASK_KEY: &str = "test-oidc-ask-shared-secret";

/// Expected issuer in tests: `public_base_url` is
/// `http://localhost:8080` unless a custom domain is mapped.
fn test_issuer(realm_id: &str) -> String {
    format!("http://localhost:8080/api/oauth/{realm_id}")
}

/// Helper: set up admin session and return the token.
async fn setup_admin_session(ctx: &mut SchemaTestContext, email: &str) -> String {
    let (admin_token, user_id) = create_admin_session_with_user(ctx, email, 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;
    admin_token
}

/// GET the OIDC discovery document for a realm.
async fn fetch_discovery(ctx: &SchemaTestContext, realm_id: &str) -> axum::response::Response {
    let request = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/oauth/{realm_id}/.well-known/openid-configuration"
        ))
        .body(Body::empty())
        .unwrap();
    ctx.create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap()
}

/// GET the JWKS document for a realm and assert it is a 200 JSON body.
async fn fetch_jwks(ctx: &SchemaTestContext, realm_id: &str) -> Value {
    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/oauth/{realm_id}/.well-known/jwks.json"))
        .body(Body::empty())
        .unwrap();
    let response = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "JWKS must be publicly readable"
    );
    response_json(response).await
}

/// Call userinfo with a Bearer token via the given HTTP method.
async fn userinfo_request(
    ctx: &SchemaTestContext,
    realm_id: &str,
    method: &str,
    bearer: Option<&str>,
) -> axum::response::Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(format!("/api/oauth/{realm_id}/userinfo"));
    if let Some(bearer) = bearer {
        builder = builder.header("authorization", format!("Bearer {bearer}"));
    }
    let request = builder.body(Body::empty()).unwrap();
    ctx.create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap()
}

/// Decode the header segment of a JWT into JSON.
fn decode_jwt_header(id_token: &str) -> Value {
    let header_b64 = id_token
        .split('.')
        .next()
        .expect("id_token must have a header segment");
    let bytes = URL_SAFE_NO_PAD
        .decode(header_b64)
        .expect("id_token header must be base64url");
    serde_json::from_slice(&bytes).expect("id_token header must be JSON")
}

/// Find a JWKS key by its `kid`.
fn jwks_key_by_kid<'a>(jwks: &'a Value, kid: &str) -> &'a Value {
    jwks["keys"]
        .as_array()
        .expect("JWKS must contain a keys array")
        .iter()
        .find(|key| key["kid"].as_str() == Some(kid))
        .unwrap_or_else(|| panic!("JWKS must publish the signing key {kid}"))
}

/// Verify an id_token exactly like a standard OIDC client would: header
/// alg/kid checks, key lookup in the live JWKS, RS256 signature validation
/// with issuer and audience enforcement. Returns the decoded claims.
fn verify_id_token_locally(id_token: &str, jwks: &Value, issuer: &str, audience: &str) -> Value {
    let header = decode_jwt_header(id_token);
    assert_eq!(
        header["alg"].as_str(),
        Some("RS256"),
        "id_token must be signed with RS256"
    );
    let kid = header["kid"]
        .as_str()
        .expect("id_token header must carry a kid");
    let key = jwks_key_by_kid(jwks, kid);
    assert_eq!(key["kty"].as_str(), Some("RSA"));
    assert_eq!(key["alg"].as_str(), Some("RS256"));
    assert_eq!(key["use"].as_str(), Some("sig"));

    // Same one-step JWKS construction the Google/Apple relying-party code
    // uses (`DecodingKey::from_rsa_components`): n/e are base64url strings.
    let decoding_key = DecodingKey::from_rsa_components(
        key["n"].as_str().expect("JWKS key must have n"),
        key["e"].as_str().expect("JWKS key must have e"),
    )
    .expect("JWKS n/e must build a decoding key");

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&[issuer]);
    validation.set_audience(&[audience]);
    let data = jsonwebtoken::decode::<Value>(id_token, &decoding_key, &validation)
        .expect("id_token must verify against the published JWKS key");
    data.claims
}

/// Drive authorize + password login for an OIDC request and return the issued
/// authorization code (asserting the login succeeded and issued a code).
async fn oidc_login_to_code(
    ctx: &SchemaTestContext,
    realm_id: &str,
    client_id: &str,
    redirect_uri: &str,
    email: &str,
    password: &str,
    state: &str,
    code_verifier: &str,
    scope: Option<&str>,
    nonce: Option<&str>,
) -> String {
    let code_challenge = compute_code_challenge(code_verifier);
    let authorize_response = oauth_authorize_with_oidc(
        ctx,
        realm_id,
        client_id,
        redirect_uri,
        state,
        &code_challenge,
        "S256",
        scope,
        nonce,
    )
    .await;
    assert_eq!(
        authorize_response.status(),
        StatusCode::FOUND,
        "OIDC authorize must still redirect to the login page"
    );

    let login_response = login_with_oauth(
        ctx,
        realm_id,
        email,
        password,
        client_id,
        redirect_uri,
        state,
    )
    .await;
    assert_eq!(
        login_response.status(),
        StatusCode::OK,
        "OIDC login must succeed"
    );
    let login_json: Value = response_json(login_response).await;
    let redirect_to = login_json["redirectTo"]
        .as_str()
        .expect("OIDC login must return redirectTo with the authorization code");
    extract_auth_code_from_redirect(redirect_to)
        .expect("redirectTo must carry the authorization code")
}

/// Exchange an authorization code at /token, asserting success, and return
/// the parsed token response. Extracting id_token/access_token stays at the
/// call sites so each scenario keeps its specific failure message.
/// Lazily bootstrap the schema's OIDC signing key (RSA-2048 keygen,
/// idempotent). The test contexts deliberately do not create one at setup —
/// every unrelated scenario would pay the keygen — so each path that mints
/// or verifies OIDC material ensures the key first.
async fn ensure_signing_key(ctx: &SchemaTestContext) {
    ctx.app_state
        .oidc_signing_key_store
        .ensure_active_key()
        .await
        .expect("Failed to bootstrap OIDC signing key for test schema");
}

async fn oidc_exchange_tokens(
    ctx: &SchemaTestContext,
    realm_id: &str,
    code: &str,
    redirect_uri: &str,
    client_id: &str,
    code_verifier: &str,
) -> Value {
    ensure_signing_key(ctx).await;

    let response = oauth_token_exchange(
        ctx,
        realm_id,
        "authorization_code",
        code,
        redirect_uri,
        client_id,
        code_verifier,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "OIDC token exchange must succeed"
    );
    response_json(response).await
}

/// Seed a profile row so the id_token carries a nickname claim.
async fn seed_profile_nickname(ctx: &SchemaTestContext, user_id: Uuid, nickname: &str) {
    sqlx::query(
        "INSERT INTO profile (id, realm_id, nickname, created_at, updated_at)
         VALUES ($1, $2, $3, NOW(), NOW())",
    )
    .bind(user_id)
    .bind(&ctx._realm_id)
    .bind(nickname)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to seed profile nickname");
}

/// Build a router over a CLONED `AppState` whose `custom_domain_ask_key` is
/// set to [`TEST_ASK_KEY`], so the rotation endpoint's shared-key gate can be
/// satisfied. The 401 tests use the default router (empty configured key →
/// every header mismatches).
fn router_with_ask_key(ctx: &SchemaTestContext) -> axum::Router {
    ctx.create_unified_test_router_with_state(|state| {
        state.custom_domain_ask_key = TEST_ASK_KEY.to_string();
    })
}

/// Build a POST rotation request, optionally attaching the `X-Herald-Ask-Key`
/// header.
fn rotate_request(ask_key: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/api/internal/oidc/signing-key/rotate");
    if let Some(key) = ask_key {
        builder = builder.header("x-herald-ask-key", key);
    }
    builder.body(Body::empty()).unwrap()
}

// =============================================================================
// Test 1: Discovery serves standard configuration
// =============================================================================

// User Story: docs/user-stories/auth/third-party-app.md - US-TP-015 (SPA SSO initiation)
// Covers: A standard OIDC client configures itself from the issuer URL alone —
// discovery must advertise the real endpoints, RS256, and PKCE S256, and must
// switch to the realm's custom domain once one is published.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oidc_discovery_returns_standard_configuration(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();

    let response = fetch_discovery(ctx, &realm_id).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "discovery must be publicly readable"
    );
    let cache_control = response
        .headers()
        .get("cache-control")
        .and_then(|v| v.to_str().ok())
        .expect("discovery must set Cache-Control");
    assert!(
        cache_control.contains("max-age=300"),
        "discovery should be publicly cacheable, got: {cache_control}"
    );

    let discovery: Value = response_json(response).await;
    let issuer = test_issuer(&realm_id);
    assert_eq!(discovery["issuer"].as_str(), Some(issuer.as_str()));
    assert_eq!(
        discovery["authorization_endpoint"].as_str(),
        Some(format!("{issuer}/authorize").as_str())
    );
    assert_eq!(
        discovery["token_endpoint"].as_str(),
        Some(format!("{issuer}/token").as_str())
    );
    assert_eq!(
        discovery["userinfo_endpoint"].as_str(),
        Some(format!("{issuer}/userinfo").as_str())
    );
    assert_eq!(
        discovery["jwks_uri"].as_str(),
        Some(format!("{issuer}/.well-known/jwks.json").as_str())
    );
    assert_eq!(discovery["scopes_supported"], serde_json::json!(["openid"]));
    assert_eq!(
        discovery["response_types_supported"],
        serde_json::json!(["code"])
    );
    assert_eq!(
        discovery["grant_types_supported"],
        serde_json::json!(["authorization_code"])
    );
    assert_eq!(discovery["subject_type"].as_str(), Some("public"));
    assert_eq!(
        discovery["id_token_signing_alg_values_supported"],
        serde_json::json!(["RS256"])
    );
    assert_eq!(
        discovery["token_endpoint_auth_methods_supported"],
        serde_json::json!(["none"])
    );
    assert_eq!(
        discovery["code_challenge_methods_supported"],
        serde_json::json!(["S256"])
    );
    let claims = discovery["claims_supported"]
        .as_array()
        .expect("claims_supported must be an array");
    for claim in [
        "sub",
        "iss",
        "aud",
        "exp",
        "iat",
        "email",
        "email_verified",
        "nickname",
        "nonce",
    ] {
        assert!(
            claims.iter().any(|c| c.as_str() == Some(claim)),
            "claims_supported must advertise {claim}"
        );
    }

    // A published custom domain re-homes the issuer onto https://{hostname}
    // so OIDC clients on the vanity domain keep same-origin endpoints.
    sqlx::query(
        "INSERT INTO custom_domain_mapping (realm_id, hostname, enabled, cname_verified, tls_ready, created_at, updated_at)
         VALUES ($1, 'oidc.example.com', true, false, false, NOW(), NOW())",
    )
    .bind(&realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to insert custom-domain mapping");

    let response = fetch_discovery(ctx, &realm_id).await;
    assert_eq!(response.status(), StatusCode::OK);
    let discovery: Value = response_json(response).await;
    let custom_issuer = format!("https://oidc.example.com/api/oauth/{realm_id}");
    assert_eq!(
        discovery["issuer"].as_str(),
        Some(custom_issuer.as_str()),
        "issuer must switch to the enabled custom domain"
    );
}

// =============================================================================
// Test 2: Unknown realm → 404 on discovery and JWKS
// =============================================================================

// User Story: docs/user-stories/auth/third-party-app.md - US-TP-006 (exception handling)
// Covers: A realm with no row must not yield partial configuration — discovery
// and JWKS answer 404 without leaking whether anything else exists.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oidc_discovery_unknown_realm_returns_404(ctx: &mut SchemaTestContext) {
    let unknown_realm = "00000000-0000-0000-0000-000000000999";

    let response = fetch_discovery(ctx, unknown_realm).await;
    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "discovery for a missing realm must return 404"
    );

    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/oauth/{unknown_realm}/.well-known/jwks.json"))
        .body(Body::empty())
        .unwrap();
    let response = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "JWKS for a missing realm must return 404"
    );
}

// =============================================================================
// Test 3: Full OIDC flow issues a locally verifiable id_token
// =============================================================================

// User Story: docs/user-stories/auth/third-party-app.md - US-TP-001 (Authorization Code + PKCE), US-TP-016 (Token exchange)
// Covers: The acceptance bar for the OIDC layer — a client that knows only the
// issuer gets a signed id_token whose kid, signature, issuer, audience, TTL,
// identity claims, nickname, and nonce echo all verify locally via JWKS.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oidc_full_flow_issues_verifiable_id_token(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let admin_token = setup_admin_session(ctx, "oidc-full-flow@test.com").await;

    let redirect_uri = "https://oidcapp.com/callback";
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "oidc-full-app",
        "OIDC Full App",
        &[redirect_uri],
    )
    .await;
    assert_eq!(create_response.status(), 201);

    let email = "oidc-full-user@test.com";
    let password = "password123";
    let user_id = create_test_user(ctx, email, password).await;
    seed_profile_nickname(ctx, user_id, "Oidc Nick").await;

    let code_verifier = generate_code_verifier();
    let state = generate_state();
    let nonce = "oidc-nonce-0123456789".to_string();
    let auth_code = oidc_login_to_code(
        ctx,
        &realm_id,
        "oidc-full-app",
        redirect_uri,
        email,
        password,
        &state,
        &code_verifier,
        Some("openid profile"),
        Some(&nonce),
    )
    .await;

    let token_json: Value = oidc_exchange_tokens(
        ctx,
        &realm_id,
        &auth_code,
        redirect_uri,
        "oidc-full-app",
        &code_verifier,
    )
    .await;
    let id_token = token_json["id_token"]
        .as_str()
        .expect("scope=openid must produce an id_token")
        .to_string();

    // Verify like a standard client: header, JWKS key, RS256 signature, iss/aud.
    let jwks = fetch_jwks(ctx, &realm_id).await;
    let claims =
        verify_id_token_locally(&id_token, &jwks, &test_issuer(&realm_id), "oidc-full-app");

    assert_eq!(claims["sub"].as_str(), Some(user_id.to_string().as_str()));
    assert_eq!(claims["aud"].as_str(), Some("oidc-full-app"));
    let exp = claims["exp"].as_i64().expect("exp must be a number");
    let iat = claims["iat"].as_i64().expect("iat must be a number");
    assert_eq!(exp - iat, 600, "id_token TTL must be 600 seconds");
    assert_eq!(claims["email"].as_str(), Some(email));
    assert_eq!(
        claims["email_verified"].as_bool(),
        Some(true),
        "a Normal-status user must present email_verified=true"
    );
    assert_eq!(claims["nickname"].as_str(), Some("Oidc Nick"));
    assert_eq!(
        claims["nonce"].as_str(),
        Some(nonce.as_str()),
        "the authorize request's nonce must echo back"
    );
}

// =============================================================================
// Test 4: Zero regression — no id_token key without the openid scope
// =============================================================================

// User Story: docs/user-stories/auth/third-party-app.md - US-TP-016 (Token exchange)
// Covers: The layer's hard compatibility guarantee — a flow without the
// `openid` scope must return exactly the pre-OIDC token response: no id_token
// key at all, every existing field unchanged.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oidc_token_response_unchanged_without_openid_scope(
    ctx: &mut SchemaTestContext,
) {
    let realm_id = ctx._realm_id.clone();
    let admin_token = setup_admin_session(ctx, "oidc-no-scope@test.com").await;

    let redirect_uri = "https://noscopeapp.com/callback";
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "oidc-noscope-app",
        "OIDC NoScope App",
        &[redirect_uri],
    )
    .await;
    assert_eq!(create_response.status(), 201);

    let email = "oidc-noscope-user@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;

    let code_verifier = generate_code_verifier();
    let state = generate_state();
    let auth_code = oidc_login_to_code(
        ctx,
        &realm_id,
        "oidc-noscope-app",
        redirect_uri,
        email,
        password,
        &state,
        &code_verifier,
        None,
        None,
    )
    .await;

    let token_json: Value = oidc_exchange_tokens(
        ctx,
        &realm_id,
        &auth_code,
        redirect_uri,
        "oidc-noscope-app",
        &code_verifier,
    )
    .await;

    let object = token_json
        .as_object()
        .expect("token response must be a JSON object");
    assert!(
        !object.contains_key("id_token"),
        "without the openid scope the response must not even contain an id_token key; got {token_json}"
    );
    // The rest of the response keeps the pre-OIDC shape.
    assert!(object.contains_key("access_token"));
    assert_eq!(token_json["token_type"].as_str(), Some("Bearer"));
    assert!(token_json["expires_in"].as_i64().unwrap() > 0);
}

// =============================================================================
// Test 5: User disabled after code issuance → 400, no tokens
// =============================================================================

// User Story: docs/user-stories/auth/third-party-app.md - US-TP-006 (exception handling)
// Covers: The authorization code outlives the login by its TTL — a user
// disabled in between must not receive an identity token (nor any token).

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oidc_disabled_user_rejected_in_exchange(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let admin_token = setup_admin_session(ctx, "oidc-disabled@test.com").await;

    let redirect_uri = "https://disabledapp.com/callback";
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "oidc-disabled-app",
        "OIDC Disabled App",
        &[redirect_uri],
    )
    .await;
    assert_eq!(create_response.status(), 201);

    let email = "oidc-disabled-user@test.com";
    let password = "password123";
    let user_id = create_test_user(ctx, email, password).await;

    let code_verifier = generate_code_verifier();
    let state = generate_state();
    let auth_code = oidc_login_to_code(
        ctx,
        &realm_id,
        "oidc-disabled-app",
        redirect_uri,
        email,
        password,
        &state,
        &code_verifier,
        Some("openid"),
        Some("nonce-disabled"),
    )
    .await;

    // Disable the user between code issuance and exchange (Forbidden = 2).
    sqlx::query("UPDATE account SET status = 2 WHERE id = $1")
        .bind(user_id)
        .execute(&ctx._app_state.pool)
        .await
        .expect("Failed to disable user");

    let token_response = oauth_token_exchange(
        ctx,
        &realm_id,
        "authorization_code",
        &auth_code,
        redirect_uri,
        "oidc-disabled-app",
        &code_verifier,
    )
    .await;
    assert_eq!(
        token_response.status(),
        StatusCode::BAD_REQUEST,
        "exchange for a disabled user must return 400"
    );
    let error_json: Value = response_json(token_response).await;
    let object = error_json
        .as_object()
        .expect("error response must be a JSON object");
    assert!(
        !object.contains_key("access_token") && !object.contains_key("id_token"),
        "no tokens of any kind may be issued; got {error_json}"
    );

    // The rejection must also run BEFORE the browser token family is written:
    // the login side issued only a downstream authorization code for this
    // user, so a rejected exchange must not strand an orphan family in Redis.
    let mut conn = ctx
        ._app_state
        .redis_manager
        .get()
        .await
        .expect("redis connection for orphan-family check");
    let families: Vec<String> = redis::cmd("SMEMBERS")
        .arg(format!("bt:user_fams:{user_id}"))
        .query_async(&mut conn)
        .await
        .expect("SMEMBERS on the user family index must succeed");
    assert!(
        families.is_empty(),
        "a rejected exchange must not leave a browser token family behind; got {families:?}"
    );
}

// =============================================================================
// Test 6: OIDC parameters do not bypass the redirect whitelist
// =============================================================================

// User Story: docs/user-stories/auth/third-party-app.md - US-TP-001 (redirect whitelist)
// Covers: scope/nonce are new authorize inputs — they must not open a path
// around the exact-match redirect URI whitelist.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oidc_non_whitelisted_redirect_rejected(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let admin_token = setup_admin_session(ctx, "oidc-whitelist@test.com").await;

    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "oidc-whitelist-app",
        "OIDC Whitelist App",
        &["https://goodapp.com/callback"],
    )
    .await;
    assert_eq!(create_response.status(), 201);

    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state = generate_state();

    let authorize_response = oauth_authorize_with_oidc(
        ctx,
        &realm_id,
        "oidc-whitelist-app",
        "https://evil.com/callback",
        &state,
        &code_challenge,
        "S256",
        Some("openid"),
        Some("nonce-whitelist"),
    )
    .await;

    assert_eq!(
        authorize_response.status(),
        StatusCode::BAD_REQUEST,
        "non-whitelisted redirect must still be rejected with OIDC parameters present"
    );
    let error_json: Value = response_json(authorize_response).await;
    let error_msg = error_json["error"]
        .as_str()
        .or_else(|| error_json["message"].as_str())
        .unwrap_or("");
    assert!(
        error_msg.to_lowercase().contains("whitelist")
            || error_msg.to_lowercase().contains("redirect"),
        "Error should mention whitelist/redirect URI, got: {error_msg}"
    );
}

// =============================================================================
// Test 7: userinfo returns the same claims as the id_token
// =============================================================================

// User Story: docs/user-stories/auth/third-party-app.md - US-TP-004 (Get user information)
// Covers: userinfo and the id_token must be same-source same-values — a client
// relying on either surface sees the identical identity, via GET and POST.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oidc_userinfo_returns_claims_matching_id_token(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let admin_token = setup_admin_session(ctx, "oidc-userinfo@test.com").await;

    let redirect_uri = "https://userinfoapp.com/callback";
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "oidc-userinfo-app",
        "OIDC Userinfo App",
        &[redirect_uri],
    )
    .await;
    assert_eq!(create_response.status(), 201);

    let email = "oidc-userinfo-user@test.com";
    let password = "password123";
    let user_id = create_test_user(ctx, email, password).await;
    seed_profile_nickname(ctx, user_id, "Info Nick").await;

    let code_verifier = generate_code_verifier();
    let state = generate_state();
    let auth_code = oidc_login_to_code(
        ctx,
        &realm_id,
        "oidc-userinfo-app",
        redirect_uri,
        email,
        password,
        &state,
        &code_verifier,
        Some("openid"),
        None,
    )
    .await;

    let token_json: Value = oidc_exchange_tokens(
        ctx,
        &realm_id,
        &auth_code,
        redirect_uri,
        "oidc-userinfo-app",
        &code_verifier,
    )
    .await;
    let id_token = token_json["id_token"]
        .as_str()
        .expect("scope=openid must produce an id_token");
    let access_token = token_json["access_token"]
        .as_str()
        .expect("token exchange must produce an access_token");
    let jwks = fetch_jwks(ctx, &realm_id).await;
    let id_claims = verify_id_token_locally(
        id_token,
        &jwks,
        &test_issuer(&realm_id),
        "oidc-userinfo-app",
    );

    let response = userinfo_request(ctx, &realm_id, "GET", Some(access_token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    // userinfo carries PII — the response must be marked uncacheable per
    // OIDC Core §5.3.2 before any claim leaves the server.
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .and_then(|value| value.to_str().ok()),
        Some("no-store"),
        "userinfo response must carry Cache-Control: no-store"
    );
    let userinfo: Value = response_json(response).await;
    assert_eq!(
        userinfo["sub"].as_str(),
        id_claims["sub"].as_str(),
        "userinfo sub must equal the id_token sub"
    );
    assert_eq!(userinfo["email"].as_str(), id_claims["email"].as_str());
    assert_eq!(
        userinfo["email_verified"].as_bool(),
        id_claims["email_verified"].as_bool()
    );
    assert_eq!(
        userinfo["nickname"].as_str(),
        id_claims["nickname"].as_str()
    );

    // POST carries the same payload for form-friendly clients.
    let response = userinfo_request(ctx, &realm_id, "POST", Some(access_token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let userinfo_post: Value = response_json(response).await;
    assert_eq!(userinfo_post["sub"].as_str(), userinfo["sub"].as_str());
}

// =============================================================================
// Test 8: userinfo rejects invalid tokens
// =============================================================================

// User Story: docs/user-stories/auth/third-party-app.md - US-TP-006 (exception handling)
// Covers: userinfo must reject missing and invalid bearer tokens with 401 and
// never leak profile fields alongside the rejection.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oidc_userinfo_rejects_invalid_token(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();

    let response = userinfo_request(ctx, &realm_id, "GET", None).await;
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "missing bearer token must yield 401"
    );
    let body: Value = response_json(response).await;
    assert!(
        !body.as_object().unwrap().contains_key("sub"),
        "no profile fields may accompany a rejection; got {body}"
    );

    let response = userinfo_request(ctx, &realm_id, "GET", Some("not-a-real-token")).await;
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "invalid bearer token must yield 401"
    );
    let body: Value = response_json(response).await;
    assert!(
        !body.as_object().unwrap().contains_key("sub"),
        "no profile fields may accompany a rejection; got {body}"
    );
}

// =============================================================================
// Test 9: Unverified email is reported honestly
// =============================================================================

// User Story: docs/user-stories/auth/third-party-app.md - US-TP-004 (Get user information)
// Covers: A user whose email has not been verified yet may still log in through
// the passkey entrance (the one path that admits WaitVerified accounts, so
// email verification can continue); both identity surfaces must report
// email_verified=false — the client, not the server, decides what an
// unverified email means.

/// Drive the passkey first-factor login with an OAuth downstream context and
/// return the issued authorization code.
async fn passkey_oidc_login_to_code(
    ctx: &SchemaTestContext,
    realm_id: &str,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    authenticator: &mut crate::tests::helpers::passkey_authenticator::Es256Authenticator,
) -> String {
    let options_request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{realm_id}/login/passkey/options"))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "9.9.9.9")
        .body(Body::from(
            serde_json::json!({
                "clientId": ctx._client_id,
                "turnstileToken": "dummy",
                "oauth": { "clientId": client_id, "redirectUri": redirect_uri, "state": state }
            })
            .to_string(),
        ))
        .unwrap();
    let response = ctx
        .create_unified_test_router()
        .oneshot(options_request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let options_body: Value = response_json(response).await;
    let options = options_body["options"].clone();
    let auth_token = options_body["authToken"]
        .as_str()
        .expect("passkey options must return authToken")
        .to_string();

    let assertion = authenticator.authenticate(&options, RP_ORIGIN);
    let verify_request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{realm_id}/login/passkey/verify"))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "9.9.9.9")
        .body(Body::from(
            serde_json::json!({ "authToken": auth_token, "assertion": assertion }).to_string(),
        ))
        .unwrap();
    let response = ctx
        .create_unified_test_router()
        .oneshot(verify_request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let verify_body: Value = response_json(response).await;
    let redirect_to = verify_body["redirectTo"]
        .as_str()
        .expect("passkey login with OAuth context must return redirectTo");
    extract_auth_code_from_redirect(redirect_to)
        .expect("redirectTo must carry the authorization code")
}

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oidc_unverified_email_reported_honestly(ctx: &mut SchemaTestContext) {
    // The passkey RP is resolved from env vars; nextest runs each test in its
    // own process, so setting them here cannot race other scenarios.
    unsafe {
        std::env::set_var("RP_ID", "localhost");
        std::env::set_var("RP_ORIGIN", RP_ORIGIN);
    }

    let realm_id = ctx._realm_id.clone();
    let admin_token = setup_admin_session(ctx, "oidc-unverified@test.com").await;
    setup_realm_passkey_config(ctx, &realm_id, true).await;

    let redirect_uri = "https://unverifiedapp.com/callback";
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "oidc-unverified-app",
        "OIDC Unverified App",
        &[redirect_uri],
    )
    .await;
    assert_eq!(create_response.status(), 201);

    let email = "oidc-unverified-user@test.com";
    let password = "password123";
    let user_id = create_test_user(ctx, email, password).await;

    // Register the passkey while the account is still Normal — password
    // login itself rejects non-active accounts, so the credential must be
    // bound before the status flips.
    let session_token =
        crate::tests::helpers::test_setup_helpers::login_user(ctx, email, password).await;
    let mut authenticator = crate::tests::helpers::passkey_authenticator::Es256Authenticator::new();
    register_one_passkey(
        ctx,
        &session_token,
        &user_id.to_string(),
        password,
        None,
        &mut authenticator,
    )
    .await;

    // WaitVerified = 0: the account exists and can complete passkey login,
    // but the email has not been verified yet.
    sqlx::query("UPDATE account SET status = 0 WHERE id = $1")
        .bind(user_id)
        .execute(&ctx._app_state.pool)
        .await
        .expect("Failed to mark user WaitVerified");

    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state = generate_state();
    let authorize_response = oauth_authorize_with_oidc(
        ctx,
        &realm_id,
        "oidc-unverified-app",
        redirect_uri,
        &state,
        &code_challenge,
        "S256",
        Some("openid"),
        None,
    )
    .await;
    assert_eq!(authorize_response.status(), StatusCode::FOUND);

    let auth_code = passkey_oidc_login_to_code(
        ctx,
        &realm_id,
        "oidc-unverified-app",
        redirect_uri,
        &state,
        &mut authenticator,
    )
    .await;

    let token_json: Value = oidc_exchange_tokens(
        ctx,
        &realm_id,
        &auth_code,
        redirect_uri,
        "oidc-unverified-app",
        &code_verifier,
    )
    .await;
    let id_token = token_json["id_token"]
        .as_str()
        .expect("scope=openid must produce an id_token");
    let jwks = fetch_jwks(ctx, &realm_id).await;
    let claims = verify_id_token_locally(
        id_token,
        &jwks,
        &test_issuer(&realm_id),
        "oidc-unverified-app",
    );
    assert_eq!(
        claims["email_verified"].as_bool(),
        Some(false),
        "id_token must report email_verified=false for a WaitVerified user"
    );

    let access_token = token_json["access_token"].as_str().unwrap();
    let response = userinfo_request(ctx, &realm_id, "GET", Some(access_token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let userinfo: Value = response_json(response).await;
    assert_eq!(
        userinfo["email_verified"].as_bool(),
        Some(false),
        "userinfo must report email_verified=false for a WaitVerified user"
    );
}

// =============================================================================
// Test 10: Signing-key rotation is transparent to clients
// =============================================================================

// User Story: docs/user-stories/auth/third-party-app.md - US-TP-016 (Token exchange)
// Covers: Rotation must never invalidate a logged-in client — the old key
// stays published through its overlap window, tokens issued before the
// rotation keep verifying, and new logins sign with the new kid immediately.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oidc_rotation_transparent_to_clients(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let admin_token = setup_admin_session(ctx, "oidc-rotation@test.com").await;

    let redirect_uri = "https://rotationapp.com/callback";
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "oidc-rotation-app",
        "OIDC Rotation App",
        &[redirect_uri],
    )
    .await;
    assert_eq!(create_response.status(), 201);

    let email = "oidc-rotation-user@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;

    // Login #1 before the rotation.
    let code_verifier = generate_code_verifier();
    let state = generate_state();
    let auth_code = oidc_login_to_code(
        ctx,
        &realm_id,
        "oidc-rotation-app",
        redirect_uri,
        email,
        password,
        &state,
        &code_verifier,
        Some("openid"),
        Some("nonce-before-rotation"),
    )
    .await;
    let token_json: Value = oidc_exchange_tokens(
        ctx,
        &realm_id,
        &auth_code,
        redirect_uri,
        "oidc-rotation-app",
        &code_verifier,
    )
    .await;
    let id_token_before = token_json["id_token"]
        .as_str()
        .expect("pre-rotation login must produce an id_token")
        .to_string();
    let kid_before = decode_jwt_header(&id_token_before)["kid"]
        .as_str()
        .expect("id_token header must carry a kid")
        .to_string();

    // Rotate with the configured ask key.
    let response = router_with_ask_key(ctx)
        .oneshot(rotate_request(Some(TEST_ASK_KEY)))
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "rotation with the correct ask key must succeed"
    );
    let rotation: Value = response_json(response).await;
    let new_kid = rotation["newKeyId"]
        .as_str()
        .expect("rotation must report the new kid")
        .to_string();
    assert_ne!(kid_before, new_kid);
    assert!(
        rotation["retainedUntil"].is_string(),
        "rotation must report the overlap deadline: {rotation}"
    );
    assert!(
        rotation["retiredKeyIds"].is_array(),
        "rotation must report retired kids: {rotation}"
    );

    // The overlap window publishes BOTH keys so old tokens keep verifying.
    let jwks = fetch_jwks(ctx, &realm_id).await;
    jwks_key_by_kid(&jwks, &kid_before);
    jwks_key_by_kid(&jwks, &new_kid);
    verify_id_token_locally(
        &id_token_before,
        &jwks,
        &test_issuer(&realm_id),
        "oidc-rotation-app",
    );

    // Login #2 after the rotation signs with the new kid and still verifies.
    let code_verifier2 = generate_code_verifier();
    let state2 = generate_state();
    let auth_code2 = oidc_login_to_code(
        ctx,
        &realm_id,
        "oidc-rotation-app",
        redirect_uri,
        email,
        password,
        &state2,
        &code_verifier2,
        Some("openid"),
        Some("nonce-after-rotation"),
    )
    .await;
    let token_json2: Value = oidc_exchange_tokens(
        ctx,
        &realm_id,
        &auth_code2,
        redirect_uri,
        "oidc-rotation-app",
        &code_verifier2,
    )
    .await;
    let id_token_after = token_json2["id_token"]
        .as_str()
        .expect("post-rotation login must produce an id_token");
    assert_eq!(
        decode_jwt_header(id_token_after)["kid"].as_str(),
        Some(new_kid.as_str()),
        "post-rotation id_tokens must be signed by the new key"
    );
    let claims = verify_id_token_locally(
        id_token_after,
        &jwks,
        &test_issuer(&realm_id),
        "oidc-rotation-app",
    );
    assert_eq!(
        claims["nonce"].as_str(),
        Some("nonce-after-rotation"),
        "post-rotation id_token must still echo the nonce"
    );
}

// =============================================================================
// Test 11: Rotation requires the ask key
// =============================================================================

// User Story: docs/user-stories/auth/third-party-app.md - US-TP-006 (exception handling)
// Covers: The rotation endpoint is an operations surface guarded by a shared
// secret — missing or wrong keys get 401 whether or not a key is configured.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oidc_rotation_requires_ask_key(ctx: &mut SchemaTestContext) {
    // Default router: the configured ask key is empty, so every attempt —
    // missing header or any value — mismatches.
    let response = ctx
        .create_unified_test_router()
        .oneshot(rotate_request(None))
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "rotation without an ask key header must return 401"
    );

    let response = ctx
        .create_unified_test_router()
        .oneshot(rotate_request(Some("any-guess")))
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "rotation against an empty configured key must return 401"
    );

    // Configured router: only the exact shared secret passes; a wrong value
    // is a plain 401 (two guesses stay far below the throttle budget).
    let response = router_with_ask_key(ctx)
        .oneshot(rotate_request(None))
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "rotation without an ask key header must return 401 even when configured"
    );

    let response = router_with_ask_key(ctx)
        .oneshot(rotate_request(Some("wrong-key")))
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "rotation with a wrong ask key must return 401"
    );
}

// =============================================================================
// Test 12: Consent gate blocks the id_token until consent is recorded
// =============================================================================

// User Story: docs/user-stories/auth/third-party-app.md - US-TP-001 (Authorization Code + PKCE)
// Covers: The login consent gate applies unchanged to OIDC logins — with
// stale consent there is no authorization code (hence no id_token), and after
// recording consent the same flow issues the id_token normally.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oidc_consent_gate_blocks_id_token_until_consent_recorded(
    ctx: &mut SchemaTestContext,
) {
    let realm_id = ctx._realm_id.clone();
    let admin_token = setup_admin_session(ctx, "oidc-consent@test.com").await;

    let redirect_uri = "https://consentapp.com/callback";
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "oidc-consent-app",
        "OIDC Consent App",
        &[redirect_uri],
    )
    .await;
    assert_eq!(create_response.status(), 201);

    let email = "oidc-consent-user@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;

    // Publish a newer ToS so the user's recorded consent becomes stale.
    let admin_email = format!("oidc-consent-admin-{}@test.com", Uuid::now_v7());
    let (publish_token, publish_user_id) =
        create_admin_session_with_user(ctx, &admin_email, 1800).await;
    grant_realm_admin_role(ctx, &publish_user_id).await;
    let publish_request = Request::builder()
        .method("PUT")
        .uri(format!(
            "/api/legal/admin/{realm_id}/agreements/terms_of_service"
        ))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {publish_token}"))
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(
            serde_json::json!({ "content": { "en": "oidc consent gate body" } }).to_string(),
        ))
        .unwrap();
    let publish_response = ctx
        .create_unified_test_router()
        .oneshot(publish_request)
        .await
        .unwrap();
    assert_eq!(
        publish_response.status(),
        StatusCode::OK,
        "admin must be able to publish a new ToS version"
    );

    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state = generate_state();
    let nonce = "nonce-consent-gate".to_string();

    let authorize_response = oauth_authorize_with_oidc(
        ctx,
        &realm_id,
        "oidc-consent-app",
        redirect_uri,
        &state,
        &code_challenge,
        "S256",
        Some("openid"),
        Some(&nonce),
    )
    .await;
    assert_eq!(authorize_response.status(), StatusCode::FOUND);

    // Stale consent: the login must withhold the authorization code — issuing
    // one would hand the downstream OIDC client a session over an
    // un-consented account.
    let login_response = login_with_oauth(
        ctx,
        &realm_id,
        email,
        password,
        "oidc-consent-app",
        redirect_uri,
        &state,
    )
    .await;
    assert_eq!(login_response.status(), StatusCode::OK);
    let login_json: Value = response_json(login_response).await;
    assert!(
        login_json["redirectTo"].as_str().is_none(),
        "no authorization code may be issued while consent is stale; got {login_json}"
    );
    assert_consent_required_and_recover(ctx, &login_json).await;

    // After recording consent the same login path issues the code and the
    // id_token flows normally.
    let login_response = login_with_oauth(
        ctx,
        &realm_id,
        email,
        password,
        "oidc-consent-app",
        redirect_uri,
        &state,
    )
    .await;
    assert_eq!(login_response.status(), StatusCode::OK);
    let login_json: Value = response_json(login_response).await;
    let redirect_to = login_json["redirectTo"]
        .as_str()
        .expect("consented login must issue the authorization code");
    let auth_code = extract_auth_code_from_redirect(redirect_to)
        .expect("redirectTo must carry the authorization code");

    let token_json: Value = oidc_exchange_tokens(
        ctx,
        &realm_id,
        &auth_code,
        redirect_uri,
        "oidc-consent-app",
        &code_verifier,
    )
    .await;
    let id_token = token_json["id_token"]
        .as_str()
        .expect("consented OIDC login must produce an id_token");
    let jwks = fetch_jwks(ctx, &realm_id).await;
    let claims =
        verify_id_token_locally(id_token, &jwks, &test_issuer(&realm_id), "oidc-consent-app");
    assert_eq!(
        claims["nonce"].as_str(),
        Some(nonce.as_str()),
        "the nonce must survive the consent round-trip"
    );
}

// =============================================================================
// Test 13: TOTP login path propagates scope and nonce
// =============================================================================

// User Story: docs/user-stories/auth/third-party-app.md - US-TP-001 (Authorization Code + PKCE)
// Covers: The TOTP second-factor issuance point must propagate the OIDC
// scope/nonce exactly like the password path — an MFA user's id_token carries
// the same claims and nonce echo.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oidc_totp_flow_propagates_scope_and_nonce(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let admin_token = setup_admin_session(ctx, "oidc-totp@test.com").await;

    unsafe {
        std::env::set_var("TOTP_SECRET_KEY", "test_key_32_bytes_long_1234567890");
    }

    let redirect_uri = "https://totpoidcapp.com/callback";
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "oidc-totp-app",
        "OIDC TOTP App",
        &[redirect_uri],
    )
    .await;
    assert_eq!(create_response.status(), 201);

    let email = "oidc-totp-user@test.com";
    let password = "password123";
    let user_id = create_test_user(ctx, email, password).await;

    // Enable TOTP for the user directly (secret encrypted the same way the
    // real setup endpoint stores it).
    let secret = UserTotpService::generate_secret();
    let secret_hash =
        UserTotpService::encrypt_secret(&secret).expect("test TOTP secret must encrypt");
    sqlx::query(
        "INSERT INTO user_totp_config
            (id, user_id, realm_id, secret_hash, key_version, enabled, verified_at, created_at, updated_at)
         VALUES ($1, $2, $3, $4, 1, true, NOW(), NOW(), NOW())",
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(&realm_id)
    .bind(&secret_hash)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to seed TOTP config");

    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state = generate_state();
    let nonce = "nonce-totp-path".to_string();

    let authorize_response = oauth_authorize_with_oidc(
        ctx,
        &realm_id,
        "oidc-totp-app",
        redirect_uri,
        &state,
        &code_challenge,
        "S256",
        Some("openid"),
        Some(&nonce),
    )
    .await;
    assert_eq!(authorize_response.status(), StatusCode::FOUND);

    let login_response = login_with_oauth(
        ctx,
        &realm_id,
        email,
        password,
        "oidc-totp-app",
        redirect_uri,
        &state,
    )
    .await;
    assert_eq!(login_response.status(), StatusCode::OK);
    let login_json: Value = response_json(login_response).await;
    assert_eq!(
        login_json["requiresTotp"].as_bool(),
        Some(true),
        "TOTP-enabled user must be asked for the second factor"
    );
    let temp_token = login_json["tempToken"]
        .as_str()
        .expect("TOTP challenge must carry a tempToken");

    let totp_code = generate_totp_code(&secret);

    let verify_response = verify_totp_with_oauth(ctx, &realm_id, temp_token, &totp_code).await;
    assert_eq!(verify_response.status(), StatusCode::OK);
    let verify_json: Value = response_json(verify_response).await;
    let redirect_to = verify_json["redirectTo"]
        .as_str()
        .expect("TOTP verify must issue the authorization code");
    let auth_code = extract_auth_code_from_redirect(redirect_to)
        .expect("redirectTo must carry the authorization code");

    let token_json: Value = oidc_exchange_tokens(
        ctx,
        &realm_id,
        &auth_code,
        redirect_uri,
        "oidc-totp-app",
        &code_verifier,
    )
    .await;
    let id_token = token_json["id_token"]
        .as_str()
        .expect("TOTP-verified OIDC login must produce an id_token");
    let jwks = fetch_jwks(ctx, &realm_id).await;
    let claims = verify_id_token_locally(id_token, &jwks, &test_issuer(&realm_id), "oidc-totp-app");
    assert_eq!(
        claims["nonce"].as_str(),
        Some(nonce.as_str()),
        "the TOTP issuance point must propagate the nonce"
    );
    assert_eq!(
        claims["sub"].as_str(),
        Some(user_id.to_string().as_str()),
        "the TOTP issuance point must bind the id_token to the user"
    );
}

// =============================================================================
// Test 14: RFC 6749 form-encoded token exchange
// =============================================================================

// User Story: docs/user-stories/auth/third-party-app.md - US-TP-015/US-TP-016
// Covers: the discovery document points standard OIDC clients (Grafana-style)
// at /token, and those clients POST application/x-www-form-urlencoded per
// RFC 6749 §4.1.3 — a JSON-only wire form would end their flow with a 415
// after a fully successful authorize+login.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oidc_token_exchange_accepts_form_encoded_body(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let admin_token = setup_admin_session(ctx, "oidc-form-exchange@test.com").await;

    let redirect_uri = "https://formapp.com/callback";
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "oidc-form-app",
        "OIDC Form App",
        &[redirect_uri],
    )
    .await;
    assert_eq!(create_response.status(), 201);

    let email = "oidc-form-user@test.com";
    let password = "password123";
    create_test_user(ctx, email, password).await;

    let code_verifier = generate_code_verifier();
    let state = generate_state();
    let auth_code = oidc_login_to_code(
        ctx,
        &realm_id,
        "oidc-form-app",
        redirect_uri,
        email,
        password,
        &state,
        &code_verifier,
        Some("openid"),
        Some("nonce-form"),
    )
    .await;

    // This scenario posts the exchange itself (form-encoded — that is the
    // point), so it cannot ride the shared exchange helper's key bootstrap.
    ensure_signing_key(ctx).await;

    // Exactly what an RFC 6749 client sends: form-urlencoded parameters.
    let form_body = format!(
        "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
        urlencoding::encode(&auth_code),
        urlencoding::encode(redirect_uri),
        urlencoding::encode("oidc-form-app"),
        urlencoding::encode(&code_verifier),
    );
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/oauth/{realm_id}/token"))
        .header("content-type", "application/x-www-form-urlencoded")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(form_body))
        .unwrap();
    let response = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "a form-encoded exchange (RFC 6749 §4.1.3) must be accepted"
    );
    let token_json: Value = response_json(response).await;
    assert!(
        token_json["access_token"].as_str().is_some(),
        "form exchange must issue tokens; got {token_json}"
    );
    assert!(
        token_json["id_token"].as_str().is_some(),
        "form exchange carries the same OIDC semantics; got {token_json}"
    );
}

// =============================================================================
// Test 15: userinfo requires an openid-consented credential
// =============================================================================

// Covers: OIDC Core §5.3.1 — identity claims are served only for tokens whose
// flow requested the `openid` scope. Without the gate any valid browser token
// (any plain OAuth flow, even a first-party console session) would read
// sub/email/email_verified. The denial is 403 so clients can tell a scope
// gate apart from an invalid token (401).

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oidc_userinfo_requires_openid_scope(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let admin_token = setup_admin_session(ctx, "oidc-scope-gate@test.com").await;

    let redirect_uri = "https://scopegateapp.com/callback";
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "oidc-scopegate-app",
        "OIDC ScopeGate App",
        &[redirect_uri],
    )
    .await;
    assert_eq!(create_response.status(), 201);

    let email = "oidc-scopegate-user@test.com";
    let password = "password123";
    create_test_user(ctx, email, password).await;

    // Complete a flow WITHOUT the openid scope; the exchange itself succeeds.
    let code_verifier = generate_code_verifier();
    let state = generate_state();
    let auth_code = oidc_login_to_code(
        ctx,
        &realm_id,
        "oidc-scopegate-app",
        redirect_uri,
        email,
        password,
        &state,
        &code_verifier,
        None,
        None,
    )
    .await;
    let token_json: Value = oidc_exchange_tokens(
        ctx,
        &realm_id,
        &auth_code,
        redirect_uri,
        "oidc-scopegate-app",
        &code_verifier,
    )
    .await;
    let access_token = token_json["access_token"]
        .as_str()
        .expect("plain OAuth exchange must still issue tokens")
        .to_string();

    let response = userinfo_request(ctx, &realm_id, "GET", Some(&access_token)).await;
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "a valid token from a flow without openid must be denied identity claims (403, not 401)"
    );

    // The first-party console session is the same class of denial: a valid
    // credential whose flow never went through openid consent.
    let response = userinfo_request(ctx, &realm_id, "GET", Some(&admin_token)).await;
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "a console session must not read the OIDC identity surface"
    );
}
