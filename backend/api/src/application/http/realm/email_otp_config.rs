// Realm Email OTP configuration handlers.
//
// Admin-only PUT/GET for the per-Realm OTP login + auto-register switches.
// Mirrors `totp_config.rs`, persisting `{enabled, auto_register}` JSON under
// `ConfigType::EmailOtp` / `config_key="settings"`. The anonymous read of the
// `enabled` flag lives in `api-auth::email_otp::status`; here we only expose
// the privileged admin view/edit.

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::HeaderMap,
};
use axum_valid::Valid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use crate::application::http::server::api_entities::{ApiError, ApiResult};
use crate::application::http::state::AppState;
use herald_api_base::application::http::auth::util::{
    ClientIp, EmailOtpSettings, user_agent_from_headers,
};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_core::domain::audit::{
    AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType, NewAuditEvent,
};
use herald_core::domain::authentication::Identity;
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::realm::RealmService;
use herald_core::domain::realm_config::{ConfigType, RealmConfigService, UpsertRealmConfigRequest};

pub use crate::application::http::server::api_entities::ErrorResponse;

/// Map a realm-config service error to an API error. Shared by the GET and
/// PUT handlers below so the two mappings cannot drift.
fn map_realm_config_error(e: CoreError) -> ApiError {
    match e {
        CoreError::Forbidden(msg) => ApiError::forbidden(msg),
        CoreError::NotFound => ApiError::not_found("Realm not found"),
        _ => ApiError::internal("Internal server error"),
    }
}

// ============================================================================
// Update Realm Email OTP Configuration
// ============================================================================

#[derive(Debug, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRealmEmailOtpConfigRequest {
    pub enabled: bool,
    pub auto_register: bool,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRealmEmailOtpConfigResponse {
    pub message: String,
    pub enabled: bool,
    pub auto_register: bool,
    pub updated_at: String,
}

/// Update realm Email OTP configuration
///
/// Enables/disables Email OTP login for a realm and separately controls whether
/// unregistered emails are auto-registered on successful verification. Requires
/// 'settings.manage' permission. Stored as `ConfigType::EmailOtp` /
/// `config_key="settings"` JSON.
#[utoipa::path(
    put,
    path = "/api/realms/{realmId}/config/email-otp",
    tag = "realms",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = UpdateRealmEmailOtpConfigRequest,
    responses(
        (status = 200, description = "Realm Email OTP configuration updated", body = UpdateRealmEmailOtpConfigResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 404, description = "Realm not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn handle_update_realm_email_otp_config(
    State(state): State<AppState>,
    Path(realm_id): Path<String>,
    Extension(identity): Extension<Identity>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Valid(Json(req)): Valid<Json<UpdateRealmEmailOtpConfigRequest>>,
) -> Result<ApiResult<UpdateRealmEmailOtpConfigResponse>, ApiError> {
    let admin =
        AdminIdentity::require(identity.clone(), &realm_id, "realm Email OTP configuration")?;
    admin
        .require_permission(&state, "settings", "manage")
        .await?;

    let service = state.service.realm_config_service();

    // Persist as a single JSON object. `auto_register` only takes
    // effect when `enabled` is true; we still store the flag regardless so an
    // admin can pre-set it before flipping the master switch.
    let email_otp_config = serde_json::json!({
        "enabled": req.enabled,
        "auto_register": req.auto_register
    });

    let config_request = UpsertRealmConfigRequest {
        config_type: ConfigType::EmailOtp,
        config_key: "settings".to_string(),
        config_value: email_otp_config.to_string(),
        is_secret: Some(false),
        enabled: Some(true),
        metadata: None,
    };

    let config = service
        .upsert_config(admin.identity().clone(), realm_id.clone(), config_request)
        .await
        .map_err(|e| {
            tracing::error!(
                "Failed to upsert Email OTP config via RealmConfigService: {:?}",
                e
            );
            map_realm_config_error(e)
        })?;

    // Audit Email OTP policy change (mirrors the passkey/TOTP config audit
    // rule: admin auth-policy changes are "关键配置变更" per the audit PRD).
    // Best-effort: an audit failure must not fail the already-succeeded write.
    let target_name = match state
        .service
        .realm_service()
        .get_realm(identity.clone(), realm_id.clone())
        .await
    {
        Ok(realm) => Some(realm.name),
        Err(e) => {
            tracing::warn!(error = %e, realm_id = %realm_id, "Failed to resolve realm name for audit event");
            None
        }
    };
    if let Err(audit_err) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: realm_id.clone(),
            category: AuditCategory::Auth,
            action: AuditAction::EmailOtpConfigUpdate,
            actor_id: identity.user_id(),
            actor_type: None,
            actor_name: identity.as_user().map(|u| u.email.clone()),
            target_type: AuditTargetType::Realm,
            target_id: realm_id.clone(),
            target_name,
            result: AuditResult::Success,
            details: Some(serde_json::json!({
                "enabled": req.enabled,
                "auto_register": req.auto_register,
            })),
            ip_address: Some(ip),
            user_agent: user_agent_from_headers(&headers),
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %audit_err, "Failed to record Email OTP config audit event");
    }

    Ok(ApiResult::ok(UpdateRealmEmailOtpConfigResponse {
        message: "Realm Email OTP configuration updated".to_string(),
        enabled: req.enabled,
        auto_register: req.auto_register,
        updated_at: config.updated_at.to_rfc3339(),
    }))
}

// ============================================================================
// Get Realm Email OTP Configuration
// ============================================================================

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetRealmEmailOtpConfigResponse {
    pub enabled: bool,
    pub auto_register: bool,
}

/// Get realm Email OTP configuration
///
/// Retrieves the Email OTP configuration (login enabled + auto-register) for a
/// realm. Requires 'settings.view' permission. Defaults to
/// `{ enabled: false, auto_register: false }` when no config exists.
#[utoipa::path(
    get,
    path = "/api/realms/{realmId}/config/email-otp",
    tag = "realms",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        (status = 200, description = "Realm Email OTP configuration retrieved", body = GetRealmEmailOtpConfigResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 404, description = "Realm not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn handle_get_realm_email_otp_config(
    State(state): State<AppState>,
    Path(realm_id): Path<String>,
    Extension(identity): Extension<Identity>,
) -> Result<ApiResult<GetRealmEmailOtpConfigResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "realm Email OTP configuration")?;
    admin.require_permission(&state, "settings", "view").await?;

    let service = state.service.realm_config_service();

    let config_entry = service
        .get_config(
            admin.identity().clone(),
            realm_id.clone(),
            ConfigType::EmailOtp.as_ref().to_string(),
            "settings".to_string(),
        )
        .await
        .map_err(|e| {
            tracing::error!(
                "Failed to get Email OTP config via RealmConfigService: {:?}",
                e
            );
            map_realm_config_error(e)
        })?;

    // Parse the JSON config_value via the shared `EmailOtpSettings` shape, so
    // the admin view and the anonymous `status` helper agree on the contract.
    // Absent or malformed payloads degrade to the all-false default.
    let settings = match config_entry {
        Some(entry) => serde_json::from_str::<EmailOtpSettings>(&entry.config_value)
            .unwrap_or_else(|e| {
                tracing::error!("Failed to parse Email OTP config JSON: {}", e);
                EmailOtpSettings::default()
            }),
        None => EmailOtpSettings::default(),
    };

    Ok(ApiResult::ok(GetRealmEmailOtpConfigResponse {
        enabled: settings.enabled,
        auto_register: settings.auto_register,
    }))
}
