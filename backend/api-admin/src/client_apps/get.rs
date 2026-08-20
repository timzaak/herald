use axum::{
    extract::{Extension, Path, State},
    http::HeaderMap,
};
use herald_core::domain::authentication::Identity;
use uuid::Uuid;

use crate::client_apps::types::ClientAppItem;
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::client::ports::ClientService;

/// Get a specific client app by ID
///
/// Retrieves the details of a specific OAuth client application.
#[utoipa::path(
    get,
    path = "/api/client/{realmId}/{clientAppId}",
    tag = "client",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("clientAppId" = Uuid, Path, description = "client app UUID"),
    ),
    responses(
        (status = 200, description = "ClientApp retrieved", body = ClientAppItem),
        (status = 404, description = "ClientApp not found", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    )
)]
pub async fn get_client_app(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, id)): Path<(String, Uuid)>,
    _headers: HeaderMap,
) -> Result<ApiResult<ClientAppItem>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "client applications")?;
    admin.require_permission(&state, "clients", "view").await?;

    tracing::debug!(
        realm_id = %realm_id,
        user_id = %admin.user_id_string(),
        "Getting client app"
    );

    // Call service layer
    let client_service = state.service.client_service();
    let client_app = client_service
        .get_client_app(admin.identity().clone(), id)
        .await
        .map_err(|e| match e {
            herald_core::domain::common::entities::app_errors::CoreError::NotFound => {
                ApiError::not_found("client_app not found")
            }
            herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                ApiError::forbidden(msg)
            }
            e => {
                tracing::error!("Failed to get client app: {}", e);
                ApiError::internal(format!("Failed to get client app: {e}"))
            }
        })?;

    // Convert domain model to API response model
    let response: ClientAppItem = client_app.into();

    Ok(ApiResult::ok(response))
}
