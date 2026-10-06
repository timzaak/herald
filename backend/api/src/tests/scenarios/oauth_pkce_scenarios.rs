// =============================================================================
// OAuth PKCE Scenario Tests
// =============================================================================
//
// Scenario tests for the OAuth 2.1 Authorization Code + PKCE flow covering
// third-party SPA integration. Tests exercise authorize -> login -> token
// exchange, TOTP integration, normal login regression, and exception scenarios.
//
// User Stories covered:
// - US-TP-001: Authorization Code + PKCE
// - US-TP-006: Exception handling
// - US-TP-008: Redirect URI whitelist
// - US-TP-015: SPA SSO initiation
// - US-TP-016: Token exchange
// - US-RU-008: Access third-party app
// - US-RU-010: Jump from third-party app to login (incl. TOTP)
//
// =============================================================================

use crate::tests::helpers::auth_helpers::{
    create_admin_session_with_user, enable_totp_for_session, generate_totp_code,
    grant_realm_admin_role,
};
use crate::tests::helpers::google_one_tap_helpers::{
    MintIdTokenOpts, full_jwks_url, mint_test_google_id_token, spawn_default_jwks,
};
use crate::tests::helpers::ldap_helpers::{enable_ldap, ldap_login_ext, mock_dir, one_mock_user};
use crate::tests::helpers::oauth_pkce_helpers::*;
use crate::tests::helpers::passkey_authenticator::Es256Authenticator;
use crate::tests::helpers::passkey_flow_helpers::{
    RP_ORIGIN, register_one_passkey, setup_realm_passkey_config,
};
use crate::tests::helpers::test_setup_helpers::{create_test_user, login_user};
use crate::tests::response_json;
use crate::tests::schema_test_context::SchemaTestContext;
use axum::http::StatusCode;
use serde_json::{Value, json};
use test_context::test_context;
use tower::ServiceExt;

/// Helper: set up admin session and return the token.
async fn setup_admin_session(ctx: &mut SchemaTestContext, email: &str) -> String {
    let (admin_token, user_id) = create_admin_session_with_user(ctx, email, 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;
    admin_token
}

/// Helper: set up realm TOTP configuration.
async fn setup_realm_totp_config(ctx: &SchemaTestContext, enabled: bool, force_enabled: bool) {
    let config_uuid = uuid::Uuid::now_v7();
    let config_value = serde_json::json!({ "enabled": enabled, "force_enabled": force_enabled });
    let metadata = serde_json::json!({ "force_enabled": force_enabled });

    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id::text FROM realm_config
         WHERE realm_id = $1 AND config_type = 'totp' AND config_key = 'settings'",
    )
    .bind(&ctx._realm_id)
    .fetch_optional(&ctx._app_state.pool)
    .await
    .unwrap();

    if let Some(id) = existing {
        sqlx::query(
            "UPDATE realm_config
             SET enabled = $1, metadata = $2::jsonb, updated_at = NOW()
             WHERE id = $3",
        )
        .bind(enabled)
        .bind(metadata.to_string())
        .bind(&id)
        .execute(&ctx._app_state.pool)
        .await
        .expect("Failed to update realm TOTP config");
    } else {
        sqlx::query(
            "INSERT INTO realm_config (id, realm_id, config_type, config_key, config_value, is_secret, enabled, metadata, created_at, updated_at)
             VALUES ($1, $2, 'totp', 'settings', $3, false, $4, $5::jsonb, NOW(), NOW())",
        )
        .bind(config_uuid)
        .bind(&ctx._realm_id)
        .bind(config_value.to_string())
        .bind(enabled)
        .bind(metadata.to_string())
        .execute(&ctx._app_state.pool)
        .await
        .expect("Failed to create realm TOTP config");
    }
}

/// Enable TOTP for a user who has already logged in.
///
/// Returns the TOTP secret (base32).
async fn enable_totp_for_user(
    ctx: &SchemaTestContext,
    session_token: &str,
    password: &str,
) -> String {
    enable_totp_for_session(ctx, session_token, password)
        .await
        .0
}

// =============================================================================
// Test 1: Full Authorization Code + PKCE flow
// =============================================================================

// User Story: US-TP-001 (Authorization Code + PKCE), US-TP-015 (SPA SSO), US-TP-016 (Token exchange), US-RU-008 (Access third-party app)
// Covers: US-TP-001 acceptance criteria scenarios 1-3, US-TP-015 scenarios 1-3, US-TP-016 scenario 1, US-RU-008 scenario 1

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oauth_pkce_full_flow_success(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let admin_token = setup_admin_session(ctx, "pkce-full-flow@test.com").await;

    // Given: Admin creates a Client App with redirect_uri
    let redirect_uri = "https://myapp.com/callback";
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "pkce-test-app",
        "PKCE Test App",
        &[redirect_uri],
    )
    .await;
    assert_eq!(
        create_response.status(),
        201,
        "Client app creation should succeed"
    );

    // Given: Generate PKCE parameters
    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state = generate_state();

    // When: Call authorize endpoint with valid PKCE parameters
    let authorize_response = oauth_authorize(
        ctx,
        &realm_id,
        "pkce-test-app",
        redirect_uri,
        &state,
        &code_challenge,
        "S256",
    )
    .await;

    // Then: Authorize succeeds (302 redirect to login page)
    assert_eq!(
        authorize_response.status(),
        StatusCode::FOUND,
        "Authorize should return 302 redirect"
    );

    let location = authorize_response
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .expect("Should have Location header");
    assert!(
        location.contains(&format!("/{}/auth/login", realm_id)),
        "Redirect should point to login page, got: {}",
        location
    );

    // Given: Create a test user
    let email = "pkce-user@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;

    // When: Call login with OAuth context
    let login_response = login_with_oauth(
        ctx,
        &realm_id,
        email,
        password,
        "pkce-test-app",
        redirect_uri,
        &state,
    )
    .await;

    // Then: Login succeeds with redirectTo containing authorization code
    assert_eq!(
        login_response.status(),
        StatusCode::OK,
        "OAuth login should succeed"
    );

    let login_json: Value = response_json(login_response).await;
    assert_eq!(login_json["message"].as_str(), Some("ok"));
    assert_eq!(login_json["requiresTotp"].as_bool(), Some(false));

    let redirect_to = login_json["redirectTo"]
        .as_str()
        .expect("redirectTo should be present for OAuth flow");

    // Verify redirectTo format: {redirect_uri}?code={auth_code}&state={state}
    assert!(
        redirect_to.starts_with(redirect_uri),
        "redirectTo should start with redirect_uri, got: {}",
        redirect_to
    );

    let auth_code = extract_auth_code_from_redirect(redirect_to)
        .expect("Should extract auth code from redirectTo");

    assert!(
        auth_code.starts_with("ac_"),
        "Auth code should have ac_ prefix, got: {}",
        auth_code
    );

    // When: Call token endpoint with authorization code and code_verifier
    let token_response = oauth_token_exchange(
        ctx,
        &realm_id,
        "authorization_code",
        &auth_code,
        redirect_uri,
        "pkce-test-app",
        &code_verifier,
    )
    .await;

    // Then: Token exchange succeeds
    assert_eq!(
        token_response.status(),
        StatusCode::OK,
        "Token exchange should succeed"
    );

    let token_json: Value = response_json(token_response).await;
    assert!(
        token_json["access_token"].is_string(),
        "access_token should be a string"
    );
    assert!(
        !token_json["access_token"].as_str().unwrap().is_empty(),
        "access_token should not be empty"
    );
    assert_eq!(
        token_json["token_type"].as_str(),
        Some("Bearer"),
        "token_type should be 'Bearer'"
    );
    assert!(
        token_json["expires_in"].is_number(),
        "expires_in should be a number"
    );
    assert!(
        token_json["expires_in"].as_i64().unwrap() > 0,
        "expires_in should be positive"
    );

    // Then: access_token is a valid Bearer token
    let access_token = token_json["access_token"].as_str().unwrap();
    let app = ctx.create_unified_test_router();
    let status_request = axum::http::Request::builder()
        .uri("/api/auth/status")
        .header("authorization", format!("Bearer {}", access_token))
        .body(axum::body::Body::empty())
        .unwrap();
    let status_response = app.oneshot(status_request).await.unwrap();
    assert_eq!(status_response.status(), StatusCode::OK);

    let status_json: Value = response_json(status_response).await;
    assert_eq!(
        status_json["authenticated"], true,
        "Session should be authenticated"
    );
}

// =============================================================================
// Test 2: TOTP + OAuth flow
// =============================================================================

