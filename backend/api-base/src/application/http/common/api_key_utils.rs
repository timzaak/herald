// Shared API Key Authentication Utilities
//
// The single home of API-key validation for every HTTP surface that
// authenticates Client API Keys (api-ext, api-points, api-mcp): the leaf
// status checks below plus the `validate_api_key` orchestration (cache → DB
// → status → Client App cascade → usage stats → cache write). Surfaces only
// decide how the credential is extracted and how failures are rendered.

use super::error_codes::ErrorCode;
use axum::http::StatusCode;
use chrono::Utc;
use herald_core::domain::client_api_keys::entities::ClientApiKey;
use herald_core::domain::client_api_keys::services::ClientApiKeyService;
use herald_core::entity::client_app;
use herald_core::infrastructure::client_api_keys::cache::ApiKeyCacheValue;
use sea_orm::EntityTrait;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::application::http::state::AppState;

/// Cache TTL for API keys in seconds (5 minutes)
const API_KEY_CACHE_TTL_SECONDS: u64 = 300;

/// Status result from validating an API key
#[derive(Debug, PartialEq)]
enum ApiKeyValidationStatus {
    Valid,
    Disabled,
    Expired,
    Invalid,
}

impl ApiKeyValidationStatus {
    fn to_error_code(&self) -> ErrorCode {
        match self {
            ApiKeyValidationStatus::Disabled => ErrorCode::ApiKeyDisabled,
            ApiKeyValidationStatus::Expired => ErrorCode::ApiKeyExpired,
            _ => ErrorCode::InvalidApiKey,
        }
    }
}

/// Typed failure of the validation orchestration; each surface renders it
/// into its own response shape (axum `json_error` or `ApiError`).
#[derive(Debug, Clone, Copy)]
pub enum ApiKeyAuthError {
    /// 401 with the specific key-related error code (missing/invalid key,
    /// disabled/expired, or disabled Client App).
    Unauthorized(ErrorCode),
    /// 500 InternalError (Redis/DB/conversion failure).
    Internal,
}

impl ApiKeyAuthError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            ApiKeyAuthError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            ApiKeyAuthError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn error_code(&self) -> ErrorCode {
        match self {
            ApiKeyAuthError::Unauthorized(code) => *code,
            ApiKeyAuthError::Internal => ErrorCode::InternalError,
        }
    }
}

/// Check if a cached API key is valid (enabled and not expired)
/// Returns detailed validation status for better error messages
fn check_cached_key_status(cached: &ApiKeyCacheValue) -> ApiKeyValidationStatus {
    if !cached.enabled {
        return ApiKeyValidationStatus::Disabled;
    }
    match &cached.expires_at {
        Some(expires_at_str) => match chrono::DateTime::parse_from_rfc3339(expires_at_str) {
            Ok(expires_at) => {
                if Utc::now() > expires_at.with_timezone(&Utc) {
                    ApiKeyValidationStatus::Expired
                } else {
                    ApiKeyValidationStatus::Valid
                }
            }
            Err(_) => ApiKeyValidationStatus::Invalid,
        },
        None => ApiKeyValidationStatus::Valid,
    }
}

/// Check if a domain entity is valid (enabled and not expired)
/// Returns detailed validation status for better error messages
fn check_entity_status(api_key: &ClientApiKey) -> ApiKeyValidationStatus {
    if !api_key.enabled {
        return ApiKeyValidationStatus::Disabled;
    }
    match api_key.expires_at {
        Some(exp) if Utc::now() > exp => ApiKeyValidationStatus::Expired,
        _ => ApiKeyValidationStatus::Valid,
    }
}

/// Convert cached value to domain entity
fn cached_to_entity(cached: ApiKeyCacheValue) -> Result<ClientApiKey, String> {
    cached.try_into()
}

