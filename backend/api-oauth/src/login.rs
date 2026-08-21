// OAuth login handler

use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use crate::helper::generate_oauth_auth_url;
use herald_api_base::application::http::auth::util::{ClientIp, rate_limit_hit};
use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::security_constants::OAUTH_UPSTREAM_LOGIN_IP_RATE_LIMIT;

#[derive(Debug, Deserialize, Serialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct OAuthLoginRequest {
    #[serde(alias = "client_id")]
    pub client_id: Option<String>,
    #[serde(alias = "redirect_uri")]
    pub redirect_uri: Option<String>,
    #[serde(alias = "downstream_state")]
    #[validate(length(min = 1, max = 512))]
    pub downstream_state: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OAuthLoginResponse {
    pub auth_url: String,
    pub state: String,
}

/// Initiate OAuth login flow for a realm
#[utoipa::path(
    get,
    path = "/api/oauth/{realmId}/{provider}/login",
    tag = "oauth",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("provider" = String, Path, description = "OAuth provider type (google, github, facebook, apple, wechat, wechat_miniprogram)"),
        ("redirect_uri" = Option<String>, Query, description = "Provider callback URI override"),
        ("downstream_state" = Option<String>, Query, description = "Existing downstream authorization transaction state")
    ),
    responses(
        (status = 200, description = "OAuth login initiated", body = OAuthLoginResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 404, description = "OAuth provider not configured for this realm", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn oauth_login(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Path((realm_id, provider)): Path<(String, String)>,
    Query(query): Query<OAuthLoginRequest>,
) -> Result<Json<OAuthLoginResponse>, ApiError> {
    query
        .validate()
        .map_err(|e| ApiError::bad_request(format!("Validation error: {e}")))?;

    // Validate provider
    let provider_type = provider.to_lowercase();
    if !matches!(
        provider_type.as_str(),
        "google" | "github" | "facebook" | "apple" | "wechat" | "wechat_miniprogram"
    ) {
        return Err(ApiError::bad_request(format!(
            "Unsupported OAuth provider: {}",
            provider
        )));
    }

    // Per-IP cap: each request costs a provider-config DB read plus a Redis
    // state write, so an unauthenticated flood can fill Redis.
    rate_limit_hit(
        &state,
        format!("rl:oauth-login:ip:{ip}"),
        OAUTH_UPSTREAM_LOGIN_IP_RATE_LIMIT.0,
        OAUTH_UPSTREAM_LOGIN_IP_RATE_LIMIT.1,
    )
    .await?;

    // Generate OAuth authorization URL and state token
    let client_id = query
        .client_id
        .unwrap_or_else(|| "admin-web-console".to_string());
    let (auth_url, state_token) = generate_oauth_auth_url(
        &state,
        realm_id,
        provider_type,
        client_id,
        query.redirect_uri,
        query.downstream_state,
    )
    .await?;

    // Return JSON response directly (B-class exception: OAuth protocol)
    Ok(Json(OAuthLoginResponse {
        auth_url,
        state: state_token,
    }))
}
