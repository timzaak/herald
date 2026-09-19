use axum::{
    Json,
    extract::{Extension, Path, State},
};
use axum_valid::Valid;
use herald_core::domain::authentication::Identity;

use crate::client_apps::types::{ClientAppCreateRequest, ClientAppItem};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::client::ports::ClientService;
use herald_core::domain::client::value_objects::CreateClientAppRequest;

/// Create a new client app
///
/// Creates a new OAuth client application with the specified configuration.
#[utoipa::path(
    post,
    path = "/api/client/{realmId}",
    tag = "client",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = ClientAppCreateRequest,
    responses(
        (status = 201, description = "ClientApp created", body = ClientAppItem),
        (status = 400, description = "Bad request", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    )
)]
pub async fn create_client_app(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
    Valid(Json(payload)): Valid<Json<ClientAppCreateRequest>>,
) -> Result<ApiResult<ClientAppItem>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "client applications")?;
    admin
        .require_permission(&state, "clients", "manage")
        .await?;

    tracing::debug!(
        realm_id = %realm_id,
        user_id = %admin.user_id_string(),
        "Creating client app"
    );

    // Create service request
    let service_request = CreateClientAppRequest {
        realm_id: realm_id.clone(),
        client_id: payload.client_id.clone(),
        name: payload.name.clone(),
        description: payload.description.clone(),
        redirect_uris: payload.redirect_uris.clone(),
        allowed_origins: payload.allowed_origins.clone(),
        email_verify_return_url: payload.email_verify_return_url.clone(),
        password_reset_return_url: payload.password_reset_return_url.clone(),
        browser_refresh_absolute_ttl_seconds: payload.browser_refresh_absolute_ttl_seconds,
        enabled: payload.enabled,
        icon_url: payload.icon_url.clone(),
        device_code_grant_enabled: payload.device_code_grant_enabled,
        turnstile_enabled: payload.turnstile_enabled,
        turnstile_site_key: payload.turnstile_site_key.clone(),
        turnstile_secret_key: payload.turnstile_secret_key.clone(),
    };

    // Call service layer
    let client_service = state.service.client_service();
    let client_app = client_service
        .create_client_app(admin.identity().clone(), service_request)
        .await
        .map_err(|e| match e {
            herald_core::domain::common::entities::app_errors::CoreError::Conflict(msg) => {
                tracing::error!("Client app conflict: {}", msg);
                ApiError::conflict(msg)
            }
            herald_core::domain::common::entities::app_errors::CoreError::BadRequest(msg) => {
                tracing::error!("Bad request: {}", msg);
                ApiError::bad_request(msg)
            }
            e => {
                tracing::error!("Failed to create client app: {}", e);
                ApiError::internal(format!("Failed to create client app: {e}"))
            }
        })?;

    // Convert domain model to API response model. Create is the one path that
    // echoes the client_secret back to the caller.
    let client_secret = client_app.client_secret.clone();
    let mut response: ClientAppItem = client_app.into();
    response.client_secret = client_secret;

    Ok(ApiResult::created(response))
}
