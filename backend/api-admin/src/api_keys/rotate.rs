use axum::extract::{Extension, Path, State};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;
use herald_core::domain::client_api_keys::services::ClientApiKeyService;

use crate::api_keys::client_app_info::client_app_name;
use crate::api_keys::types::RotateApiKeyResponse;

/// Rotate an API Key
///
/// Generates a new key for the API key, replacing the old hash.
/// The old key is immediately invalidated. Returns the new plaintext key (shown once).
#[utoipa::path(
    post,
    path = "/api/api-keys/{realmId}/{apiKeyId}/rotate",
    tag = "api-keys",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("apiKeyId" = String, Path, description = "API Key ID"),
    ),
    responses(
        (status = 200, description = "API Key rotated", body = RotateApiKeyResponse),
        (status = 403, description = "Forbidden", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 404, description = "API Key not found", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    )
)]
pub async fn rotate_api_key(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, api_key_id)): Path<(String, String)>,
) -> Result<ApiResult<RotateApiKeyResponse>, ApiError> {
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

    // The API-key cache is keyed by the key's SHA-256 digest, which the DB
    // record carries — evict the old digest so the rotated-out key stops
    // authenticating immediately instead of living out the cache TTL. The
    // new digest has never been cached.
    if let Err(e) = state.api_key_cache.delete(&api_key.api_key_hash).await {
        tracing::warn!("Failed to evict rotated API key from cache: {e}");
    }

    // Generate new key and hash
    let plaintext_key = ClientApiKeyService::generate_api_key();
    let new_hash = ClientApiKeyService::hash_api_key(&plaintext_key);
    api_key.api_key_hash = new_hash;

    let saved = state.api_key_repo.update(&api_key).await.map_err(|e| {
        tracing::error!("Failed to rotate API key: {e}");
        ApiError::internal("Failed to rotate API key")
    })?;

    let response = RotateApiKeyResponse {
        client_app_name: client_app_name(&state, saved.client_app_id).await?,
        id: saved.id,
        name: saved.name,
        key: plaintext_key,
        realm_id: saved.realm_id,
        client_app_id: saved.client_app_id,
        enabled: saved.enabled,
        expires_at: saved.expires_at.map(|dt| dt.to_rfc3339()),
        created_at: saved.created_at.to_rfc3339(),
        role_binding_error: None,
    };

    Ok(ApiResult::ok(response))
}
