//! OAuth authorization endpoint for third-party application integration (Authorization Code + PKCE)
//!
//! This endpoint implements the first step of the OAuth 2.1 Authorization Code + PKCE flow:
//! 1. Validates client_id, redirect_uri, and PKCE code_challenge
//! 2. Stores state token with PKCE parameters in Redis (CSRF protection)
//! 3. Redirects to the Herald login page with OAuth parameters
//! 4. After login, an authorization_code is generated for token exchange

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use utoipa::ToSchema;

use herald_api_base::application::http::auth::util::{ClientIp, rate_limit_hit};
use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::security_constants::{
    OAUTH_AUTHORIZE_EXTRA_PARAM_MAX_BYTES, OAUTH_AUTHORIZE_IP_RATE_LIMIT, OAUTH_STATE_TTL_SECONDS,
};

#[derive(Debug, Deserialize, ToSchema)]
pub struct AuthorizeQueryParams {
    pub client_id: String,
    pub redirect_uri: String,
    pub state: String,
    #[serde(default = "default_response_type")]
    pub response_type: String,
    pub code_challenge: String,
    pub code_challenge_method: Option<String>,
    /// Space-delimited OAuth/OIDC scope tokens. Only the presence of the
    /// literal `openid` token has meaning (triggers id_token issuance); other
    /// tokens are stored verbatim and never interpreted. The built-in MCP
    /// client is the exception: its scope may only contain the four
    /// `mcp:*:read` tokens.
    pub scope: Option<String>,
    /// OIDC protocol security parameter, echoed back into the issued
    /// id_token's `nonce` claim.
    pub nonce: Option<String>,
    /// RFC 8707 resource indicator. Required for (and only valid on) the
    /// built-in MCP client, where it must equal the realm's canonical
    /// `{origin}/mcp/{realmId}` resource URI.
    pub resource: Option<String>,
}

fn default_response_type() -> String {
    "code".to_string()
}

