// Device confirm endpoint (user approves or denies on confirmation page)
//
// After verifying the user_code, the user sees the client app info and
// chooses to approve or deny. This endpoint transitions the device state
// from "verified" to "authorized" or "denied".

use axum::{
    Extension, Json,
    extract::{Path, State},
};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use herald_api_base::application::http::rate_limit::rate_limit_hit;
use herald_api_base::application::http::server::api_entities::ApiError;
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema)]
pub struct DeviceConfirmErrorResponse {
    pub error: String,
    pub error_description: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeviceConfirmRequest {
    pub user_code: String,
    pub approved: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeviceConfirmResponse {
    pub status: String,
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/api/device/{realmId}/confirm",
    tag = "device",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
    ),
    request_body = DeviceConfirmRequest,
    responses(
        (status = 200, description = "Device code confirmed", body = DeviceConfirmResponse),
        (status = 400, description = "Invalid request", body = DeviceConfirmErrorResponse),
        (status = 404, description = "Not found", body = DeviceConfirmErrorResponse),
        (status = 409, description = "Conflict", body = DeviceConfirmErrorResponse),
    )
)]
pub async fn device_confirm(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Json(payload): Json<DeviceConfirmRequest>,
) -> Result<Json<DeviceConfirmResponse>, ApiError> {
    if identity.realm_id() != realm_id {
        return Err(ApiError::forbidden(
            "Access denied: cannot confirm device code for a different realm",
        ));
    }

    // Same per-user throttle as device_verify: confirm also probes live
    // user_codes, and an unthrottled confirm endpoint would give a bound
    // guesser a second brute-force channel against the code space.
    rate_limit_hit(
        &state,
        format!("rl:device-confirm:user:{}", identity.user_id()),
        20,
        300,
    )
    .await?;

    // user_code is stored uppercased (device_verify normalizes before the
    // index write); normalize here too or a lowercase confirm silently 404s.
    let user_code = payload.user_code.to_uppercase();

    let mut conn = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Internal server error"))?;

    // Lookup device_code from user_code index
    let user_code_key = format!("deviceUserCode:{}", user_code);
    let device_code: Option<String> = conn.get(&user_code_key).await.map_err(|e| {
        tracing::error!(error = %e, "Redis GET failed: user code lookup");
        ApiError::internal("Internal server error")
    })?;

    let Some(device_code) = device_code else {
        return Err(ApiError::with_json(
            axum::http::StatusCode::NOT_FOUND,
            DeviceConfirmErrorResponse {
                error: "not_found".to_string(),
                error_description: "Device code not found or expired".to_string(),
            },
        ));
    };

    // Lookup device state
    let device_key = format!("device:{}", device_code);
    let state_json: Option<String> = conn.get(&device_key).await.map_err(|e| {
        tracing::error!(error = %e, "Redis GET failed: device state lookup");
        ApiError::internal("Internal server error")
    })?;

    let Some(state_json) = state_json else {
        return Err(ApiError::with_json(
            axum::http::StatusCode::NOT_FOUND,
            DeviceConfirmErrorResponse {
                error: "not_found".to_string(),
                error_description: "Device code not found or expired".to_string(),
            },
        ));
    };

    let mut device_state: serde_json::Value = serde_json::from_str(&state_json).map_err(|e| {
        tracing::error!(error = %e, "Failed to parse device state JSON");
        ApiError::internal("Internal server error")
    })?;

    // Validate status is "verified"
    let status = device_state
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if status != "verified" {
        if status == "pending" {
            return Err(ApiError::bad_request_json(DeviceConfirmErrorResponse {
                error: "invalid_request".to_string(),
                error_description: "Device code has not been verified yet".to_string(),
            }));
        }
        // authorized, denied, consumed
        return Err(ApiError::conflict_json(DeviceConfirmErrorResponse {
            error: "already_used".to_string(),
            error_description: "Device code has already been authorized, denied, or consumed"
                .to_string(),
        }));
    }

    // Validate realm isolation
    let stored_realm_id = device_state
        .get("realm_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if stored_realm_id != realm_id {
        return Err(ApiError::bad_request_json(DeviceConfirmErrorResponse {
            error: "invalid_request".to_string(),
            error_description: "Realm mismatch".to_string(),
        }));
    }

    // Validate same-user constraint
    let stored_user_id = device_state
        .get("user_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if stored_user_id != identity.user_id() {
        return Err(ApiError::conflict_json(DeviceConfirmErrorResponse {
            error: "already_used".to_string(),
            error_description: "Device code was verified by a different user".to_string(),
        }));
    }

    // Transition to authorized or denied
    let new_status = if payload.approved {
        "authorized"
    } else {
        "denied"
    };
    device_state["status"] = serde_json::Value::String(new_status.to_string());

    let updated_json = serde_json::to_string(&device_state).map_err(|e| {
        tracing::error!(error = %e, "Failed to serialize device state");
        ApiError::internal("Internal server error")
    })?;

    // Write back preserving TTL
    redis::cmd("SET")
        .arg(&device_key)
        .arg(&updated_json)
        .arg("KEEPTTL")
        .query_async::<String>(&mut conn)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Redis SET KEEPTTL failed");
            ApiError::internal("Internal server error")
        })?;

    Ok(Json(DeviceConfirmResponse {
        status: new_status.to_string(),
    }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confirm_response_approved() {
        let resp = DeviceConfirmResponse {
            status: "authorized".to_string(),
        };
        assert_eq!(resp.status, "authorized");
    }

    #[test]
    fn test_confirm_response_denied() {
        let resp = DeviceConfirmResponse {
            status: "denied".to_string(),
        };
        assert_eq!(resp.status, "denied");
    }
}
