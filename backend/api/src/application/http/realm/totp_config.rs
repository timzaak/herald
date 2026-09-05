// Realm TOTP configuration handlers

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
use herald_api_base::application::http::auth::util::{ClientIp, user_agent_from_headers};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_core::domain::audit::{
    AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType, NewAuditEvent,
};
use herald_core::domain::authentication::Identity;
use herald_core::domain::realm::RealmService;
use herald_core::domain::realm_config::{ConfigType, RealmConfigService, UpsertRealmConfigRequest};
use herald_core::domain::user_totp::{RealmTotpConfigRepository, RealmTotpStatistics};
use herald_core::infrastructure::user_totp::PostgresRealmTotpConfigRepository;

pub use crate::application::http::server::api_entities::ErrorResponse;

// ============================================================================
// Update Realm TOTP Configuration
// ============================================================================

#[derive(Debug, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRealmTotpConfigRequest {
    pub enabled: bool,
    pub force_enabled: bool,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRealmTotpConfigResponse {
    pub message: String,
    pub enabled: bool,
    pub force_enabled: bool,
    pub updated_at: String,
}

/// Update realm TOTP configuration
///
/// Updates the TOTP (Time-based One-Time Password) configuration for a realm, including whether TOTP is enabled
/// and whether it is force-required for all users. Requires 'settings.manage' permission.
#[utoipa::path(
    put,
    path = "/api/realms/{realmId}/config/totp",
    tag = "realms",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = UpdateRealmTotpConfigRequest,
    responses(
        (status = 200, description = "Realm TOTP configuration updated", body = UpdateRealmTotpConfigResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 404, description = "Realm not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn handle_update_realm_totp_config(
    State(state): State<AppState>,
    Path(realm_id): Path<String>,
    Extension(identity): Extension<Identity>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Valid(Json(req)): Valid<Json<UpdateRealmTotpConfigRequest>>,
) -> Result<ApiResult<UpdateRealmTotpConfigResponse>, ApiError> {
    let admin = AdminIdentity::require(identity.clone(), &realm_id, "realm TOTP configuration")?;
    admin
        .require_permission(&state, "settings", "manage")
        .await?;

    // Use RealmConfigService to store TOTP configuration
    let service = state.service.realm_config_service();

    // Store TOTP config as a single JSON object in config_value
    let totp_config = serde_json::json!({
        "enabled": req.enabled,
        "force_enabled": req.force_enabled
    });

    let config_request = UpsertRealmConfigRequest {
        config_type: ConfigType::Totp,
        config_key: "settings".to_string(),
        config_value: totp_config.to_string(),
        is_secret: Some(false),
        enabled: Some(true), // Config entry itself is enabled
        metadata: None,      // No additional metadata needed
    };

    let config = service
        .upsert_config(admin.identity().clone(), realm_id.clone(), config_request)
        .await
        .map_err(|e| {
            tracing::error!(
                "Failed to upsert TOTP config via RealmConfigService: {:?}",
                e
            );
            match e {
                herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                    ApiError::forbidden(msg)
                }
                herald_core::domain::common::entities::app_errors::CoreError::NotFound => {
                    ApiError::not_found("Realm not found")
                }
                _ => ApiError::internal("Internal server error"),
            }
        })?;

    // Audit TOTP policy change (mirrors the passkey config audit rule: admin
    // MFA-policy changes are "关键配置变更" per the audit PRD). Best-effort: an
    // audit failure must not fail the already-succeeded config write.
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
            action: AuditAction::TotpConfigUpdate,
            actor_id: identity.user_id(),
            actor_type: None,
            actor_name: identity.as_user().map(|u| u.email.clone()),
            target_type: AuditTargetType::Realm,
            target_id: realm_id.clone(),
            target_name,
            result: AuditResult::Success,
            details: Some(serde_json::json!({
                "enabled": req.enabled,
                "force_enabled": req.force_enabled,
            })),
            ip_address: Some(ip),
            user_agent: user_agent_from_headers(&headers),
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %audit_err, "Failed to record TOTP config audit event");
    }

    Ok(ApiResult::ok(UpdateRealmTotpConfigResponse {
        message: "Realm TOTP configuration updated".to_string(),
        enabled: req.enabled,
        force_enabled: req.force_enabled,
        updated_at: config.updated_at.to_rfc3339(),
    }))
}

// ============================================================================
// Get Realm TOTP Configuration and Statistics
// ============================================================================

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetRealmTotpConfigResponse {
    pub enabled: bool,
    pub force_enabled: bool,
    pub statistics: RealmTotpStatisticsResponse,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RealmTotpStatisticsResponse {
    pub total_users: i64,
    pub totp_enabled_users: i64,
    pub totp_disabled_users: i64,
    pub enablement_rate: f64,
}

impl From<RealmTotpStatistics> for RealmTotpStatisticsResponse {
    fn from(stats: RealmTotpStatistics) -> Self {
        Self {
            total_users: stats.total_users,
            totp_enabled_users: stats.totp_enabled_users,
            totp_disabled_users: stats.totp_disabled_users,
            enablement_rate: stats.enablement_rate,
        }
    }
}

/// Get realm TOTP configuration and statistics
///
/// Retrieves the TOTP configuration for a realm along with statistics about TOTP adoption
/// including total users, enabled users, disabled users, and enablement rate. Requires 'settings.view' permission.
#[utoipa::path(
    get,
    path = "/api/realms/{realmId}/config/totp",
    tag = "realms",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        (status = 200, description = "Realm TOTP configuration retrieved", body = GetRealmTotpConfigResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 404, description = "Realm not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn handle_get_realm_totp_config(
    State(state): State<AppState>,
    Path(realm_id): Path<String>,
    Extension(identity): Extension<Identity>,
) -> Result<ApiResult<GetRealmTotpConfigResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "realm TOTP configuration")?;
    admin.require_permission(&state, "settings", "view").await?;

    // Use RealmConfigService to get TOTP configuration
    let service = state.service.realm_config_service();

    let config_entry = service
        .get_config(
            admin.identity().clone(),
            realm_id.clone(),
            ConfigType::Totp.as_ref().to_string(),
            "settings".to_string(),
        )
        .await
        .map_err(|e| {
            tracing::error!("Failed to get TOTP config via RealmConfigService: {:?}", e);
            match e {
                herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                    ApiError::forbidden(msg)
                }
                herald_core::domain::common::entities::app_errors::CoreError::NotFound => {
                    ApiError::not_found("Realm not found")
                }
                _ => ApiError::internal("Internal server error"),
            }
        })?;

    // Parse TOTP config from JSON in config_value, or use defaults if not exists
    let (enabled, force_enabled) = if let Some(entry) = config_entry {
        // Parse JSON from config_value
        match serde_json::from_str::<serde_json::Value>(&entry.config_value) {
            Ok(config) => {
                let enabled = config
                    .get("enabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let force_enabled = config
                    .get("force_enabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                (enabled, force_enabled)
            }
            Err(e) => {
                tracing::error!("Failed to parse TOTP config JSON: {}", e);
                (false, false)
            }
        }
    } else {
        (false, false)
    };

    // Get statistics using the existing repository (statistics are TOTP-specific)
    let repo = PostgresRealmTotpConfigRepository::new(state.db.clone());
    let statistics = repo
        .get_realm_totp_statistics(&realm_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get realm TOTP statistics: {:?}", e);
            ApiError::from(e)
        })?;

    Ok(ApiResult::ok(GetRealmTotpConfigResponse {
        enabled,
        force_enabled,
        statistics: statistics.into(),
    }))
}