// User Story: US-RU-010 (Jump from third-party app to login including TOTP), US-TP-001
// Covers: US-RU-010 scenario 2, US-TP-001 scenario 2 (with TOTP)

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oauth_pkce_totp_flow_success(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let admin_token = setup_admin_session(ctx, "pkce-totp-flow@test.com").await;

    // Set TOTP_SECRET_KEY for TOTP encryption
    unsafe {
        std::env::set_var("TOTP_SECRET_KEY", "test_key_32_bytes_long_1234567890");
    }

    // Given: Enable realm TOTP
    setup_realm_totp_config(ctx, true, false).await;

    // Given: Create Client App
    let redirect_uri = "https://totpapp.com/callback";
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "pkce-totp-app",
        "PKCE TOTP App",
        &[redirect_uri],
    )
    .await;
    assert_eq!(create_response.status(), 201);

    // Given: Create user and enable TOTP
    let email = "pkce-totp-user@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;

    // Login normally first to enable TOTP
    let session_token = login_user(ctx, email, password).await;
    let _totp_secret = enable_totp_for_user(ctx, &session_token, password).await;

    // Given: Generate PKCE parameters and call authorize
    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state = generate_state();

    let authorize_response = oauth_authorize(
        ctx,
        &realm_id,
        "pkce-totp-app",
        redirect_uri,
        &state,
        &code_challenge,
        "S256",
    )
    .await;
    assert_eq!(authorize_response.status(), StatusCode::FOUND);

    // When: Call login with OAuth context for TOTP-enabled user
    let login_response = login_with_oauth(
        ctx,
        &realm_id,
        email,
        password,
        "pkce-totp-app",
        redirect_uri,
        &state,
    )
    .await;

    // Then: Login returns TOTP required response
    assert_eq!(login_response.status(), StatusCode::OK);

    let login_json: Value = response_json(login_response).await;
    assert_eq!(login_json["requiresTotp"].as_bool(), Some(true));
    assert!(
        login_json["tempToken"].is_string(),
        "Should return tempToken for TOTP"
    );
    assert!(
        login_json["redirectTo"].is_null() || login_json["redirectTo"].is_string(),
        "redirectTo should not be set yet (TOTP required)"
    );

    let _temp_token = login_json["tempToken"].as_str().unwrap().to_string();

    // Create a second user to capture the TOTP secret for code generation
    let email2 = "pkce-totp-user2@test.com";
    let _user_id2 = create_test_user(ctx, email2, password).await;
    let session_token2 = login_user(ctx, email2, password).await;
    let totp_secret = enable_totp_for_user(ctx, &session_token2, password).await;

    // Generate new PKCE params for the second user's flow
    let code_verifier2 = generate_code_verifier();
    let code_challenge2 = compute_code_challenge(&code_verifier2);
    let state2 = generate_state();

    let authorize_response2 = oauth_authorize(
        ctx,
        &realm_id,
        "pkce-totp-app",
        redirect_uri,
        &state2,
        &code_challenge2,
        "S256",
    )
    .await;
    assert_eq!(authorize_response2.status(), StatusCode::FOUND);

    let login_response2 = login_with_oauth(
        ctx,
        &realm_id,
        email2,
        password,
        "pkce-totp-app",
        redirect_uri,
        &state2,
    )
    .await;
    assert_eq!(login_response2.status(), StatusCode::OK);

    let login_json2: Value = response_json(login_response2).await;
    assert_eq!(login_json2["requiresTotp"].as_bool(), Some(true));
    let temp_token2 = login_json2["tempToken"].as_str().unwrap().to_string();

    // When: Generate valid TOTP code and verify
    let totp_code = generate_totp_code(&totp_secret);
    let verify_response = verify_totp_with_oauth(ctx, &realm_id, &temp_token2, &totp_code).await;

    // Then: verify_totp succeeds with redirectTo
    assert_eq!(
        verify_response.status(),
        StatusCode::OK,
        "TOTP verify should succeed"
    );

    let verify_json: Value = response_json(verify_response).await;
    let redirect_to = verify_json["redirectTo"]
        .as_str()
        .expect("redirectTo should be present after TOTP verify");

    let auth_code = extract_auth_code_from_redirect(redirect_to)
        .expect("Should extract auth code from redirectTo");

    // When: Call token endpoint with authorization code and code_verifier
    let token_response = oauth_token_exchange(
        ctx,
        &realm_id,
        "authorization_code",
        &auth_code,
        redirect_uri,
        "pkce-totp-app",
        &code_verifier2,
    )
    .await;

    // Then: Token exchange succeeds
    assert_eq!(
        token_response.status(),
        StatusCode::OK,
        "Token exchange should succeed"
    );

    let token_json: Value = response_json(token_response).await;
    assert!(
        token_json["access_token"].is_string(),
        "access_token should be a string"
    );
    assert_eq!(token_json["token_type"].as_str(), Some("Bearer"));
}

// =============================================================================
// Test 3: Normal login regression
// =============================================================================

// User Story: Regression test -- normal login flow completely unaffected by OAuth changes
// Covers: Dev slot handoff constraint: "normal login flow completely unaffected"

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oauth_pkce_normal_login_regression(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();

    // Given: Create a test user (no TOTP, no OAuth)
    let email = "normal-login@test.com";
    let password = "password123";
    let user_id = create_test_user(ctx, email, password).await;

    // When: Call login WITHOUT any OAuth parameters
    let app = ctx.create_unified_test_router();
    let login_payload = serde_json::json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });

    let login_request = axum::http::Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(axum::body::Body::from(login_payload.to_string()))
        .unwrap();

    let login_response = app.clone().oneshot(login_request).await.unwrap();

    // Then: Login succeeds with a browser Bearer token and no cookie.
    assert_eq!(
        login_response.status(),
        StatusCode::OK,
        "Normal login should succeed"
    );

    assert!(
        !login_response
            .headers()
            .contains_key(axum::http::header::SET_COOKIE)
    );
    let (_login_response, token) = crate::tests::extract_bearer_token(login_response).await;
    let token = token.expect("Normal login should return accessToken");

    // Then: Bearer token is valid (status endpoint returns authenticated)
    let status_request = axum::http::Request::builder()
        .uri("/api/auth/status")
        .header("authorization", format!("Bearer {}", token))
        .body(axum::body::Body::empty())
        .unwrap();

    let status_response = app.oneshot(status_request).await.unwrap();
    assert_eq!(status_response.status(), StatusCode::OK);

    let status_json: Value = response_json(status_response).await;
    let status_data = status_json.get("data").unwrap_or(&status_json);
    assert_eq!(
        status_data["authenticated"], true,
        "Should be authenticated"
    );
    assert_eq!(
        status_data["userId"].as_str(),
        Some(user_id.to_string().as_str()),
        "userId should match"
    );
}

// =============================================================================
// Test 4: Authorization code replay rejection
// =============================================================================

// User Story: US-TP-001 (scenario 4), US-TP-006 (scenario 4), US-TP-016 (scenario 5)
// Covers: US-TP-001 acceptance criteria scenario 4, US-TP-006 scenario 4, US-TP-016 scenario 5

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oauth_pkce_code_replay_rejected(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let admin_token = setup_admin_session(ctx, "pkce-replay@test.com").await;

    // Given: Create Client App
    let redirect_uri = "https://replayapp.com/callback";
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "pkce-replay-app",
        "PKCE Replay App",
        &[redirect_uri],
    )
    .await;
    assert_eq!(create_response.status(), 201);

    // Given: Complete full OAuth PKCE flow to get authorization code
    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state = generate_state();

    let _authorize_response = oauth_authorize(
        ctx,
        &realm_id,
        "pkce-replay-app",
        redirect_uri,
        &state,
        &code_challenge,
        "S256",
    )
    .await;

    let email = "pkce-replay-user@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;

    let login_response = login_with_oauth(
        ctx,
        &realm_id,
        email,
        password,
        "pkce-replay-app",
        redirect_uri,
        &state,
    )
    .await;
    assert_eq!(login_response.status(), StatusCode::OK);

    let login_json: Value = response_json(login_response).await;
    let redirect_to = login_json["redirectTo"]
        .as_str()
        .expect("Should have redirectTo");
    let auth_code = extract_auth_code_from_redirect(redirect_to).expect("Should extract code");

    // When: Exchange code for token (first time) -- succeeds
    let first_exchange = oauth_token_exchange(
        ctx,
        &realm_id,
        "authorization_code",
        &auth_code,
        redirect_uri,
        "pkce-replay-app",
        &code_verifier,
    )
    .await;
    assert_eq!(
        first_exchange.status(),
        StatusCode::OK,
        "First exchange should succeed"
    );

    // When: Exchange same code for token again (second time)
    let second_exchange = oauth_token_exchange(
        ctx,
        &realm_id,
        "authorization_code",
        &auth_code,
        redirect_uri,
        "pkce-replay-app",
        &code_verifier,
    )
    .await;

    // Then: Second exchange returns 400 error, authorization code already consumed
    assert_eq!(
        second_exchange.status(),
        StatusCode::BAD_REQUEST,
        "Replayed code should return 400"
    );

    let error_json: Value = response_json(second_exchange).await;
    assert!(
        error_json["error"].is_string() || error_json["message"].is_string(),
        "Response should contain error information"
    );
}

// =============================================================================
// Test 5: PKCE verification failure (verifier mismatch)
// =============================================================================

// User Story: US-TP-001 (scenario 6), US-TP-016 (scenario 2)
// Covers: US-TP-001 acceptance criteria scenario 6, US-TP-016 scenario 2

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oauth_pkce_verifier_mismatch_rejected(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let admin_token = setup_admin_session(ctx, "pkce-verifier-mismatch@test.com").await;

    // Given: Create Client App
    let redirect_uri = "https://verifierapp.com/callback";
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "pkce-verifier-app",
        "PKCE Verifier App",
        &[redirect_uri],
    )
    .await;
    assert_eq!(create_response.status(), 201);

    // Given: Authorize with code_verifier_A's challenge
    let code_verifier_a = generate_code_verifier();
    let code_challenge_a = compute_code_challenge(&code_verifier_a);
    let state = generate_state();

    let _authorize_response = oauth_authorize(
        ctx,
        &realm_id,
        "pkce-verifier-app",
        redirect_uri,
        &state,
        &code_challenge_a,
        "S256",
    )
    .await;

    let email = "pkce-verifier-user@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;

    let login_response = login_with_oauth(
        ctx,
        &realm_id,
        email,
        password,
        "pkce-verifier-app",
        redirect_uri,
        &state,
    )
    .await;
    assert_eq!(login_response.status(), StatusCode::OK);

    let login_json: Value = response_json(login_response).await;
    let redirect_to = login_json["redirectTo"]
        .as_str()
        .expect("Should have redirectTo");
    let auth_code = extract_auth_code_from_redirect(redirect_to).expect("Should extract code");

    // When: Call token endpoint with code_verifier_B (different from A)
    let code_verifier_b = generate_code_verifier();
    assert_ne!(
        code_verifier_a, code_verifier_b,
        "Verifiers should be different"
    );

    let token_response = oauth_token_exchange(
        ctx,
        &realm_id,
        "authorization_code",
        &auth_code,
        redirect_uri,
        "pkce-verifier-app",
        &code_verifier_b,
    )
    .await;

    // Then: Token exchange returns 400 error, PKCE verification failed
    assert_eq!(
        token_response.status(),
        StatusCode::BAD_REQUEST,
        "PKCE mismatch should return 400"
    );

    let error_json: Value = response_json(token_response).await;
    let error_msg = error_json["error"]
        .as_str()
        .or_else(|| error_json["message"].as_str())
        .unwrap_or("");
    assert!(
        error_msg.to_lowercase().contains("pkce")
            || error_msg.to_lowercase().contains("verification"),
        "Error should mention PKCE verification failure, got: {}",
        error_msg
    );
}

// =============================================================================
// Test 6: State mismatch / missing state
// =============================================================================

