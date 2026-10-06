// OAuth token endpoint: authorization-code exchange (PKCE) and the standard
// refresh grant for MCP credentials.
//
// Browser clients exchange an authorization code (obtained via the authorize
// + login flow) for a Bearer token family; PKCE ensures the code cannot be
// intercepted and reused. MCP clients use the same code exchange to mint an
// MCP-scoped family bound to the RFC 8707 resource, and refresh it through
// the standard `refresh_token` grant — the only rotation path for MCP
// families (the browser refresh endpoint rejects them).

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use herald_api_base::application::http::auth::util::{
    ClientIp, rate_limit_hit, user_agent_from_headers,
};
use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::{CredentialScope, RefreshBinding, RefreshError};
use herald_core::domain::client::is_mcp_client;
use herald_core::domain::client::ports::ClientService;
use herald_core::domain::security_constants::OAUTH_TOKEN_IP_RATE_LIMIT;
use herald_core::domain::user::UserRepository;
use herald_core::infrastructure::authentication::RedisBrowserTokenService;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use utoipa::ToSchema;

const FIRST_PARTY_CALLBACK_PATH: &str = "/callback";

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

/// OAuth 2.0 token request (RFC 6749), grant-discriminated.
///
/// Field names use snake_case per OAuth 2.0 specification rather than the
/// project-wide camelCase convention. `authorization_code` requires
/// code/redirect_uri/client_id/code_verifier; `refresh_token` (MCP only this
/// round) requires refresh_token/client_id/resource and accepts an optional
/// scope (absent = inherit, explicit = must equal the original set).
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct TokenRequest {
    pub grant_type: String,
    pub code: Option<String>,
    pub redirect_uri: Option<String>,
    pub client_id: Option<String>,
    pub code_verifier: Option<String>,
    pub refresh_token: Option<String>,
    /// RFC 8707 resource indicator. The MCP authorization_code exchange must
    /// repeat the code's resource; the refresh grant requires the bound
    /// resource.
    pub resource: Option<String>,
    /// Refresh grant only: absent inherits the original scope set, an
    /// explicit value must equal it exactly.
    pub scope: Option<String>,
}

/// OAuth 2.0 token response (RFC 6749)
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub refresh_expires_in: u64,
    /// OIDC identity token (RS256 JWT). Present only when the authorization
    /// request carried the `openid` scope; omitted otherwise so non-OIDC
    /// responses stay byte-identical.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
}

/// MCP token response: the RFC 6749 fields plus the granted `scope`
/// (stable-sorted, space-joined). Never carries an id_token.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct McpTokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub refresh_expires_in: u64,
    pub scope: String,
}

/// RFC 6749 §5.2 token error body for the MCP grant surface.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct OAuthTokenError {
    pub error: String,
    pub error_description: String,
}

// ---------------------------------------------------------------------------
// PKCE verification
// ---------------------------------------------------------------------------

/// Verify PKCE code_verifier against stored code_challenge.
///
/// Computes BASE64URL(SHA256(code_verifier)) and compares to the stored challenge.
fn verify_pkce(code_verifier: &str, code_challenge: &str) -> bool {
    let mut hasher = Sha256::new();
    hasher.update(code_verifier.as_bytes());
    let hash = hasher.finalize();
    let computed = URL_SAFE_NO_PAD.encode(hash);
    computed == code_challenge
}

/// RFC 7636 §4.1 verifier shape: 43..=128 characters from the unreserved set
/// `ALPHA / DIGIT / "-" / "." / "_" / "~"`. The MCP clients rely on the
/// full range; checking it here rejects malformed verifiers before the
/// challenge comparison would silently fail on a non-matching digest.
fn validate_pkce_verifier(code_verifier: &str) -> Result<(), (String, String)> {
    let len = code_verifier.len();
    let charset_ok = code_verifier
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'));
    if (43..=128).contains(&len) && charset_ok {
        Ok(())
    } else {
        Err((
            "invalid_request".to_string(),
            "code_verifier must be 43-128 characters from the unreserved set".to_string(),
        ))
    }
}

