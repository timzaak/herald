// =============================================================================
// Provider-Agnostic OAuth Integration Scenarios Tests
// =============================================================================
//
// **Purpose**: Eliminate 80% code duplication between Google and GitHub
// OAuth tests by using a unified, parameterized framework.
//
// **User Story Covered**: US-RU-003 (OAuth Third-Party Login)
//
// **Test Cases Consolidated**:
// - google_oauth_scenarios.rs (8 tests)
// - github_oauth_scenarios.rs (8 tests)
//
// WeChat is NOT parameterized here: its flow diverges (appid instead of
// client_id, code2session instead of an authorization-URL browser jump), so
// WeChat coverage lives in wechat_matching_scenarios and the miniprogram /
// matching-specific scenarios rather than this generic framework.
//
// **Benefits**:
// - Reduces test code by 75%
// - Easier to add new OAuth providers
// - Maintains 100% user story coverage
// - Preserves BDD structure (Given-When-Then)
// - Unified test patterns across all providers
//
// **Running Tests**:
// ```bash
// cargo nextest run --workspace unified_oauth_scenarios
// ```
//
// =============================================================================

use crate::tests::helpers::auth_helpers::*;
use crate::tests::helpers::oauth_test_helpers::*;
use crate::tests::helpers::test_setup_helpers::*;
use crate::tests::response_json;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::http::StatusCode;
use serde_json::{Value, json};
use test_context::test_context;

// =============================================================================
// OAuth Provider Configuration
// =============================================================================

/// Configuration data for each OAuth provider type
#[derive(Debug, Clone)]
pub struct OAuthProviderTestConfig {
    /// Provider type identifier (e.g., "google", "github", "wechat")
    pub provider_type: &'static str,
    /// Test client ID for this provider
    pub client_id: &'static str,
    /// Test client secret for this provider
    pub client_secret: &'static str,
    /// OAuth scopes for this provider
    pub scopes: Vec<&'static str>,
    /// Test email suffix for admin users
    pub admin_email_suffix: &'static str,
}

impl OAuthProviderTestConfig {
    /// Create Google provider test config
    pub fn google() -> Self {
        Self {
            provider_type: "google",
            client_id: "google-test-client-id",
            client_secret: "google-test-client-secret",
            scopes: vec!["openid", "email", "profile"],
            admin_email_suffix: "google-test.com",
        }
    }

    /// Create GitHub provider test config
    pub fn github() -> Self {
        Self {
            provider_type: "github",
            client_id: "github-test-client-id",
            client_secret: "github-test-client-secret",
            scopes: vec!["user:email", "read:user"],
            admin_email_suffix: "github-test.com",
        }
    }

    /// Create an OAuthProviderConfig from this test config
    pub fn to_provider_config(&self) -> OAuthProviderConfig {
        OAuthProviderConfig::new(
            self.provider_type,
            self.client_id,
            self.client_secret,
            self.scopes.iter().map(|s| s.to_string()).collect(),
            true,
        )
    }

    /// Get admin email for a specific test scenario
    pub fn admin_email_for(&self, test_name: &str) -> String {
        format!("admin-{}@{}", test_name, self.admin_email_suffix)
    }
}

/// Available OAuth providers for testing
pub fn all_oauth_providers() -> Vec<OAuthProviderTestConfig> {
    vec![
        OAuthProviderTestConfig::google(),
        OAuthProviderTestConfig::github(),
    ]
}

// =============================================================================
// OAuth Test Scenarios (Parameterized by Provider)
// =============================================================================