// User Story: US-TP-001 (scenario 7)
// Covers: US-TP-001 acceptance criteria scenario 7

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oauth_pkce_state_mismatch_rejected(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let admin_token = setup_admin_session(ctx, "pkce-state-mismatch@test.com").await;

    // Given: Create Client App
    let redirect_uri = "https://stateapp.com/callback";
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "pkce-state-app",
        "PKCE State App",
        &[redirect_uri],
    )
    .await;
    assert_eq!(create_response.status(), 201);

    // Given: Call authorize with one state value
    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state_original = generate_state();

    let _authorize_response = oauth_authorize(
        ctx,
        &realm_id,
        "pkce-state-app",
        redirect_uri,
        &state_original,
        &code_challenge,
        "S256",
    )
    .await;

    let email = "pkce-state-user@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;

    // When: Call login with a DIFFERENT state value
    let state_different = generate_state();
    assert_ne!(
        state_original, state_different,
        "States should be different"
    );

    let login_response = login_with_oauth(
        ctx,
        &realm_id,
        email,
        password,
        "pkce-state-app",
        redirect_uri,
        &state_different,
    )
    .await;

    // Then: Login returns 400 error, state validation fails
    assert_eq!(
        login_response.status(),
        StatusCode::BAD_REQUEST,
        "State mismatch should return 400"
    );

    let error_json: Value = response_json(login_response).await;
    assert!(
        error_json["error"].is_string() || error_json["message"].is_string(),
        "Response should contain error information"
    );
}

// =============================================================================
// Test 7: Redirect URI not whitelisted
// =============================================================================

// User Story: US-TP-001 (scenario 8), US-TP-008 (scenario 6)
// Covers: US-TP-001 acceptance criteria scenario 8, US-TP-008 scenario 6

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oauth_pkce_redirect_uri_not_whitelisted_rejected(
    ctx: &mut SchemaTestContext,
) {
    let realm_id = ctx._realm_id.clone();
    let admin_token = setup_admin_session(ctx, "pkce-redirect-whitelist@test.com").await;

    // Given: Client App with redirect_uri whitelist ["https://myapp.com/callback"]
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "pkce-whitelist-app",
        "PKCE Whitelist App",
        &["https://myapp.com/callback"],
    )
    .await;
    assert_eq!(create_response.status(), 201);

    // When: Call authorize with redirect_uri=https://evil.com/callback
    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state = generate_state();

    let authorize_response = oauth_authorize(
        ctx,
        &realm_id,
        "pkce-whitelist-app",
        "https://evil.com/callback",
        &state,
        &code_challenge,
        "S256",
    )
    .await;

    // Then: Authorize returns 400 error, redirect URI not in whitelist
    assert_eq!(
        authorize_response.status(),
        StatusCode::BAD_REQUEST,
        "Non-whitelisted redirect URI should return 400"
    );

    let error_json: Value = response_json(authorize_response).await;
    let error_msg = error_json["error"]
        .as_str()
        .or_else(|| error_json["message"].as_str())
        .unwrap_or("");
    assert!(
        error_msg.to_lowercase().contains("whitelist")
            || error_msg.to_lowercase().contains("redirect"),
        "Error should mention whitelist/redirect URI, got: {}",
        error_msg
    );
}

// =============================================================================
// Test 8: Redirect URI prefix bypass rejected
// =============================================================================

// User Story: US-TP-001 (scenario 9)
// Covers: US-TP-001 acceptance criteria scenario 9 (exact match, no prefix matching)

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oauth_pkce_redirect_uri_prefix_bypass_rejected(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let admin_token = setup_admin_session(ctx, "pkce-prefix-bypass@test.com").await;

    // Given: Client App with whitelist ["https://myapp.com/callback"]
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "pkce-prefix-app",
        "PKCE Prefix App",
        &["https://myapp.com/callback"],
    )
    .await;
    assert_eq!(create_response.status(), 201);

    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state = generate_state();

    // When: Call authorize with redirect_uri that has extra path (prefix of whitelisted URI)
    let authorize_response_subpath = oauth_authorize(
        ctx,
        &realm_id,
        "pkce-prefix-app",
        "https://myapp.com/callback/extra",
        &state,
        &code_challenge,
        "S256",
    )
    .await;

    // Then: Authorize returns 400 error
    assert_eq!(
        authorize_response_subpath.status(),
        StatusCode::BAD_REQUEST,
        "Subpath redirect URI should be rejected (exact match required)"
    );

    // When: Call authorize with redirect_uri that has domain-like bypass
    let state2 = generate_state();
    let authorize_response_domain = oauth_authorize(
        ctx,
        &realm_id,
        "pkce-prefix-app",
        "https://myapp.com.evil.com/callback",
        &state2,
        &code_challenge,
        "S256",
    )
    .await;

    // Then: Authorize returns 400 error
    assert_eq!(
        authorize_response_domain.status(),
        StatusCode::BAD_REQUEST,
        "Domain bypass redirect URI should be rejected (exact match required)"
    );
}

// =============================================================================
// Test 9: Unsupported code_challenge_method rejected
// =============================================================================

// User Story: US-TP-001 (scenario 1 -- only S256 supported)
// Covers: US-TP-001 acceptance criteria scenario 1 (only S256 code_challenge_method supported)

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oauth_pkce_unsupported_challenge_method_rejected(
    ctx: &mut SchemaTestContext,
) {
    let realm_id = ctx._realm_id.clone();
    let admin_token = setup_admin_session(ctx, "pkce-challenge-method@test.com").await;

    // Given: Create Client App
    let redirect_uri = "https://methodapp.com/callback";
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "pkce-method-app",
        "PKCE Method App",
        &[redirect_uri],
    )
    .await;
    assert_eq!(create_response.status(), 201);

    // When: Call authorize with code_challenge_method="plain" (not S256)
    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state = generate_state();

    let authorize_response = oauth_authorize(
        ctx,
        &realm_id,
        "pkce-method-app",
        redirect_uri,
        &state,
        &code_challenge,
        "plain",
    )
    .await;

    // Then: Authorize returns 400 error, unsupported code_challenge_method
    assert_eq!(
        authorize_response.status(),
        StatusCode::BAD_REQUEST,
        "Unsupported code_challenge_method should return 400"
    );

    let error_json: Value = response_json(authorize_response).await;
    let error_msg = error_json["error"]
        .as_str()
        .or_else(|| error_json["message"].as_str())
        .unwrap_or("");
    assert!(
        error_msg.to_lowercase().contains("unsupported")
            || error_msg.to_lowercase().contains("code_challenge_method"),
        "Error should mention unsupported code_challenge_method, got: {}",
        error_msg
    );
}

// =============================================================================
// Test 10: response_type=token rejected
// =============================================================================

// User Story: US-TP-001 (Implicit Flow replaced by Authorization Code + PKCE)
// Covers: Design constraint -- response_type=token must be rejected as Implicit Flow is removed

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oauth_pkce_response_type_token_rejected(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let admin_token = setup_admin_session(ctx, "pkce-response-type@test.com").await;

    // Given: Create Client App
    let redirect_uri = "https://responsetypeapp.com/callback";
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "pkce-resptype-app",
        "PKCE RespType App",
        &[redirect_uri],
    )
    .await;
    assert_eq!(create_response.status(), 201);

    // When: Call authorize with response_type=token
    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state = generate_state();

    let app = ctx.create_unified_test_router();
    let query = format!(
        "/api/oauth/{}/authorize?client_id={}&redirect_uri={}&state={}&response_type=token&code_challenge={}&code_challenge_method=S256",
        realm_id,
        urlencoding::encode("pkce-resptype-app"),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(&state),
        urlencoding::encode(&code_challenge),
    );

    let request = axum::http::Request::builder()
        .method("GET")
        .uri(&query)
        .body(axum::body::Body::empty())
        .unwrap();

    let authorize_response = app.oneshot(request).await.unwrap();

    // Then: Authorize returns 400 error, only `code` is accepted
    assert_eq!(
        authorize_response.status(),
        StatusCode::BAD_REQUEST,
        "response_type=token should return 400"
    );

    let error_json: Value = response_json(authorize_response).await;
    let error_msg = error_json["error"]
        .as_str()
        .or_else(|| error_json["message"].as_str())
        .unwrap_or("");
    assert!(
        error_msg.to_lowercase().contains("response_type"),
        "Error should mention response_type, got: {}",
        error_msg
    );
}

// =============================================================================
// Test 11: Client_id / redirect_uri mismatch at token endpoint
// =============================================================================

// User Story: US-TP-016 (scenarios 3-4)
// Covers: US-TP-016 acceptance criteria scenarios 3, 4

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oauth_pkce_token_client_mismatch_rejected(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let admin_token = setup_admin_session(ctx, "pkce-client-mismatch@test.com").await;

    // Given: Create Client App A
    let redirect_uri_a = "https://clienta.com/callback";
    let create_response_a = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "pkce-client-a",
        "PKCE Client A",
        &[redirect_uri_a],
    )
    .await;
    assert_eq!(create_response_a.status(), 201);

    // Given: Create Client App B
    let redirect_uri_b = "https://clientb.com/callback";
    let create_response_b = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "pkce-client-b",
        "PKCE Client B",
        &[redirect_uri_b],
    )
    .await;
    assert_eq!(create_response_b.status(), 201);

    // Given: Complete authorize + login to get authorization code for client_id=A, redirect_uri=X
    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state = generate_state();

    let _authorize_response = oauth_authorize(
        ctx,
        &realm_id,
        "pkce-client-a",
        redirect_uri_a,
        &state,
        &code_challenge,
        "S256",
    )
    .await;

    let email = "pkce-mismatch-user@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;

    let login_response = login_with_oauth(
        ctx,
        &realm_id,
        email,
        password,
        "pkce-client-a",
        redirect_uri_a,
        &state,
    )
    .await;
    assert_eq!(login_response.status(), StatusCode::OK);

    let login_json: Value = response_json(login_response).await;
    let redirect_to = login_json["redirectTo"]
        .as_str()
        .expect("Should have redirectTo");
    let auth_code = extract_auth_code_from_redirect(redirect_to).expect("Should extract code");

    // When: Call token endpoint with client_id=B (different from A)
    let token_response_wrong_client = oauth_token_exchange(
        ctx,
        &realm_id,
        "authorization_code",
        &auth_code,
        redirect_uri_a,
        "pkce-client-b", // wrong client_id
        &code_verifier,
    )
    .await;

    // Then: Token exchange returns 400 error, client_id mismatch
    assert_eq!(
        token_response_wrong_client.status(),
        StatusCode::BAD_REQUEST,
        "client_id mismatch should return 400"
    );

    let error_json: Value = response_json(token_response_wrong_client).await;
    let error_msg = error_json["error"]
        .as_str()
        .or_else(|| error_json["message"].as_str())
        .unwrap_or("");
    assert!(
        error_msg.to_lowercase().contains("client_id")
            || error_msg.to_lowercase().contains("mismatch"),
        "Error should mention client_id mismatch, got: {}",
        error_msg
    );

    // The code was consumed by the GETDEL above, so we need a new flow for redirect_uri test.
    // Create new authorization code for redirect_uri mismatch test.
    let code_verifier2 = generate_code_verifier();
    let code_challenge2 = compute_code_challenge(&code_verifier2);
    let state2 = generate_state();

    let _authorize_response2 = oauth_authorize(
        ctx,
        &realm_id,
        "pkce-client-a",
        redirect_uri_a,
        &state2,
        &code_challenge2,
        "S256",
    )
    .await;

    let login_response2 = login_with_oauth(
        ctx,
        &realm_id,
        email,
        password,
        "pkce-client-a",
        redirect_uri_a,
        &state2,
    )
    .await;
    assert_eq!(login_response2.status(), StatusCode::OK);

    let login_json2: Value = response_json(login_response2).await;
    let redirect_to2 = login_json2["redirectTo"]
        .as_str()
        .expect("Should have redirectTo");
    let auth_code2 = extract_auth_code_from_redirect(redirect_to2).expect("Should extract code");

    // When: Call token endpoint with redirect_uri=Y (different from X)
    let token_response_wrong_uri = oauth_token_exchange(
        ctx,
        &realm_id,
        "authorization_code",
        &auth_code2,
        redirect_uri_b, // wrong redirect_uri
        "pkce-client-a",
        &code_verifier2,
    )
    .await;

    // Then: Token exchange returns 400 error, redirect_uri mismatch
    assert_eq!(
        token_response_wrong_uri.status(),
        StatusCode::BAD_REQUEST,
        "redirect_uri mismatch should return 400"
    );

    let error_json2: Value = response_json(token_response_wrong_uri).await;
    let error_msg2 = error_json2["error"]
        .as_str()
        .or_else(|| error_json2["message"].as_str())
        .unwrap_or("");
    assert!(
        error_msg2.to_lowercase().contains("redirect_uri")
            || error_msg2.to_lowercase().contains("mismatch"),
        "Error should mention redirect_uri mismatch, got: {}",
        error_msg2
    );
}

