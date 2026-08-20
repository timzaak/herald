use axum::extract::{Extension, Path, State};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;

/// Delete an API Key
///
/// Permanently deletes an API key.
#[utoipa::path(
    delete,
    path = "/api/api-keys/{realmId}/{apiKeyId}",
    tag = "api-keys",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("apiKeyId" = String, Path, description = "API Key ID"),
    ),
    responses(
        (status = 204, description = "API Key deleted"),
        (status = 403, description = "Forbidden", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 404, description = "API Key not found", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    )
)]
pub async fn delete_api_key(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, api_key_id)): Path<(String, String)>,
) -> Result<ApiResult<()>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "api keys")?;
    admin
        .require_permission(&state, "api_keys", "manage")
        .await?;

    let api_key = state
        .api_key_repo
        .find_by_id(&api_key_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to find API key: {e}");
            ApiError::internal("Failed to find API key")
        })?
        .ok_or_else(|| ApiError::not_found("API key not found"))?;

    if api_key.realm_id != realm_id {
        return Err(ApiError::not_found("API key not found"));
    }

    state.api_key_repo.delete(&api_key_id).await.map_err(|e| {
        tracing::error!("Failed to delete API key: {e}");
        ApiError::internal("Failed to delete API key")
    })?;

    // The API-key cache is keyed by the key's SHA-256 digest, which the DB
    // record carries — evict it so the deleted key stops authenticating
    // immediately instead of living out the cache TTL.
    if let Err(e) = state.api_key_cache.delete(&api_key.api_key_hash).await {
        tracing::warn!("Failed to evict deleted API key from cache: {e}");
    }

    Ok(ApiResult::no_content())
}