/// Parse the token request body.
///
/// Standard OIDC/OAuth clients POST `application/x-www-form-urlencoded` per
/// RFC 6749 §4.1.3 — the discovery document advertises this endpoint to them
/// (Grafana-style "issuer URL only" setups) — while the first-party flow this
/// endpoint was built on uses JSON. Both content types deserialize into the
/// same `TokenRequest` (field names are snake_case, matching the OAuth
/// parameter names), so one struct serves both wire forms.
fn parse_token_request(headers: &HeaderMap, body: &[u8]) -> Result<TokenRequest, ApiError> {
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let is_form = content_type.split(';').next().is_some_and(|mime| {
        mime.trim()
            .eq_ignore_ascii_case("application/x-www-form-urlencoded")
    });
    if is_form {
        serde_urlencoded::from_bytes(body)
            .map_err(|_| ApiError::bad_request("invalid token request body"))
    } else {
        serde_json::from_slice(body)
            .map_err(|_| ApiError::bad_request("invalid token request body"))
    }
}

/// 400 with the RFC 6742 §5.2 error body (MCP grant surface).
fn token_grant_error(error: &str, description: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        [(header::CACHE_CONTROL, "no-store")],
        axum::Json(OAuthTokenError {
            error: error.to_string(),
            error_description: description.to_string(),
        }),
    )
        .into_response()
}

fn mcp_token_response(response: &McpTokenResponse) -> Response {
    // RFC 6749 §5.1: token responses must not be stored.
    (
        StatusCode::OK,
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::PRAGMA, "no-cache"),
        ],
        axum::Json(response),
    )
        .into_response()
}

/// Wire form of a granted MCP scope set: stable sort, space-joined.
fn mcp_scope_wire(scopes: &HashSet<CredentialScope>) -> String {
    let mut wires: Vec<&str> = scopes.iter().filter_map(|s| s.mcp_wire()).collect();
    wires.sort_unstable();
    wires.join(" ")
}

/// Parse a (normalize-at-authorize) MCP scope string into the scope set.
/// Every token must be one of the four MCP wires — authorize already
/// enforced this, so an unknown token here means a tampered state record.
fn parse_mcp_scope(scope: &str) -> Result<HashSet<CredentialScope>, (String, String)> {
    let mut scopes = HashSet::new();
    for token in scope.split_whitespace() {
        match CredentialScope::from_mcp_wire(token) {
            Some(scope) => {
                scopes.insert(scope);
            }
            None => {
                return Err((
                    "invalid_scope".to_string(),
                    format!("unknown scope token '{token}'"),
                ));
            }
        }
    }
    Ok(scopes)
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/api/oauth/{realmId}/token",
    tag = "oauth",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
    ),
    request_body(
        description = "Dual encoding per openid-connect.md §5.1: JSON (first-party flow) or RFC 6749 §4.1.3 form-urlencoded (standard OIDC/MCP clients); both deserialize into the same snake_case field names. Grants: authorization_code (all clients), refresh_token (built-in MCP client only)",
        content(
            (TokenRequest = "application/json"),
            (TokenRequest = "application/x-www-form-urlencoded")
        )
    ),
    responses(
        (status = 200, description = "Access token issued", body = TokenResponse),
        (status = 400, description = "Bad request / grant error", body = ErrorResponse),
    )
)]
#[tracing::instrument(
    // Governance: body carries authorization code, PKCE
    // code_verifier, refresh tokens, client_id — all credentials/secrets.
    // state holds handles; realm_id conservatively skipped; headers carries
    // User-Agent/cookies, ip may be PII. Only http.route is recorded.
    skip(state, body, headers, ip),
    fields(http.route = "/api/oauth/{realmId}/token")
)]
pub async fn oauth_token(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    // Per-IP cap mirroring /authorize: each request costs a Redis GETDEL plus
    // client_app/user DB reads, so an unauthenticated code flood must not hit
    // Redis/DB at network speed.
    rate_limit_hit(
        &state,
        format!("rl:oauth-token:ip:{ip}"),
        OAUTH_TOKEN_IP_RATE_LIMIT.0,
        OAUTH_TOKEN_IP_RATE_LIMIT.1,
    )
    .await?;

    let req = parse_token_request(&headers, &body)?;

    // Request-side MCP discrimination: the error-body contract differs by
    // client class (design §4.2/§4.3). MCP grant errors are the RFC 6749
    // OAuthTokenError shape; every other client keeps the historical
    // ApiError shape so existing integrations' error parsing is untouched.
    let is_mcp_request = req.client_id.as_deref().is_some_and(is_mcp_client);

    match req.grant_type.as_str() {
        "authorization_code" => {
            token_authorization_code(&state, &realm_id, &ip, &headers, req, is_mcp_request).await
        }
        "refresh_token" => token_refresh(&state, &realm_id, &ip, req).await,
        _ => {
            if is_mcp_request {
                Ok(token_grant_error(
                    "unsupported_grant_type",
                    "grant_type must be 'authorization_code' or 'refresh_token'",
                ))
            } else {
                Err(ApiError::bad_request(
                    "grant_type must be 'authorization_code'",
                ))
            }
        }
    }
}

