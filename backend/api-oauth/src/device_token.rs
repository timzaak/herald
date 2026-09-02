// Device token polling endpoint (RFC 8628 section 3.4, 3.5)
//
// CLI tools poll this endpoint to obtain an access token after the user
// has authorized the device code on the verification page.

use axum::{
    Form, Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use herald_api_base::application::http::server::api_entities::ApiError;
use herald_api_base::application::http::state::AppState;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeviceTokenRequest {
    pub grant_type: String,
    pub device_code: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeviceTokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeviceTokenErrorResponse {
    pub error: String,
    pub error_description: String,
}

// ---------------------------------------------------------------------------
// Redis Function
// ---------------------------------------------------------------------------

const DEVICE_TOKEN_FUNCTION_LIBRARY: &str = "herald_device_token";

/// Redis Function for atomic device token polling state management.
///
/// Atomically handles all state transitions and interval enforcement in a
/// single FCALL invocation, eliminating race conditions between concurrent
/// poll requests.
///
/// Operation order (terminal states checked first):
/// 1. Key missing           -> expired_token
/// 2. status == consumed    -> invalid_request
/// 3. status == denied      -> access_denied
/// 4. status == authorized  -> consume + return user data
/// 5. interval too fast     -> slow_down (interval += 5)
/// 6. pending / verified    -> authorization_pending
const DEVICE_TOKEN_FUNCTION_CODE: &str = "#!lua name=herald_device_token\n\
\n\
local function device_token_poll(keys, args)\n\
  local key = keys[1]\n\
  local now = tonumber(args[1])\n\
\n\
  local data = redis.call('GET', key)\n\
  if not data then\n\
    return cjson.encode({ok=false, error='expired_token'})\n\
  end\n\
\n\
  local state = cjson.decode(data)\n\
\n\
  -- Terminal states first\n\
  if state.status == 'consumed' then\n\
    return cjson.encode({ok=false, error='invalid_request'})\n\
  end\n\
\n\
  if state.status == 'denied' then\n\
    return cjson.encode({ok=false, error='access_denied'})\n\
  end\n\
\n\
  -- Authorized: consume and return user data\n\
  if state.status == 'authorized' then\n\
    state.status = 'consumed'\n\
    state.last_poll_at = now\n\
    redis.call('SET', key, cjson.encode(state), 'KEEPTTL')\n\
    return cjson.encode({\n\
      ok=true,\n\
      user_id=state.user_id,\n\
      realm_id=state.realm_id,\n\
      client_id=state.client_id\n\
    })\n\
  end\n\
\n\
  -- Check polling interval\n\
  if state.last_poll_at > 0 then\n\
    local elapsed = now - state.last_poll_at\n\
    if elapsed < state.interval then\n\
      state.interval = state.interval + 5\n\
      state.last_poll_at = now\n\
      redis.call('SET', key, cjson.encode(state), 'KEEPTTL')\n\
      return cjson.encode({ok=false, error='slow_down'})\n\
    end\n\
  end\n\
\n\
  -- Still pending or verified\n\
  state.last_poll_at = now\n\
  redis.call('SET', key, cjson.encode(state), 'KEEPTTL')\n\
  return cjson.encode({ok=false, error='authorization_pending'})\n\
end\n\
\n\
redis.register_function('device_token_poll', device_token_poll)\n\
";

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Load the device token Redis Function library.
///
/// Idempotent -- safe to call multiple times (REPLACE semantics).
pub async fn init_device_token_function(state: &AppState) -> Result<(), ApiError> {
    let mut conn = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Internal server error"))?;

    redis::cmd("FUNCTION")
        .arg("LOAD")
        .arg("REPLACE")
        .arg(DEVICE_TOKEN_FUNCTION_CODE)
        .query_async::<String>(&mut conn)
        .await
        .map_err(|e| {
            tracing::error!("Failed to load device token function library: {e}");
            ApiError::internal("Internal server error")
        })?;

    tracing::info!(
        "Redis Function library '{}' loaded successfully",
        DEVICE_TOKEN_FUNCTION_LIBRARY
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/api/device/{realmId}/token",
    tag = "device",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
    ),
    request_body = DeviceTokenRequest,
    responses(
        (status = 200, description = "Access token issued", body = DeviceTokenResponse),
        (status = 400, description = "Bad request / pending / slow_down / expired", body = DeviceTokenErrorResponse),
        (status = 403, description = "Access denied", body = DeviceTokenErrorResponse),
    )
)]
pub async fn device_token(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    Form(payload): Form<DeviceTokenRequest>,
) -> Result<Json<DeviceTokenResponse>, ApiError> {
    // Validate grant_type
    if payload.grant_type != "urn:ietf:params:oauth:grant-type:device_code" {
        return Err(ApiError::bad_request_json(DeviceTokenErrorResponse {
            error: "invalid_request".to_string(),
            error_description: "grant_type must be 'urn:ietf:params:oauth:grant-type:device_code'"
                .to_string(),
        }));
    }

    // Validate device_code is present
    if payload.device_code.is_empty() {
        return Err(ApiError::bad_request_json(DeviceTokenErrorResponse {
            error: "invalid_request".to_string(),
            error_description: "device_code is required".to_string(),
        }));
    }

    // Execute Redis Function
    let mut conn = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Internal server error"))?;

    let now = chrono::Utc::now().timestamp();
    let key = format!("device:{}", payload.device_code);

    let result: String = redis::cmd("FCALL")
        .arg("device_token_poll")
        .arg(1) // num_keys
        .arg(&key)
        .arg(now)
        .query_async(&mut conn)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Redis FCALL device_token_poll failed");
            ApiError::internal("Internal server error")
        })?;

    // Parse result
    let parsed: serde_json::Value = serde_json::from_str(&result).map_err(|e| {
        tracing::error!(error = %e, "Failed to parse Redis function result");
        ApiError::internal("Internal server error")
    })?;

    let ok = parsed.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);

    if ok {
        // Success: authorized -> consumed, generate JWT
        let user_id = parsed.get("user_id").and_then(|v| v.as_str()).unwrap_or("");
        let stored_realm_id = parsed
            .get("realm_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Validate realm match
        if stored_realm_id != realm_id {
            return Err(ApiError::bad_request_json(DeviceTokenErrorResponse {
                error: "invalid_request".to_string(),
                error_description: "Realm mismatch".to_string(),
            }));
        }

        let jwt_secret = crate::helper::jwt_secret(&state)?;
        let jwt_token = crate::helper::generate_jwt_token(user_id, stored_realm_id, jwt_secret)?;
        let expires_in = crate::helper::jwt_expiration_seconds()?;

        Ok(Json(DeviceTokenResponse {
            access_token: jwt_token,
            token_type: "Bearer".to_string(),
            expires_in,
        }))
    } else {
        // Error response
        let error = parsed
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("invalid_request");
        let (status, description) = match error {
            "authorization_pending" => (
                axum::http::StatusCode::BAD_REQUEST,
                "The authorization request is still pending",
            ),
            "slow_down" => (
                axum::http::StatusCode::BAD_REQUEST,
                "Polling too fast; increase interval by 5 seconds",
            ),
            "expired_token" => (
                axum::http::StatusCode::BAD_REQUEST,
                "The device code has expired",
            ),
            "access_denied" => (
                axum::http::StatusCode::FORBIDDEN,
                "The user denied the authorization request",
            ),
            _ => (axum::http::StatusCode::BAD_REQUEST, "Invalid request"),
        };

        Err(ApiError::with_json(
            status,
            DeviceTokenErrorResponse {
                error: error.to_string(),
                error_description: description.to_string(),
            },
        ))
    }
}
