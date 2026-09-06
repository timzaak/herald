use axum::{
    extract::{Extension, Path, State},
    http::HeaderMap,
};
use herald_core::domain::authentication::{BrowserTokenService, Identity};
use herald_core::infrastructure::authentication::RedisBrowserTokenService;
use uuid::Uuid;

use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::client::ports::ClientService;

/// Delete a client app
///
/// Deletes an OAuth client application and its associated roles.
/// The built-in admin console client cannot be deleted.
#[utoipa::path(
    delete,
    path = "/api/client/{realmId}/{clientAppId}",
    tag = "client",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("clientAppId" = Uuid, Path, description = "Client App UUID"),
    ),
    responses(
        (status = 204, description = "Client App deleted"),
        (status = 400, description = "Cannot delete built-in admin console", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Insufficient permissions", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 404, description = "Client App not found", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 409, description = "Client App is bound to an API Key", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    )
)]
pub async fn delete_client_app(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, id)): Path<(String, Uuid)>,
    _headers: HeaderMap,
) -> Result<ApiResult<()>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "client applications")?;
    admin
        .require_permission(&state, "clients", "manage")
        .await?;

    tracing::debug!(
        realm_id = %realm_id,
        user_id = %admin.user_id_string(),
        "Deleting client app"
    );

    // Revoke browser token families before deleting the client app so that a
    // deleted OAuth client cannot keep validating tokens.
    // The realm boundary must be verified BEFORE revoking: revoke acts on the
    // raw id, so revoking ahead of the service-layer check would let a realm
    // admin kill another realm's active sessions by passing a foreign
    // clientAppId with their own realmId in the path.
    let client_service = state.service.client_service();
    client_service
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
                tracing::error!("Failed to load client app before revocation: {e}");
                ApiError::internal("Failed to load client app")
            }
        })?;
    RedisBrowserTokenService::new(state.redis_manager.clone())
        .revoke_client_families(id)
        .await
        .map_err(|e| {
            ApiError::internal(format!(
                "Browser token revocation failed before deleting client app: {e}"
            ))
        })?;

    // Call service layer
    client_service
        .delete_client_app(admin.identity().clone(), id)
        .await
        .map_err(|e| match e {
            herald_core::domain::common::entities::app_errors::CoreError::NotFound => {
                ApiError::not_found("client_app not found")
            }
            herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                ApiError::forbidden(msg)
            }
            herald_core::domain::common::entities::app_errors::CoreError::BadRequest(msg) => {
                ApiError::bad_request(msg)
            }
            herald_core::domain::common::entities::app_errors::CoreError::Conflict(msg) => {
                ApiError::conflict(msg)
            }
            e => {
                tracing::error!("Failed to delete client app: {}", e);
                ApiError::internal(format!("Failed to delete client app: {e}"))
            }
        })?;

    Ok(ApiResult::no_content())
}