// =============================================================================
// Test 12: Authorization code expiry rejected
// =============================================================================

// User Story: US-TP-001 (scenario 5)
// Covers: US-TP-001 acceptance criteria scenario 5

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oauth_pkce_code_expired_rejected(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let admin_token = setup_admin_session(ctx, "pkce-code-expired@test.com").await;

    // Given: Create Client App
    let redirect_uri = "https://expiredapp.com/callback";
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "pkce-expired-app",
        "PKCE Expired App",
        &[redirect_uri],
    )
    .await;
    assert_eq!(create_response.status(), 201);

    // Given: Complete authorize + login to get authorization code
    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state = generate_state();

    let _authorize_response = oauth_authorize(
        ctx,
        &realm_id,
        "pkce-expired-app",
        redirect_uri,
        &state,
        &code_challenge,
        "S256",
    )
    .await;

    let email = "pkce-expired-user@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;

    let login_response = login_with_oauth(
        ctx,
        &realm_id,
        email,
        password,
        "pkce-expired-app",
        redirect_uri,
        &state,
    )
    .await;
    assert_eq!(login_response.status(), StatusCode::OK);

    let login_json: Value = response_json(login_response).await;
    let redirect_to = login_json["redirectTo"]
        .as_str()
        .expect("Should have redirectTo");
    let auth_code = extract_auth_code_from_redirect(redirect_to).expect("Should extract code");

    // Given: Delete oauth:code:{code} from Redis to simulate expiry
    delete_oauth_code_redis(ctx, &auth_code).await;

    // When: Call token endpoint with the expired code and valid code_verifier
    let token_response = oauth_token_exchange(
        ctx,
        &realm_id,
        "authorization_code",
        &auth_code,
        redirect_uri,
        "pkce-expired-app",
        &code_verifier,
    )
    .await;

    // Then: Token exchange returns 400 error, authorization code expired or invalid
    assert_eq!(
        token_response.status(),
        StatusCode::BAD_REQUEST,
        "Expired code should return 400"
    );

    let error_json: Value = response_json(token_response).await;
    let error_msg = error_json["error"]
        .as_str()
        .or_else(|| error_json["message"].as_str())
        .unwrap_or("");
    assert!(
        error_msg.to_lowercase().contains("expired")
            || error_msg.to_lowercase().contains("invalid")
            || error_msg.to_lowercase().contains("authorization code"),
        "Error should mention expired/invalid authorization code, got: {}",
        error_msg
    );
}

/// 回归（审计 run-2：oauth-state-seeding-unbounded-state-and-code-challenge）：
/// authorize 曾只对 scope/nonce 设 4096 字节上限 —— state 直接成为 Redis
/// 键名、code_challenge 原样进入存储值，未认证调用者可将攻击者体量的
/// 字节写入共享 Redis（300s TTL）。修复后：state 与 code_challenge 同样
/// 受 OAUTH_AUTHORIZE_EXTRA_PARAM_MAX_BYTES 限制，超限 400。
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oauth_pkce_oversized_state_and_challenge_rejected(
    ctx: &mut SchemaTestContext,
) {
    let realm_id = ctx._realm_id.clone();
    let admin_token = setup_admin_session(ctx, "pkce-oversized@test.com").await;

    let redirect_uri = "https://oversizedapp.com/callback";
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "pkce-oversized-app",
        "PKCE Oversized App",
        &[redirect_uri],
    )
    .await;
    assert_eq!(create_response.status(), 201);

    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let app = ctx.create_unified_test_router();

    let authorize_uri = |state: &str, challenge: &str| {
        format!(
            "/api/oauth/{}/authorize?client_id={}&redirect_uri={}&state={}&response_type=code&code_challenge={}&code_challenge_method=S256",
            realm_id,
            urlencoding::encode("pkce-oversized-app"),
            urlencoding::encode(redirect_uri),
            urlencoding::encode(state),
            urlencoding::encode(challenge),
        )
    };

    // 8KB state（超过 4096 上限）：必须 400，而不是成为 300s 的 Redis 键。
    let oversized_state = "s".repeat(8192);
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri(authorize_uri(&oversized_state, &code_challenge))
                .header("x-forwarded-for", "5.5.5.5")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        axum::http::StatusCode::BAD_REQUEST,
        "an oversized state must be rejected, not seeded into Redis"
    );

    // 6KB code_challenge（超过 4096 上限）：同样 400。
    let oversized_challenge = "c".repeat(6000);
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri(authorize_uri(&generate_state(), &oversized_challenge))
                .header("x-forwarded-for", "5.5.5.6")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        axum::http::StatusCode::BAD_REQUEST,
        "an oversized code_challenge must be rejected"
    );

    // 对照：正常大小参数仍然 302 进入登录页。
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri(authorize_uri(&generate_state(), &code_challenge))
                .header("x-forwarded-for", "5.5.5.7")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::FOUND);
}

// =============================================================================
// MCP resource/scope scenarios (discovery, registration, authorize/token,
// V2 five-login-entrance preservation)
// =============================================================================
//
// User Story: docs/user-stories/integration/mcp-server.md US-MCP-001
// (agent 客户端经浏览器授权接入 Herald：PRM/AS 发现 → 受控注册取得
// client_id → authorize(resource/scope/PKCE) → 登录 → code → token)。
// 每个 WHY 注释说明被防住的回归；断言面向状态码/错误码/头/集合，不做
// 仅 200 的弱断言。
// =============================================================================

/// The MCP client's canonical loopback callback used by the V2 chains: an
/// arbitrary port on the registered 127.0.0.1 template (RFC 8252 §7.3
/// dynamic-port rule under test).
const MCP_TEST_REDIRECT_URI: &str = "http://127.0.0.1:43119/callback";

/// Sort a space-delimited scope wire string so set-equality can be asserted
/// order-independently (the registration endpoint returns a joined string,
/// not an array).
fn sorted_scope_tokens(scope: &str) -> Vec<String> {
    let mut tokens: Vec<String> = scope.split_whitespace().map(str::to_string).collect();
    tokens.sort();
    tokens
}

/// Extract the `ac_*` code from an entrance's redirectTo, exchange it at
/// /token repeating the canonical MCP resource, and assert the MCP token
/// contract: 200, Bearer tokens, the exact stable-sorted scope wire, the
/// RFC 6749 §5.1 no-store headers, and no id_token. Returns the token body.
///
/// WHY this helper exists: every V2 entrance must prove the resource/scope
/// survived state → code → token; a shared exchange-and-assert keeps the
/// five entrances from drifting into five different (weaker) assertion sets.
async fn exchange_mcp_code_and_assert(
    ctx: &SchemaTestContext,
    realm_id: &str,
    redirect_to: &str,
    redirect_uri: &str,
    code_verifier: &str,
    expected_scope: &str,
) -> Value {
    let auth_code = extract_auth_code_from_redirect(redirect_to)
        .expect("MCP login redirectTo must carry an authorization code");
    assert!(
        auth_code.starts_with("ac_"),
        "MCP auth code must use the ac_ prefix, got {auth_code}"
    );

    let response = oauth_token_exchange_with_resource(
        ctx,
        realm_id,
        &auth_code,
        redirect_uri,
        MCP_CLIENT_ID,
        code_verifier,
        Some(&mcp_canonical_resource(ctx, realm_id)),
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "MCP token exchange repeating the code's resource must succeed"
    );
    // RFC 6749 §5.1: token responses must not be stored — losing these
    // headers would let intermediaries cache credentials.
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok()),
        Some("no-store"),
        "MCP token response must carry Cache-Control: no-store"
    );
    assert_eq!(
        response
            .headers()
            .get("pragma")
            .and_then(|v| v.to_str().ok()),
        Some("no-cache"),
        "MCP token response must carry Pragma: no-cache"
    );

    let body: Value = response_json(response).await;
    assert!(
        body["access_token"].as_str().is_some_and(|t| !t.is_empty()),
        "MCP token response must carry a non-empty access_token"
    );
    assert!(
        body["refresh_token"]
            .as_str()
            .is_some_and(|t| !t.is_empty()),
        "MCP token response must carry a refresh_token (standard refresh is the only MCP rotation path)"
    );
    assert_eq!(
        body["token_type"].as_str(),
        Some("Bearer"),
        "token_type must be Bearer"
    );
    assert_eq!(
        body["scope"].as_str(),
        Some(expected_scope),
        "granted scope must be the stable-sorted wire form of the authorized set"
    );
    assert!(
        body.get("id_token").is_none(),
        "the MCP token response must never carry an id_token (self face, not OIDC)"
    );
    body
}