/// Validate a Client API key and return its domain entity.
///
/// Flow:
/// 1. Compute the SHA-256 digest of the key — the cache and DB are both
///    keyed by digest, never the plaintext credential
/// 2. Check the Redis cache (first layer)
/// 3. Fall back to PostgreSQL (find by hash)
/// 4. Validate enabled and expiration (shared status checks above)
/// 5. Live-check the bound Client App on EVERY path (cache and DB) so
///    disabling an app immediately blocks its keys
/// 6. Update usage stats asynchronously (fire-and-forget)
/// 7. On DB success, write the cache entry for the next request
///
/// Callers keep: credential extraction, error rendering, rate limiting,
/// and identity injection.
pub async fn validate_api_key(
    state: &AppState,
    api_key: &str,
) -> Result<ClientApiKey, ApiKeyAuthError> {
    let api_key_hash = ClientApiKeyService::hash_api_key(api_key);
    let cached: Option<ApiKeyCacheValue> = match state.api_key_cache.get(&api_key_hash).await {
        Ok(v) => v,
        Err(e) => {
            error!("Redis cache error: {e}");
            return Err(ApiKeyAuthError::Internal);
        }
    };

    if let Some(cached) = cached {
        let status = check_cached_key_status(&cached);
        if status != ApiKeyValidationStatus::Valid {
            warn!("Cached API key is invalid: {status:?}");
            return Err(ApiKeyAuthError::Unauthorized(status.to_error_code()));
        }

        if let Some(app_id) = cached.client_app_id {
            ensure_client_app_enabled(state, app_id).await?;
        }

        let entity = cached_to_entity(cached).map_err(|e| {
            error!("Failed to convert cached API key: {e}");
            ApiKeyAuthError::Internal
        })?;

        record_usage_stats(state, &entity.id);
        debug!(
            "API key authenticated via cache (realm: {})",
            entity.realm_id
        );
        return Ok(entity);
    }

    let record = match state.api_key_repo.find_by_hash(&api_key_hash).await {
        Ok(Some(record)) => record,
        Ok(None) => {
            warn!("No valid API key found");
            return Err(ApiKeyAuthError::Unauthorized(ErrorCode::InvalidApiKey));
        }
        Err(e) => {
            error!("Database error finding API key: {e}");
            return Err(ApiKeyAuthError::Internal);
        }
    };

    let status = check_entity_status(&record);
    if status != ApiKeyValidationStatus::Valid {
        warn!("API key is invalid (disabled or expired): {:?}", record.id);
        return Err(ApiKeyAuthError::Unauthorized(status.to_error_code()));
    }

    if let Some(app_id) = record.client_app_id {
        ensure_client_app_enabled(state, app_id).await?;
    }

    record_usage_stats(state, &record.id);

    // The bound app (if any) is verified enabled above, so the cached flag
    // is always true here.
    let mut cache_value: ApiKeyCacheValue = (&record).into();
    cache_value.client_app_enabled = true;
    if let Err(e) = state
        .api_key_cache
        .set(&api_key_hash, &cache_value, API_KEY_CACHE_TTL_SECONDS)
        .await
    {
        warn!("Failed to cache API key: {e}");
    }

    info!(
        "API key authenticated via database (realm: {})",
        record.realm_id
    );
    Ok(record)
}

/// Check that the Client App bound to the key (if any) is still enabled.
/// A missing app counts as disabled (its key must not work).
async fn ensure_client_app_enabled(state: &AppState, app_id: Uuid) -> Result<(), ApiKeyAuthError> {
    match client_app::Entity::find_by_id(app_id)
        .one(state.db.as_ref())
        .await
    {
        Ok(Some(app)) if app.enabled => Ok(()),
        Ok(_) => {
            warn!("API key references a disabled or non-existent Client App: {app_id}");
            Err(ApiKeyAuthError::Unauthorized(ErrorCode::ClientAppDisabled))
        }
        Err(e) => {
            error!("Database error checking Client App enabled status: {e}");
            Err(ApiKeyAuthError::Internal)
        }
    }
}

/// Fire-and-forget usage stats update; never blocks or fails the request.
fn record_usage_stats(state: &AppState, api_key_id: &str) {
    let repo = state.api_key_repo.clone();
    let key_id = api_key_id.to_string();
    tokio::spawn(async move {
        if let Err(e) = repo.update_usage_stats(&key_id, Utc::now()).await {
            error!("Failed to update API key usage stats: {e}");
        }
    });
}