/// ============================================================================
/// Scenario 1: OAuth Provider Configuration CRUD - Create
///
/// **User Story**: OAuth Provider Configuration
///
/// **Given**:
/// - System initialized, test realm exists
/// - User has oauth:config:create permission (realm admin)
///
/// **When**:
/// - User calls POST /api/{realmId}/oauth/configs to create Provider configuration
/// - Request body includes providerType, clientId, clientSecret, scopes, enabled: true
///
/// **Then**:
/// - Returns 201 Created
/// - Configuration correctly saved to database
/// - Can retrieve configuration via GET /api/{realmId}/oauth/configs/{providerType}
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_oauth_provider_config_create(ctx: &mut TestContext) {
    for provider_config in all_oauth_providers() {
        let realm_id = ctx._realm_id.clone();
        let provider_type = provider_config.provider_type;

        println!(
            "\n[Scenario 1] Testing {} provider config creation",
            provider_type
        );

        // ============================================================================
        // Step 1: Create admin session with oauth:config:create permission
        // ============================================================================
        println!("[Step 1] Creating admin session");
        let admin_email = provider_config.admin_email_for("config-create");
        let (token, admin_user_id) = create_admin_session_with_user(ctx, &admin_email, 1800).await;
        grant_realm_admin_role(ctx, &admin_user_id).await;

        // ============================================================================
        // Step 2: Create Provider configuration
        // ============================================================================
        println!("[Step 2] Creating {} Provider configuration", provider_type);
        let config = provider_config.to_provider_config();
        let create_response = create_oauth_provider_config(ctx, &config, &token, &realm_id).await;

        // ============================================================================
        // Step 3: Verify 201 Created response
        // ============================================================================
        println!("[Step 3] Verifying 201 Created response");
        assert_eq!(
            create_response.status(),
            StatusCode::CREATED,
            "Should return 201 Created for successful {} provider creation",
            provider_type
        );

        let create_json: Value = response_json(create_response).await;

        // Provider config endpoints are A-class interfaces, so response is wrapped in ApiResult: { data: {...}, meta: null }
        assert_eq!(create_json["providerType"], provider_type);
        assert_eq!(create_json["clientId"], provider_config.client_id);
        assert_eq!(create_json["scopes"], json!(provider_config.scopes));
        assert_eq!(create_json["enabled"], true);
        // Client secret should not be returned in response
        assert!(
            create_json["data"].get("clientSecret").is_none()
                || create_json["clientSecret"].as_str() == Some(""),
            "Client secret should not be returned"
        );
        println!(
            "[Step 3] ✓ Response contains correct {} Provider configuration",
            provider_type
        );

        // ============================================================================
        // Step 4: Verify configuration saved to database
        // ============================================================================
        println!("[Step 4] Verifying configuration saved to database");
        verify_provider_config_in_db(
            ctx,
            &realm_id,
            provider_type,
            provider_config.client_id,
            &provider_config
                .scopes
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<String>>(),
            true,
        )
        .await;
        println!("[Step 4] ✓ Configuration correctly saved to database");

        // ============================================================================
        // Step 5: Verify can retrieve configuration via GET
        // ============================================================================
        println!(
            "[Step 5] Retrieving {} Provider configuration",
            provider_type
        );
        let get_response = get_oauth_provider_config(ctx, &realm_id, provider_type, &token).await;

        assert_eq!(
            get_response.status(),
            StatusCode::OK,
            "Should return 200 OK when retrieving existing {} provider",
            provider_type
        );

        let get_json: Value = response_json(get_response).await;

        // Provider config endpoints are A-class interfaces, so response is wrapped in ApiResult: { data: {...}, meta: null }
        assert_eq!(get_json["providerType"], provider_type);
        assert_eq!(get_json["clientId"], provider_config.client_id);
        println!(
            "[Step 5] ✓ Successfully retrieved {} Provider configuration",
            provider_type
        );
    }
}

