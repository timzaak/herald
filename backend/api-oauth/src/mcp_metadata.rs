//! RFC 9728 Protected Resource Metadata and RFC 8414 Authorization Server
//! metadata for the MCP resource.
//!
//! These are the discovery entry points an MCP client hits first: an
//! unauthenticated request to `/mcp/{realmId}` returns a 401 challenge
//! pointing at the PRM document here, which points at the per-realm
//! authorization server, whose AS metadata (also served here, at the
//! RFC 8414 path-style well-known insertion for the path-style issuer
//! `{origin}/api/oauth/{realmId}`) advertises the authorize/token endpoints.
//!
//! Field names are snake_case per the respective RFCs — a deliberate
//! exception to the project-wide camelCase convention, shared with the other
//! protocol-style OAuth endpoints.

use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use herald_api_base::application::http::auth::util::{ClientIp, rate_limit_hit};
use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::MCP_SCOPES_WIRE;
use herald_core::domain::client::MCP_CLIENT_ID;
use herald_core::domain::security_constants::{
    MCP_CLIENT_REGISTRATION_IP_RATE_LIMIT, OAUTH_DISCOVERY_IP_RATE_LIMIT,
};

/// Canonical MCP resource URI for a realm: `{origin}/mcp/{realmId}`, no
/// trailing slash. The origin is the configured/enabled-custom-domain base
/// (`build_realm_oauth_origin`), never a request-supplied Host.
pub(crate) fn build_mcp_resource_uri(origin: &str, realm_id: &str) -> String {
    herald_core::domain::client::mcp_resource_uri(origin, realm_id)
}

/// The scope list every discovery document (PRM, RFC 8414 AS, OIDC) must
/// advertise identically: openid plus the four MCP read scopes.
pub(crate) fn openid_plus_mcp_scopes() -> Vec<String> {
    std::iter::once("openid".to_string())
        .chain(MCP_SCOPES_WIRE.iter().map(|scope| scope.to_string()))
        .collect()
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpProtectedResourceMetadata {
    pub resource: String,
    pub authorization_servers: Vec<String>,
    pub scopes_supported: Vec<String>,
    pub bearer_methods_supported: Vec<String>,
    pub resource_name: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpAuthorizationServerMetadata {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    /// RFC 7591 dynamic client registration entry (DEC-mcp-server-007):
    /// the gated pass-through endpoint that hands out the preset public
    /// MCP client. Mainstream MCP clients (Claude Code, VS Code) treat a
    /// missing registration_endpoint as a hard incompatibility.
    pub registration_endpoint: String,
    pub scopes_supported: Vec<String>,
    pub response_types_supported: Vec<String>,
    pub grant_types_supported: Vec<String>,
    pub token_endpoint_auth_methods_supported: Vec<String>,
    pub code_challenge_methods_supported: Vec<String>,
}

/// RFC 9728 Protected Resource Metadata for a realm's MCP endpoint.
#[utoipa::path(
    get,
    path = "/.well-known/oauth-protected-resource/mcp/{realmId}",
    tag = "oauth",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
    ),
    responses(
        (status = 200, description = "Protected Resource Metadata for the realm's MCP endpoint", body = McpProtectedResourceMetadata),
        (status = 404, description = "Realm not found", body = ErrorResponse),
        (status = 429, description = "Rate limit exceeded", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    )
)]
pub async fn mcp_protected_resource_metadata(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
) -> Result<impl axum::response::IntoResponse, ApiError> {
    rate_limit_hit(
        &state,
        format!("rl:oauth-discovery:ip:{ip}"),
        OAUTH_DISCOVERY_IP_RATE_LIMIT.0,
        OAUTH_DISCOVERY_IP_RATE_LIMIT.1,
    )
    .await?;

    let ((), origin) = tokio::try_join!(
        crate::oidc_discovery::ensure_realm_exists(&state, &realm_id),
        crate::oidc_discovery::build_realm_oauth_origin(&state, &realm_id),
    )?;

    let response = McpProtectedResourceMetadata {
        resource: build_mcp_resource_uri(&origin, &realm_id),
        // Exactly one authorization server: this realm's path-style issuer.
        authorization_servers: vec![crate::oidc_discovery::build_issuer(&origin, &realm_id)],
        scopes_supported: MCP_SCOPES_WIRE.iter().map(|s| s.to_string()).collect(),
        bearer_methods_supported: vec!["header".to_string()],
        resource_name: "Herald MCP".to_string(),
    };

    tracing::debug!(realm_id = %realm_id, "MCP protected-resource metadata served");
    Ok(crate::oidc_discovery::public_cached_json(response))
}

