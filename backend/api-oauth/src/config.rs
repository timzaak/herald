// OAuth provider configuration CRUD handlers

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use herald_api_base::application::http::auth::util::{ClientIp, user_agent_from_headers};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_core::domain::audit::AuditContext;
use herald_core::domain::authentication::Identity;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

pub use herald_api_base::application::http::server::api_entities::ErrorResponse;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::oauth::entities::{
    CreateOAuthProviderConfigRequest, OAuthProviderConfig, ProviderType,
    UpdateOAuthProviderConfigRequest,
};

#[derive(Debug, Deserialize, Serialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct CreateOAuthConfigRequest {
    pub provider_type: String,
    #[validate(length(min = 1))]
    pub client_id: String,
    #[validate(length(min = 1))]
    pub client_secret: String,
    pub scopes: Option<Vec<String>>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOAuthConfigRequest {
    #[validate(length(min = 1))]
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub scopes: Option<Vec<String>>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OAuthConfigResponse {
    pub id: Uuid,
    pub realm_id: String,
    pub provider_type: String,
    pub client_id: String,
    pub scopes: Vec<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

// Helper function to convert domain entity to response
fn to_response(config: OAuthProviderConfig) -> OAuthConfigResponse {
    OAuthConfigResponse {
        id: config.id,
        realm_id: config.realm_id,
        provider_type: config.provider_type.as_str().to_string(),
        client_id: config.client_id,
        // Note: client_secret should not be exposed in GET responses
        scopes: config.scopes,
        enabled: config.enabled,
        created_at: config.created_at.to_rfc3339(),
        updated_at: config.updated_at.to_rfc3339(),
    }
}

/// List all OAuth provider configurations for a realm
#[utoipa::path(
    get,
    path = "/api/oauth/{realmId}/configs",
    tag = "oauth",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        (status = 200, description = "List of OAuth provider configurations", body = Vec<OAuthConfigResponse>),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn list_oauth_configs(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
) -> Result<ApiResult<Vec<OAuthConfigResponse>>, ApiError> {
    let oauth_config_service = state.service.oauth_config_service();

    let identity_realm_id = identity.realm_id();
    let current_user_id = identity.user_id();

    tracing::debug!(
        realm_id = %identity_realm_id,
        user_id = %current_user_id,
        "Listing OAuth configs"
    );

    // In-handler gate mirroring the service-layer policy (settings.view +
    // realm match) so the handler layer stays protected even if the wired
    // policy regresses to an AllowAll test double.
    AdminIdentity::require(identity.clone(), &realm_id, "oauth configs")?
        .require_permission(&state, "settings", "view")
        .await?;

    let configs = oauth_config_service
        .list_configs(identity, &realm_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to list oauth configs: {e}");
            match e {
                herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                    ApiError::forbidden(msg)
                }
                _ => ApiError::internal("Internal server error".to_string()),
            }
        })?;

    let responses = configs.into_iter().map(to_response).collect();
    Ok(ApiResult::ok(responses))
}

/// Get OAuth provider configuration by provider type
#[utoipa::path(
    get,
    path = "/api/oauth/{realmId}/configs/{providerType}",
    tag = "oauth",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("providerType" = String, Path, description = "Provider type (google, github, facebook, apple)")
    ),
    responses(
        (status = 200, description = "OAuth provider configuration", body = OAuthConfigResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_oauth_config(
    Path((realm_id, provider_type)): Path<(String, String)>,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
) -> Result<ApiResult<OAuthConfigResponse>, ApiError> {
    let oauth_config_service = state.service.oauth_config_service();

    let identity_realm_id = identity.realm_id();
    let current_user_id = identity.user_id();

    tracing::debug!(
        realm_id = %identity_realm_id,
        user_id = %current_user_id,
        "Getting OAuth config"
    );

    AdminIdentity::require(identity.clone(), &realm_id, "oauth configs")?
        .require_permission(&state, "settings", "view")
        .await?;

    let config = oauth_config_service
        .get_config(identity, &realm_id, &provider_type)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get oauth config: {e}");
            match e {
                herald_core::domain::common::entities::app_errors::CoreError::NotFound => {
                    ApiError::not_found("Provider config not found".to_string())
                }
                herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                    ApiError::forbidden(msg)
                }
                _ => ApiError::internal("Internal server error".to_string()),
            }
        })?;

    Ok(ApiResult::ok(to_response(config)))
}