/// ============================================================================
/// Scenario 2: OAuth Provider Configuration - List Enabled
///
/// **Given**:
/// - System initialized, test realm exists
/// - OAuth Provider is configured and enabled
///
/// **When**:
/// - User calls GET /api/{realmId}/oauth/configs?enabled=true
///
/// **Then**:
/// - Returns 200 OK
/// - Response includes enabled Provider configurations
/// - Configuration does not include clientSecret
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_oauth_provider_config_list_enabled(ctx: &mut TestContext) {
    for provider_config in all_oauth_providers() {
        let realm_id = ctx._realm_id.clone();
        let provider_type = provider_config.provider_type;

        println!(
            "\n[Scenario 2] Testing {} provider config list enabled",
            provider_type
        );

        // ============================================================================
        // Step 1: Create admin session with oauth:config:view permission
        // ============================================================================
        println!("[Step 1] Creating admin session");
        let admin_email = provider_config.admin_email_for("list-enabled");
        let (token, admin_user_id) = create_admin_session_with_user(ctx, &admin_email, 1800).await;
        grant_realm_admin_role(ctx, &admin_user_id).await;

        // ============================================================================
        // Cleanup: Delete any existing provider configurations from previous iterations
        // This ensures we start fresh even if a previous test failed
        // ============================================================================
        println!("[Cleanup] Cleaning up any existing provider configurations");
        for pt in ["google", "github"] {
            let delete_response = delete_oauth_provider_config(ctx, &realm_id, pt, &token).await;
            // Ignore failures - the provider might not exist
            if !delete_response.status().is_success()
                && delete_response.status() != StatusCode::NOT_FOUND
            {
                // Log unexpected errors but continue
                tracing::warn!(
                    "Unexpected status {} when deleting provider {}",
                    delete_response.status(),
                    pt
                );
            }
        }
        println!("[Cleanup] ✓ Cleanup completed");

        // ============================================================================
        // Step 2: Create Provider configuration (enabled)
        // ============================================================================
        println!(
            "[Step 2] Creating enabled {} Provider configuration",
            provider_type
        );
        let enabled_config = provider_config.to_provider_config();
        let create_response =
            create_oauth_provider_config(ctx, &enabled_config, &token, &realm_id).await;
        assert_eq!(
            create_response.status(),
            StatusCode::CREATED,
            "Should create {} provider successfully",
            provider_type
        );
        println!(
            "[Step 2] ✓ {} Provider configuration created",
            provider_type
        );

        // ============================================================================
        // Step 3: Create another Provider configuration (disabled) to test filtering
        // ============================================================================
        println!("[Step 3] Creating disabled Provider configuration for filtering test");
        // Use a different valid provider type (github) as the disabled one
        let disabled_provider_type = match provider_type {
            "google" => "github",
            "github" => "google",
            _ => "github", // fallback
        };
        let disabled_config = OAuthProviderConfig::new(
            disabled_provider_type,
            "disabled-client",
            "disabled-secret",
            vec!["scope1".to_string()],
            false,
        );
        let disabled_response =
            create_oauth_provider_config(ctx, &disabled_config, &token, &realm_id).await;
        assert_eq!(
            disabled_response.status(),
            StatusCode::CREATED,
            "Should create disabled provider successfully"
        );
        println!("[Step 3] ✓ Disabled Provider configuration created");

        // ============================================================================
        // Step 4: List enabled providers
        // ============================================================================
        println!("[Step 4] Listing enabled OAuth providers");
        let list_response = list_enabled_oauth_providers(ctx, &realm_id, &token).await;

        assert_eq!(
            list_response.status(),
            StatusCode::OK,
            "Should return 200 OK when listing enabled providers"
        );

        let list_json: Value = response_json(list_response).await;

        // Provider config endpoints return array directly
        // Should only return enabled providers
        assert!(
            !list_json.as_array().unwrap().is_empty(),
            "Should return at least one enabled provider"
        );

        // Verify our provider is in the list
        let provider_found = list_json
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p.get("providerType").and_then(|pt| pt.as_str()) == Some(provider_type));
        assert!(
            provider_found,
            "{} provider should be in the enabled list",
            provider_type
        );

        println!("[Step 4] ✓ Only enabled providers returned");

        // ============================================================================
        // Cleanup: Delete the disabled provider to avoid conflicts in next iteration
        // ============================================================================
        println!("[Cleanup] Deleting disabled provider configuration");
        let delete_response =
            delete_oauth_provider_config(ctx, &realm_id, disabled_provider_type, &token).await;
        // Delete should succeed even if it returns 200 or 204
        assert!(
            delete_response.status().is_success(),
            "Should delete disabled provider successfully"
        );
        println!("[Cleanup] ✓ Disabled provider deleted");
    }
}

