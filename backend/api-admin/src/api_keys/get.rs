use axum::extract::{Extension, Path, State};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;

use crate::api_keys::client_app_info::{api_key_role_summaries, client_app_name};
use crate::api_keys::types::ApiKeyListItem;

/// Get a specific API Key by ID
///
/// Retrieves details of an API key. Hash and plaintext are never exposed.
#[utoipa::path(
    get,
    path = "/api/api-keys/{apiKeyId}",
    tag = "api-keys",
    params(
        ("apiKeyId" = String, Path, description = "API Key ID"),
    ),
    responses(
        (status = 200, description = "API Key details", body = ApiKeyListItem),
        (status = 403, description = "Forbidden", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 404, description = "API Key not found", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    )
)]
pub async fn get_api_key(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(api_key_id): Path<String>,
) -> Result<ApiResult<ApiKeyListItem>, ApiError> {
    let admin = AdminIdentity::require(identity, "api keys")?;
    let realm_id = admin.realm_id().to_string();
    admin.require_permission(&state, "api_keys", "view").await?;

    let api_key = state
        .api_key_repo
        .find_by_id(&api_key_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get API key: {e}");
            ApiError::internal("Failed to get API key")
        })?
        .ok_or_else(|| ApiError::not_found("API key not found"))?;

    if api_key.realm_id != realm_id {
        return Err(ApiError::not_found("API key not found"));
    }

    let roles = api_key_role_summaries(&state, admin.identity(), &realm_id, &api_key_id).await?;

    let response = ApiKeyListItem {
        id: api_key.id,
        name: api_key.name,
        realm_id: api_key.realm_id,
        client_app_id: api_key.client_app_id,
        client_app_name: client_app_name(&state, api_key.client_app_id).await?,
        enabled: api_key.enabled,
        expires_at: api_key.expires_at.map(|dt| dt.to_rfc3339()),
        last_used_at: api_key.last_used_at.map(|dt| dt.to_rfc3339()),
        created_at: api_key.created_at.to_rfc3339(),
        roles,
    };

    Ok(ApiResult::ok(response))
}
