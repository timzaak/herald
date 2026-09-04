// Realm Passkey configuration handlers

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::HeaderMap,
};
use axum_valid::Valid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

fn validate_user_verification(value: &str) -> Result<(), validator::ValidationError> {
    match value {
        "preferred" | "required" => Ok(()),
        _ => Err(validator::ValidationError::new("invalid_user_verification")),
    }
}

use crate::application::http::server::api_entities::{ApiError, ApiResult};
use crate::application::http::state::AppState;
use herald_api_base::application::http::auth::util::{ClientIp, user_agent_from_headers};
use herald_core::domain::audit::{
    AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType, NewAuditEvent,
};
use herald_core::domain::authentication::Identity;
use herald_core::domain::authorization::PermissionService;
use herald_core::domain::realm::RealmService;
use herald_core::domain::realm_config::{ConfigType, RealmConfigService, UpsertRealmConfigRequest};

pub use crate::application::http::server::api_entities::ErrorResponse;

const DEFAULT_USER_VERIFICATION: &str = "preferred";
const DEFAULT_CROSS_PLATFORM_AUTHENTICATOR: bool = true;
const DEFAULT_FORCE_ENABLED: bool = false;

// ============================================================================
// Update Realm Passkey Configuration
// ============================================================================

#[derive(Debug, Deserialize, Serialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRealmPasskeyConfigRequest {
    pub enabled: bool,
    /// Force mode: guide users without a passkey to register one (frontend
    /// guidance only — login is never blocked).
    #[serde(default)]
    pub force_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(custom(function = "validate_user_verification"))]
    pub user_verification: Option<String>,
    pub cross_platform_authenticator: Option<bool>,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRealmPasskeyConfigResponse {
    pub message: String,
    pub enabled: bool,
    pub force_enabled: bool,
    pub updated_at: String,
}

