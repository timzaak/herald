// =============================================================================
// OAuth PKCE Test Helpers
// =============================================================================
//
// Helper functions for OAuth Authorization Code + PKCE scenario tests.
// Provides PKCE cryptographic utilities, authorize/token endpoint helpers,
// and client app setup with redirect URIs.
//
// =============================================================================

#![allow(dead_code)]

use crate::tests::schema_test_context::SchemaTestContext;
use axum::{body::Body, http::Request};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use redis::AsyncCommands;
use serde_json::json;
use sha2::{Digest, Sha256};
use tower::ServiceExt;

// =============================================================================
// PKCE Cryptographic Utilities
// =============================================================================

/// Generate a random 43-character Base64url string suitable for use as a
/// PKCE code_verifier (RFC 7636 requires 43-128 chars).
pub fn generate_code_verifier() -> String {
    let bytes: [u8; 32] = rand::random();
    URL_SAFE_NO_PAD.encode(bytes) // produces exactly 43 chars
}

/// Compute the PKCE code_challenge from a code_verifier:
/// BASE64URL(SHA256(code_verifier)) with no padding.
pub fn compute_code_challenge(code_verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(code_verifier.as_bytes());
    let hash = hasher.finalize();
    URL_SAFE_NO_PAD.encode(hash)
}

/// Generate a random state token for CSRF protection.
pub fn generate_state() -> String {
    let bytes: [u8; 32] = rand::random();
    URL_SAFE_NO_PAD.encode(bytes)
}

// =============================================================================
// OAuth Endpoint Helpers
// =============================================================================

/// Call the authorize endpoint: GET /api/oauth/{realmId}/authorize
///
/// Sends all required OAuth + PKCE query parameters and returns the raw
/// response (302 redirect on success, or error status).
pub async fn oauth_authorize(
    ctx: &SchemaTestContext,
    realm_id: &str,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    code_challenge: &str,
    code_challenge_method: &str,
) -> axum::response::Response {
    oauth_authorize_with_oidc(
        ctx,
        realm_id,
        client_id,
        redirect_uri,
        state,
        code_challenge,
        code_challenge_method,
        None,
        None,
    )
    .await
}

/// Call the authorize endpoint with optional OIDC parameters.
///
/// `scope` and `nonce` are appended to the query only when `Some`, so a flow
/// without them stays byte-identical to the pre-OIDC authorize request.
pub async fn oauth_authorize_with_oidc(
    ctx: &SchemaTestContext,
    realm_id: &str,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    code_challenge: &str,
    code_challenge_method: &str,
    scope: Option<&str>,
    nonce: Option<&str>,
) -> axum::response::Response {
    let app = ctx.create_unified_test_router();

    let mut query = format!(
        "/api/oauth/{}/authorize?client_id={}&redirect_uri={}&state={}&response_type=code&code_challenge={}&code_challenge_method={}",
        realm_id,
        urlencoding::encode(client_id),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(state),
        urlencoding::encode(code_challenge),
        urlencoding::encode(code_challenge_method),
    );
    if let Some(scope) = scope {
        query.push_str(&format!("&scope={}", urlencoding::encode(scope)));
    }
    if let Some(nonce) = nonce {
        query.push_str(&format!("&nonce={}", urlencoding::encode(nonce)));
    }

    let request = Request::builder()
        .method("GET")
        .uri(&query)
        .body(Body::empty())
        .unwrap();

    app.oneshot(request).await.unwrap()
}

/// Call the token exchange endpoint: POST /api/oauth/{realmId}/token
///
/// Sends a JSON body with snake_case field names per OAuth spec.
pub async fn oauth_token_exchange(
    ctx: &SchemaTestContext,
    realm_id: &str,
    grant_type: &str,
    code: &str,
    redirect_uri: &str,
    client_id: &str,
    code_verifier: &str,
) -> axum::response::Response {
    let app = ctx.create_unified_test_router();

    let payload = json!({
        "grant_type": grant_type,
        "code": code,
        "redirect_uri": redirect_uri,
        "client_id": client_id,
        "code_verifier": code_verifier,
    });

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/oauth/{}/token", realm_id))
        .header("content-type", "application/json")
        // token issuance now requires a verifiable client IP (fail-loud);
        // mirror the proxy header the production ingress sets.
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(payload.to_string()))
        .unwrap();

    app.oneshot(request).await.unwrap()
}

// =============================================================================
// Client App Setup Helper
// =============================================================================

/// Create a Client App with specified redirect URIs via the admin API.
///
/// Sends POST /api/client/{realmId} with admin session cookie, enabled=true,
/// and the given redirect URI whitelist.
pub async fn create_client_app_with_redirect_uris(
    ctx: &mut SchemaTestContext,
    realm_id: &str,
    admin_token: &str,
    client_id: &str,
    name: &str,
    redirect_uris: &[&str],
) -> axum::response::Response {
    let app = ctx.create_unified_test_router();

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/client/{}", realm_id))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", admin_token))
        .body(Body::from(
            json!({
                "clientId": client_id,
                "name": name,
                "description": format!("{} test app", name),
                "redirectUris": redirect_uris,
                "enabled": true,
                "browserRefreshAbsoluteTtlSeconds": 86400,
            })
            .to_string(),
        ))
        .unwrap();

    app.oneshot(request).await.unwrap()
}

// =============================================================================
// Login + TOTP Helpers (OAuth context)
// =============================================================================

/// Call the login endpoint with OAuth context fields.
///
/// Sends POST /api/auth/{realmId}/login with camelCase fields including
/// oauthClientId, redirectUri, and state.
pub async fn login_with_oauth(
    ctx: &SchemaTestContext,
    realm_id: &str,
    email: &str,
    password: &str,
    oauth_client_id: &str,
    redirect_uri: &str,
    state: &str,
) -> axum::response::Response {
    let app = ctx.create_unified_test_router();

    let payload = json!({
        "clientId": "admin-web-console",
        "email": email,
        "password": password,
        "turnstileToken": "dummy",
        "oauthClientId": oauth_client_id,
        "redirectUri": redirect_uri,
        "state": state,
    });

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(payload.to_string()))
        .unwrap();

    app.oneshot(request).await.unwrap()
}

/// Call the verify-totp endpoint with a temp token and TOTP code.
///
/// Sends POST /api/auth/{realmId}/login/verify-totp with camelCase fields.
pub async fn verify_totp_with_oauth(
    ctx: &SchemaTestContext,
    realm_id: &str,
    temp_token: &str,
    totp_code: &str,
) -> axum::response::Response {
    let app = ctx.create_unified_test_router();

    let payload = json!({
        "tempToken": temp_token,
        "code": totp_code,
    });

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login/verify-totp", realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(payload.to_string()))
        .unwrap();

    app.oneshot(request).await.unwrap()
}

// =============================================================================
// Redis Direct Helpers
// =============================================================================

/// Delete the oauth:code:{code} key from Redis to simulate code expiry.
pub async fn delete_oauth_code_redis(ctx: &SchemaTestContext, code: &str) {
    let mut conn = ctx._app_state.redis_manager.get().await.unwrap();
    let key = format!("oauth:code:{}", code);
    let _: () = conn.del(&key).await.unwrap();
}

/// Extract the authorization code from a redirectTo URL.
///
/// Parses the `code` query parameter from a URL like:
/// `{redirect_uri}?code={auth_code}&state={state}`
pub fn extract_auth_code_from_redirect(redirect_to: &str) -> Option<String> {
    let url = url::Url::parse(redirect_to).ok()?;
    for (key, value) in url.query_pairs() {
        if key == "code" {
            return Some(value.to_string());
        }
    }
    None
}