/// OAuth authorize endpoint (Authorization Code + PKCE)
///
/// Initiates the Authorization Code + PKCE flow:
/// 1. Validates client_id exists and is enabled
/// 2. Validates redirect_uri is in whitelist (exact match only)
/// 3. Validates PKCE code_challenge_method (must be S256 if provided)
/// 4. Stores state token with PKCE parameters in Redis (5 minutes TTL)
/// 5. Redirects to login page with OAuth parameters
///
/// # Arguments
/// * `realm_id` - Realm identifier
/// * `params` - OAuth query parameters (client_id, redirect_uri, state, response_type, code_challenge, code_challenge_method)
///
/// # Returns
/// * 302 redirect to login page with OAuth parameters
///
/// # Errors
/// * 400 - Invalid parameters (missing client_id, redirect_uri, state, or code_challenge)
/// * 400 - Invalid response_type (must be "code")
/// * 400 - Unsupported code_challenge_method (must be "S256")
/// * 404 - Client not found
/// * 403 - Client app is disabled
/// * 400 - Redirect URI not in whitelist
#[utoipa::path(
    get,
    path = "/api/oauth/{realmId}/authorize",
    tag = "oauth",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("client_id" = String, Query, description = "OAuth Client ID"),
        ("redirect_uri" = String, Query, description = "Redirect URI (must be in whitelist, exact match)"),
        ("state" = String, Query, description = "State token (CSRF protection)"),
        ("response_type" = String, Query, description = "Response type (must be 'code')"),
        ("code_challenge" = String, Query, description = "PKCE code challenge (SHA256 + Base64url)"),
        ("code_challenge_method" = Option<String>, Query, description = "PKCE method (must be 'S256' if provided, defaults to S256)"),
        ("scope" = Option<String>, Query, description = "Space-delimited scope tokens; presence of the literal 'openid' token triggers id_token issuance, other tokens are not interpreted. The built-in MCP client accepts only the four mcp:*:read scopes (absent defaults to mcp:profile:read)"),
        ("nonce" = Option<String>, Query, description = "OIDC nonce, echoed into the id_token nonce claim when openid is requested"),
        ("resource" = Option<String>, Query, description = "RFC 8707 resource indicator; required on (and only valid for) the built-in MCP client, where it must equal the realm's canonical {origin}/mcp/{realmId} URI")
    ),
    responses(
        (status = 302, description = "Redirect to /{realmId}/auth/login with OAuth parameters"),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 403, description = "Client app is disabled", body = ErrorResponse),
        (status = 404, description = "Client not found", body = ErrorResponse)
    )
)]
pub async fn oauth_authorize(
    Path(realm_id): Path<String>,
    Query(params): Query<AuthorizeQueryParams>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
) -> Result<impl IntoResponse, ApiError> {
    // Validate response_type (must be "code" for Authorization Code + PKCE)
    if params.response_type != "code" {
        return Err(ApiError::bad_request(format!(
            "Invalid response_type '{}'. Only 'code' is supported.",
            params.response_type
        )));
    }

    // Per-IP cap: each request costs a client_app DB read and a Redis state
    // write, so an unauthenticated flood can fill Redis.
    rate_limit_hit(
        &state,
        format!("rl:oauth-authorize:ip:{ip}"),
        OAUTH_AUTHORIZE_IP_RATE_LIMIT.0,
        OAUTH_AUTHORIZE_IP_RATE_LIMIT.1,
    )
    .await?;

    // Validate code_challenge_method (only S256 is supported)
    if let Some(ref method) = params.code_challenge_method
        && method != "S256"
    {
        return Err(ApiError::bad_request(format!(
            "Unsupported code_challenge_method '{}'. Only 'S256' is supported.",
            method
        )));
    }

    // The parameters stored server-side need a size bound so an unauthenticated
    // caller cannot turn the authorize endpoint into a Redis-storage write
    // primitive: scope/nonce/resource are stored verbatim in the state JSON
    // value, and `state` itself becomes the Redis KEY name while
    // `code_challenge` is stored in the value (audit run-2:
    // oauth-state-seeding-unbounded-state-and-code-challenge).
    for (name, value) in [
        ("scope", params.scope.as_deref()),
        ("nonce", params.nonce.as_deref()),
        ("resource", params.resource.as_deref()),
        ("code_challenge", Some(params.code_challenge.as_str())),
        ("state", Some(params.state.as_str())),
    ] {
        if let Some(value) = value
            && value.len() > OAUTH_AUTHORIZE_EXTRA_PARAM_MAX_BYTES
        {
            return Err(ApiError::bad_request(format!(
                "'{name}' exceeds {OAUTH_AUTHORIZE_EXTRA_PARAM_MAX_BYTES} bytes"
            )));
        }
    }

    // Validate client_id and redirect_uri
    let client_row = sqlx::query_as::<_, (String, String, bool, bool)>(
        "SELECT id::text, redirect_uris::text, enabled, is_first_party FROM client_app
         WHERE realm_id = $1 AND client_id = $2",
    )
    .bind(&realm_id)
    .bind(&params.client_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(
            realm_id = %realm_id,
            client_id = %params.client_id,
            error = %e,
            "Database query failed: client_app lookup"
        );
        ApiError::internal("Database query failed".to_string())
    })?;

    let Some((_id, redirect_uris, enabled, is_first_party)) = client_row else {
        tracing::debug!(
            realm_id = %realm_id,
            client_id = %params.client_id,
            "OAuth authorize failed: client app not found"
        );
        return Err(ApiError::not_found(format!(
            "Client app with client_id '{}' not found in realm '{}'",
            params.client_id, realm_id
        )));
    };

    if !enabled {
        tracing::debug!(
            realm_id = %realm_id,
            client_id = %params.client_id,
            "OAuth authorize failed: client app is disabled"
        );
        return Err(ApiError::forbidden("Client app is disabled".to_string()));
    }

    // Validate redirect_uri is in whitelist
    let allowed_uris: Vec<String> = serde_json::from_str(&redirect_uris)
        .map_err(|_| ApiError::internal("Failed to parse redirect URIs".to_string()))?;

    let client_is_mcp = herald_core::domain::client::is_mcp_client(&params.client_id);

    if client_is_mcp {
        // MCP-only loopback matching (RFC 8252 §7.3): scheme/host/path must
        // equal a registered template with ANY port, and HTTP loopback is
        // acceptable in production (the general HTTPS rule below does not
        // apply to this whitelist class).
        if !mcp_loopback_redirect_matches(&allowed_uris, &params.redirect_uri) {
            tracing::debug!(
                realm_id = %realm_id,
                client_id = %params.client_id,
                redirect_uri = %params.redirect_uri,
                "OAuth authorize failed: MCP redirect_uri not in loopback whitelist"
            );
            return Err(ApiError::bad_request(format!(
                "Redirect URI '{}' is not in the whitelist for client '{}'",
                params.redirect_uri, params.client_id
            )));
        }
    } else {
        // Enforce HTTPS in production. Local OAuth clients use localhost HTTP in dev/demo.
        herald_core::domain::client::validation::validate_redirect_uri(
            &params.redirect_uri,
            !herald_core::config::is_production(&state.app_env),
        )
        .map_err(|e| ApiError::bad_request(format!("Invalid redirect_uri: {}", e)))?;

        let is_whitelisted = if is_first_party {
            crate::token::validate_first_party_redirect(
                &state.public_base_url,
                &params.redirect_uri,
            )
            .is_ok()
        } else {
            allowed_uris.contains(&params.redirect_uri)
        };

        if !is_whitelisted {
            tracing::debug!(
                realm_id = %realm_id,
                client_id = %params.client_id,
                redirect_uri = %params.redirect_uri,
                "OAuth authorize failed: redirect_uri not in whitelist"
            );
            return Err(ApiError::bad_request(format!(
                "Redirect URI '{}' is not in the whitelist for client '{}'",
                params.redirect_uri, params.client_id
            )));
        }

        // A resource parameter on a non-MCP client has no supported target
        // this round: browser credentials never carry an audience.
        if params.resource.is_some() {
            return Err(ApiError::bad_request(
                "The 'resource' parameter is not supported for this client".to_string(),
            ));
        }
    }

    // MCP contract checks run after the callback was validated, so their
    // failures can propagate through the standard OAuth error redirect.
    let mcp_resource = if client_is_mcp {
        let origin = crate::oidc_discovery::build_realm_oauth_origin(&state, &realm_id).await?;
        let canonical = crate::mcp_metadata::build_mcp_resource_uri(&origin, &realm_id);

        let resource = match params.resource.as_deref() {
            Some(resource) if resource == canonical => resource.to_string(),
            _ => {
                tracing::debug!(
                    realm_id = %realm_id,
                    resource = ?params.resource,
                    "MCP authorize failed: missing/mismatched resource parameter"
                );
                return Ok(mcp_authorize_error_redirect(
                    &params.redirect_uri,
                    &params.state,
                    "invalid_target",
                    "The 'resource' parameter must equal this realm's canonical MCP resource URI",
                ));
            }
        };

        // Scope: every token must be one of the four MCP scopes (absent
        // defaults to the profile-read minimum; an explicitly empty scope is
        // the empty set — the client can still connect and list tools, every
        // self tool will challenge). `openid` and anything else is rejected.
        let normalized_scope = match params.scope.as_deref() {
            None => Some(herald_core::domain::authentication::MCP_DEFAULT_SCOPE.to_string()),
            Some("") => Some(String::new()),
            Some(scope) => {
                let mut tokens = Vec::new();
                for token in scope.split_whitespace() {
                    match herald_core::domain::authentication::CredentialScope::from_mcp_wire(token)
                    {
                        Some(_) => tokens.push(token.to_string()),
                        None => {
                            tracing::debug!(
                                realm_id = %realm_id,
                                token = %token,
                                "MCP authorize failed: invalid scope token"
                            );
                            return Ok(mcp_authorize_error_redirect(
                                &params.redirect_uri,
                                &params.state,
                                "invalid_scope",
                                "Only the mcp:profile:read, mcp:points:read, mcp:transactions:read and mcp:subscriptions:read scopes are supported",
                            ));
                        }
                    }
                }
                Some(tokens.join(" "))
            }
        };

        Some((resource, normalized_scope))
    } else {
        None
    };

    // Store state token in Redis with PKCE parameters (5 minutes TTL, CSRF protection)
    let state_key = format!("oauth:state:{}", params.state);
    let mut state_value = serde_json::json!({
        "client_id": params.client_id,
        "realm_id": realm_id,
        "redirect_uri": params.redirect_uri,
        "code_challenge": params.code_challenge,
        "code_challenge_method": params.code_challenge_method.as_deref().unwrap_or("S256"),
    });
    // OIDC fields are only added when present, keeping the stored state (and
    // every downstream behavior) byte-identical for non-OIDC flows. The MCP
    // flow writes its normalized scope (defaulted/validated above) and the
    // canonical resource instead of the raw parameters.
    let (stored_scope, stored_resource) = match &mcp_resource {
        Some((resource, normalized_scope)) => (normalized_scope.clone(), Some(resource.clone())),
        None => (params.scope.clone(), None),
    };
    herald_api_auth::oauth_oidc::insert_optional_oidc_fields(
        &mut state_value,
        herald_api_auth::oauth_oidc::OptionalAuthorizeParams {
            scope: stored_scope.as_deref(),
            nonce: params.nonce.as_deref(),
            resource: stored_resource.as_deref(),
        },
    );
    let state_value = state_value.to_string();

    let mut conn = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Internal server error".to_string()))?;
    // SET NX: a state token must not overwrite an existing pending
    // transaction — otherwise anyone who learns a victim's state value could
    // re-seed it with their own client_id/redirect_uri/PKCE before the login
    // completes (state fixation). A reused pending state is rejected; clients
    // generate a fresh random state per flow.
    let seeded: Option<String> = redis::cmd("SET")
        .arg(&state_key)
        .arg(&state_value)
        .arg("NX")
        .arg("EX")
        .arg(OAUTH_STATE_TTL_SECONDS)
        .query_async(&mut conn)
        .await
        .map_err(|_| ApiError::internal("Internal server error".to_string()))?;
    if seeded.is_none() {
        tracing::warn!(
            realm_id = %realm_id,
            client_id = %params.client_id,
            "OAuth authorize rejected: state already pending (replay/fixation attempt)"
        );
        return Err(ApiError::bad_request(
            "state is already in use; start a new authorize flow with a fresh state".to_string(),
        ));
    }

    tracing::debug!(
        realm_id = %realm_id,
        client_id = %params.client_id,
        redirect_uri = %params.redirect_uri,
        "OAuth authorize successful: redirecting to login"
    );

    // Redirect to login page with OAuth parameters (camelCase query params matching frontend route)
    let login_url = format!(
        "/{}/auth/login?clientId=admin-web-console&oauthClientId={}&redirectUri={}&state={}",
        urlencoding::encode(&realm_id),
        urlencoding::encode(&params.client_id),
        urlencoding::encode(&params.redirect_uri),
        urlencoding::encode(&params.state)
    );

    Ok((
        StatusCode::FOUND,
        [(axum::http::header::LOCATION, login_url)],
    )
        .into_response())
}

