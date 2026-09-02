use axum::{
    Json,
    extract::{Extension, Path, State},
};
use axum_valid::Valid;
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;

use crate::api_keys::client_app_info::client_app_name;
use crate::api_keys::types::{ApiKeyListItem, UpdateApiKeyRequest};

/// Update an API Key
///
/// Partially updates an API key's name, enabled status, or expiration.
#[utoipa::path(
    put,
    path = "/api/api-keys/{realmId}/{apiKeyId}",
    tag = "api-keys",
    request_body = UpdateApiKeyRequest,
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("apiKeyId" = String, Path, description = "API Key ID"),
    ),
    responses(
        (status = 200, description = "API Key updated", body = ApiKeyListItem),
        (status = 403, description = "Forbidden", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 404, description = "API Key not found", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    )
)]
pub async fn update_api_key(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, api_key_id)): Path<(String, String)>,
    Valid(Json(payload)): Valid<Json<UpdateApiKeyRequest>>,
) -> Result<ApiResult<ApiKeyListItem>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "api keys")?;
    admin
        .require_permission(&state, "api_keys", "manage")
        .await?;

    let mut api_key = state
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

    // Apply partial updates
    if let Some(name) = payload.name {
        api_key.name = name;
    }
    if let Some(enabled) = payload.enabled {
        api_key.enabled = enabled;
    }
    if let Some(expires_at) = payload.expires_at {
        api_key.expires_at = match expires_at {
            Some(s) => Some(
                chrono::DateTime::parse_from_rfc3339(&s)
                    .map_err(|e| ApiError::bad_request(format!("Invalid expiresAt format: {e}")))?
                    .with_timezone(&chrono::Utc),
            ),
            None => None,
        };
    }

    let saved = state.api_key_repo.update(&api_key).await.map_err(|e| {
        tracing::error!("Failed to update API key: {e}");
        ApiError::internal("Failed to update API key")
    })?;

    // Authentication caches the enabled/expiry state by this digest. Evict
    // only after the database update: a concurrent request may refill the old
    // value before the update commits, and this final delete must win so a
    // disabled or newly-expired key stops authenticating immediately.
    if let Err(e) = state.api_key_cache.delete(&saved.api_key_hash).await {
        tracing::error!("Failed to evict updated API key from cache: {e}");
        return Err(ApiError::internal(
            "API key was updated but its authentication cache could not be invalidated",
        ));
    }

    let response = ApiKeyListItem {
        id: saved.id,
        name: saved.name,
        realm_id: saved.realm_id,
        client_app_id: saved.client_app_id,
        client_app_name: client_app_name(&state, saved.client_app_id).await?,
        enabled: saved.enabled,
        expires_at: saved.expires_at.map(|dt| dt.to_rfc3339()),
        last_used_at: saved.last_used_at.map(|dt| dt.to_rfc3339()),
        created_at: saved.created_at.to_rfc3339(),
        roles: Vec::new(),
    };

    Ok(ApiResult::ok(response))
}