/// RFC 8414 Authorization Server metadata for the per-realm OAuth issuer
/// behind the MCP resource. Served at the standard path-style well-known
/// insertion (well-known between host and issuer path); the legacy OIDC
/// append-form discovery URL keeps working unchanged.
#[utoipa::path(
    get,
    path = "/.well-known/oauth-authorization-server/api/oauth/{realmId}",
    tag = "oauth",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
    ),
    responses(
        (status = 200, description = "OAuth Authorization Server metadata for the realm", body = McpAuthorizationServerMetadata),
        (status = 404, description = "Realm not found", body = ErrorResponse),
        (status = 429, description = "Rate limit exceeded", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    )
)]
pub async fn oauth_authorization_server_metadata(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
) -> Result<impl axum::response::IntoResponse, ApiError> {
    rate_limit_hit(
        &state,
        format!("rl:oauth-discovery:ip:{ip}"),
        OAUTH_DISCOVERY_IP_RATE_LIMIT.0,
        OAUTH_DISCOVERY_IP_RATE_LIMIT.1,
    )
    .await?;

    let ((), origin) = tokio::try_join!(
        crate::oidc_discovery::ensure_realm_exists(&state, &realm_id),
        crate::oidc_discovery::build_realm_oauth_origin(&state, &realm_id),
    )?;
    let issuer = crate::oidc_discovery::build_issuer(&origin, &realm_id);

    let response = McpAuthorizationServerMetadata {
        issuer: issuer.clone(),
        authorization_endpoint: format!("{issuer}/authorize"),
        token_endpoint: format!("{issuer}/token"),
        registration_endpoint: format!("{issuer}/mcp/register"),
        // The only scopes with semantics: `openid` (OIDC flows) and the four
        // MCP self-face tokens. Advertised as-is so strict clients do not
        // attempt undocumented values.
        scopes_supported: openid_plus_mcp_scopes(),
        response_types_supported: vec!["code".to_string()],
        grant_types_supported: vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
        ],
        // /token is a pure PKCE public-client endpoint and never validates a
        // client secret.
        token_endpoint_auth_methods_supported: vec!["none".to_string()],
        code_challenge_methods_supported: vec!["S256".to_string()],
    };

    tracing::debug!(realm_id = %realm_id, "MCP authorization-server metadata served");
    Ok(crate::oidc_discovery::public_cached_json(response))
}

// ---------------------------------------------------------------------------
// Gated pass-through client registration (RFC 7591 shape, DEC-mcp-server-007)
// ---------------------------------------------------------------------------

/// RFC 7591 registration request. Only `client_name` and `redirect_uris`
/// carry meaning here; every other field (and any unknown field) is accepted
/// and ignored — real MCP clients send heterogeneous metadata sets.
#[derive(Debug, Deserialize, ToSchema)]
pub struct McpClientRegistrationRequest {
    pub client_name: Option<String>,
    pub redirect_uris: Option<Vec<String>>,
}

/// RFC 7591 client information response for the preset public MCP client.
/// No `client_secret` (PKCE-only public client), no `registration_client_uri`
/// and no `client_id_expires_at` — the endpoint is a stateless read
/// equivalent: every gated-in caller receives the same preset identity.
#[derive(Debug, Serialize, ToSchema)]
pub struct McpClientRegistrationResponse {
    pub client_id: String,
    pub client_name: String,
    pub redirect_uris: Vec<String>,
    pub grant_types: Vec<String>,
    pub response_types: Vec<String>,
    pub token_endpoint_auth_method: String,
    pub scope: String,
}