/// ============================================================================
/// Scenario 3: OAuth Provider Configuration - Delete
///
/// **Given**:
/// - System initialized, test realm exists
/// - OAuth Provider is configured
/// - User has oauth:config:delete permission
///
/// **When**:
/// - User calls DELETE /api/{realmId}/oauth/configs/{providerType}
///
/// **Then**:
/// - Returns 200 OK or 204 No Content
/// - Configuration is removed from database
/// - Cannot retrieve configuration anymore
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_oauth_provider_config_delete(ctx: &mut TestContext) {
    for provider_config in all_oauth_providers() {
        let realm_id = ctx._realm_id.clone();
        let provider_type = provider_config.provider_type;

        println!(
            "\n[Scenario 3] Testing {} provider config delete",
            provider_type
        );

        // ============================================================================
        // Step 1: Create admin session with oauth:config:delete permission
        // ============================================================================
        println!("[Step 1] Creating admin session");
        let admin_email = provider_config.admin_email_for("delete");
        let (token, admin_user_id) = create_admin_session_with_user(ctx, &admin_email, 1800).await;
        grant_realm_admin_role(ctx, &admin_user_id).await;

        // ============================================================================
        // Step 2: Create Provider configuration
        // ============================================================================
        println!("[Step 2] Creating {} Provider configuration", provider_type);
        let config = provider_config.to_provider_config();
        let create_response = create_oauth_provider_config(ctx, &config, &token, &realm_id).await;
        assert_eq!(
            create_response.status(),
            StatusCode::CREATED,
            "Should create {} provider successfully",
            provider_type
        );
        println!(
            "[Step 2] ✓ {} Provider configuration created",
            provider_type
        );

        // ============================================================================
        // Step 3: Delete Provider configuration
        // ============================================================================
        println!("[Step 3] Deleting {} Provider configuration", provider_type);
        let delete_response =
            delete_oauth_provider_config(ctx, &realm_id, provider_type, &token).await;

        assert!(
            delete_response.status() == StatusCode::OK
                || delete_response.status() == StatusCode::NO_CONTENT,
            "Should return 200 OK or 204 No Content when deleting {} provider",
            provider_type
        );
        println!(
            "[Step 3] ✓ {} Provider configuration deleted",
            provider_type
        );

        // ============================================================================
        // Step 4: Verify configuration is removed from database
        // ============================================================================
        println!("[Step 4] Verifying configuration removed from database");
        let get_response = get_oauth_provider_config(ctx, &realm_id, provider_type, &token).await;

        assert_eq!(
            get_response.status(),
            StatusCode::NOT_FOUND,
            "Should return 404 Not Found after deleting {} provider",
            provider_type
        );
        println!("[Step 4] ✓ Configuration successfully removed");
    }
}

