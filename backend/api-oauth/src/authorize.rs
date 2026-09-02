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
    OAUTH_AUTHORIZE_IP_RATE_LIMIT, OAUTH_STATE_TTL_SECONDS,
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
/// * 404 - Client not found or disabled
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
        ("code_challenge_method" = Option<String>, Query, description = "PKCE method (must be 'S256' if provided, defaults to S256)")
    ),
    responses(
        (status = 302, description = "Redirect to /{realmId}/auth/login with OAuth parameters"),
        (status = 400, description = "Bad request", body = ErrorResponse),
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

    // Enforce HTTPS in production. Local OAuth clients use localhost HTTP in dev/demo.
    herald_core::domain::client::validation::validate_redirect_uri(
        &params.redirect_uri,
        state.app_env != "production",
    )
    .map_err(|e| ApiError::bad_request(format!("Invalid redirect_uri: {}", e)))?;

    let is_whitelisted = if is_first_party {
        crate::token::validate_first_party_redirect(&state.public_base_url, &params.redirect_uri)
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

    // Store state token in Redis with PKCE parameters (5 minutes TTL, CSRF protection)
    let state_key = format!("oauth:state:{}", params.state);
    let state_value = serde_json::json!({
        "client_id": params.client_id,
        "realm_id": realm_id,
        "redirect_uri": params.redirect_uri,
        "code_challenge": params.code_challenge,
        "code_challenge_method": params.code_challenge_method.as_deref().unwrap_or("S256"),
    })
    .to_string();

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