/// Validate the registration request shape (the "gated" half of the
/// pass-through). Every redirect URI must look like a genuine MCP desktop
/// client callback — a loopback host on one of the registered /callback
/// templates with any port — so an arbitrary https callback pointed at an
/// attacker's server never obtains the client identity from this endpoint.
fn validate_registration_request(
    request: &McpClientRegistrationRequest,
) -> Result<(), (String, String)> {
    let client_name = request.client_name.as_deref().unwrap_or("");
    // Count characters, not bytes: MCP clients register with localized
    // names, and a CJK name under 100 characters would exceed 100 bytes.
    if client_name.trim().is_empty() || client_name.chars().count() > 100 {
        return Err((
            "invalid_client_metadata".to_string(),
            "'client_name' must be 1-100 characters".to_string(),
        ));
    }
    let redirect_uris = request.redirect_uris.as_deref().unwrap_or(&[]);
    if redirect_uris.is_empty() {
        return Err((
            "invalid_client_metadata".to_string(),
            "'redirect_uris' must be a non-empty array".to_string(),
        ));
    }
    // The same matcher authorize uses against the seeded whitelist: loopback
    // hosts only, exact /callback path, any port, no userinfo/query/fragment.
    let templates: Vec<String> =
        herald_core::infrastructure::client::MCP_LOOPBACK_REDIRECT_TEMPLATES
            .iter()
            .map(|t| t.to_string())
            .collect();
    for uri in redirect_uris {
        if !crate::authorize::mcp_loopback_redirect_matches(&templates, uri) {
            return Err((
                "invalid_redirect_uri".to_string(),
                "every 'redirect_uris' entry must be an http loopback /callback URL on 127.0.0.1, localhost or [::1] (any port)".to_string(),
            ));
        }
    }
    Ok(())
}

fn registration_error(error: &str, description: &str) -> axum::response::Response {
    registration_error_at(StatusCode::BAD_REQUEST, error, description)
}

/// Same body shape as `registration_error` with a caller-chosen status —
/// the oversize-body branch is a transport limit, not invalid metadata.
fn registration_error_at(
    status: StatusCode,
    error: &str,
    description: &str,
) -> axum::response::Response {
    (
        status,
        axum::Json(serde_json::json!({
            "error": error,
            "error_description": description,
        })),
    )
        .into_response()
}