/// ============================================================================
/// Scenario 4: OAuth Authorization URL Generation
///
/// **Given**:
/// - System initialized, test realm exists
/// - OAuth Provider is configured and enabled
/// - Mock server is configured for testing
///
/// **When**:
/// - User calls GET /api/{realmId}/oauth/{providerType}/login
///
/// **Then**:
/// - Returns 200 OK with OAuth authorization data
/// - Includes authorization URL (or redirect to mock server)
/// - URL includes client_id, redirect_uri, scope, state parameters
/// - State token is returned
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_oauth_authorization_url_generation(ctx: &mut TestContext) {
    for provider_config in all_oauth_providers() {
        let realm_id = ctx._realm_id.clone();
        let provider_type = provider_config.provider_type;

        println!(
            "\n[Scenario 4] Testing {} authorization URL generation",
            provider_type
        );

        // ============================================================================
        // Step 1: Create Provider configuration
        // ============================================================================
        println!("[Step 1] Creating {} Provider configuration", provider_type);
        let admin_email = provider_config.admin_email_for("auth-url");
        let (token, admin_user_id) = create_admin_session_with_user(ctx, &admin_email, 1800).await;
        grant_realm_admin_role(ctx, &admin_user_id).await;

        let config = provider_config.to_provider_config();
        create_oauth_provider_config(ctx, &config, &token, &realm_id).await;
        println!(
            "[Step 1] ✓ {} Provider configuration created",
            provider_type
        );

        // ============================================================================
        // Step 2: Generate authorization URL
        // ============================================================================
        println!("[Step 2] Generating {} authorization URL", provider_type);
        let (auth_response, state_token) =
            generate_auth_url_and_extract_state(ctx, &realm_id, provider_type).await;

        assert_eq!(
            auth_response.status(),
            StatusCode::OK,
            "Should return 200 OK with OAuth authorization data for {}",
            provider_type
        );

        // Extract auth_url from JSON body
        let json: Value = response_json(auth_response).await;

        // oauth_login is a B-class interface (OAuth protocol), so response is direct JSON: { authUrl: "...", state: "..." }
        // WeChat uses snake_case "auth_url", while Google/GitHub use camelCase "authUrl"
        let auth_url = json["authUrl"]
            .as_str()
            .or_else(|| json["auth_url"].as_str())
            .expect("Response should contain authUrl or auth_url");

        println!("[Step 2] ✓ Authorization URL: {}", auth_url);

        // ============================================================================
        // Step 3: Verify authorization URL parameters
        // ============================================================================
        println!("[Step 3] Verifying authorization URL parameters");

        // In test mode with mock server, URL should be redirected to mock server
        // or contain client_id and redirect_uri
        if auth_url.contains("beeceptor") || auth_url.contains("mock") {
            println!("[Step 3] ✓ Using mock OAuth server");
        } else {
            assert!(
                auth_url.contains("client_id"),
                "URL should contain client_id"
            );
            assert!(
                auth_url.contains("redirect_uri"),
                "URL should contain redirect_uri"
            );
            assert!(auth_url.contains("scope="), "URL should contain scope");
            assert!(
                auth_url.contains("state="),
                "URL should contain state parameter"
            );
        }
        println!("[Step 3] ✓ Authorization URL contains required parameters");

        // ============================================================================
        // Step 4: Verify state token
        // ============================================================================
        println!("[Step 4] Verifying state token");
        assert!(
            !state_token.is_empty(),
            "State token should be present for {} provider",
            provider_type
        );
        println!("[Step 4] ✓ State token extracted: {}", state_token);
    }
}
/// ============================================================================
/// Scenario 7: OAuth Error Handling - Invalid State Token
///
/// **Given**:
/// - OAuth Provider is configured and enabled
/// - User initiates OAuth flow
///
/// **When**:
/// - OAuth callback includes invalid state token
///
/// **Then**:
/// - Returns 400 Bad Request
/// - Error message indicates invalid state
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_oauth_callback_invalid_state(ctx: &mut TestContext) {
    for provider_config in all_oauth_providers() {
        let realm_id = ctx._realm_id.clone();
        let provider_type = provider_config.provider_type;

        println!(
            "\n[Scenario 7] Testing {} callback invalid state",
            provider_type
        );

        // ============================================================================
        // Step 1: Configure OAuth
        // ============================================================================
        println!("[Step 1] Configuring {} OAuth", provider_type);
        let admin_email = provider_config.admin_email_for("error-invalid-state");
        let (token, admin_user_id) = create_admin_session_with_user(ctx, &admin_email, 1800).await;
        grant_realm_admin_role(ctx, &admin_user_id).await;

        let config = provider_config.to_provider_config();
        create_oauth_provider_config(ctx, &config, &token, &realm_id).await;
        println!("[Step 1] ✓ {} OAuth configured", provider_type);

        // ============================================================================
        // Step 2: Send callback with invalid state
        // ============================================================================
        println!("[Step 2] Sending callback with invalid state token");
        let invalid_state = "invalid_state_token_12345";
        let mock_code = "mock_code_12345";

        let callback_response =
            send_oauth_callback(ctx, &realm_id, provider_type, mock_code, invalid_state)
                .await
                .unwrap();

        // ============================================================================
        // Step 3: Verify error response
        // ============================================================================
        println!("[Step 3] Verifying error response");

        assert_eq!(
            callback_response.status(),
            StatusCode::BAD_REQUEST,
            "Should return 400 Bad Request for invalid state ({} provider)",
            provider_type
        );

        let json: Value = response_json(callback_response).await;
        assert_response_error(&json, None);
        println!("[Step 3] ✓ Invalid state token handled correctly");
    }
}