/// MCP-only redirect matching (RFC 8252 §7.3): the request must equal a
/// registered template on scheme/host/path with an arbitrary port. The
/// templates themselves are port-free and cover the three loopback host
/// forms (127.0.0.1, localhost, [::1]); user info, query and fragment are
/// rejected and no non-loopback host can ever match. Shared with the
/// pass-through registration endpoint's redirect-shape gate.
pub(crate) fn mcp_loopback_redirect_matches(templates: &[String], redirect_uri: &str) -> bool {
    let Ok(url) = url::Url::parse(redirect_uri) else {
        return false;
    };
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    if url.query().is_some() || url.fragment().is_some() {
        return false;
    }
    let is_loopback = match url.host() {
        Some(url::Host::Domain(host)) => host == "localhost",
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    if !is_loopback {
        return false;
    }
    templates.iter().any(|template| {
        url::Url::parse(template).is_ok_and(|tpl| {
            tpl.port().is_none()
                && tpl.scheme() == url.scheme()
                && tpl.host_str() == url.host_str()
                && tpl.path() == url.path()
        })
    })
}

/// OAuth-standard error redirect for an MCP authorize rejection: 302 back to
/// the (already validated) loopback callback carrying `error`,
/// `error_description` and `state` per RFC 6749 §4.1.2.1, so the agent
/// client surfaces the failure instead of the browser.
fn mcp_authorize_error_redirect(
    redirect_uri: &str,
    state: &str,
    error: &str,
    description: &str,
) -> axum::response::Response {
    let mut url = url::Url::parse(redirect_uri).unwrap_or_else(|_| {
        url::Url::parse("http://127.0.0.1/callback").expect("valid fallback URL")
    });
    url.query_pairs_mut()
        .append_pair("error", error)
        .append_pair("error_description", description)
        .append_pair("state", state);
    (
        StatusCode::FOUND,
        [(axum::http::header::LOCATION, url.to_string())],
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn templates() -> Vec<String> {
        [
            "http://127.0.0.1/callback",
            "http://localhost/callback",
            "http://[::1]/callback",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    // WHY: the dynamic-port exemption is scoped to exactly the loopback
    // templates — anything broader would silently widen the MCP callback
    // whitelist class beyond what the real-client verification froze.
    #[test]
    fn loopback_matcher_accepts_any_port_on_registered_templates() {
        let t = templates();
        for uri in [
            "http://127.0.0.1/callback",
            "http://127.0.0.1:43119/callback",
            "http://localhost:8080/callback",
            "http://[::1]:65535/callback",
        ] {
            assert!(mcp_loopback_redirect_matches(&t, uri), "must match {uri}");
        }
    }

    #[test]
    fn loopback_matcher_rejects_unregistered_shapes() {
        let t = templates();
        for uri in [
            // path drift
            "http://127.0.0.1:43119/other",
            "http://127.0.0.1:43119/callback/extra",
            // non-loopback hosts (incl. loopback subdomains)
            "http://evil.example/callback",
            "http://sub.localhost/callback",
            "http://127.0.0.2/callback",
            // scheme drift
            "https://127.0.0.1/callback",
            "http+unix:///run/socket",
            // extra URL parts
            "http://127.0.0.1:43119/callback?x=1",
            "http://127.0.0.1:43119/callback#frag",
            "http://user@127.0.0.1:43119/callback",
            "not a url",
        ] {
            assert!(!mcp_loopback_redirect_matches(&t, uri), "must reject {uri}");
        }
    }
}
