// Protocol-level API Key authentication + per-key rate limiting for /mcp.
//
// Transport forms: `X-API-Key` (preferred) or `Authorization: Bearer <key>`
// (fallback). Unlike api-points' flexible middleware, a Bearer credential is
// ALWAYS interpreted as a Client API Key here — the MCP surface has no
// first-party browser-token semantics, and accepting one would bypass the
// "realm from credential + ThirdParty principal" model every tool relies on.
//
// The validation orchestration (cache → DB → status → Client App cascade →
// usage stats) is the shared `validate_api_key` core in api-base's
// api_key_utils (the same core /api/ext and /api/points use); this
// middleware adds only the MCP-specific transport extraction, the per-key
// rate limit, and identity injection. Failures render with the same
// error-code set and HTTP 401 so operators see one consistent API-key
// failure vocabulary across surfaces.

use axum::{
    extract::{Request, State},
    http::{StatusCode, header::AUTHORIZATION},
    middleware::Next,
    response::{IntoResponse, Response},
};
use herald_api_base::application::http::common::api_key_utils::validate_api_key;
use herald_api_base::application::http::common::error_codes::ErrorCode;
use herald_api_base::application::http::common::error_helpers::json_error;
use herald_api_base::application::http::rate_limit::{RateLimitConfig, rate_limit};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;
use tracing::{debug, info, warn};

/// Per-key quota: 60 requests / 60s. Read-only, agent-paced traffic —
/// comfortably above interactive use, far below abuse throughput. The key is
/// the authenticated API key id, so quotas can never leak across keys or
/// realms. Enforced in dev/test so scenario suites exercise the real path.
pub const MCP_RATE_LIMIT: RateLimitConfig = RateLimitConfig {
    max_requests: 60,
    window_secs: 60,
    enforce_in_dev: true,
};

fn extract_api_key(req: &Request) -> Option<String> {
    if let Some(key) = req.headers().get("X-API-Key").and_then(|v| v.to_str().ok()) {
        let key = key.trim();
        if !key.is_empty() {
            return Some(key.to_string());
        }
    }
    let bearer = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())?
        .trim();
    let token = bearer
        .strip_prefix("Bearer ")
        .or_else(|| bearer.strip_prefix("bearer "))?;
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_string())
}

pub async fn mcp_api_key_auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let api_key = match extract_api_key(&req) {
        Some(key) => key,
        None => {
            warn!("MCP request without an API key (X-API-Key or Bearer)");
            return json_error(StatusCode::UNAUTHORIZED, ErrorCode::MissingApiKey);
        }
    };

    let api_key_entity = match validate_api_key(&state, &api_key).await {
        Ok(entity) => entity,
        Err(e) => return json_error(e.status_code(), e.error_code()),
    };

    let api_key_id = api_key_entity.id.clone();
    let realm_id = api_key_entity.realm_id.clone();

    // Rate limit after successful auth: the quota key is the authenticated
    // key id, so budgets are per-key by construction.
    if let Err(e) = rate_limit(&state, format!("mcp:key:{api_key_id}"), MCP_RATE_LIMIT).await {
        warn!("MCP rate limit exceeded for key {api_key_id} (realm {realm_id})");
        return e.into_response();
    }

    debug!("MCP API key authenticated (realm: {realm_id})");
    req.extensions_mut()
        .insert(Identity::ThirdParty(api_key_entity));
    info!("MCP request authenticated (realm: {realm_id})");

    next.run(req).await
}