/// Drive the password entrance's MCP chain (authorize → login → code) and
/// return `(auth_code, code_verifier)` for the caller's exchange assertions.
///
/// WHY a named helper: the token-contract scenario needs several codes from
/// the same entrance; a helper keeps every negative branch on the identical
/// happy-path prefix so failures isolate to the branch under test.
async fn mcp_password_login_to_code(
    ctx: &SchemaTestContext,
    realm_id: &str,
    email: &str,
    password: &str,
    scope: Option<&str>,
) -> (String, String) {
    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state = generate_state();
    let authorize = oauth_authorize_with_mcp(
        ctx,
        realm_id,
        MCP_CLIENT_ID,
        MCP_TEST_REDIRECT_URI,
        &state,
        &code_challenge,
        "S256",
        scope,
        Some(&mcp_canonical_resource(ctx, realm_id)),
    )
    .await;
    assert_eq!(
        authorize.status(),
        StatusCode::FOUND,
        "MCP authorize must reach the login page"
    );
    let login = login_with_oauth(
        ctx,
        realm_id,
        email,
        password,
        MCP_CLIENT_ID,
        MCP_TEST_REDIRECT_URI,
        &state,
    )
    .await;
    assert_eq!(login.status(), StatusCode::OK);
    let login_json: Value = response_json(login).await;
    let redirect_to = login_json["redirectTo"]
        .as_str()
        .expect("MCP password login must issue a downstream code");
    let code =
        extract_auth_code_from_redirect(redirect_to).expect("redirectTo must carry the code");
    (code, code_verifier)
}

// -----------------------------------------------------------------------------
// Test 13: RFC 9728 PRM + RFC 8414 AS metadata + OIDC capability sharing
// -----------------------------------------------------------------------------

// User Story: docs/user-stories/integration/mcp-server.md US-MCP-001
// Covers: the discovery entry points an MCP client hits first. WHY pin every
// field: PRM/AS metadata is the machine-read bootstrap contract — a client
// derives the canonical resource, the issuer, and the registration endpoint
// from it, so a drifted field value breaks URL-only clients silently rather
// than loudly.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_mcp_discovery_metadata_contract(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let issuer = format!("http://localhost:8080/api/oauth/{realm_id}");

    // --- PRM (RFC 9728) ---
    let prm_response = fetch_mcp_protected_resource_metadata(ctx, &realm_id).await;
    assert_eq!(prm_response.status(), StatusCode::OK, "PRM must be served");
    let prm: Value = response_json(prm_response).await;
    assert_eq!(
        prm["resource"].as_str(),
        Some(mcp_canonical_resource(ctx, &realm_id).as_str()),
        "PRM must publish the realm's canonical MCP resource URI"
    );
    assert_eq!(
        prm["authorization_servers"],
        json!([issuer]),
        "PRM must point at exactly this realm's path-style issuer"
    );
    let prm_scopes: Vec<&str> = prm["scopes_supported"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s.as_str())
        .collect();
    assert_eq!(
        sorted_scope_tokens(&prm_scopes.join(" ")),
        vec![
            "mcp:points:read",
            "mcp:profile:read",
            "mcp:subscriptions:read",
            "mcp:transactions:read",
        ],
        "PRM must advertise exactly the four MCP self scopes (no openid — PRM is not OIDC)"
    );
    assert_eq!(
        prm["bearer_methods_supported"],
        json!(["header"]),
        "the MCP endpoint only accepts the Authorization header"
    );
    assert_eq!(
        prm["resource_name"].as_str(),
        Some("Herald MCP"),
        "PRM resource_name is the human-readable label MCP clients display"
    );

    // PRM for an unknown realm: 404, not a document for some other realm.
    let unknown = fetch_mcp_protected_resource_metadata(ctx, "no-such-realm").await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);

    // --- AS metadata (RFC 8414 path-insertion) ---
    let as_response = fetch_mcp_as_metadata(ctx, &realm_id).await;
    assert_eq!(
        as_response.status(),
        StatusCode::OK,
        "AS metadata must be served at the RFC 8414 well-known path"
    );
    let as_metadata: Value = response_json(as_response).await;
    assert_eq!(as_metadata["issuer"].as_str(), Some(issuer.as_str()));
    assert_eq!(
        as_metadata["authorization_endpoint"].as_str(),
        Some(format!("{issuer}/authorize").as_str())
    );
    assert_eq!(
        as_metadata["token_endpoint"].as_str(),
        Some(format!("{issuer}/token").as_str())
    );
    // DEC-007: mainstream clients refuse to connect without a registration
    // endpoint, and it must resolve to the gated pass-through registrar.
    assert_eq!(
        as_metadata["registration_endpoint"].as_str(),
        Some(format!("{issuer}/mcp/register").as_str()),
        "registration_endpoint must point at the pass-through MCP registrar"
    );
    let as_scopes: Vec<&str> = as_metadata["scopes_supported"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s.as_str())
        .collect();
    assert_eq!(
        sorted_scope_tokens(&as_scopes.join(" ")),
        vec![
            "mcp:points:read",
            "mcp:profile:read",
            "mcp:subscriptions:read",
            "mcp:transactions:read",
            "openid",
        ],
        "AS metadata must advertise openid plus the four MCP scopes"
    );
    assert_eq!(
        as_metadata["response_types_supported"],
        json!(["code"]),
        "only the authorization code flow exists"
    );
    let grant_types: Vec<&str> = as_metadata["grant_types_supported"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s.as_str())
        .collect();
    assert!(grant_types.contains(&"authorization_code"));
    assert!(
        grant_types.contains(&"refresh_token"),
        "AS metadata must advertise the standard refresh grant (MCP-only this round)"
    );
    assert_eq!(
        as_metadata["token_endpoint_auth_methods_supported"],
        json!(["none"]),
        "/token is a PKCE public-client endpoint and never validates a secret"
    );
    assert_eq!(
        as_metadata["code_challenge_methods_supported"],
        json!(["S256"]),
        "PKCE S256 is mandatory"
    );

    // AS metadata for an unknown realm: 404.
    let unknown_as = fetch_mcp_as_metadata(ctx, "no-such-realm").await;
    assert_eq!(unknown_as.status(), StatusCode::NOT_FOUND);

    // --- OIDC discovery (legacy append form) shares capabilities, no regress ---
    let oidc_response = fetch_oidc_discovery_document(ctx, &realm_id).await;
    assert_eq!(
        oidc_response.status(),
        StatusCode::OK,
        "the legacy OIDC discovery URL must keep working"
    );
    let oidc: Value = response_json(oidc_response).await;
    assert_eq!(oidc["issuer"].as_str(), Some(issuer.as_str()));
    // The MCP-era capabilities must be shared into the OIDC document...
    let oidc_scopes: Vec<&str> = oidc["scopes_supported"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s.as_str())
        .collect();
    assert_eq!(
        sorted_scope_tokens(&oidc_scopes.join(" ")),
        vec![
            "mcp:points:read",
            "mcp:profile:read",
            "mcp:subscriptions:read",
            "mcp:transactions:read",
            "openid",
        ],
        "OIDC discovery must advertise the same scope set as the AS metadata"
    );
    let oidc_grants: Vec<&str> = oidc["grant_types_supported"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s.as_str())
        .collect();
    assert!(
        oidc_grants.contains(&"refresh_token"),
        "OIDC discovery must not hide the refresh grant capability"
    );
    assert_eq!(
        oidc["token_endpoint_auth_methods_supported"],
        json!(["none"]),
        "OIDC discovery must declare the public-client auth method"
    );
    // ...while the OIDC-only fields stay (no capability regression).
    for field in [
        "userinfo_endpoint",
        "jwks_uri",
        "claims_supported",
        "subject_type",
        "id_token_signing_alg_values_supported",
    ] {
        assert!(
            oidc[field].is_array() || oidc[field].is_string(),
            "OIDC discovery must keep the {field} field"
        );
    }
}

// -----------------------------------------------------------------------------
// Test 14: gated pass-through client registration (DEC-007)
// -----------------------------------------------------------------------------

