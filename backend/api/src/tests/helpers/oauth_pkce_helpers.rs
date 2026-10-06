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
/// Sends POST /api/client with admin session cookie, enabled=true,
/// and the given redirect URI whitelist.
pub async fn create_client_app_with_redirect_uris(
    ctx: &mut SchemaTestContext,
    _realm_id: &str,
    admin_token: &str,
    client_id: &str,
    name: &str,
    redirect_uris: &[&str],
) -> axum::response::Response {
    let app = ctx.create_unified_test_router();

    let request = Request::builder()
        .method("POST")
        .uri("/api/client".to_string())
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

// =============================================================================
// MCP OAuth Helpers (resource/scope face, RFC 8707/9728/8414)
// =============================================================================
//
// Covers the /mcp authorization surface: the authorize endpoint's `resource`
// variant, the token exchange repeating the resource (JSON and
// form-urlencoded), the RFC 9728/8414 discovery documents, and the gated
// pass-through client registration endpoint.
//
// User Stories: docs/user-stories/integration/mcp-server.md
// =============================================================================

pub use herald_core::domain::client::MCP_CLIENT_ID;

/// Canonical MCP resource URI for a realm under the test origin, derived
/// the same way the middleware does (from the configured public base URL —
/// never a hardcoded literal that would keep passing if the pin moved).
pub fn mcp_canonical_resource(ctx: &SchemaTestContext, realm_id: &str) -> String {
    herald_core::domain::client::mcp_resource_uri(
        ctx._app_state.public_base_url.trim_end_matches('/'),
        realm_id,
    )
}

/// The port-free loopback redirect templates the built-in MCP client is
/// seeded with (migration 0011 / `seed_mcp_client_app`): three loopback host
/// forms, exact `/callback` path, any port at match time.
pub const MCP_LOOPBACK_REDIRECT_TEMPLATES: [&str; 3] = [
    "http://127.0.0.1/callback",
    "http://localhost/callback",
    "http://[::1]/callback",
];

/// Idempotently seed the realm's built-in MCP client, mirroring migration
/// `0011_mcp_oauth_client.sql` (same column values; uuidv7 + timestamps from
/// the application side like `seed_realm_api_key_client`).
///
/// WHY a test-side seed: the shared test schema's template realm is inserted
/// by `init_template_data` AFTER `run_template_migrations` runs, so migration
/// 0011's per-realm seed loop sees zero realms and the template realm has no
/// `herald-mcp` row. Realms created through `POST /api/realms` DO get the
/// seed via `create_realm` — covered by the client-app scenarios.
pub async fn ensure_mcp_client_seeded(ctx: &SchemaTestContext) -> uuid::Uuid {
    let redirect_uris = serde_json::json!(MCP_LOOPBACK_REDIRECT_TEMPLATES).to_string();
    let inserted: Option<(uuid::Uuid,)> = sqlx::query_as(
        "INSERT INTO client_app (
            id, realm_id, client_id, name, description, redirect_uris, allowed_origins,
            browser_refresh_absolute_ttl_seconds, is_first_party, enabled, client_secret,
            device_code_grant_enabled, turnstile_enabled, mcp_token_generation
        ) VALUES (
            $1, $2, 'herald-mcp', 'Herald MCP', 'Built-in read-only AI agent access client',
            $3::jsonb, '[]'::jsonb, 2592000, false, true, NULL, false, false, 0
        )
        ON CONFLICT (realm_id, client_id) DO NOTHING
        RETURNING id",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(&ctx._realm_id)
    .bind(&redirect_uris)
    .fetch_optional(&ctx._app_state.pool)
    .await
    .expect("failed to seed built-in MCP client app");

    match inserted {
        Some((id,)) => id,
        None => sqlx::query_scalar(
            "SELECT id FROM client_app WHERE realm_id = $1 AND client_id = 'herald-mcp'",
        )
        .bind(&ctx._realm_id)
        .fetch_one(&ctx._app_state.pool)
        .await
        .expect("built-in MCP client app must exist after seed"),
    }
}

/// Call the authorize endpoint with the RFC 8707 `resource` indicator (plus
/// optional scope). `scope`/`resource` are appended to the query only when
/// `Some`, so the MCP flow's absent/empty distinction stays observable.
pub async fn oauth_authorize_with_mcp(
    ctx: &SchemaTestContext,
    realm_id: &str,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    code_challenge: &str,
    code_challenge_method: &str,
    scope: Option<&str>,
    resource: Option<&str>,
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
    if let Some(resource) = resource {
        query.push_str(&format!("&resource={}", urlencoding::encode(resource)));
    }

    let request = Request::builder()
        .method("GET")
        .uri(&query)
        .body(Body::empty())
        .unwrap();

    app.oneshot(request).await.unwrap()
}

/// Exchange an authorization code repeating the MCP `resource` (JSON body,
/// snake_case fields). `resource` is only added when `Some`, so its absence
/// is the behavior under test in the invalid_target scenarios.
pub async fn oauth_token_exchange_with_resource(
    ctx: &SchemaTestContext,
    realm_id: &str,
    code: &str,
    redirect_uri: &str,
    client_id: &str,
    code_verifier: &str,
    resource: Option<&str>,
) -> axum::response::Response {
    let app = ctx.create_unified_test_router();

    let mut payload = json!({
        "grant_type": "authorization_code",
        "code": code,
        "redirect_uri": redirect_uri,
        "client_id": client_id,
        "code_verifier": code_verifier,
    });
    if let Some(resource) = resource {
        payload["resource"] = json!(resource);
    }

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/oauth/{}/token", realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(payload.to_string()))
        .unwrap();

    app.oneshot(request).await.unwrap()
}

/// RFC 6749 §4.1.3 form-urlencoded token exchange (standard OIDC/MCP client
/// wire form) with optional `resource` — must be equivalent to the JSON body
/// of [`oauth_token_exchange_with_resource`].
pub async fn oauth_token_exchange_form_with_resource(
    ctx: &SchemaTestContext,
    realm_id: &str,
    code: &str,
    redirect_uri: &str,
    client_id: &str,
    code_verifier: &str,
    resource: Option<&str>,
) -> axum::response::Response {
    let app = ctx.create_unified_test_router();

    let mut form = format!(
        "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
        urlencoding::encode(code),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(client_id),
        urlencoding::encode(code_verifier),
    );
    if let Some(resource) = resource {
        form.push_str(&format!("&resource={}", urlencoding::encode(resource)));
    }

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/oauth/{}/token", realm_id))
        .header("content-type", "application/x-www-form-urlencoded")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(form))
        .unwrap();

    app.oneshot(request).await.unwrap()
}

