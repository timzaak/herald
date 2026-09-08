// =============================================================================
// OAuth Test Helpers
// =============================================================================
//
// Reusable helper functions for OAuth provider testing to eliminate code duplication
// between google_oauth_scenarios.rs and github_oauth_scenarios.rs
//
// =============================================================================

use crate::tests::schema_test_context::SchemaTestContext;
use axum::{
    body::Body,
    http::{Method, Request},
};
use serde_json::json;
use tower::ServiceExt;

/// OAuth provider configuration data
#[derive(Debug, Clone)]
pub struct OAuthProviderConfig {
    pub provider_type: String,
    pub client_id: String,
    pub client_secret: String,
    pub scopes: Vec<String>,
    pub enabled: bool,
}

impl OAuthProviderConfig {
    pub fn new(
        provider_type: &str,
        client_id: &str,
        client_secret: &str,
        scopes: Vec<String>,
        enabled: bool,
    ) -> Self {
        Self {
            provider_type: provider_type.to_string(),
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            scopes,
            enabled,
        }
    }
}

/// Create OAuth provider configuration
///
/// Returns the token and creates the provider config in the database
pub async fn create_oauth_provider_config(
    ctx: &mut SchemaTestContext,
    config: &OAuthProviderConfig,
    token: &str,
    realm_id: &str,
) -> axum::response::Response {
    let app = ctx.create_unified_test_router();

    let create_payload = json!({
        "providerType": config.provider_type,
        "clientId": config.client_id,
        "clientSecret": config.client_secret,
        "scopes": config.scopes,
        "enabled": config.enabled
    });

    let create_request = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/oauth/{}/configs", realm_id))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", token))
        .body(Body::from(create_payload.to_string()))
        .unwrap();

    app.oneshot(create_request).await.unwrap()
}

