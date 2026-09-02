// WeChat Mini Program OAuth handlers (code2session)

use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::helper::{exchange_code_for_user_info, find_or_create_user};
use herald_api_base::application::http::auth::util::{ClientIp, rate_limit_hit};
use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::oauth::value_objects::OAuthConfig;
use herald_core::domain::security_constants::OAUTH_UPSTREAM_LOGIN_IP_RATE_LIMIT;

#[derive(Debug, Deserialize, Serialize, ToSchema, Validate)]
pub struct WeChatMiniProgramLoginRequest {
    #[validate(length(min = 1))]
    pub code: String,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WeChatMiniProgramLoginResponse {
    pub access_token: String,
    pub user_id: Uuid,
    pub expires_in: i64,
}

/// WeChat Mini Program login (code2session)
#[utoipa::path(
    post,
    path = "/api/oauth/{realmId}/wechat-miniprogram/login",
    tag = "oauth",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = WeChatMiniProgramLoginRequest,
    responses(
        (status = 200, description = "Login successful", body = WeChatMiniProgramLoginResponse),
        (status = 400, description = "Bad request (invalid code or validation error)", body = ErrorResponse),
        (status = 404, description = "WeChat Mini Program provider not configured or not enabled", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn wechat_miniprogram_login(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Json(payload): Json<WeChatMiniProgramLoginRequest>,
) -> Result<Json<WeChatMiniProgramLoginResponse>, ApiError> {
    // Validate request
    payload
        .validate()
        .map_err(|e| ApiError::bad_request(format!("Validation error: {}", e)))?;

    // Per-IP cap before any upstream call: code2session hits WeChat's API,
    // so unthrottled requests amplify into outbound traffic.
    rate_limit_hit(
        &state,
        format!("rl:oauth-wechat-miniprogram:ip:{ip}"),
        OAUTH_UPSTREAM_LOGIN_IP_RATE_LIMIT.0,
        OAUTH_UPSTREAM_LOGIN_IP_RATE_LIMIT.1,
    )
    .await?;

    tracing::info!(
        realm_id = %realm_id,
        "WeChat Mini Program login request"
    );

    // Get WeChat Mini Program provider config
    let config = state
        .service
        .oauth_config_service()
        .list_enabled_providers(&realm_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to list OAuth providers: {e}");
            ApiError::internal("Failed to get provider configuration".to_string())
        })?
        .into_iter()
        .find(|c| c.provider_type.as_str() == "wechat_miniprogram")
        .ok_or_else(|| {
            ApiError::not_found(
                "WeChat Mini Program provider not configured or not enabled".to_string(),
            )
        })?;

    // Convert OAuthConfig from domain to value object
    // Note: redirect_uri and scopes are not used for mini program
    let oauth_config = OAuthConfig {
        client_id: config.client_id.clone(),
        client_secret: config.client_secret.clone(),
        redirect_uri: String::new(),
        scopes: vec![],
    };

    // Exchange code for user info
    let user_info =
        exchange_code_for_user_info("wechat_miniprogram", payload.code, oauth_config).await?;

    tracing::info!(
        user_id = %user_info.provider_user_id,
        "WeChat Mini Program user authenticated"
    );

    // Find or create user
    let user_id = find_or_create_user(&state, &realm_id, &user_info).await?;

    // Generate JWT token
    let jwt_secret = crate::helper::jwt_secret(&state)?;
    let jwt_token = crate::helper::generate_jwt_token(&user_id.to_string(), &realm_id, jwt_secret)?;

    // Return JSON response directly (B-class exception: OAuth protocol)
    Ok(Json(WeChatMiniProgramLoginResponse {
        access_token: jwt_token,
        user_id,
        expires_in: crate::helper::jwt_expiration_seconds()?,
    }))
}