/// Read one query parameter off a Location header value (used to unpack the
/// OAuth error redirect the MCP authorize branch emits).
pub fn redirect_query_param(location: &str, key: &str) -> Option<String> {
    let url = url::Url::parse(location).ok()?;
    url.query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.to_string())
}

// -----------------------------------------------------------------------------
// MCP discovery + registration endpoints
// -----------------------------------------------------------------------------

/// GET `/.well-known/oauth-protected-resource/mcp/{realmId}` (RFC 9728
/// path-insertion form).
pub async fn fetch_mcp_protected_resource_metadata(
    ctx: &SchemaTestContext,
    realm_id: &str,
) -> axum::response::Response {
    let request = Request::builder()
        .method("GET")
        .uri(format!(
            "/.well-known/oauth-protected-resource/mcp/{}",
            urlencoding::encode(realm_id)
        ))
        .body(Body::empty())
        .unwrap();
    ctx.create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap()
}

/// GET `/.well-known/oauth-authorization-server/api/oauth/{realmId}`
/// (RFC 8414 path-insertion form for the path-style issuer).
pub async fn fetch_mcp_as_metadata(
    ctx: &SchemaTestContext,
    realm_id: &str,
) -> axum::response::Response {
    let request = Request::builder()
        .method("GET")
        .uri(format!(
            "/.well-known/oauth-authorization-server/api/oauth/{}",
            urlencoding::encode(realm_id)
        ))
        .body(Body::empty())
        .unwrap();
    ctx.create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap()
}

/// GET `/api/oauth/{realmId}/.well-known/openid-configuration` (legacy OIDC
/// append-form discovery — must share the MCP-era capabilities without
/// regressing the OIDC-specific fields).
pub async fn fetch_oidc_discovery_document(
    ctx: &SchemaTestContext,
    realm_id: &str,
) -> axum::response::Response {
    let request = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/oauth/{}/.well-known/openid-configuration",
            urlencoding::encode(realm_id)
        ))
        .body(Body::empty())
        .unwrap();
    ctx.create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap()
}

/// POST `/api/oauth/{realmId}/mcp/register` (gated pass-through RFC 7591
/// registration, DEC-mcp-server-007) with a JSON body and an explicit
/// Content-Type, so malformed-content-type rejections stay observable.
pub async fn mcp_register_client(
    ctx: &SchemaTestContext,
    realm_id: &str,
    body: &serde_json::Value,
    content_type: &str,
) -> axum::response::Response {
    let request = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/oauth/{}/mcp/register",
            urlencoding::encode(realm_id)
        ))
        .header("content-type", content_type)
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(body.to_string()))
        .unwrap();
    ctx.create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap()
}
