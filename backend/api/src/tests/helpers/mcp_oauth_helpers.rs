// =============================================================================
// MCP OAuth Test Helpers
// =============================================================================
//
// Drives the full browser-authorized MCP credential chain over the real HTTP
// routes: authorize (RFC 8707 resource + PKCE + MCP scopes) -> password login
// -> authorization-code exchange. Every step asserts its wire shape (302
// login redirect, `ac_` code prefix, snake_case token fields, no-store
// headers) so a contract drift in any hop fails inside the helper with a
// precise message instead of surfacing later as an opaque 401.
//
// User Story: docs/user-stories/integration/mcp-server.md (US-MCP-001)
// =============================================================================

#![allow(dead_code)]

use crate::tests::helpers::oauth_pkce_helpers::{
    compute_code_challenge, extract_auth_code_from_redirect, generate_code_verifier, generate_state,
};
use crate::tests::schema_test_context::SchemaTestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use serde_json::{Value, json};
use tower::ServiceExt;

/// The built-in public MCP client, seeded per realm.
pub const MCP_CLIENT_ID: &str = "herald-mcp";

/// All four self-face MCP scopes, in the server's stable (sorted) wire order.
pub const MCP_ALL_SCOPES: &str =
    "mcp:points:read mcp:profile:read mcp:subscriptions:read mcp:transactions:read";

/// The minimal default scope authorize grants when none is requested.
pub const MCP_PROFILE_SCOPE: &str = "mcp:profile:read";

/// A loopback callback on the localhost template (port is arbitrary for
/// authorize; the token exchange repeats the exact same URI).
const MCP_TEST_REDIRECT_URI: &str = "http://localhost:34567/callback";

/// Tokens obtained from one completed MCP authorization chain.
pub struct McpTokenSet {
    pub access_token: String,
    pub refresh_token: String,
    /// The granted scope set as the server renders it (sorted, space-joined).
    pub scope: String,
}

/// The realm's canonical MCP resource URI. Derived from the configured
/// `public_base_url` — the same source the middleware uses — never from the
/// ephemeral listener a test happens to connect through, because audience
/// validation only ever sees the configured canonical.
pub fn mcp_canonical_resource(ctx: &SchemaTestContext, realm_id: &str) -> String {
    let origin = ctx._app_state.public_base_url.trim_end_matches('/');
    format!("{origin}/mcp/{realm_id}")
}

/// The RFC 9728 protected-resource metadata URL the 401/403 challenges carry.
pub fn mcp_prm_url(ctx: &SchemaTestContext, realm_id: &str) -> String {
    let origin = ctx._app_state.public_base_url.trim_end_matches('/');
    format!("{origin}/.well-known/oauth-protected-resource/mcp/{realm_id}")
}

/// Render a scope set exactly as the token endpoint does: sorted, space-joined.
pub fn sorted_scope_wire(scope: &str) -> String {
    let mut tokens: Vec<&str> = scope.split_whitespace().collect();
    tokens.sort_unstable();
    tokens.join(" ")
}

/// The expected token-response scope for an authorize `scope` parameter:
/// absent defaults to the profile-read minimum (the server-side rule).
pub fn expected_granted_scope(authorize_scope: Option<&str>) -> String {
    match authorize_scope {
        None => MCP_PROFILE_SCOPE.to_string(),
        Some(scope) => sorted_scope_wire(scope),
    }
}

