// API Key Authentication Middleware for Client API
//
// This middleware extracts the X-API-Key header and delegates validation
// (cache → DB → status → Client App cascade → usage stats) to the shared
// `validate_api_key` orchestration in api-base; on success it injects
// `Identity::ThirdParty` into the request.

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use herald_core::domain::authentication::Identity;
use tracing::warn;

use herald_api_base::application::http::common::api_key_utils::validate_api_key;
use herald_api_base::application::http::common::error_codes::ErrorCode;
use herald_api_base::application::http::common::error_helpers::json_error;
use herald_api_base::application::http::state::AppState;

/// API Key authentication middleware
///
/// Flow:
/// 1. Extract X-API-Key header (401 MissingApiKey if absent or empty)
/// 2. Validate via the shared orchestration: SHA-256 digest → Redis cache
///    → PostgreSQL fallback → enabled/expiration → live Client App check
///    → async usage stats → cache write
/// 3. Inject Identity::ThirdParty into the request
pub async fn api_key_auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
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

    let api_key_entity = match validate_api_key(&state, api_key).await {
        Ok(entity) => entity,
        Err(e) => return json_error(e.status_code(), e.error_code()),
    };

    req.extensions_mut()
        .insert(Identity::ThirdParty(api_key_entity));

    next.run(req).await
}