/// Generate OAuth authorization URL and extract state token
pub async fn generate_auth_url_and_extract_state(
    ctx: &mut SchemaTestContext,
    realm_id: &str,
    provider_type: &str,
) -> (axum::response::Response, String) {
    let app = ctx.create_unified_test_router();

    let auth_request = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/oauth/{}/{}/login", realm_id, provider_type))
        .body(Body::empty())
        .unwrap();

    let auth_response = app.oneshot(auth_request).await.unwrap();

    // Save the original status
    let status = auth_response.status();

    // Extract JSON body to get auth_url and state
    let body = axum::body::to_bytes(auth_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");

    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Handle error responses (404, 400, etc.) that don't have state
    if !status.is_success() {
        return (
            axum::response::Response::builder()
                .status(status)
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
            String::new(),
        );
    }

    // oauth_login is a B-class interface (OAuth protocol), so response is direct JSON: { state: "...", authUrl: "..." }
    let state_token = json["state"]
        .as_str()
        .expect("Response should contain state")
        .to_string();

    // Recreate response with the original status and body
    let auth_response = axum::response::Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();

    (auth_response, state_token)
}

/// Send OAuth callback with code and state
pub async fn send_oauth_callback(
    ctx: &mut SchemaTestContext,
    realm_id: &str,
    provider_type: &str,
    code: &str,
    state_token: &str,
) -> Result<axum::response::Response, tower::BoxError> {
    let app = ctx.create_unified_test_router();

    let callback_query = format!("code={}&state={}", code, state_token);

    let callback_request = Request::builder()
        .method(Method::GET)
        .uri(format!(
            "/api/oauth/{}/{}/callback?{}",
            realm_id, provider_type, callback_query
        ))
        .body(Body::empty())
        .unwrap();

    app.oneshot(callback_request).await.map_err(|e| match e {})
}

/// Get OAuth provider configuration by type
pub async fn get_oauth_provider_config(
    ctx: &mut SchemaTestContext,
    realm_id: &str,
    provider_type: &str,
    token: &str,
) -> axum::response::Response {
    let app = ctx.create_unified_test_router();

    let get_request = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/oauth/{}/configs/{}", realm_id, provider_type))
        .header("authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    app.oneshot(get_request).await.unwrap()
}

/// List enabled OAuth providers
pub async fn list_enabled_oauth_providers(
    ctx: &mut SchemaTestContext,
    realm_id: &str,
    token: &str,
) -> axum::response::Response {
    let app = ctx.create_unified_test_router();

    let list_request = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/oauth/{}/configs?enabled=true", realm_id))
        .header("authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    app.oneshot(list_request).await.unwrap()
}

/// Delete OAuth provider configuration
pub async fn delete_oauth_provider_config(
    ctx: &mut SchemaTestContext,
    realm_id: &str,
    provider_type: &str,
    token: &str,
) -> axum::response::Response {
    let app = ctx.create_unified_test_router();

    let delete_request = Request::builder()
        .method(Method::DELETE)
        .uri(format!("/api/oauth/{}/configs/{}", realm_id, provider_type))
        .header("authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    app.oneshot(delete_request).await.unwrap()
}

/// Verify provider config in database
pub async fn verify_provider_config_in_db(
    ctx: &SchemaTestContext,
    realm_id: &str,
    provider_type: &str,
    expected_client_id: &str,
    expected_scopes: &[String],
    expected_enabled: bool,
) {
    let (db_client_id, db_scopes, db_enabled): (String, Vec<String>, bool) = sqlx::query_as(
        "SELECT client_id, scopes, enabled FROM oauth_provider_config WHERE realm_id = $1 AND provider_type = $2"
    )
    .bind(realm_id)
    .bind(provider_type)
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("Failed to query oauth_providers");

    assert_eq!(db_client_id, expected_client_id);
    assert_eq!(db_scopes, expected_scopes);
    assert_eq!(db_enabled, expected_enabled);
}

/// Assert the consentRequired shape a gated OAuth login entrance must answer
/// with (fresh user, zero consent records), then run the recovery loop the
/// real client would: record explicit consent through the consent-restricted
/// browser family via the actual POST /api/legal/{realm}/consent endpoint.
/// After this returns, re-triggering the same entrance yields its normal
/// response (the downstream_state is deliberately left unconsumed by the
/// gated path).
///
/// `body` is the parsed JSON body of the gated entrance response.
pub async fn assert_consent_required_and_recover(
    ctx: &mut SchemaTestContext,
    body: &serde_json::Value,
) {
    // The entrance must withhold the downstream authorization code and answer
    // consentRequired with a consent-restricted browser family instead —
    // issuing the code now would hand the downstream app a session over an
    // un-consented account.
    assert_eq!(
        body["consentRequired"].as_bool(),
        Some(true),
        "downstream mode must run the login consent gate before code issuance; got {body}"
    );
    assert!(
        body["redirectUri"].as_str().is_none(),
        "no downstream code may be issued while consent is required; got {body}"
    );
    let agreements = body["agreements"]
        .as_array()
        .expect("consentRequired response must list current agreement summaries");
    assert!(
        agreements.len() >= 2,
        "seed defaults provide ToS + Privacy; got {agreements:?}"
    );
    let restricted_access_token = body["restrictedSession"]["accessToken"]
        .as_str()
        .expect("consentRequired response must carry a restricted session")
        .to_string();

    // The provider credential is stateless (id_token re-verified against
    // JWKS), so the same entrance replays cleanly after consent.
    let consent_items: Vec<serde_json::Value> = agreements
        .iter()
        .map(|a| {
            json!({
                "agreement_type": a["agreement_type"],
                "version_id": a["version_id"],
            })
        })
        .collect();
    let consent_req = Request::builder()
        .method("POST")
        .uri(format!("/api/legal/{}/consent", ctx._realm_id))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {restricted_access_token}"))
        .body(Body::from(
            json!({ "agreements": consent_items }).to_string(),
        ))
        .expect("failed to build consent request");
    let consent_resp = ctx
        .create_unified_test_router()
        .oneshot(consent_req)
        .await
        .expect("consent request must dispatch");
    assert_eq!(
        consent_resp.status(),
        axum::http::StatusCode::NO_CONTENT,
        "restricted session must be able to record consent"
    );
}