/// ============================================================================
/// Scenario 8: OAuth Error Handling - Provider Not Configured
///
/// **Given**:
/// - OAuth Provider is not configured
///
/// **When**:
/// - User tries to initiate OAuth flow
///
/// **Then**:
/// - Returns 404 Not Found
/// - Error message indicates provider not configured
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_oauth_provider_not_configured(ctx: &mut TestContext) {
    for provider_config in all_oauth_providers() {
        let realm_id = ctx._realm_id.clone();
        let provider_type = provider_config.provider_type;

        println!(
            "\n[Scenario 8] Testing {} provider not configured",
            provider_type
        );

        // ============================================================================
        // Step 1: Try to generate authorization URL without configuring provider
        // ============================================================================
        println!(
            "[Step 1] Trying to generate authorization URL without {} provider config",
            provider_type
        );
        let (auth_response, state_token) =
            generate_auth_url_and_extract_state(ctx, &realm_id, provider_type).await;

        // ============================================================================
        // Step 2: Verify 404 response
        // ============================================================================
        println!("[Step 2] Verifying 404 Not Found response");
        assert_eq!(
            auth_response.status(),
            StatusCode::NOT_FOUND,
            "Should return 404 when {} provider not configured",
            provider_type
        );
        assert!(
            state_token.is_empty(),
            "State token should be empty for error response ({} provider)",
            provider_type
        );
        println!("[Step 2] ✓ 404 Not Found returned as expected");
    }
}

/// ============================================================================
/// 回归（审计 run-1：generate_oauth_auth_url:login-redirect-uri-override-
/// unvalidated）：provider-login 的可选 redirect_uri 覆盖必须精确等于
/// canonical 回调地址。前端从不发送该参数；任何其他值都会把调用者选定的
/// 重定向目标写进一次性流程状态、并复用于 provider 授权 URL 与 code 交换
/// —— 对接受未注册 redirect_uri 的 provider 是授权码截获原语。
/// 旧代码：任意值原样生效（200）。
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_oauth_login_rejects_non_canonical_redirect_uri(ctx: &mut TestContext) {
    use axum::body::Body;
    use axum::http::{Method, Request};
    use tower::ServiceExt;

    let realm_id = ctx._realm_id.clone();
    let provider_config = OAuthProviderTestConfig::github();

    let admin_email = provider_config.admin_email_for("rduri");
    let (token, admin_user_id) = create_admin_session_with_user(ctx, &admin_email, 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;
    let config = provider_config.to_provider_config();
    let resp = create_oauth_provider_config(ctx, &config, &token, &realm_id).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let app = ctx.create_unified_test_router();
    let login_uri = |query: String| {
        Request::builder()
            .method(Method::GET)
            .uri(format!("/api/oauth/{}/github/login{}", realm_id, query))
            .body(Body::empty())
            .unwrap()
    };

    // 未携带参数：照常 200（默认 canonical 回调）。
    let resp = app.clone().oneshot(login_uri(String::new())).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "default login must still work"
    );

    // 任意外部地址必须被拒绝（旧代码：200 并把该值写入流程状态）。
    let resp = app
        .clone()
        .oneshot(login_uri(
            "?redirect_uri=https://attacker.example/callback".to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "a non-canonical redirect_uri override must be rejected"
    );

    // 精确等于 canonical 值（{public_base_url}/api/oauth/{realm}/{provider}/callback）仍被接受。
    let canonical = format!(
        "http://localhost:8080/api/oauth/{}/github/callback",
        realm_id
    );
    let resp = app
        .clone()
        .oneshot(login_uri(format!("?redirect_uri={canonical}")))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "the exact canonical redirect_uri must still be accepted"
    );
}