/// Create OAuth provider configuration
#[utoipa::path(
    post,
    path = "/api/oauth/{realmId}/configs",
    tag = "oauth",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = CreateOAuthConfigRequest,
    responses(
        (status = 201, description = "OAuth provider configuration created", body = OAuthConfigResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 409, description = "Conflict - config already exists", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn create_oauth_config(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(payload): Json<CreateOAuthConfigRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let user_agent = user_agent_from_headers(&headers);
    // Validate request
    payload
        .validate()
        .map_err(|e| ApiError::bad_request(format!("Validation error: {}", e)))?;

    // Parse and validate provider type
    let provider_type = payload.provider_type.parse::<ProviderType>().map_err(|_| {
        ApiError::bad_request(format!("Invalid provider type: {}", payload.provider_type))
    })?;

    let oauth_config_service = state.service.oauth_config_service();

    let identity_realm_id = identity.realm_id();
    let current_user_id = identity.user_id();

    tracing::debug!(
        realm_id = %identity_realm_id,
        user_id = %current_user_id,
        "Creating OAuth config"
    );

    AdminIdentity::require(identity.clone(), &realm_id, "oauth configs")?
        .require_permission(&state, "settings", "manage")
        .await?;

    let request = CreateOAuthProviderConfigRequest {
        realm_id,
        provider_type,
        client_id: payload.client_id,
        client_secret: payload.client_secret,
        scopes: payload.scopes,
        enabled: payload.enabled,
    };

    let ctx = AuditContext::admin(&identity, ip, user_agent);
    let config = oauth_config_service
        .create_config(identity, ctx, request)
        .await
        .map_err(|e| {
            tracing::error!("Failed to create oauth config: {e}");
            match e {
                herald_core::domain::common::entities::app_errors::CoreError::BadRequest(msg) => {
                    ApiError::bad_request(msg)
                }
                herald_core::domain::common::entities::app_errors::CoreError::Conflict(msg) => {
                    ApiError::conflict(msg)
                }
                herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                    ApiError::forbidden(msg)
                }
                _ => ApiError::internal("Internal server error".to_string()),
            }
        })?;

    // Return 201 Created with ApiResult wrapped response
    let response = (StatusCode::CREATED, ApiResult::ok(to_response(config))).into_response();
    Ok(response)
}

/// Update OAuth provider configuration
#[utoipa::path(
    put,
    path = "/api/oauth/{realmId}/configs/{providerType}",
    tag = "oauth",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("providerType" = String, Path, description = "Provider type")
    ),
    request_body = UpdateOAuthConfigRequest,
    responses(
        (status = 200, description = "OAuth provider configuration updated", body = OAuthConfigResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn update_oauth_config(
    Path((realm_id, provider_type)): Path<(String, String)>,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(payload): Json<UpdateOAuthConfigRequest>,
) -> Result<ApiResult<OAuthConfigResponse>, ApiError> {
    let ctx = AuditContext::admin(&identity, ip, user_agent_from_headers(&headers));
    // Validate request
    payload
        .validate()
        .map_err(|e| ApiError::bad_request(format!("Validation error: {}", e)))?;

    let oauth_config_service = state.service.oauth_config_service();

    let identity_realm_id = identity.realm_id();
    let current_user_id = identity.user_id();

    tracing::debug!(
        realm_id = %identity_realm_id,
        user_id = %current_user_id,
        "Updating OAuth config"
    );

    AdminIdentity::require(identity.clone(), &realm_id, "oauth configs")?
        .require_permission(&state, "settings", "manage")
        .await?;

    // Get existing config to obtain its ID
    let existing_config = oauth_config_service
        .get_config(identity.clone(), &realm_id, &provider_type)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get oauth config: {e}");
            match e {
                herald_core::domain::common::entities::app_errors::CoreError::NotFound => {
                    ApiError::not_found("Provider config not found".to_string())
                }
                herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                    ApiError::forbidden(msg)
                }
                _ => ApiError::internal("Internal server error".to_string()),
            }
        })?;

    // Filter out empty client_secret (don't update if empty)
    let update_request = UpdateOAuthProviderConfigRequest {
        client_id: payload.client_id,
        client_secret: payload.client_secret.filter(|s| !s.is_empty()),
        scopes: payload.scopes,
        enabled: payload.enabled,
    };

    let config = oauth_config_service
        .update_config(identity, ctx, existing_config.id, update_request)
        .await
        .map_err(|e| {
            tracing::error!("Failed to update oauth config: {e}");
            match e {
                herald_core::domain::common::entities::app_errors::CoreError::BadRequest(msg) => {
                    ApiError::bad_request(msg)
                }
                herald_core::domain::common::entities::app_errors::CoreError::NotFound => {
                    ApiError::not_found("Provider config not found".to_string())
                }
                herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                    ApiError::forbidden(msg)
                }
                _ => ApiError::internal("Internal server error".to_string()),
            }
        })?;

    Ok(ApiResult::ok(to_response(config)))
}

/// Delete OAuth provider configuration
#[utoipa::path(
    delete,
    path = "/api/oauth/{realmId}/configs/{providerType}",
    tag = "oauth",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("providerType" = String, Path, description = "Provider type")
    ),
    responses(
        (status = 204, description = "OAuth provider configuration deleted"),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn delete_oauth_config(
    Path((realm_id, provider_type)): Path<(String, String)>,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let ctx = AuditContext::admin(&identity, ip, user_agent_from_headers(&headers));
    let oauth_config_service = state.service.oauth_config_service();

    let identity_realm_id = identity.realm_id();
    let current_user_id = identity.user_id();

    tracing::debug!(
        realm_id = %identity_realm_id,
        user_id = %current_user_id,
        "Deleting OAuth config"
    );

    AdminIdentity::require(identity.clone(), &realm_id, "oauth configs")?
        .require_permission(&state, "settings", "manage")
        .await?;

    // Get existing config to obtain its ID
    let existing_config = oauth_config_service
        .get_config(identity.clone(), &realm_id, &provider_type)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get oauth config: {e}");
            match e {
                herald_core::domain::common::entities::app_errors::CoreError::NotFound => {
                    ApiError::not_found("Provider config not found".to_string())
                }
                herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                    ApiError::forbidden(msg)
                }
                _ => ApiError::internal("Internal server error".to_string()),
            }
        })?;

    oauth_config_service
        .delete_config(identity, ctx, existing_config.id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to delete oauth config: {e}");
            match e {
                herald_core::domain::common::entities::app_errors::CoreError::NotFound => {
                    ApiError::not_found("Provider config not found".to_string())
                }
                herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                    ApiError::forbidden(msg)
                }
                _ => ApiError::internal("Internal server error".to_string()),
            }
        })?;

    Ok(StatusCode::NO_CONTENT)
}
