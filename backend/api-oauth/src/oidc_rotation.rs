//! Internal OIDC signing-key rotation endpoint.
//!
//! Operations surface for rotating the platform signing key: the previous
//! key moves into its 7-day retained overlap window (still published in
//! JWKS), a fresh active key takes over signing immediately. Guarded by the
//! same `X-Herald-Ask-Key` shared secret as the Caddy on-demand-TLS ask
//! endpoint — constant-time comparison plus a local failure throttle.

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::{
    ActorType, AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType,
    NewAuditEvent,
};
use herald_core::domain::realm::ADMIN_REALM_ID;
use herald_core::infrastructure::oidc_signing_key::{OidcSigningKeyError, RotationOutcome};

// Own budget so rotation-key guessing cannot lock out (or be masked by) the
// custom-domain ask gate's throttle.
static ROTATE_KEY_FAILURE_THROTTLE:
    herald_api_base::application::http::internal_auth::FailureThrottle =
    herald_api_base::application::http::internal_auth::FailureThrottle::new();

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OidcRotationResponse {
    pub message: String,
    /// kid of the newly promoted active key.
    pub new_key_id: String,
    /// Overlap-window deadline for the key that was just demoted (RFC3339).
    pub retained_until: String,
    /// Keys whose overlap window had already expired and were retired by this
    /// rotation's housekeeping.
    pub retired_key_ids: Vec<String>,
}

fn map_key_error(error: OidcSigningKeyError) -> ApiError {
    match error {
        OidcSigningKeyError::ConcurrentRotation => {
            ApiError::conflict("A concurrent signing-key rotation is in progress")
        }
        error => {
            tracing::error!(%error, "OIDC signing key rotation failed");
            ApiError::internal("Internal server error")
        }
    }
}

/// Best-effort `oidc.signing_key_rotate` audit — never blocks the rotation.
async fn audit_rotation(state: &AppState, outcome: &RotationOutcome) {
    if let Err(error) = state
        .audit_event_repository
        .create(NewAuditEvent {
            // Platform-level asset: audit_events.realm_id is NOT NULL, so the
            // rotation lands in the admin realm where operations reads it.
            realm_id: ADMIN_REALM_ID.to_string(),
            category: AuditCategory::OAuth,
            action: AuditAction::OidcSigningKeyRotate,
            actor_id: "system".to_string(),
            actor_type: Some(ActorType::System),
            actor_name: None,
            target_type: AuditTargetType::Realm,
            target_id: ADMIN_REALM_ID.to_string(),
            target_name: None,
            result: AuditResult::Success,
            details: Some(serde_json::json!({
                "new_kid": outcome.new_kid,
                "retained_until": outcome.retained_until.to_rfc3339(),
                "retired_kids": outcome.retired_kids,
            })),
            ip_address: None,
            user_agent: None,
            trace_id: None,
        })
        .await
    {
        tracing::warn!(%error, "Failed to record OIDC signing-key rotation audit event");
    }
}

#[utoipa::path(
    post,
    path = "/api/internal/oidc/signing-key/rotate",
    tag = "oauth",
    responses(
        (status = 200, description = "Signing key rotated; previous key retained through its overlap window", body = OidcRotationResponse),
        (status = 401, description = "Missing or mismatched X-Herald-Ask-Key", body = ErrorResponse),
        (status = 409, description = "Concurrent rotation in progress", body = ErrorResponse),
        (status = 429, description = "Too many ask key attempts", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    )
)]
pub async fn oidc_rotate_signing_key(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<OidcRotationResponse>), ApiError> {
    // Same shared-secret gate as the Caddy ask endpoint (constant-time
    // compare, guesses throttled); see `require_ask_key` for the protocol.
    herald_api_base::application::http::internal_auth::require_ask_key(
        &headers,
        &state.custom_domain_ask_key,
        &ROTATE_KEY_FAILURE_THROTTLE,
    )?;

    let outcome = state
        .oidc_signing_key_store
        .rotate()
        .await
        .map_err(map_key_error)?;

    audit_rotation(&state, &outcome).await;

    Ok((
        StatusCode::OK,
        Json(OidcRotationResponse {
            message: "OIDC signing key rotated".to_string(),
            new_key_id: outcome.new_kid,
            retained_until: outcome.retained_until.to_rfc3339(),
            retired_key_ids: outcome.retired_kids,
        }),
    ))
}