// User Story: docs/user-stories/integration/mcp-server.md US-MCP-001
// Covers: the pass-through DCR gate. WHY the gate matters: the endpoint hands
// out a client identity, so only requests shaped like a genuine MCP desktop
// client (loopback /callback) may receive it, and the response must be the
// preset public client — never a secret, never per-registration state.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_mcp_registration_pass_through_contract(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();

    let valid = json!({
        "client_name": "Claude Code",
        "redirect_uris": [
            "http://127.0.0.1:43119/callback",
            "http://localhost:8080/callback",
        ],
    });

    // 201 full shape (set assertions: order is not the contract).
    let first = mcp_register_client(ctx, &realm_id, &valid, "application/json").await;
    assert_eq!(
        first.status(),
        StatusCode::CREATED,
        "a loopback-shaped registration must be accepted"
    );
    let body: Value = response_json(first).await;
    assert_eq!(body["client_id"].as_str(), Some("herald-mcp"));
    assert_eq!(body["client_name"].as_str(), Some("Herald MCP"));
    assert_eq!(
        body["redirect_uris"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        MCP_LOOPBACK_REDIRECT_TEMPLATES
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        "registration must return the three port-free loopback templates"
    );
    assert_eq!(
        body["grant_types"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        ["authorization_code", "refresh_token"]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        "registration must advertise both MCP grants"
    );
    assert_eq!(
        body["response_types"],
        json!(["code"]),
        "registration must advertise the code response type"
    );
    assert_eq!(
        body["token_endpoint_auth_method"].as_str(),
        Some("none"),
        "the MCP client is a PKCE public client"
    );
    assert_eq!(
        sorted_scope_tokens(body["scope"].as_str().unwrap()),
        vec![
            "mcp:points:read",
            "mcp:profile:read",
            "mcp:subscriptions:read",
            "mcp:transactions:read",
        ],
        "registration scope must be the four MCP scopes"
    );
    // No secret, no RFC 7592 management handles, no expiry — the endpoint is
    // a stateless read equivalent and must not pretend otherwise.
    for absent in [
        "client_secret",
        "registration_client_uri",
        "client_id_expires_at",
    ] {
        assert!(
            body.get(absent).is_none(),
            "registration response must not carry {absent}"
        );
    }

    // Idempotent: a second gated-in registration receives the same client.
    let second = mcp_register_client(ctx, &realm_id, &valid, "application/json").await;
    assert_eq!(second.status(), StatusCode::CREATED);
    let second_body: Value = response_json(second).await;
    assert_eq!(
        second_body["client_id"], body["client_id"],
        "every valid registration returns the same preset client"
    );

    // Unknown/extra RFC 7591 fields are accepted and ignored — real clients
    // send heterogeneous metadata sets.
    let with_extras = json!({
        "client_name": "VS Code",
        "redirect_uris": ["http://[::1]:65535/callback"],
        "software_id": "vscode",
        "software_version": "1.95",
        "token_endpoint_auth_method": "none",
        "grant_types": ["authorization_code", "refresh_token"],
    });
    let extras = mcp_register_client(ctx, &realm_id, &with_extras, "application/json").await;
    assert_eq!(
        extras.status(),
        StatusCode::CREATED,
        "unknown RFC 7591 fields must not be rejected"
    );

    // Gate: missing client_name.
    let missing_name = json!({ "redirect_uris": ["http://127.0.0.1/callback"] });
    let resp = mcp_register_client(ctx, &realm_id, &missing_name, "application/json").await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let error: Value = response_json(resp).await;
    assert_eq!(
        error["error"].as_str(),
        Some("invalid_client_metadata"),
        "a missing client_name must be an OAuth-shaped invalid_client_metadata"
    );
    assert!(
        error["error_description"]
            .as_str()
            .is_some_and(|d| !d.is_empty()),
        "the error must carry a non-empty description"
    );

    // Gate: empty redirect_uris.
    let empty_redirects = json!({ "client_name": "x", "redirect_uris": [] });
    let resp = mcp_register_client(ctx, &realm_id, &empty_redirects, "application/json").await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let error: Value = response_json(resp).await;
    assert_eq!(error["error"].as_str(), Some("invalid_client_metadata"));

    // Gate: every redirect entry must be a loopback /callback — an https
    // callback pointed at an attacker server must never obtain the identity.
    for bad_redirect in [
        "https://evil.example/cb",
        "http://127.0.0.1:43119/callback?x=1",
        "http://user@127.0.0.1:43119/callback",
        "http://evil.example/callback",
    ] {
        let bad = json!({ "client_name": "probe", "redirect_uris": [bad_redirect] });
        let resp = mcp_register_client(ctx, &realm_id, &bad, "application/json").await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "non-loopback/malformed redirect {bad_redirect} must be rejected"
        );
        let error: Value = response_json(resp).await;
        assert_eq!(
            error["error"].as_str(),
            Some("invalid_redirect_uri"),
            "redirect-shape rejections must use invalid_redirect_uri ({bad_redirect})"
        );
    }

    // Gate: JSON only per RFC 7591 §1.2 — a form body must not parse as one.
    let resp =
        mcp_register_client(ctx, &realm_id, &valid, "application/x-www-form-urlencoded").await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let error: Value = response_json(resp).await;
    assert_eq!(error["error"].as_str(), Some("invalid_client_metadata"));

    // Gate: the 4 KiB body ceiling is a transport limit (413), not invalid
    // metadata — an oversized payload never reaches the metadata parser.
    let oversize = json!({
        "client_name": "oversize",
        "redirect_uris": ["http://localhost:1/callback"],
        "padding": "x".repeat(5 * 1024),
    });
    let resp = mcp_register_client(ctx, &realm_id, &oversize, "application/json").await;
    assert_eq!(
        resp.status(),
        StatusCode::PAYLOAD_TOO_LARGE,
        "a body over the 4 KiB ceiling must be refused as 413"
    );

    // Unknown realm: 404 before any client information is handed out.
    let resp = mcp_register_client(ctx, "no-such-realm", &valid, "application/json").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// -----------------------------------------------------------------------------
// Test 15: authorize MCP contract (resource/scope/loopback/state)
// -----------------------------------------------------------------------------

// User Story: docs/user-stories/integration/mcp-server.md US-MCP-001
// Covers: the authorize gate for the MCP client. WHY each branch: the resource
// is the audience binding (a wrong target must reach the agent client as a
// standard OAuth error redirect, not a browser page), the loopback whitelist
// plus dynamic-port rule is the callback contract real desktop clients rely
// on, and the state NX rule is the anti-fixation boundary.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_mcp_authorize_contract(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    ensure_mcp_client_seeded(ctx).await;
    let canonical = mcp_canonical_resource(ctx, &realm_id);

    let code_challenge = compute_code_challenge(&generate_code_verifier());

    // Happy path: 302 into the realm login page, state NX seeded.
    let state = generate_state();
    let response = oauth_authorize_with_mcp(
        ctx,
        &realm_id,
        MCP_CLIENT_ID,
        MCP_TEST_REDIRECT_URI,
        &state,
        &code_challenge,
        "S256",
        Some("mcp:profile:read mcp:points:read"),
        Some(&canonical),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FOUND);
    let location = response
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .expect("authorize must redirect to the login page");
    assert!(
        location.contains(&format!("/{}/auth/login", realm_id)),
        "authorize must land on the realm login page, got {location}"
    );
    assert!(
        location.contains("oauthClientId=herald-mcp"),
        "the login redirect must carry the MCP oauth client id, got {location}"
    );

    // Missing resource → 302 error redirect back to the (validated) callback.
    let state = generate_state();
    let response = oauth_authorize_with_mcp(
        ctx,
        &realm_id,
        MCP_CLIENT_ID,
        MCP_TEST_REDIRECT_URI,
        &state,
        &code_challenge,
        "S256",
        Some("mcp:profile:read"),
        None,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::FOUND,
        "an MCP authorize rejection must be an error redirect, not a browser error page"
    );
    let location = response
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .expect("invalid_target must redirect to the callback");
    assert!(
        location.starts_with(MCP_TEST_REDIRECT_URI),
        "the error redirect must go back to the request's loopback callback, got {location}"
    );
    assert_eq!(
        redirect_query_param(location, "error").as_deref(),
        Some("invalid_target")
    );
    assert!(redirect_query_param(location, "error_description").is_some_and(|d| !d.is_empty()));
    assert_eq!(
        redirect_query_param(location, "state").as_deref(),
        Some(state.as_str()),
        "the error redirect must echo the state so the client can correlate"
    );

    // Wrong resource (another realm's canonical / an arbitrary URI) → same
    // invalid_target redirect shape.
    for wrong in [
        mcp_canonical_resource(ctx, "other-realm"),
        "https://attacker.example/resource".to_string(),
    ] {
        let state = generate_state();
        let response = oauth_authorize_with_mcp(
            ctx,
            &realm_id,
            MCP_CLIENT_ID,
            MCP_TEST_REDIRECT_URI,
            &state,
            &code_challenge,
            "S256",
            Some("mcp:profile:read"),
            Some(&wrong),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FOUND);
        let location = response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .expect("wrong resource must redirect");
        assert_eq!(
            redirect_query_param(location, "error").as_deref(),
            Some("invalid_target"),
            "resource {wrong} must be rejected as invalid_target"
        );
        assert_eq!(
            redirect_query_param(location, "state").as_deref(),
            Some(state.as_str())
        );
    }

    // Unknown scope token (incl. openid) → 302 invalid_scope redirect.
    for bad_scope in ["openid", "mcp:profile:read mcp:admin:write"] {
        let state = generate_state();
        let response = oauth_authorize_with_mcp(
            ctx,
            &realm_id,
            MCP_CLIENT_ID,
            MCP_TEST_REDIRECT_URI,
            &state,
            &code_challenge,
            "S256",
            Some(bad_scope),
            Some(&canonical),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FOUND);
        let location = response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .expect("invalid scope must redirect");
        assert_eq!(
            redirect_query_param(location, "error").as_deref(),
            Some("invalid_scope"),
            "scope {bad_scope} must be rejected as invalid_scope"
        );
        assert_eq!(
            redirect_query_param(location, "state").as_deref(),
            Some(state.as_str())
        );
    }

    // Redirect whitelist: userinfo/query/non-loopback shapes are 400 (ApiError
    // shape, no redirect) — the callback is NOT validated, so redirecting an
    // error there would itself be an open redirect.
    for bad_redirect in [
        "http://user@127.0.0.1:43119/callback",
        "http://127.0.0.1:43119/callback?next=/evil",
        "http://evil.example/callback",
        "http://127.0.0.2/callback",
    ] {
        let response = oauth_authorize_with_mcp(
            ctx,
            &realm_id,
            MCP_CLIENT_ID,
            bad_redirect,
            &generate_state(),
            &code_challenge,
            "S256",
            Some("mcp:profile:read"),
            Some(&canonical),
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "redirect {bad_redirect} must be rejected without redirecting"
        );
        let error: Value = response_json(response).await;
        assert!(
            error["message"]
                .as_str()
                .is_some_and(|m| m.contains("whitelist")),
            "rejection must say the redirect is not whitelisted, got {error}"
        );
    }

    // RFC 8252 §7.3: all three registered loopback host forms match with ANY
    // port — the dynamic-port rule real desktop clients depend on.
    for redirect in [
        "http://127.0.0.1:1/callback",
        "http://localhost:9999/callback",
        "http://[::1]:80/callback",
    ] {
        let response = oauth_authorize_with_mcp(
            ctx,
            &realm_id,
            MCP_CLIENT_ID,
            redirect,
            &generate_state(),
            &code_challenge,
            "S256",
            Some("mcp:profile:read"),
            Some(&canonical),
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::FOUND,
            "loopback callback {redirect} with an arbitrary port must be accepted"
        );
    }

    // State replay (SET NX conflict): a reused pending state is rejected, so a
    // state-fixation attacker cannot re-seed a victim's flow.
    let replay_state = generate_state();
    let first = oauth_authorize_with_mcp(
        ctx,
        &realm_id,
        MCP_CLIENT_ID,
        MCP_TEST_REDIRECT_URI,
        &replay_state,
        &code_challenge,
        "S256",
        Some("mcp:profile:read"),
        Some(&canonical),
    )
    .await;
    assert_eq!(first.status(), StatusCode::FOUND);
    let replay = oauth_authorize_with_mcp(
        ctx,
        &realm_id,
        MCP_CLIENT_ID,
        MCP_TEST_REDIRECT_URI,
        &replay_state,
        &code_challenge,
        "S256",
        Some("mcp:profile:read"),
        Some(&canonical),
    )
    .await;
    assert_eq!(
        replay.status(),
        StatusCode::BAD_REQUEST,
        "a pending state must not be re-seeded"
    );
    let error: Value = response_json(replay).await;
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|m| m.contains("state")),
        "replay rejection must mention the state, got {error}"
    );

    // A non-MCP client must not gain an audience by carrying `resource` —
    // browser credentials never carry one this round.
    let admin_token = setup_admin_session(ctx, "mcp-authorize-nonmcp@test.com").await;
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "mcp-nonmcp-app",
        "MCP NonMCP App",
        &["https://nonmcpapp.com/callback"],
    )
    .await;
    assert_eq!(create_response.status(), 201);
    let response = oauth_authorize_with_mcp(
        ctx,
        &realm_id,
        "mcp-nonmcp-app",
        "https://nonmcpapp.com/callback",
        &generate_state(),
        &code_challenge,
        "S256",
        None,
        Some(&canonical),
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "a non-MCP client must not request a resource"
    );
    let error: Value = response_json(response).await;
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|m| m.contains("not supported")),
        "rejection must say the resource parameter is unsupported for this client, got {error}"
    );
}