/// MCP dynamic client registration — gated pass-through form
/// (DEC-mcp-server-007). Advertised from the AS metadata as
/// `registration_endpoint` because mainstream MCP clients refuse to connect
/// without one; after the gate (rate limit + RFC 7591 shape + loopback
/// redirect validation) it returns the realm's preset public `herald-mcp`
/// client and persists NOTHING — the abuse surface therefore equals the
/// preset-client status quo (the real security boundaries stay in
/// authorize's loopback whitelist, mandatory PKCE, and the tool-layer
/// RBAC/scope gates).
#[utoipa::path(
    post,
    path = "/api/oauth/{realmId}/mcp/register",
    tag = "oauth",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
    ),
    request_body(
        description = "RFC 7591 registration request. Only client_name (1-100 chars) and redirect_uris (non-empty, every entry an http loopback /callback URL) are validated; other fields are accepted and ignored. Returns the preset public MCP client — no client_secret, no registration state.",
        content((McpClientRegistrationRequest = "application/json"))
    ),
    responses(
        (status = 201, description = "Client information for the preset Herald MCP public client", body = McpClientRegistrationResponse),
        (status = 400, description = "Invalid registration metadata (missing fields or non-loopback redirect URIs)", body = ErrorResponse),
        (status = 404, description = "Realm not found", body = ErrorResponse),
        (status = 413, description = "Registration body exceeds the 4 KiB limit", body = ErrorResponse),
        (status = 429, description = "Rate limit exceeded", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    )
)]
pub async fn mcp_client_registration(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    body: Bytes,
) -> Result<axum::response::Response, ApiError> {
    rate_limit_hit(
        &state,
        format!("rl:mcp-register:ip:{ip}"),
        MCP_CLIENT_REGISTRATION_IP_RATE_LIMIT.0,
        MCP_CLIENT_REGISTRATION_IP_RATE_LIMIT.1,
    )
    .await?;

    crate::oidc_discovery::ensure_realm_exists(&state, &realm_id).await?;

    if body.len() > 4 * 1024 {
        return Ok(registration_error_at(
            StatusCode::PAYLOAD_TOO_LARGE,
            "invalid_request",
            "registration request body exceeds 4 KiB",
        ));
    }
    // JSON only per RFC 7591 §1.2; the discovery endpoints advertise this
    // endpoint to JSON-native MCP clients.
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if !content_type
        .split(';')
        .next()
        .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
    {
        return Ok(registration_error(
            "invalid_client_metadata",
            "content type must be application/json",
        ));
    }
    let request: McpClientRegistrationRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(_) => {
            return Ok(registration_error(
                "invalid_client_metadata",
                "registration request body is not valid JSON",
            ));
        }
    };
    if let Err((error, description)) = validate_registration_request(&request) {
        return Ok(registration_error(&error, &description));
    }

    tracing::debug!(realm_id = %realm_id, "MCP client registration served (pass-through)");

    let response = McpClientRegistrationResponse {
        client_id: MCP_CLIENT_ID.to_string(),
        client_name: "Herald MCP".to_string(),
        redirect_uris: herald_core::infrastructure::client::MCP_LOOPBACK_REDIRECT_TEMPLATES
            .iter()
            .map(|t| t.to_string())
            .collect(),
        grant_types: vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
        ],
        response_types: vec!["code".to_string()],
        token_endpoint_auth_method: "none".to_string(),
        scope: MCP_SCOPES_WIRE.join(" "),
    };
    Ok((StatusCode::CREATED, Json(response)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(
        redirect_uris: Option<Vec<String>>,
        client_name: Option<&str>,
    ) -> McpClientRegistrationRequest {
        McpClientRegistrationRequest {
            client_name: client_name.map(str::to_string),
            redirect_uris,
        }
    }

    // WHY: the gate is what keeps this DCR "controlled" — only requests that
    // look like a genuine MCP desktop client (loopback /callback, any port)
    // may receive the preset client identity.
    #[test]
    fn registration_validation_accepts_loopback_callback_shapes() {
        for uri in [
            "http://127.0.0.1:43119/callback",
            "http://localhost:8080/callback",
            "http://[::1]:65535/callback",
            "http://localhost/callback",
        ] {
            let req = request(Some(vec![uri.to_string()]), Some("Claude Code"));
            assert!(
                validate_registration_request(&req).is_ok(),
                "must accept {uri}"
            );
        }
    }

    #[test]
    fn registration_validation_rejects_non_loopback_or_malformed_redirects() {
        for uri in [
            "https://attacker.example/callback",
            "http://evil.example/callback",
            "http://127.0.0.1:43119/other",
            "http://127.0.0.1:43119/callback?x=1",
            "http://127.0.0.2/callback",
            "not a url",
        ] {
            let req = request(Some(vec![uri.to_string()]), Some("probe"));
            let err = validate_registration_request(&req).unwrap_err();
            assert_eq!(err.0, "invalid_redirect_uri", "must reject {uri}");
        }
    }

    #[test]
    fn registration_validation_requires_name_and_redirects() {
        assert!(validate_registration_request(&request(None, None)).is_err());
        assert!(validate_registration_request(&request(Some(vec![]), Some("x"))).is_err());
        let too_long = "a".repeat(101);
        assert!(
            validate_registration_request(&request(
                Some(vec!["http://localhost/callback".to_string()]),
                Some(&too_long)
            ))
            .is_err()
        );
        let exactly_100 = "a".repeat(100);
        assert!(
            validate_registration_request(&request(
                Some(vec!["http://localhost/callback".to_string()]),
                Some(&exactly_100)
            ))
            .is_ok()
        );
    }
}