async fn token_authorization_code(
    state: &AppState,
    realm_id: &str,
    ip: &str,
    headers: &HeaderMap,
    req: TokenRequest,
    is_mcp_request: bool,
) -> Result<Response, ApiError> {
    let user_agent = user_agent_from_headers(headers);
    let missing_field = |field: &str| -> Result<Response, ApiError> {
        if is_mcp_request {
            Ok(token_grant_error("invalid_request", field))
        } else {
            Err(ApiError::bad_request("invalid token request body"))
        }
    };

    let Some(code) = req.code.clone() else {
        return missing_field("'code' is required");
    };
    let Some(request_redirect_uri) = req.redirect_uri.clone() else {
        return missing_field("'redirect_uri' is required");
    };
    let Some(request_client_id) = req.client_id.clone() else {
        return missing_field("'client_id' is required");
    };
    let Some(code_verifier) = req.code_verifier.clone() else {
        return missing_field("'code_verifier' is required");
    };
    // The verifier shape bound (43..=128 unreserved) is part of the MCP
    // client contract; applying it to every client would reject legacy
    // third-party verifiers that only ever faced the digest comparison.
    if is_mcp_request && let Err((error, description)) = validate_pkce_verifier(&code_verifier) {
        return Ok(token_grant_error(&error, &description));
    }

    // Atomically get-and-delete authorization code (one-time use)
    let mut conn = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Internal server error".to_string()))?;

    let key = format!("oauth:code:{}", code);
    let code_json: Option<String> = redis::cmd("GETDEL")
        .arg(&key)
        .query_async(&mut conn)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Redis GETDEL failed for OAuth authorization code");
            ApiError::internal("Internal server error".to_string())
        })?;

    let code_json = match code_json {
        Some(json) => json,
        // The code was already consumed, expired, or never existed. For the
        // MCP grant surface the RFC 6749 error body is the client contract
        // (a 200-capable agent retries authorization, never a form login);
        // the first-party flow keeps its historical ApiError shape.
        None => {
            if is_mcp_client(&request_client_id) {
                return Ok(token_grant_error(
                    "invalid_grant",
                    "authorization code is invalid, expired, or already used",
                ));
            }
            return Err(ApiError::bad_request(
                "Invalid or expired authorization code".to_string(),
            ));
        }
    };

    let stored: serde_json::Value = serde_json::from_str(&code_json).map_err(|e| {
        tracing::error!(error = %e, "Failed to parse authorization code data");
        ApiError::internal("Internal server error".to_string())
    })?;

    let stored_client_id = stored["client_id"].as_str().unwrap_or("");
    let stored_redirect_uri = stored["redirect_uri"].as_str().unwrap_or("");
    let stored_realm_id = stored["realm_id"].as_str().unwrap_or("");
    let stored_user_id = stored["user_id"].as_str().unwrap_or("");
    let stored_code_challenge = stored["code_challenge"].as_str().unwrap_or("");

    let is_mcp = is_mcp_client(stored_client_id);

    // The MCP code's resource binding is mandatory and must be repeated by
    // the exchange request; any other audience this round is refused.
    let mcp_resource = if is_mcp {
        match stored["resource"].as_str() {
            Some(resource) if req.resource.as_deref() == Some(resource) => {
                Some(resource.to_string())
            }
            _ => {
                return Ok(token_grant_error(
                    "invalid_target",
                    "the 'resource' parameter must repeat the resource the authorization code was issued for",
                ));
            }
        }
    } else {
        if req.resource.is_some() {
            // Mirrors authorize's non-MCP rejection, including its ApiError
            // body: this is the pre-existing non-MCP error surface.
            return Err(ApiError::bad_request(
                "The 'resource' parameter is not supported for this client",
            ));
        }
        None
    };

    if let Err(e) = validate_code_bindings(
        stored_client_id,
        stored_redirect_uri,
        stored_realm_id,
        stored_code_challenge,
        realm_id,
        &req,
    ) {
        if is_mcp {
            return Ok(token_grant_error("invalid_grant", &e.to_string()));
        }
        return Err(e);
    }

    let client_app = state
        .service
        .client_service()
        .get_client_app_by_client_id(realm_id, &request_client_id)
        .await
        .map_err(map_client_error)?;
    if !client_app.enabled {
        if is_mcp {
            return Ok(token_grant_error(
                "invalid_grant",
                "OAuth client app is not enabled",
            ));
        }
        return Err(ApiError::bad_request("OAuth client app is not enabled"));
    }
    if client_app.is_first_party {
        validate_first_party_redirect(&state.public_base_url, &request_redirect_uri)?;
    }

    let user_id = match uuid::Uuid::parse_str(stored_user_id) {
        Ok(user_id) => user_id,
        Err(_) => {
            // MCP clients parse the RFC 6749 error body; a corrupted code
            // record is an invalid_grant for them, not an ApiError.
            if is_mcp {
                return Ok(token_grant_error(
                    "invalid_grant",
                    "authorization code user is invalid",
                ));
            }
            return Err(ApiError::bad_request("authorization code user is invalid"));
        }
    };
    let user = state
        .user_repository
        .get_user_by_id(user_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, %user_id, "OAuth token user lookup failed");
            ApiError::bad_request("authorization code user is invalid")
        })?;
    if user.realm_id != realm_id {
        if is_mcp {
            return Ok(token_grant_error(
                "invalid_grant",
                "authorization code user is invalid",
            ));
        }
        return Err(ApiError::bad_request("authorization code user is invalid"));
    }
    // The login entrances already reject disabled users, but the
    // authorization code outlives the login by up to its TTL; a user
    // disabled in between must not receive credentials of any face.
    if is_mcp && user.status.is_disabled() {
        return Ok(token_grant_error(
            "invalid_grant",
            "authorization code user is invalid",
        ));
    }

    let token_service = RedisBrowserTokenService::new(state.redis_manager.clone());

    if is_mcp {
        let Some(mcp_resource) = mcp_resource else {
            return Err(ApiError::internal("Internal server error"));
        };
        let granted_scopes = match stored["scope"].as_str() {
            // Authorize normalizes the MCP scope (absent → the profile-read
            // minimum, empty → the empty set).
            Some(scope) => parse_mcp_scope(scope).map_err(|(error, description)| {
                ApiError::internal(format!("stored MCP scope invalid: {error} {description}"))
            })?,
            None => HashSet::from([CredentialScope::McpProfileRead]),
        };
        let scope_wire_value = mcp_scope_wire(&granted_scopes);

        let tokens = token_service
            .create_mcp_token_family(
                &user,
                &client_app,
                &mcp_resource,
                granted_scopes,
                user_agent,
                Some(ip.to_string()),
            )
            .await
            .map_err(|error| {
                tracing::error!(%error, "MCP token issuance failed");
                ApiError::internal("Internal server error")
            })?;

        return Ok(mcp_token_response(&McpTokenResponse {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            token_type: tokens.token_type,
            expires_in: tokens.expires_in,
            refresh_expires_in: tokens.refresh_expires_in,
            scope: scope_wire_value,
        }));
    }

    // OIDC layer: mint an id_token only when the authorization request asked
    // for the `openid` scope. Everything inside this branch is incremental —
    // a flow without `openid` produces the identical response as before.
    //
    // The mint runs BEFORE the token family is written: every step in it can
    // fail (disabled user, profile read, signing-key fetch/decrypt/sign), and
    // once the family lands in Redis the response may no longer fail — a 500
    // after that point would burn the one-time code, strand an orphan token
    // family per retry, and force the user back through login.
    let stored_scope = stored["scope"].as_str();
    let openid_requested = herald_api_auth::oauth_oidc::scope_requests_openid(stored_scope);
    let id_token = if openid_requested {
        // The login-side entrances already reject disabled users, but the
        // authorization code outlives the login by up to its TTL; a user
        // disabled in between must not receive an identity token.
        if user.status.is_disabled() {
            return Err(ApiError::bad_request("authorization code user is invalid"));
        }
        let nonce = stored["nonce"].as_str().map(str::to_string);
        // The profile row, the active signing key, and the realm origin are
        // independent reads — fetch them concurrently instead of paying three
        // serial DB round-trips on the login critical path.
        let (nickname, active_key, origin) = tokio::try_join!(
            crate::oidc_id_token::profile_nickname(state, &user),
            async {
                state
                    .oidc_signing_key_store
                    .active_signing_key()
                    .await
                    .map_err(|error| {
                        tracing::error!(%error, %realm_id, "OIDC active signing key unavailable");
                        ApiError::internal("Internal server error")
                    })
            },
            crate::oidc_discovery::build_realm_oauth_origin(state, realm_id),
        )?;
        Some(crate::oidc_id_token::issue_id_token(
            realm_id,
            &request_client_id,
            &user,
            nickname,
            nonce,
            &active_key,
            &origin,
        )?)
    } else {
        None
    };

    let tokens = token_service
        .create_oauth_token_family(
            &user,
            &client_app,
            user_agent,
            Some(ip.to_string()),
            openid_requested,
        )
        .await
        .map_err(|error| {
            tracing::error!(%error, "OAuth browser token issuance failed");
            ApiError::internal("Internal server error")
        })?;

    // Best-effort audit after the family write — it never fails the exchange.
    if openid_requested {
        crate::oidc_id_token::audit_id_token_issued(state, realm_id, &user, &request_client_id, ip)
            .await;
    }

    Ok(axum::Json(TokenResponse {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        token_type: tokens.token_type,
        expires_in: tokens.expires_in,
        refresh_expires_in: tokens.refresh_expires_in,
        id_token,
    })
    .into_response())
}

