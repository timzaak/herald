// API Key Authentication Middleware for Client API
//
// This middleware extracts and validates X-API-Key header,
// checks Redis cache first, then falls back to PostgreSQL.
// Updates usage statistics asynchronously.

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use chrono::Utc;
use herald_core::domain::authentication::Identity;
use herald_core::domain::client_api_keys::services::ClientApiKeyService;
use herald_core::infrastructure::client_api_keys::cache::ApiKeyCacheValue;
use tracing::{debug, error, info, warn};

use herald_api_base::application::http::common::api_key_utils::{
    API_KEY_CACHE_TTL_SECONDS, ApiKeyValidationStatus, cached_to_entity, check_cached_key_status,
    check_entity_status,
};
use herald_api_base::application::http::common::error_codes::ErrorCode;
use herald_api_base::application::http::common::error_helpers::json_error;
use herald_api_base::application::http::state::AppState;
use herald_core::entity::client_app;
use sea_orm::EntityTrait;

/// API Key authentication middleware
///
/// Flow:
/// 1. Extract X-API-Key header
/// 2. Compute SHA-256 hash of the API key
/// 3. Check Redis cache (first layer) - using API key plaintext as key
/// 4. Fall back to PostgreSQL if cache miss
///    a. Query by hash (O(1) lookup with SHA-256 deterministic salt)
///    b. Verify the hash matches
/// 5. Validate enabled and expiration
/// 6. Update usage stats asynchronously
/// 7. Inject Identity::ThirdParty into request
pub async fn api_key_auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    // 1. Extract X-API-Key header
    let api_key_header = match req.headers().get("X-API-Key").and_then(|v| v.to_str().ok()) {
        Some(key) => key,
        None => {
            warn!("Missing X-API-Key header");
            return json_error(StatusCode::UNAUTHORIZED, ErrorCode::MissingApiKey);
        }
    };

    let api_key = api_key_header.trim();

    if api_key.is_empty() {
        warn!("Empty X-API-Key header");
        return json_error(StatusCode::UNAUTHORIZED, ErrorCode::MissingApiKey);
    }

    debug!("API Key authentication attempt (length: {})", api_key.len());

    // 2. Check Redis cache, keyed by the key's SHA-256 digest — never the
    // plaintext credential (a Redis reader must not recover usable keys from
    // key names; the digest matches the DB lookup below).
    let api_key_hash = ClientApiKeyService::hash_api_key(api_key);
    let cached: Option<ApiKeyCacheValue> = match state.api_key_cache.get(&api_key_hash).await {
        Ok(Some(cached)) => {
            debug!(enabled = cached.enabled, "Cache hit for API key");
            Some(cached)
        }
        Ok(None) => {
            debug!("Cache miss for API key");
            None
        }
        Err(e) => {
            error!("Redis cache error: {}", e);
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InternalError);
        }
    };

    if let Some(cached) = cached {
        debug!("Cache hit for API key, validating...");

        // Use shared validation function (returns detailed status for better error messages)
        let validation_status = check_cached_key_status(&cached);
        if validation_status != ApiKeyValidationStatus::Valid {
            warn!("Cached API key is invalid: {:?}", validation_status);
            return json_error(
                StatusCode::UNAUTHORIZED,
                validation_status.to_error_code_enum(),
            );
        }

        // Check the associated Client App live, even on cache hit, so disabling
        // a Client App immediately blocks its API keys.
        if let Some(app_id) = cached.client_app_id {
            match client_app::Entity::find_by_id(app_id)
                .one(state.db.as_ref())
                .await
            {
                Ok(Some(app)) if app.enabled => {}
                Ok(Some(_)) => {
                    warn!(
                        "API key's Client App is disabled (cache path): {:?}",
                        cached.id
                    );
                    return json_error(StatusCode::UNAUTHORIZED, ErrorCode::ClientAppDisabled);
                }
                Ok(None) => {
                    warn!("API key references non-existent Client App: {}", app_id);
                    return json_error(StatusCode::UNAUTHORIZED, ErrorCode::ClientAppDisabled);
                }
                Err(e) => {
                    error!("Database error checking Client App enabled status: {}", e);
                    return json_error(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InternalError);
                }
            }
        }

        // Convert cached value to domain entity and inject identity
        let api_key_entity = match cached_to_entity(cached) {
            Ok(entity) => entity,
            Err(e) => {
                error!("Failed to convert cached value: {}", e);
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InternalError);
            }
        };

        let api_key_id = api_key_entity.id.clone();
        let realm_id = api_key_entity.realm_id.clone();
        req.extensions_mut()
            .insert(Identity::ThirdParty(api_key_entity));

        debug!("API key authenticated via cache (realm: {})", realm_id);

        // Update usage stats asynchronously (even on cache hit)
        let repo = state.api_key_repo.clone();
        tokio::spawn(async move {
            if let Err(e) = repo.update_usage_stats(&api_key_id, Utc::now()).await {
                error!("Failed to update API key usage stats: {}", e);
            }
        });

        return next.run(req).await;
    }

    debug!("Cache miss, querying database");

    // 3./4. Query database by hash (O(1) lookup; hash computed above)
    let api_key_record = match state.api_key_repo.find_by_hash(&api_key_hash).await {
        Ok(Some(record)) => record,
        Ok(None) => {
            warn!("No valid API key found");
            return json_error(StatusCode::UNAUTHORIZED, ErrorCode::InvalidApiKey);
        }
        Err(e) => {
            error!("Database error finding API key: {}", e);
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InternalError);
        }
    };

    // 5. Validate enabled and expiration using shared validation function
    let validation_status = check_entity_status(&api_key_record);
    if validation_status != ApiKeyValidationStatus::Valid {
        warn!(
            "API key is invalid (disabled or expired): {:?}",
            api_key_record.id
        );
        return json_error(
            StatusCode::UNAUTHORIZED,
            validation_status.to_error_code_enum(),
        );
    }

    // 5b. Check if the associated Client App is enabled (if client_app_id is set)
    let mut client_app_enabled = true;
    if let Some(app_id) = api_key_record.client_app_id {
        match client_app::Entity::find_by_id(app_id)
            .one(state.db.as_ref())
            .await
        {
            Ok(Some(app)) => {
                client_app_enabled = app.enabled;
            }
            Ok(None) => {
                warn!("API key references non-existent Client App: {}", app_id);
                client_app_enabled = false;
            }
            Err(e) => {
                error!("Database error checking Client App enabled status: {}", e);
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InternalError);
            }
        }
    }

    if !client_app_enabled {
        warn!(
            "API key's Client App is disabled (DB path): {:?}",
            api_key_record.id
        );
        return json_error(StatusCode::UNAUTHORIZED, ErrorCode::ClientAppDisabled);
    }

    // 6. Update usage stats asynchronously (don't block request)
    let api_key_id = api_key_record.id.clone();
    let repo = state.api_key_repo.clone();
    tokio::spawn(async move {
        if let Err(e) = repo.update_usage_stats(&api_key_id, Utc::now()).await {
            error!("Failed to update API key usage stats: {}", e);
        }
    });

    // 7. Write to cache for next request (digest cache key, see step 2)
    let mut cache_value: ApiKeyCacheValue = (&api_key_record).into();
    cache_value.client_app_enabled = client_app_enabled;
    if let Err(e) = state
        .api_key_cache
        .set(&api_key_hash, &cache_value, API_KEY_CACHE_TTL_SECONDS)
        .await
    {
        warn!("Failed to cache API key: {}", e);
    }

    // 8. Inject Identity
    let realm_id = api_key_record.realm_id.clone();
    req.extensions_mut()
        .insert(Identity::ThirdParty(api_key_record));

    info!("API key authenticated via database (realm: {})", realm_id);

    next.run(req).await
}