/// Update realm Passkey configuration
///
/// Updates the Passkey configuration for a realm, including whether Passkey authentication is enabled.
/// Requires 'settings.manage' permission.
#[utoipa::path(
    put,
    path = "/api/realms/{realmId}/config/passkey",
    tag = "realms",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = UpdateRealmPasskeyConfigRequest,
    responses(
        (status = 200, description = "Realm Passkey configuration updated", body = UpdateRealmPasskeyConfigResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 404, description = "Realm not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn handle_update_realm_passkey_config(
    State(state): State<AppState>,
    Path(realm_id): Path<String>,
    Extension(identity): Extension<Identity>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Valid(Json(req)): Valid<Json<UpdateRealmPasskeyConfigRequest>>,
) -> Result<ApiResult<UpdateRealmPasskeyConfigResponse>, ApiError> {
    let user_agent = user_agent_from_headers(&headers);
    let has_permission = state
        .permission_checker
        .check_permission(
            &identity.realm_id(),
            &identity.user_id(),
            "settings",
            "manage",
        )
        .await
        .map_err(|e| {
            tracing::error!("Failed to check permission: {}", e);
            ApiError::internal("Failed to check permission")
        })?;

    if !has_permission {
        tracing::warn!(
            realm_id = %realm_id,
            user_id = %identity.user_id(),
            "Permission denied: settings.manage required for Passkey configuration"
        );
        return Err(ApiError::forbidden(
            "Insufficient permissions to update Passkey configuration. Requires 'settings.manage' permission.",
        ));
    }

    if !identity.has_access_to_realm(&realm_id) {
        return Err(ApiError::forbidden(
            "Access denied: cannot modify Passkey config in a different realm",
        ));
    }

    let service = state.service.realm_config_service();

    let passkey_config = serde_json::json!({
        "enabled": req.enabled,
        "force_enabled": req.force_enabled,
        "user_verification": req
            .user_verification
            .as_deref()
            .unwrap_or(DEFAULT_USER_VERIFICATION),
        "cross_platform_authenticator": req
            .cross_platform_authenticator
            .unwrap_or(DEFAULT_CROSS_PLATFORM_AUTHENTICATOR),
    });

    let config_request = UpsertRealmConfigRequest {
        config_type: ConfigType::Passkey,
        config_key: "settings".to_string(),
        config_value: passkey_config.to_string(),
        is_secret: Some(false),
        enabled: Some(true),
        metadata: None,
    };

    let config = service
        .upsert_config(identity.clone(), realm_id.clone(), config_request)
        .await
        .map_err(|e| {
            tracing::error!(
                "Failed to upsert Passkey config via RealmConfigService: {:?}",
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

    // Audit Passkey policy change (PRD §4.1 audit rule: "管理员变更 Passkey 策略").
    // Resolve the realm name for the target display; best-effort: a lookup
    // failure must not fail the (already-succeeded) config write.
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
            action: AuditAction::PasskeyConfigUpdate,
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
            user_agent,
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %audit_err, "Failed to record passkey config audit event");
    }

    Ok(ApiResult::ok(UpdateRealmPasskeyConfigResponse {
        message: "Realm Passkey configuration updated".to_string(),
        enabled: req.enabled,
        force_enabled: req.force_enabled,
        updated_at: config.updated_at.to_rfc3339(),
    }))
}

// ============================================================================
// Get Realm Passkey Configuration
// ============================================================================

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetRealmPasskeyConfigResponse {
    pub enabled: bool,
    pub force_enabled: bool,
    pub user_verification: String,
    pub cross_platform_authenticator: bool,
}

/// Get realm Passkey configuration
///
/// Retrieves the Passkey configuration for a realm. Requires 'settings.view' permission.
#[utoipa::path(
    get,
    path = "/api/realms/{realmId}/config/passkey",
    tag = "realms",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        (status = 200, description = "Realm Passkey configuration retrieved", body = GetRealmPasskeyConfigResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 404, description = "Realm not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn handle_get_realm_passkey_config(
    State(state): State<AppState>,
    Path(realm_id): Path<String>,
    Extension(identity): Extension<Identity>,
) -> Result<ApiResult<GetRealmPasskeyConfigResponse>, ApiError> {
    let has_permission = state
        .permission_checker
        .check_permission(
            &identity.realm_id(),
            &identity.user_id(),
            "settings",
            "view",
        )
        .await
        .map_err(|e| {
            tracing::error!("Failed to check permission: {}", e);
            ApiError::internal("Failed to check permission")
        })?;

    if !has_permission {
        tracing::warn!(
            realm_id = %realm_id,
            user_id = %identity.user_id(),
            "Permission denied: settings.view required for Passkey configuration"
        );
        return Err(ApiError::forbidden(
            "Insufficient permissions to view Passkey configuration. Requires 'settings.view' permission.",
        ));
    }

    if !identity.has_access_to_realm(&realm_id) {
        return Err(ApiError::forbidden(
            "Access denied: cannot view Passkey config from a different realm",
        ));
    }

    let service = state.service.realm_config_service();

    let config_entry = service
        .get_config(
            identity.clone(),
            realm_id.clone(),
            ConfigType::Passkey.as_ref().to_string(),
            "settings".to_string(),
        )
        .await
        .map_err(|e| {
            tracing::error!(
                "Failed to get Passkey config via RealmConfigService: {:?}",
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

    let config = config_entry.as_ref().and_then(|entry| {
        match serde_json::from_str::<serde_json::Value>(&entry.config_value) {
            Ok(config) => Some(config),
            Err(e) => {
                tracing::error!("Failed to parse Passkey config JSON: {}", e);
                None
            }
        }
    });

    let enabled = config
        .as_ref()
        .and_then(|config| config.get("enabled"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let force_enabled = config
        .as_ref()
        .and_then(|config| config.get("force_enabled"))
        .and_then(|value| value.as_bool())
        .unwrap_or(DEFAULT_FORCE_ENABLED);
    let user_verification = config
        .as_ref()
        .and_then(|config| config.get("user_verification"))
        .and_then(|value| value.as_str())
        .unwrap_or(DEFAULT_USER_VERIFICATION)
        .to_string();
    let cross_platform_authenticator = config
        .as_ref()
        .and_then(|config| config.get("cross_platform_authenticator"))
        .and_then(|value| value.as_bool())
        .unwrap_or(DEFAULT_CROSS_PLATFORM_AUTHENTICATOR);

    Ok(ApiResult::ok(GetRealmPasskeyConfigResponse {
        enabled,
        force_enabled,
        user_verification,
        cross_platform_authenticator,
    }))
}