// -----------------------------------------------------------------------------
// Test 16: token MCP contract (resource repeat, PKCE, replay, scope wire,
// form encoding)
// -----------------------------------------------------------------------------

// User Story: docs/user-stories/integration/mcp-server.md US-MCP-001
// Covers: the token-side MCP binding checks. WHY: the code's resource binding
// must be repeated (audience smuggling via a different resource is
// invalid_target), PKCE/binding failures are invalid_grant, and the granted
// scope set must be echoed in a stable-sorted wire form.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_mcp_token_exchange_contract(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    ensure_mcp_client_seeded(ctx).await;
    let canonical = mcp_canonical_resource(ctx, &realm_id);

    let email = "mcp-token-user@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;

    // --- resource binding ---
    let (code, code_verifier) =
        mcp_password_login_to_code(ctx, &realm_id, email, password, Some("mcp:profile:read")).await;

    // Missing resource at the exchange → invalid_target (OAuth error shape).
    let response = oauth_token_exchange_with_resource(
        ctx,
        &realm_id,
        &code,
        MCP_TEST_REDIRECT_URI,
        MCP_CLIENT_ID,
        &code_verifier,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let error: Value = response_json(response).await;
    assert_eq!(
        error["error"].as_str(),
        Some("invalid_target"),
        "omitting the resource at exchange must be invalid_target"
    );

    // GETDEL runs before the resource check, so the rejected exchange above
    // already consumed its code — obtain a fresh one for the wrong-resource
    // branch.
    let (code, code_verifier) =
        mcp_password_login_to_code(ctx, &realm_id, email, password, Some("mcp:profile:read")).await;
    let response = oauth_token_exchange_with_resource(
        ctx,
        &realm_id,
        &code,
        MCP_TEST_REDIRECT_URI,
        MCP_CLIENT_ID,
        &code_verifier,
        Some(&mcp_canonical_resource(ctx, "other-realm")),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let error: Value = response_json(response).await;
    assert_eq!(
        error["error"].as_str(),
        Some("invalid_target"),
        "a mismatched resource must be invalid_target"
    );

    // --- PKCE verifier mismatch → invalid_grant ---
    let (code, _code_verifier) =
        mcp_password_login_to_code(ctx, &realm_id, email, password, Some("mcp:profile:read")).await;
    let wrong_verifier = generate_code_verifier();
    let response = oauth_token_exchange_with_resource(
        ctx,
        &realm_id,
        &code,
        MCP_TEST_REDIRECT_URI,
        MCP_CLIENT_ID,
        &wrong_verifier,
        Some(&canonical),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let error: Value = response_json(response).await;
    assert_eq!(
        error["error"].as_str(),
        Some("invalid_grant"),
        "a PKCE verifier mismatch must be invalid_grant on the MCP grant surface"
    );

    // --- redirect binding mismatch → invalid_grant ---
    let (code, code_verifier) =
        mcp_password_login_to_code(ctx, &realm_id, email, password, Some("mcp:profile:read")).await;
    let response = oauth_token_exchange_with_resource(
        ctx,
        &realm_id,
        &code,
        "http://127.0.0.1:1/callback",
        MCP_CLIENT_ID,
        &code_verifier,
        Some(&canonical),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let error: Value = response_json(response).await;
    assert_eq!(
        error["error"].as_str(),
        Some("invalid_grant"),
        "the code stays bound to its exact redirect URI (port included)"
    );

    // --- success + scope echo (stable-sorted wire) ---
    // Deliberately unsorted request; the response must come back sorted.
    let (code, code_verifier) = mcp_password_login_to_code(
        ctx,
        &realm_id,
        email,
        password,
        Some("mcp:subscriptions:read mcp:profile:read mcp:points:read mcp:transactions:read"),
    )
    .await;
    let response = oauth_token_exchange_with_resource(
        ctx,
        &realm_id,
        &code,
        MCP_TEST_REDIRECT_URI,
        MCP_CLIENT_ID,
        &code_verifier,
        Some(&canonical),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response_json(response).await;
    assert_eq!(
        body["scope"].as_str(),
        Some("mcp:points:read mcp:profile:read mcp:subscriptions:read mcp:transactions:read"),
        "the granted scope must be echoed in stable-sorted order"
    );

    // --- code replay → rejected, no second token ---
    let response = oauth_token_exchange_with_resource(
        ctx,
        &realm_id,
        &code,
        MCP_TEST_REDIRECT_URI,
        MCP_CLIENT_ID,
        &code_verifier,
        Some(&canonical),
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "a consumed code must never exchange twice"
    );
    let error: Value = response_json(response).await;
    assert_eq!(
        error["error"].as_str(),
        Some("invalid_grant"),
        "the MCP grant surface must answer replay with the RFC 6749 error body, got {error}"
    );

    // --- absent scope at authorize defaults to the profile-read minimum ---
    let (code, code_verifier) =
        mcp_password_login_to_code(ctx, &realm_id, email, password, None).await;
    let response = oauth_token_exchange_with_resource(
        ctx,
        &realm_id,
        &code,
        MCP_TEST_REDIRECT_URI,
        MCP_CLIENT_ID,
        &code_verifier,
        Some(&canonical),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response_json(response).await;
    assert_eq!(
        body["scope"].as_str(),
        Some("mcp:profile:read"),
        "an absent MCP scope must normalize to the profile-read minimum"
    );

    // --- explicit empty scope is the empty set (connect-and-list-tools) ---
    let (code, code_verifier) =
        mcp_password_login_to_code(ctx, &realm_id, email, password, Some("")).await;
    let response = oauth_token_exchange_with_resource(
        ctx,
        &realm_id,
        &code,
        MCP_TEST_REDIRECT_URI,
        MCP_CLIENT_ID,
        &code_verifier,
        Some(&canonical),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response_json(response).await;
    assert_eq!(
        body["scope"].as_str(),
        Some(""),
        "an explicitly empty MCP scope must grant the empty set, not the default"
    );

    // --- RFC 6749 form-urlencoded equivalence ---
    let (code, code_verifier) = mcp_password_login_to_code(
        ctx,
        &realm_id,
        email,
        password,
        Some("mcp:profile:read mcp:points:read"),
    )
    .await;
    let response = oauth_token_exchange_form_with_resource(
        ctx,
        &realm_id,
        &code,
        MCP_TEST_REDIRECT_URI,
        MCP_CLIENT_ID,
        &code_verifier,
        Some(&canonical),
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "standard clients POST form-encoded per RFC 6749 §4.1.3 — must be equivalent"
    );
    let cache_control = response
        .headers()
        .get("cache-control")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let body: Value = response_json(response).await;
    assert_eq!(
        body["scope"].as_str(),
        Some("mcp:points:read mcp:profile:read")
    );
    assert_eq!(cache_control.as_deref(), Some("no-store"));
}

// -----------------------------------------------------------------------------
// Test 17: V2 — password entrance preserves resource and scope
// -----------------------------------------------------------------------------

// User Story: docs/user-stories/integration/mcp-server.md US-MCP-001
// Covers: the main V2 chain (password login) must carry the authorize-time
// resource/scope through state → code → token. WHY: resource/scope are only
// ever copied from the server-side pending state; if any hop dropped them the
// token would be audience-less or over/under-scoped.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_mcp_v2_password_login_preserves_resource_and_scope(
    ctx: &mut SchemaTestContext,
) {
    let realm_id = ctx._realm_id.clone();
    ensure_mcp_client_seeded(ctx).await;

    let email = "mcp-v2-password@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;

    let (auth_code, code_verifier) = mcp_password_login_to_code(
        ctx,
        &realm_id,
        email,
        password,
        Some("mcp:points:read mcp:profile:read"),
    )
    .await;

    let response = oauth_token_exchange_with_resource(
        ctx,
        &realm_id,
        &auth_code,
        MCP_TEST_REDIRECT_URI,
        MCP_CLIENT_ID,
        &code_verifier,
        Some(&mcp_canonical_resource(ctx, &realm_id)),
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the password entrance's code must exchange against the canonical resource"
    );
    let body: Value = response_json(response).await;
    assert_eq!(
        body["scope"].as_str(),
        Some("mcp:points:read mcp:profile:read"),
        "the authorized scope set must survive login and exchange unchanged"
    );
}

// -----------------------------------------------------------------------------
// Test 18: V2 — TOTP entrance preserves resource and scope
// -----------------------------------------------------------------------------

// User Story: docs/user-stories/integration/mcp-server.md US-MCP-001
// Covers: the TOTP second-factor handoff must not lose the pending MCP
// transaction — the temp session carries the OAuth context, and the code
// issued after verification still binds resource/scope.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_mcp_v2_totp_login_preserves_resource_and_scope(ctx: &mut SchemaTestContext) {
    unsafe {
        std::env::set_var("TOTP_SECRET_KEY", "test_key_32_bytes_long_1234567890");
    }
    let realm_id = ctx._realm_id.clone();
    ensure_mcp_client_seeded(ctx).await;
    setup_realm_totp_config(ctx, true, false).await;

    let email = "mcp-v2-totp@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;
    let session_token = login_user(ctx, email, password).await;
    let totp_secret = enable_totp_for_user(ctx, &session_token, password).await;

    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state = generate_state();
    let authorize = oauth_authorize_with_mcp(
        ctx,
        &realm_id,
        MCP_CLIENT_ID,
        MCP_TEST_REDIRECT_URI,
        &state,
        &code_challenge,
        "S256",
        Some("mcp:profile:read mcp:transactions:read"),
        Some(&mcp_canonical_resource(ctx, &realm_id)),
    )
    .await;
    assert_eq!(authorize.status(), StatusCode::FOUND);

    let login = login_with_oauth(
        ctx,
        &realm_id,
        email,
        password,
        MCP_CLIENT_ID,
        MCP_TEST_REDIRECT_URI,
        &state,
    )
    .await;
    assert_eq!(login.status(), StatusCode::OK);
    let login_json: Value = response_json(login).await;
    assert_eq!(
        login_json["requiresTotp"].as_bool(),
        Some(true),
        "the TOTP user must be routed to the second factor"
    );
    let temp_token = login_json["tempToken"]
        .as_str()
        .expect("TOTP branch must issue a temp token")
        .to_string();

    let totp_code = generate_totp_code(&totp_secret);
    let verify = verify_totp_with_oauth(ctx, &realm_id, &temp_token, &totp_code).await;
    assert_eq!(verify.status(), StatusCode::OK);
    let verify_json: Value = response_json(verify).await;
    let redirect_to = verify_json["redirectTo"]
        .as_str()
        .expect("TOTP verification must complete the MCP authorization");

    exchange_mcp_code_and_assert(
        ctx,
        &realm_id,
        redirect_to,
        MCP_TEST_REDIRECT_URI,
        &code_verifier,
        "mcp:profile:read mcp:transactions:read",
    )
    .await;
}

// -----------------------------------------------------------------------------
// Test 19: V2 — LDAP entrance preserves resource and scope
// -----------------------------------------------------------------------------

// User Story: docs/user-stories/integration/mcp-server.md US-MCP-001
// Covers: the enterprise-directory entrance shares the downstream-OAuth code
// builder; its state copy must include the MCP resource/scope exactly like
// the password entrance.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_mcp_v2_ldap_login_preserves_resource_and_scope(ctx: &mut SchemaTestContext) {
    enable_ldap(ctx).await;
    let realm_id = ctx._realm_id.clone();
    ensure_mcp_client_seeded(ctx).await;

    let email = format!("mcp-v2-ldap-{}@test.com", uuid::Uuid::now_v7());
    let user_id = create_test_user(ctx, &email, "password123").await;
    let dn = "uid=mcp-v2,dc=example,dc=com";
    sqlx::query(
        "INSERT INTO provider (id, realm_id, type, open_id, user_id, created_at, updated_at)
         VALUES ($1, $2, 'ldap', $3, $4, NOW(), NOW())",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(&realm_id)
    .bind(dn)
    .bind(user_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("failed to link the LDAP DN");

    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state = generate_state();
    let authorize = oauth_authorize_with_mcp(
        ctx,
        &realm_id,
        MCP_CLIENT_ID,
        MCP_TEST_REDIRECT_URI,
        &state,
        &code_challenge,
        "S256",
        Some("mcp:subscriptions:read mcp:profile:read"),
        Some(&mcp_canonical_resource(ctx, &realm_id)),
    )
    .await;
    assert_eq!(authorize.status(), StatusCode::FOUND);

    let mock = mock_dir(one_mock_user("mcp-v2", dn, Some(&email), "corp-pw-1"));
    let login = ldap_login_ext(
        ctx,
        &mock,
        "mcp-v2",
        "corp-pw-1",
        None,
        json!({
            "oauthClientId": MCP_CLIENT_ID,
            "redirectUri": MCP_TEST_REDIRECT_URI,
            "state": state,
        }),
    )
    .await;
    assert_eq!(login.status(), StatusCode::OK);
    let login_json: Value = response_json(login).await;
    let redirect_to = login_json["redirectTo"]
        .as_str()
        .expect("LDAP entrance must complete the MCP authorization");

    exchange_mcp_code_and_assert(
        ctx,
        &realm_id,
        redirect_to,
        MCP_TEST_REDIRECT_URI,
        &code_verifier,
        "mcp:profile:read mcp:subscriptions:read",
    )
    .await;
}

// -----------------------------------------------------------------------------
// Test 20: V2 — passkey entrance preserves resource and scope
// -----------------------------------------------------------------------------

// User Story: docs/user-stories/integration/mcp-server.md US-MCP-001
// Covers: the WebAuthn first-factor entrance shares the same code builder;
// the MCP transaction must survive the begin/verify ceremony.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_mcp_v2_passkey_login_preserves_resource_and_scope(
    ctx: &mut SchemaTestContext,
) {
    // The passkey RP is resolved from env vars; nextest runs each test in its
    // own process, so setting them here cannot race other scenarios.
    unsafe {
        std::env::set_var("RP_ID", "localhost");
        std::env::set_var("RP_ORIGIN", RP_ORIGIN);
    }
    let realm_id = ctx._realm_id.clone();
    ensure_mcp_client_seeded(ctx).await;
    setup_realm_passkey_config(ctx, &realm_id, true).await;

    let email = "mcp-v2-passkey@test.com";
    let password = "password123";
    let user_id = create_test_user(ctx, email, password).await;
    let session_token = login_user(ctx, email, password).await;
    let mut authenticator = Es256Authenticator::new();
    register_one_passkey(
        ctx,
        &session_token,
        &user_id.to_string(),
        password,
        None,
        &mut authenticator,
    )
    .await;

    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state = generate_state();
    let authorize = oauth_authorize_with_mcp(
        ctx,
        &realm_id,
        MCP_CLIENT_ID,
        MCP_TEST_REDIRECT_URI,
        &state,
        &code_challenge,
        "S256",
        Some("mcp:points:read mcp:profile:read"),
        Some(&mcp_canonical_resource(ctx, &realm_id)),
    )
    .await;
    assert_eq!(authorize.status(), StatusCode::FOUND);

    // First factor with the MCP downstream context.
    let options_request = axum::http::Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{realm_id}/login/passkey/options"))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "9.9.9.9")
        .body(axum::body::Body::from(
            json!({
                "clientId": ctx._client_id,
                "turnstileToken": "dummy",
                "oauth": {
                    "clientId": MCP_CLIENT_ID,
                    "redirectUri": MCP_TEST_REDIRECT_URI,
                    "state": state,
                }
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
    let auth_token = options_body["authToken"]
        .as_str()
        .expect("passkey options must return authToken")
        .to_string();

    let assertion = authenticator.authenticate(&options_body["options"], RP_ORIGIN);
    let verify_request = axum::http::Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{realm_id}/login/passkey/verify"))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "9.9.9.9")
        .body(axum::body::Body::from(
            json!({ "authToken": auth_token, "assertion": assertion }).to_string(),
        ))
        .unwrap();
    let response = ctx
        .create_unified_test_router()
        .oneshot(verify_request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let verify_json: Value = response_json(response).await;
    let redirect_to = verify_json["redirectTo"]
        .as_str()
        .expect("passkey entrance must complete the MCP authorization");

    exchange_mcp_code_and_assert(
        ctx,
        &realm_id,
        redirect_to,
        MCP_TEST_REDIRECT_URI,
        &code_verifier,
        "mcp:points:read mcp:profile:read",
    )
    .await;
}

// -----------------------------------------------------------------------------
// Test 21: V2 — social (Google One Tap) entrance preserves resource and scope
// -----------------------------------------------------------------------------

// User Story: docs/user-stories/integration/mcp-server.md US-MCP-001
// Covers: the social entrance consumes the same pending state through the
// typed DownstreamAuthorizationState; its issued code must carry the MCP
// resource/scope. The Google identity is simulated with the shared wiremock
// JWKS fixture and an email-matched pre-existing account (no JIT needed).
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_mcp_v2_google_one_tap_preserves_resource_and_scope(
    ctx: &mut SchemaTestContext,
) {
    // Enabled Google provider for the realm (mirrors google_one_tap_scenarios'
    // direct-SQL seeding: the minted ID token's aud matches this client_id).
    sqlx::query(
        "INSERT INTO oauth_provider_config (id, realm_id, provider_type, client_id, client_secret, scopes, enabled)
         VALUES ($1, $2, 'google', 'google-test-client-id', 'google-test-client-secret',
                 ARRAY['openid', 'email', 'profile'], true)
         ON CONFLICT (realm_id, provider_type)
         DO UPDATE SET enabled = true",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("failed to seed enabled Google provider config");

    let realm_id = ctx._realm_id.clone();
    ensure_mcp_client_seeded(ctx).await;

    // Pre-create the account with the Google token's email (consent recorded
    // by the shared helper) so find_or_create matches by email.
    let email = format!("mcp-v2-social-{}@test.com", uuid::Uuid::now_v7());
    let _user_id = create_test_user(ctx, &email, "password123").await;

    let jwks = spawn_default_jwks().await;
    let jwks_url = full_jwks_url(&jwks.0.uri());

    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state = generate_state();
    let authorize = oauth_authorize_with_mcp(
        ctx,
        &realm_id,
        MCP_CLIENT_ID,
        MCP_TEST_REDIRECT_URI,
        &state,
        &code_challenge,
        "S256",
        Some("mcp:profile:read mcp:points:read"),
        Some(&mcp_canonical_resource(ctx, &realm_id)),
    )
    .await;
    assert_eq!(authorize.status(), StatusCode::FOUND);

    let id_token = mint_test_google_id_token(&MintIdTokenOpts {
        sub: format!("mcp-v2-social-{}", uuid::Uuid::now_v7()),
        email: email.clone(),
        ..Default::default()
    });
    let request = axum::http::Request::builder()
        .method("POST")
        .uri(format!("/api/oauth/{realm_id}/google/one-tap"))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "5.5.5.5")
        .body(axum::body::Body::from(
            json!({
                "credential": id_token,
                "clientId": MCP_CLIENT_ID,
                "downstreamState": state,
            })
            .to_string(),
        ))
        .unwrap();
    let response = ctx
        .create_unified_test_router_with_state(|s| {
            s.google_jwks_url = jwks_url.to_string();
        })
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the social entrance must accept the verified Google identity"
    );
    let body: Value = response_json(response).await;
    let redirect_to = body["redirectUri"]
        .as_str()
        .expect("one-tap downstream mode must return the redirect URI");

    exchange_mcp_code_and_assert(
        ctx,
        &realm_id,
        redirect_to,
        MCP_TEST_REDIRECT_URI,
        &code_verifier,
        "mcp:points:read mcp:profile:read",
    )
    .await;
}