/// Standard refresh grant — MCP families only this round. The trusted family
/// record supplies the audience/generation/scope bindings; the request may
/// only repeat them, never change them.
async fn token_refresh(
    state: &AppState,
    realm_id: &str,
    ip: &str,
    req: TokenRequest,
) -> Result<Response, ApiError> {
    let Some(refresh_token) = req.refresh_token.as_deref() else {
        return Ok(token_grant_error(
            "invalid_request",
            "'refresh_token' is required",
        ));
    };
    let Some(request_client_id) = req.client_id.as_deref() else {
        return Ok(token_grant_error(
            "invalid_request",
            "'client_id' is required",
        ));
    };
    if !is_mcp_client(request_client_id) {
        return Ok(token_grant_error(
            "unsupported_grant_type",
            "the refresh_token grant is only available to the built-in MCP client",
        ));
    }
    let Some(request_resource) = req.resource.as_deref() else {
        return Ok(token_grant_error(
            "invalid_request",
            "'resource' is required for the refresh_token grant",
        ));
    };

    let token_service = RedisBrowserTokenService::new(state.redis_manager.clone());
    let Some(current) = token_service
        .lookup_refresh_context(refresh_token)
        .await
        .map_err(|error| {
            tracing::error!(%error, "MCP refresh context lookup failed");
            ApiError::internal("Internal server error")
        })?
    else {
        return Ok(token_grant_error(
            "invalid_grant",
            "refresh token is invalid or expired",
        ));
    };
    let Some(family) = token_service
        .lookup_refresh_family(current.family_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, "MCP refresh family lookup failed");
            ApiError::internal("Internal server error")
        })?
    else {
        return Ok(token_grant_error(
            "invalid_grant",
            "refresh token is invalid or expired",
        ));
    };

    // Trusted bindings from the family record — the request can only repeat
    // them, and the rotation function re-checks every field atomically.
    let (Some(family_audience), Some(family_generation)) =
        (family.audience.clone(), family.mcp_token_generation)
    else {
        return Ok(token_grant_error(
            "invalid_grant",
            "refresh token is invalid or expired",
        ));
    };

    if current.realm_id != realm_id {
        return Ok(token_grant_error("invalid_grant", "realm mismatch"));
    }
    if family_audience != request_resource {
        return Ok(token_grant_error(
            "invalid_target",
            "the 'resource' parameter must equal the resource this refresh token was issued for",
        ));
    }
    if let Some(scope) = req.scope.as_deref() {
        let requested = match parse_mcp_scope(scope) {
            Ok(requested) => requested,
            Err((error, description)) => {
                return Ok(token_grant_error(&error, &description));
            }
        };
        if requested != family.allowed_scopes {
            return Ok(token_grant_error(
                "invalid_scope",
                "the refresh grant cannot change the granted scope set; restart authorization for a different scope",
            ));
        }
    }

    // Live DB rechecks: client enabled, generation current, user valid.
    let client_app = state
        .service
        .client_service()
        .get_client_app_by_client_id(realm_id, request_client_id)
        .await
        .map_err(map_client_error)?;
    if !client_app.enabled {
        return Ok(token_grant_error(
            "invalid_grant",
            "client app is not enabled",
        ));
    }
    if client_app.id != family.client_app_id {
        return Ok(token_grant_error("invalid_grant", "client mismatch"));
    }
    if client_app.mcp_token_generation != family_generation {
        // Disabled (and possibly re-enabled) since issuance: the whole
        // family is dead by construction (DEC-mcp-server-006).
        return Ok(token_grant_error(
            "invalid_grant",
            "refresh token is invalid or expired",
        ));
    }

    let user_id = uuid::Uuid::parse_str(&current.user_id)
        .map_err(|_| ApiError::internal("Internal server error".to_string()))?;
    let user = state
        .user_repository
        .get_user_by_id(user_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, %user_id, "MCP refresh user lookup failed");
            ApiError::internal("Internal server error")
        })?;
    if user.realm_id != realm_id || user.status.is_disabled() {
        return Ok(token_grant_error(
            "invalid_grant",
            "refresh token is invalid or expired",
        ));
    }

    let scope_wire_value = mcp_scope_wire(&family.allowed_scopes);
    let binding = RefreshBinding {
        realm_id: family.realm_id.clone(),
        client_app_id: family.client_app_id,
        audience: family_audience,
        mcp_token_generation: family_generation,
    };
    match token_service
        .refresh_mcp(refresh_token, &current, &family, &binding)
        .await
    {
        Ok(tokens) => Ok(mcp_token_response(&McpTokenResponse {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            token_type: tokens.token_type,
            expires_in: tokens.expires_in,
            refresh_expires_in: tokens.refresh_expires_in,
            scope: scope_wire_value,
        })),
        Err(RefreshError::Invalid) | Err(RefreshError::ReuseDetected) => {
            tracing::warn!(realm_id = %realm_id, client_id = %request_client_id, ip = %ip, "MCP refresh rejected");
            Ok(token_grant_error(
                "invalid_grant",
                "refresh token is invalid, expired, or revoked",
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn map_client_error(
    error: herald_core::domain::common::entities::app_errors::CoreError,
) -> ApiError {
    use herald_core::domain::common::entities::app_errors::CoreError;
    match error {
        CoreError::NotFound => ApiError::bad_request("OAuth client app is not enabled"),
        error => {
            tracing::error!(%error, "OAuth Client App lookup failed");
            ApiError::internal("Internal server error")
        }
    }
}

fn validate_code_bindings(
    stored_client_id: &str,
    stored_redirect_uri: &str,
    stored_realm_id: &str,
    stored_code_challenge: &str,
    realm_id: &str,
    request: &TokenRequest,
) -> Result<(), ApiError> {
    if stored_client_id != request.client_id.as_deref().unwrap_or("") {
        return Err(ApiError::bad_request("client_id mismatch"));
    }
    if stored_redirect_uri != request.redirect_uri.as_deref().unwrap_or("") {
        return Err(ApiError::bad_request("redirect_uri mismatch"));
    }
    if stored_realm_id != realm_id {
        return Err(ApiError::bad_request("realm_id mismatch"));
    }
    if !verify_pkce(
        request.code_verifier.as_deref().unwrap_or(""),
        stored_code_challenge,
    ) {
        return Err(ApiError::bad_request("PKCE verification failed"));
    }
    Ok(())
}

pub(crate) fn validate_first_party_redirect(
    frontend_url: &str,
    redirect_uri: &str,
) -> Result<(), ApiError> {
    let expected = format!(
        "{}{}",
        frontend_url.trim_end_matches('/'),
        FIRST_PARTY_CALLBACK_PATH
    );
    url::Url::parse(&expected).map_err(|_| ApiError::internal("Frontend URL is invalid"))?;
    if redirect_uri != expected {
        return Err(ApiError::bad_request("redirect_uri mismatch"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::{CONTENT_TYPE, HeaderMap};

    fn json_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());
        headers
    }

    fn form_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            "application/x-www-form-urlencoded".parse().unwrap(),
        );
        headers
    }

    // WHY: standard OIDC clients (Grafana-style) POST form-encoded per RFC
    // 6749 §4.1.3 and the discovery document points them at this endpoint —
    // the JSON body the first-party flow uses must not be the only wire form.
    #[test]
    fn parse_token_request_accepts_form_and_json_bodies() {
        let from_form = parse_token_request(
            &form_headers(),
            b"grant_type=authorization_code&code=code-123&redirect_uri=https%3A%2F%2Fapp.example.com%2Fcb&client_id=client-a&code_verifier=verifier",
        )
        .expect("form-encoded token request must parse");
        assert_eq!(from_form.grant_type, "authorization_code");
        assert_eq!(from_form.code.as_deref(), Some("code-123"));
        assert_eq!(
            from_form.redirect_uri.as_deref(),
            Some("https://app.example.com/cb")
        );
        assert_eq!(from_form.client_id.as_deref(), Some("client-a"));
        assert_eq!(from_form.code_verifier.as_deref(), Some("verifier"));

        let json_body = serde_json::json!({
            "grant_type": "authorization_code",
            "code": "code-123",
            "redirect_uri": "https://app.example.com/cb",
            "client_id": "client-a",
            "code_verifier": "verifier",
        })
        .to_string()
        .into_bytes();
        let from_json = parse_token_request(&json_headers(), &json_body)
            .expect("JSON token request must keep parsing");
        assert_eq!(from_json.code, from_form.code);
    }

    #[test]
    fn parse_token_request_accepts_refresh_grant_fields() {
        let from_form = parse_token_request(
            &form_headers(),
            b"grant_type=refresh_token&refresh_token=rt&client_id=herald-mcp&resource=https%3A%2F%2Fh.example%2Fmcp%2Facme",
        )
        .expect("refresh grant form body must parse");
        assert_eq!(from_form.grant_type, "refresh_token");
        assert_eq!(from_form.refresh_token.as_deref(), Some("rt"));
        assert_eq!(
            from_form.resource.as_deref(),
            Some("https://h.example/mcp/acme")
        );
    }

    #[test]
    fn parse_token_request_rejects_unparseable_bodies() {
        // A form body of unknown-but-well-formed pairs parses (fields are
        // all-Option and grant-validated later); form_urlencoded is lenient
        // about stray bytes. The JSON branch rejects non-JSON bytes and
        // wrongly-typed fields outright.
        assert!(parse_token_request(&form_headers(), b"grant_type=code&x=1").is_ok());
        assert!(parse_token_request(&json_headers(), b"not json at all").is_err());
        assert!(parse_token_request(&json_headers(), br#"{"grant_type": 5}"#).is_err());
    }

    // WHY (openid-connect.md §5.1 双编码): the wire handler accepts both
    // encodings, but standard OIDC clients generate calls from the OpenAPI
    // contract — a JSON-only requestBody leaves the RFC 6749 form path
    // undocumented to anyone reading the spec.
    #[test]
    fn openapi_token_request_body_declares_json_and_form() {
        use utoipa::OpenApi as _;
        let doc = crate::ApiDoc::openapi();
        let item = doc
            .paths
            .paths
            .get("/api/oauth/{realmId}/token")
            .expect("token path must be registered in ApiDoc");
        let post = item
            .post
            .as_ref()
            .expect("token path must have a POST operation");
        let content = &post
            .request_body
            .as_ref()
            .expect("token operation must declare a request body")
            .content;
        for content_type in ["application/json", "application/x-www-form-urlencoded"] {
            assert!(
                content.contains_key(content_type),
                "token requestBody must declare {content_type}; declared: {:?}",
                content.keys().collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn verify_pkce_correct_challenge() {
        // Known test vector: code_verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"
        // SHA256 + BASE64URL(no pad) = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert!(verify_pkce(verifier, challenge));
    }

    #[test]
    fn verify_pkce_wrong_verifier_fails() {
        let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert!(!verify_pkce("wrong_verifier", challenge));
    }

    #[test]
    fn verify_pkce_empty_inputs() {
        // Empty verifier with empty challenge: SHA256("") base64url'd
        let hash = {
            let mut h = Sha256::new();
            h.update(b"");
            URL_SAFE_NO_PAD.encode(h.finalize())
        };
        assert!(verify_pkce("", &hash));
    }

    // WHY: RFC 7636 verifier bounds are part of the MCP PKCE contract; a
    // too-short or charset-violating verifier must fail validation, not just
    // digest-mismatch.
    #[test]
    fn pkce_verifier_shape_follows_rfc7636() {
        assert!(validate_pkce_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk").is_ok());
        assert!(validate_pkce_verifier("short").is_err());
        let long = "a".repeat(129);
        assert!(validate_pkce_verifier(&long).is_err());
        assert!(validate_pkce_verifier(&"b".repeat(43)).is_ok());
        let bad_charset = "c".repeat(42) + "!";
        assert!(validate_pkce_verifier(&bad_charset).is_err());
    }

    // WHY: the scope wire set is the MCP capability contract; the response
    // must echo it in a stable order so clients can diff authorizations.
    #[test]
    fn mcp_scope_wire_is_sorted_and_space_joined() {
        let scopes: HashSet<CredentialScope> = [
            CredentialScope::McpSubscriptionsRead,
            CredentialScope::McpProfileRead,
        ]
        .into_iter()
        .collect();
        assert_eq!(
            mcp_scope_wire(&scopes),
            "mcp:profile:read mcp:subscriptions:read"
        );
        assert_eq!(mcp_scope_wire(&HashSet::new()), "");
    }

    #[test]
    fn parse_mcp_scope_rejects_non_mcp_tokens() {
        assert!(parse_mcp_scope("mcp:profile:read mcp:points:read").is_ok());
        assert!(parse_mcp_scope("openid").is_err());
        assert!(parse_mcp_scope("mcp:admin:write").is_err());
        assert!(parse_mcp_scope("").unwrap().is_empty());
    }

    #[test]
    fn first_party_redirect_must_exactly_match_server_frontend_callback() {
        assert!(
            validate_first_party_redirect("https://herald.test/", "https://herald.test/callback")
                .is_ok()
        );
        assert!(
            validate_first_party_redirect("https://herald.test", "https://evil.test/callback")
                .is_err()
        );
        assert!(
            validate_first_party_redirect(
                "https://herald.test",
                "https://herald.test/callback/extra"
            )
            .is_err()
        );
    }

    #[test]
    fn first_party_wrong_client_app_is_rejected_by_code_binding() {
        let request = TokenRequest {
            grant_type: "authorization_code".into(),
            code: Some("one-time-code".into()),
            redirect_uri: Some("https://herald.test/callback".into()),
            client_id: Some("attacker-client".into()),
            code_verifier: Some("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".into()),
            refresh_token: None,
            resource: None,
            scope: None,
        };
        assert!(
            validate_code_bindings(
                "admin-web-console",
                "https://herald.test/callback",
                "admin",
                "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
                "admin",
                &request,
            )
            .is_err(),
            "an authorization code must remain bound to its original Client App"
        );
    }
}
