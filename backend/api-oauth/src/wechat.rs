// WeChat OAuth handlers (website application login)

use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    callback::issue_callback_token_response,
    helper::{generate_oauth_auth_url, handle_oauth_callback},
};
use herald_api_base::application::http::auth::util::{
    ClientIp, rate_limit_hit, user_agent_from_headers,
};
use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::security_constants::OAUTH_UPSTREAM_LOGIN_IP_RATE_LIMIT;
use validator::Validate;

#[derive(Debug, Deserialize, Serialize, ToSchema, Validate)]
pub struct WeChatAuthUrlRequest {
    #[serde(alias = "client_id")]
    pub client_id: Option<String>,
    #[serde(alias = "redirect_uri")]
    pub redirect_uri: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WeChatAuthUrlResponse {
    pub auth_url: String,
    pub state: String,
}

/// Generate WeChat authorization URL (QRconnect)
#[utoipa::path(
    get,
    path = "/api/oauth/{realmId}/wechat/login",
    tag = "oauth",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("redirect_uri" = Option<String>, Query, description = "Redirect URI after successful login")
    ),
    responses(
        (status = 200, description = "Authorization URL generated", body = WeChatAuthUrlResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 404, description = "WeChat provider not configured or not enabled", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn wechat_login(
    Path(realm_id): Path<String>,
    Query(query): Query<WeChatAuthUrlRequest>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
) -> Result<Json<WeChatAuthUrlResponse>, ApiError> {
    tracing::info!(
        realm_id = %realm_id,
        "WeChat authorization URL requested"
    );

    // Per-IP cap: each request costs a provider-config DB read plus a Redis
    // state write, so an unauthenticated flood can fill Redis.
    rate_limit_hit(
        &state,
        format!("rl:oauth-wechat-login:ip:{ip}"),
        OAUTH_UPSTREAM_LOGIN_IP_RATE_LIMIT.0,
        OAUTH_UPSTREAM_LOGIN_IP_RATE_LIMIT.1,
    )
    .await?;

    // Generate OAuth authorization URL and state token using the helper function
    let realm_id_clone = realm_id.clone();
    let (auth_url, state_token) = generate_oauth_auth_url(
        &state,
        realm_id_clone,
        "wechat".to_string(),
        query
            .client_id
            .unwrap_or_else(|| "admin-web-console".to_string()),
        query.redirect_uri,
        None,
    )
    .await?;

    tracing::debug!(
        realm_id = %realm_id,
        "WeChat authorization URL generated successfully"
    );

    // Return JSON response directly (B-class exception: OAuth protocol)
    Ok(Json(WeChatAuthUrlResponse {
        auth_url,
        state: state_token,
    }))
}

/// Handle WeChat OAuth callback
#[utoipa::path(
    get,
    path = "/api/oauth/{realmId}/wechat/callback",
    tag = "oauth",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("code" = String, Query, description = "Authorization code"),
        ("state" = String, Query, description = "OAuth state for CSRF protection")
    ),
    responses(
        (status = 302, description = "Redirect to specified redirect_uri or default"),
        (status = 400, description = "Bad request (invalid state or code)", body = ErrorResponse),
        (status = 401, description = "Unauthorized (OAuth failed)", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn wechat_callback(
    Path(realm_id): Path<String>,
    Query(query): Query<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let user_agent = user_agent_from_headers(&headers);

    let code = query
        .get("code")
        .ok_or_else(|| ApiError::bad_request("Missing code parameter".to_string()))?
        .clone();

    let state_token = query
        .get("state")
        .ok_or_else(|| ApiError::bad_request("Missing state parameter".to_string()))?
        .clone();

    tracing::info!(
        realm_id = %realm_id,
        "WeChat callback received"
    );

    // Handle OAuth callback using the helper function
    let realm_id_clone = realm_id.clone();
    let callback = handle_oauth_callback(
        &state,
        realm_id_clone,
        "wechat".to_string(),
        code,
        state_token,
    )
    .await?;

    if let Some(redirect_uri) = callback.downstream_redirect_uri {
        return Ok(Redirect::temporary(&redirect_uri).into_response());
    }

    let user_id = callback.user_id;
    let client_id = callback.client_id;

    tracing::info!(
        realm_id = %realm_id,
        user_id = %user_id,
        "WeChat callback processed successfully"
    );

    issue_callback_token_response(&state, &realm_id, user_id, &client_id, user_agent, Some(ip))
        .await
}