/// Run the full MCP authorization chain for an existing user:
/// authorize -> login -> code -> token. Asserts each hop's contract shape.
pub async fn obtain_mcp_tokens(
    ctx: &SchemaTestContext,
    realm_id: &str,
    email: &str,
    password: &str,
    scope: Option<&str>,
) -> McpTokenSet {
    // The schema-test template realm predates migration 0011's seed loop
    // (test-db migrates before the template realm exists), so the built-in
    // MCP client must be ensured before this chain can authorize — and it
    // seeds DISABLED by default, so the admin opt-in enable is part of the
    // precondition for every browser-authorized MCP chain.
    crate::tests::helpers::oauth_pkce_helpers::ensure_mcp_client_enabled(ctx).await;
    let resource = mcp_canonical_resource(ctx, realm_id);
    let redirect_uri = MCP_TEST_REDIRECT_URI;
    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state = generate_state();

    // Authorize: MCP requires the canonical resource; success is a 302 to the
    // realm login page carrying the OAuth client parameters.
    let mut query = format!(
        "/api/oauth/{realm_id}/authorize?client_id={client_id}&redirect_uri={redirect_uri}&state={state}&response_type=code&code_challenge={code_challenge}&code_challenge_method=S256",
        client_id = urlencoding::encode(MCP_CLIENT_ID),
        redirect_uri = urlencoding::encode(redirect_uri),
        state = urlencoding::encode(&state),
        code_challenge = urlencoding::encode(&code_challenge),
    );
    if let Some(scope) = scope {
        query.push_str(&format!("&scope={}", urlencoding::encode(scope)));
    }
    query.push_str(&format!("&resource={}", urlencoding::encode(&resource)));

    let authorize_response = ctx
        .create_unified_test_router()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(&query)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        authorize_response.status(),
        StatusCode::FOUND,
        "MCP authorize must redirect to the login page"
    );
    let location = authorize_response
        .headers()
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("authorize must send a Location header")
        .to_string();
    assert!(
        location.starts_with(&format!(
            "/{realm_id}/auth/login?clientId=admin-web-console&oauthClientId={MCP_CLIENT_ID}"
        )),
        "authorize must hand the OAuth context to the login page, got: {location}"
    );

    // Login with the OAuth context; the pending server-side state (never the
    // login form) supplies the resource/scope the code will be bound to.
    let login_response = ctx
        .create_unified_test_router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/auth/{realm_id}/login"))
                .header("content-type", "application/json")
                .header("x-forwarded-for", "3.3.3.3")
                .body(Body::from(
                    json!({
                        "clientId": "admin-web-console",
                        "email": email,
                        "password": password,
                        "turnstileToken": "dummy",
                        "oauthClientId": MCP_CLIENT_ID,
                        "redirectUri": redirect_uri,
                        "state": state,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        login_response.status(),
        StatusCode::OK,
        "login inside the MCP chain must succeed"
    );
    let login_json: Value = crate::tests::response_json(login_response).await;
    assert_eq!(
        login_json["requiresTotp"].as_bool(),
        Some(false),
        "the helper only drives the password path"
    );
    let redirect_to = login_json["redirectTo"]
        .as_str()
        .expect("login must return redirectTo for the OAuth flow")
        .to_string();
    assert!(
        redirect_to.starts_with(redirect_uri),
        "redirectTo must target the loopback callback, got: {redirect_to}"
    );
    let code = extract_auth_code_from_redirect(&redirect_to)
        .expect("redirectTo must carry the authorization code");
    assert!(
        code.starts_with("ac_"),
        "authorization codes use the ac_ prefix, got: {code}"
    );

    // Token exchange: JSON body, snake_case fields, resource repeated. The
    // response must carry the MCP scope wire form and no-store cache headers.
    let token_response = ctx
        .create_unified_test_router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/oauth/{realm_id}/token"))
                .header("content-type", "application/json")
                .header("x-forwarded-for", "3.3.3.3")
                .body(Body::from(
                    json!({
                        "grant_type": "authorization_code",
                        "code": code,
                        "redirect_uri": redirect_uri,
                        "client_id": MCP_CLIENT_ID,
                        "code_verifier": code_verifier,
                        "resource": resource,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        token_response.status(),
        StatusCode::OK,
        "the MCP token exchange must succeed"
    );
    assert_eq!(
        token_response
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok()),
        Some("no-store"),
        "token responses must not be cacheable"
    );
    assert_eq!(
        token_response
            .headers()
            .get(header::PRAGMA)
            .and_then(|v| v.to_str().ok()),
        Some("no-cache"),
        "token responses must not be cacheable"
    );
    let token_json: Value = crate::tests::response_json(token_response).await;
    let access_token = token_json["access_token"]
        .as_str()
        .expect("access_token must be a string")
        .to_string();
    let refresh_token = token_json["refresh_token"]
        .as_str()
        .expect("refresh_token must be a string")
        .to_string();
    assert!(!access_token.is_empty() && !refresh_token.is_empty());
    assert_eq!(token_json["token_type"].as_str(), Some("Bearer"));
    assert!(
        token_json["expires_in"].as_i64().unwrap_or(0) > 0,
        "expires_in must be positive"
    );
    assert!(
        token_json["refresh_expires_in"].as_i64().unwrap_or(0) > 0,
        "refresh_expires_in must be positive"
    );
    assert!(
        token_json.get("id_token").is_none(),
        "the MCP grant never issues an id_token"
    );
    let expected_scope = expected_granted_scope(scope);
    assert_eq!(
        token_json["scope"].as_str(),
        Some(expected_scope.as_str()),
        "the granted scope set must be echoed in its stable wire form"
    );

    McpTokenSet {
        access_token,
        refresh_token,
        scope: expected_scope,
    }
}

/// POST the standard MCP refresh grant (the only rotation path for MCP
/// families). `scope` is optional: absent inherits, explicit must equal.
pub async fn mcp_refresh_request(
    ctx: &SchemaTestContext,
    realm_id: &str,
    refresh_token: &str,
    client_id: &str,
    resource: &str,
    scope: Option<&str>,
) -> axum::response::Response {
    let mut payload = json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "client_id": client_id,
        "resource": resource,
    });
    if let Some(scope) = scope {
        payload["scope"] = json!(scope);
    }
    ctx.create_unified_test_router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/oauth/{realm_id}/token"))
                .header("content-type", "application/json")
                .header("x-forwarded-for", "3.3.3.3")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

/// POST the browser refresh endpoint with an arbitrary refresh token — used
/// to prove MCP refresh tokens are rejected there without being consumed.
pub async fn browser_refresh_request(
    ctx: &SchemaTestContext,
    refresh_token: &str,
) -> axum::response::Response {
    ctx.create_unified_test_router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/browser-token/refresh")
                .header("content-type", "application/json")
                .header("x-forwarded-for", "3.3.3.3")
                .body(Body::from(
                    json!({ "refreshToken": refresh_token }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap()
}
