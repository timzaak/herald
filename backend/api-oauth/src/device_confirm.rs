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

use herald_api_auth::consent_gate::evaluate_login_consent_gate;
use herald_api_base::application::http::rate_limit::rate_limit_hit;
use herald_api_base::application::http::server::api_entities::ApiError;
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;
use herald_core::domain::legal::LegalAgreementSummary;
use herald_core::domain::user::ports::UserRepository;

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
    /// Present when the login consent gate blocked an approval: the device was
    /// NOT authorized. The browser session must record consent (POST
    /// /api/user/consent) for the listed agreements and re-confirm;
    /// the device state stays "verified" so the retry completes without
    /// restarting the device flow.
    pub consent_required: Option<bool>,
    pub agreements: Option<Vec<LegalAgreementSummary>>,
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

    // Login consent gate (legal-consent PRD §4.1 — the rule covers every login
    // entrance, device code included): approving a device authorization is a
    // business function whose poll endpoint mints a full token family for the
    // CLI, so a stale-consent session (including a consent-restricted family)
    // must not pass it. The gate runs on the session identity only — the
    // device state is validated solely by the FCALL below. The device state
    // deliberately stays "verified" — the user records consent via
    // POST /api/user/consent and simply re-confirms. Denials are not gated:
    // they issue nothing.
    if payload.approved {
        let confirm_user_id = uuid::Uuid::parse_str(&identity.user_id())
            .map_err(|_| ApiError::unauthorized("Invalid session identity"))?;
        let user = state
            .user_repository
            .get_user_by_id(confirm_user_id)
            .await
            .map_err(|_| ApiError::unauthorized("Session user no longer exists"))?;
        if let Some(summaries) =
            evaluate_login_consent_gate(&state, &user, &realm_id, None, None, None).await
        {
            tracing::info!(
                realm_id = %realm_id,
                user_id = %confirm_user_id,
                user_code = %user_code,
                "device confirm blocked at consent gate; device stays verified"
            );
            return Ok(Json(DeviceConfirmResponse {
                status: "consent_required".to_string(),
                consent_required: Some(true),
                agreements: Some(summaries),
            }));
        }
    }

    // Transition to authorized or denied. The FCALL is the single validation
    // authority: status/realm/user are checked atomically with the write, so
    // a concurrent confirm cannot overwrite a terminal state (device-code
    // PRD: transitions are irreversible) — the losing caller gets
    // already_used.
    let device_key = format!("device:{}", device_code);
    let result: String = redis::cmd("FCALL")
        .arg("device_confirm_transition")
        .arg(1) // num_keys
        .arg(&device_key)
        .arg(&realm_id)
        .arg(identity.user_id())
        .arg(if payload.approved { "1" } else { "0" })
        .query_async(&mut conn)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Redis FCALL device_confirm_transition failed");
            ApiError::internal("Internal server error")
        })?;

    let parsed: serde_json::Value = serde_json::from_str(&result).map_err(|e| {
        tracing::error!(error = %e, "Failed to parse Redis function result");
        ApiError::internal("Internal server error")
    })?;

    if !parsed.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        let error = parsed
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("not_found");
        return Err(match error {
            "not_verified" => ApiError::bad_request_json(DeviceConfirmErrorResponse {
                error: "invalid_request".to_string(),
                error_description: "Device code has not been verified yet".to_string(),
            }),
            "realm_mismatch" => ApiError::bad_request_json(DeviceConfirmErrorResponse {
                error: "invalid_request".to_string(),
                error_description: "Realm mismatch".to_string(),
            }),
            "already_used" => ApiError::conflict_json(DeviceConfirmErrorResponse {
                error: "already_used".to_string(),
                error_description: "Device code was verified by a different user, or has already been authorized, denied, or consumed"
                    .to_string(),
            }),
            _ => ApiError::with_json(
                axum::http::StatusCode::NOT_FOUND,
                DeviceConfirmErrorResponse {
                    error: "not_found".to_string(),
                    error_description: "Device code not found or expired".to_string(),
                },
            ),
        });
    }

    let new_status = parsed
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("authorized");

    Ok(Json(DeviceConfirmResponse {
        status: new_status.to_string(),
        consent_required: None,
        agreements: None,
    }))
}
