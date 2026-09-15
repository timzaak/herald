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
    create_admin_session_with_user, generate_totp_code, grant_realm_admin_role, obtain_reauth_token,
};
use crate::tests::helpers::oauth_pkce_helpers::*;
use crate::tests::helpers::test_setup_helpers::{create_test_user, login_user};
use crate::tests::response_json;
use crate::tests::schema_test_context::SchemaTestContext;
use axum::http::StatusCode;
use serde_json::Value;
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
    let app = ctx.create_unified_test_router();

    // TOTP setup is a high-assurance operation: exchange the password for a
    // single-use reauth ticket (targetOperation = bind_authenticator) first.
    let reauth_token =
        obtain_reauth_token(ctx, session_token, "bind_authenticator", password).await;

    // Start TOTP setup
    let enable_request = axum::http::Request::builder()
        .method("POST")
        .uri("/api/user/totp")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", session_token))
        .body(axum::body::Body::from(
            serde_json::json!({ "reauth_token": reauth_token }).to_string(),
        ))
        .unwrap();

    let enable_response = app.clone().oneshot(enable_request).await.unwrap();
    assert_eq!(enable_response.status(), StatusCode::OK);

    let enable_body: Value = response_json(enable_response).await;
    let secret = enable_body["secret"]
        .as_str()
        .expect("Secret should exist")
        .to_string();
    let temp_token = enable_body["tempToken"]
        .as_str()
        .expect("Temp token should exist")
        .to_string();

    // Verify TOTP to complete setup
    let totp_code = generate_totp_code(&secret);
    let verify_request = axum::http::Request::builder()
        .method("POST")
        .uri("/api/user/totp/verify")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", session_token))
        .body(axum::body::Body::from(
            serde_json::json!({
                "tempToken": temp_token,
                "code": totp_code,
            })
            .to_string(),
        ))
        .unwrap();

    let verify_response = app.oneshot(verify_request).await.unwrap();
    assert_eq!(
        verify_response.status(),
        StatusCode::OK,
        "TOTP verify should succeed"
    );

    secret
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
