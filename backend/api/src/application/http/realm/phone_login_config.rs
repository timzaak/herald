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
    ClientIp, PhoneLoginSettings, is_sms_configured, is_supported_country_code,
    user_agent_from_headers,
};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_core::domain::audit::{
    AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType, NewAuditEvent,
};
use herald_core::domain::authentication::Identity;
use herald_core::domain::realm_config::{ConfigType, RealmConfigService, UpsertRealmConfigRequest};

pub use crate::application::http::server::api_entities::ErrorResponse;

fn map_realm_config_error(
    e: herald_core::domain::common::entities::app_errors::CoreError,
) -> ApiError {
    match e {
        herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
            ApiError::forbidden(msg)
        }
        herald_core::domain::common::entities::app_errors::CoreError::NotFound => {
            ApiError::not_found("Realm not found")
        }
        _ => ApiError::internal("Internal server error"),
    }
}

#[derive(Debug, Deserialize, Serialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct PhoneLoginConfigRequest {
    pub enabled: bool,
    /// ISO 3166-1 alpha-2, uppercase; malformed values are a 400.
    #[validate(length(min = 2, max = 2))]
    pub default_country_code: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PhoneLoginConfigResponse {
    pub enabled: bool,
    pub default_country_code: String,
    /// False while the SMS channel is not fully configured (enable would be rejected).
    pub sms_configured: bool,
}

#[utoipa::path(
    put,
    path = "/api/realms/{realmId}/config/phone-login",
    operation_id = "update_realm_phone_login_config",
    tag = "realms",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = PhoneLoginConfigRequest,
    responses(
        (status = 200, description = "Phone-login configuration updated (echoes the persisted state)", body = PhoneLoginConfigResponse),
        (status = 400, description = "Enable without SMS configuration / invalid country code", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions (settings.manage)", body = ErrorResponse),
        (status = 404, description = "Realm not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn handle_update_realm_phone_login_config(
    State(state): State<AppState>,
    Path(realm_id): Path<String>,
    Extension(identity): Extension<Identity>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Valid(Json(req)): Valid<Json<PhoneLoginConfigRequest>>,
) -> Result<ApiResult<PhoneLoginConfigResponse>, ApiError> {
    let admin = AdminIdentity::require_in_realm(
        identity.clone(),
        &realm_id,
        "realm phone-login configuration",
    )?;
    admin
        .require_permission(&state, "settings", "manage")
        .await?;

    // Must resolve in phone metadata: it feeds every normalize_phone call in the realm.
    if !is_supported_country_code(&req.default_country_code) {
        return Err(ApiError::bad_request(
            "defaultCountryCode must be a supported ISO 3166-1 alpha-2 code (uppercase)",
        ));
    }

    let service = state.service.realm_config_service();

    let sms_configured = is_sms_configured(&state, &realm_id).await?;

    // Fail-closed: enabling without a configured SMS channel would dead-end
    // every login at "send code".
    if req.enabled && !sms_configured {
        return Err(ApiError::bad_request(
            "Cannot enable phone login without SMS configuration".to_string(),
        ));
    }

    let previous = service
        .get_config(
            admin.identity().clone(),
            realm_id.clone(),
            ConfigType::PhoneLogin.as_ref().to_string(),
            "settings".to_string(),
        )
        .await
        .map_err(map_realm_config_error)?;
    let old_enabled = previous
        .as_ref()
        .and_then(|entry| serde_json::from_str::<PhoneLoginSettings>(&entry.config_value).ok())
        .map(|settings| settings.enabled);

    let config_value = serde_json::json!({
        "enabled": req.enabled,
        "default_country_code": req.default_country_code,
    });
    let config_request = UpsertRealmConfigRequest {
        config_type: ConfigType::PhoneLogin,
        config_key: "settings".to_string(),
        config_value: config_value.to_string(),
        is_secret: Some(false),
        enabled: Some(true),
        metadata: None,
    };

    service
        .upsert_config(admin.identity().clone(), realm_id.clone(), config_request)
        .await
        .map_err(|e| {
            tracing::error!("Failed to upsert phone-login config: {:?}", e);
            map_realm_config_error(e)
        })?;

    if let Err(audit_err) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: realm_id.clone(),
            category: AuditCategory::Auth,
            action: AuditAction::PhoneLoginConfigUpdate,
            actor_id: identity.user_id(),
            actor_type: None,
            actor_name: identity.as_user().map(|u| u.email.clone()),
            target_type: AuditTargetType::Realm,
            target_id: realm_id.clone(),
            target_name: None,
            result: AuditResult::Success,
            details: Some(serde_json::json!({
                "old_enabled": old_enabled,
                "new_enabled": req.enabled,
                "default_country_code": req.default_country_code,
            })),
            ip_address: Some(ip),
            user_agent: user_agent_from_headers(&headers),
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %audit_err, "Failed to record phone-login config audit event");
    }

    Ok(ApiResult::ok(PhoneLoginConfigResponse {
        enabled: req.enabled,
        default_country_code: req.default_country_code,
        sms_configured,
    }))
}

#[utoipa::path(
    get,
    path = "/api/realms/{realmId}/config/phone-login",
    operation_id = "get_realm_phone_login_config",
    tag = "realms",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        (status = 200, description = "Phone-login configuration retrieved", body = PhoneLoginConfigResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions (settings.view)", body = ErrorResponse),
        (status = 404, description = "Realm not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn handle_get_realm_phone_login_config(
    State(state): State<AppState>,
    Path(realm_id): Path<String>,
    Extension(identity): Extension<Identity>,
) -> Result<ApiResult<PhoneLoginConfigResponse>, ApiError> {
    let admin =
        AdminIdentity::require_in_realm(identity, &realm_id, "realm phone-login configuration")?;
    admin.require_permission(&state, "settings", "view").await?;

    let service = state.service.realm_config_service();

    let config_entry = service
        .get_config(
            admin.identity().clone(),
            realm_id.clone(),
            ConfigType::PhoneLogin.as_ref().to_string(),
            "settings".to_string(),
        )
        .await
        .map_err(map_realm_config_error)?;

    // Parse via the shared `PhoneLoginSettings` so admin view and runtime
    // gates agree; malformed rows read as disabled.
    let settings = match config_entry {
        Some(entry) => serde_json::from_str::<PhoneLoginSettings>(&entry.config_value)
            .unwrap_or_else(|e| {
                tracing::error!("Failed to parse phone-login config JSON: {}", e);
                PhoneLoginSettings::disabled_default()
            }),
        None => PhoneLoginSettings::disabled_default(),
    };

    let sms_configured = is_sms_configured(&state, &realm_id).await?;

    Ok(ApiResult::ok(PhoneLoginConfigResponse {
        enabled: settings.enabled,
        default_country_code: settings.default_country_code,
        sms_configured,
    }))
}
