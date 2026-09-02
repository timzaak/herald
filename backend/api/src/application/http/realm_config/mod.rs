pub use herald_api_base::application::http::common::public_helper;

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
};
use herald_core::domain::audit::{
    AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType, NewAuditEvent,
};
use herald_core::domain::authentication::Identity;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::application::http::server::api_entities::{ApiError, ErrorResponse};
use crate::application::http::state::AppState;
use herald_api_base::application::http::auth::util::is_email_configured;
use herald_api_base::application::http::auth::util::{ClientIp, user_agent_from_headers};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::rate_limit::rate_limit_hit_forced;
use herald_core::domain::authorization::PermissionService;
use herald_core::domain::realm_config::{
    BatchUpsertRealmConfigRequest, ConfigType, RealmConfig, RealmConfigService,
    UpsertRealmConfigRequest,
};
use herald_core::third::email::EmailService;

fn parse_config_type(value: String) -> Result<ConfigType, ApiError> {
    ConfigType::try_from_str(&value).map_err(ApiError::bad_request)
}

/// Detect "leave-empty-to-preserve" requests for any payment provider's
/// sensitive config key.
///
/// When an admin edits a provider form without re-entering a secret, the
/// frontend submits the secret field as an empty string (design support-iap
/// §5.4 / §4.5 "buildProviderConfigRequest"). To avoid clobbering the stored
/// secret with empty, the upsert path must short-circuit and return the
/// existing stored row unchanged.
///
/// Returns the provider's config_type string name (e.g. `"stripe"`) when the
/// incoming write should be preserved, or `None` otherwise. The caller uses
/// the returned name to reload the matching existing config row.
///
/// Covers all four payment providers (stripe / creem / apple / google); the
/// sensitive key sets are per design support-iap §4.3.2.
fn is_empty_secret_to_preserve(
    config_type: &ConfigType,
    config_key: &str,
    config_value: &str,
) -> Option<&'static str> {
    if !config_value.trim().is_empty() {
        return None;
    }
    // Only the per-provider sensitive key sets qualify as "preserve on empty";
    // the provider name itself comes from `provider_string_for_config_type`
    // (single source of truth for the ConfigType → provider-string mapping).
    let is_preservable_secret = match config_type {
        ConfigType::Stripe => matches!(config_key, "api_key" | "webhook_secret"),
        ConfigType::Creem => config_key == "api_key",
        ConfigType::Apple => config_key == "private_key_p8",
        ConfigType::Google => config_key == "service_account_json",
        ConfigType::Wechat => matches!(config_key, "private_key" | "v3_key"),
        // LDAP service-account password: an empty submit preserves the
        // stored secret, same admin-edit UX as the payment providers.
        ConfigType::Ldap => config_key == "bind_password",
        _ => false,
    };
    if !is_preservable_secret {
        return None;
    }
    match config_type {
        // LDAP is not a payment provider; its own config_type string is the
        // reload key for preserving the stored secret row.
        ConfigType::Ldap => Some(config_type.as_static_str()),
        _ => provider_string_for_config_type(config_type),
    }
}

/// Map a payment-provider ConfigType to its `payment_provider` column value
/// for active-subscription protection queries. Returns `None` for non-payment
/// config types so the caller can skip the guard entirely.
fn provider_string_for_config_type(config_type: &ConfigType) -> Option<&'static str> {
    match config_type {
        ConfigType::Stripe
        | ConfigType::Creem
        | ConfigType::Apple
        | ConfigType::Google
        | ConfigType::Wechat => Some(config_type.as_static_str()),
        _ => None,
    }
}

/// Provider endpoint overrides exist for local wiremock-based tests only.
/// Accepting one from a tenant in production would turn payment operations
/// and reconciliation workers into an authenticated SSRF primitive.
fn reject_production_provider_base_url(
    app_env: &str,
    config_type: &ConfigType,
    config_key: &str,
) -> Result<(), ApiError> {
    if app_env == "production"
        && config_key == "base_url"
        && matches!(
            config_type,
            ConfigType::Stripe
                | ConfigType::Creem
                | ConfigType::Apple
                | ConfigType::Google
                | ConfigType::Wechat
        )
    {
        return Err(ApiError::bad_request(
            "Payment provider base_url overrides are disabled in production",
        ));
    }
    Ok(())
}

/// Server-side classification of credential-bearing config keys.
///
/// `is_secret` arrives from the client and is stored verbatim; GET responses
/// mask `config_value` only when it is true. If a caller omits (or clears)
/// the flag on a sensitive key, the stored credential would be echoed back in
/// plaintext, so the server must force the flag regardless of the payload.
/// Key sets mirror the frontend convention (see the `isSecret` field mappings
/// in frontend/src/lib/*-config-utils.ts).
fn is_sensitive_config_key(config_type: &ConfigType, config_key: &str) -> bool {
    match config_type {
        ConfigType::Stripe => matches!(config_key, "api_key" | "webhook_secret"),
        ConfigType::Creem => matches!(config_key, "api_key" | "webhook_secret"),
        ConfigType::Apple => config_key == "private_key_p8",
        ConfigType::Google => config_key == "service_account_json",
        ConfigType::Wechat => matches!(config_key, "private_key" | "v3_key"),
        ConfigType::Turnstile => config_key == "secret_key",
        ConfigType::Email => matches!(config_key, "resend_api_key" | "smtp_password"),
        // LDAP service-account password is always masked on read, even if a
        // caller clears the client-side is_secret flag.
        ConfigType::Ldap => config_key == "bind_password",
        // Realm-wide TOTP encryption key: every key of this type is a
        // credential (internally written as `version_1` with is_secret=true).
        // Force the flag so the generic config API can neither store it with
        // is_secret=false nor echo it back in plaintext on GET.
        ConfigType::TotpKey => true,
        _ => false,
    }
}

/// Validate an LDAP `settings` row before persisting (shape + credential
/// channel). Shared by the single upsert and
/// batch upsert paths so both enforce identical rules. Admin surface, so
/// field-specific 400 messages are intended.
fn validate_ldap_settings_row(
    config_type: &ConfigType,
    config_key: &str,
    config_value: &str,
) -> Result<(), ApiError> {
    if *config_type != ConfigType::Ldap || config_key != "settings" {
        return Ok(());
    }
    herald_core::domain::ldap::validate_ldap_settings_json(config_value)
        .map(|_| ())
        .map_err(|e| match e {
            herald_core::domain::common::entities::app_errors::CoreError::BadRequest(msg) => {
                ApiError::bad_request(msg)
            }
            _ => ApiError::bad_request("Invalid LDAP settings".to_string()),
        })
}

/// Block deletion of a payment provider's configuration while that provider
/// has active subscriptions in the realm (design support-iap §5.4).
///
/// Generalized from the Stripe-only `ensure_stripe_config_deletable` to cover
/// stripe / creem / apple / google: the guard filters active subscriptions by
/// the provider string matching the config_type being deleted.
async fn ensure_provider_config_deletable(
    state: &AppState,
    realm_id: &str,
    config_type: &ConfigType,
) -> Result<(), ApiError> {
    let Some(provider) = provider_string_for_config_type(config_type) else {
        // Non-payment config type: no active-subscription guard applies.
        return Ok(());
    };

    let active_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM subscription
         WHERE realm_id = $1
           AND payment_provider = $2
           AND status IN ('active', 'trialing', 'past_due', 'scheduled_cancel')",
    )
    .bind(realm_id)
    .bind(provider)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(
            realm_id = %realm_id,
            provider = %provider,
            error = %e,
            "Failed to count active subscriptions for provider"
        );
        ApiError::internal("Failed to validate provider configuration deletion")
    })?;

    if active_count > 0 {
        return Err(ApiError::bad_request(format!(
            "Cannot delete {provider} configuration while active {provider} subscriptions exist"
        )));
    }

    Ok(())
}

/// Best-effort audit write for payment-provider config changes (PRD
/// wechat-support §4.1: "所有 WeChat 配置变更与支付操作必须记录审计日志").
/// Audits every payment provider config type, not only WeChat, since all of
/// them share this single config write path. An audit failure must never fail
/// the already-succeeded config write.
async fn audit_payment_config_change(
    state: &AppState,
    identity: &Identity,
    realm_id: &str,
    provider: &str,
    config_key: &str,
    action: AuditAction,
    ip: String,
    user_agent: Option<String>,
) {
    if let Err(e) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: realm_id.to_string(),
            category: AuditCategory::Billing,
            action,
            actor_id: identity.user_id(),
            actor_type: None,
            actor_name: identity.as_user().map(|u| u.email.clone()),
            target_type: AuditTargetType::Realm,
            target_id: realm_id.to_string(),
            target_name: Some(format!("{provider}/{config_key}")),
            result: AuditResult::Success,
            details: Some(serde_json::json!({
                "provider": provider,
                "config_key": config_key,
            })),
            ip_address: Some(ip),
            user_agent,
            trace_id: None,
        })
        .await
    {
        tracing::warn!(
            error = %e,
            realm_id = %realm_id,
            provider = %provider,
            "Failed to record payment config audit event"
        );
    }
}

#[derive(Debug, Deserialize, Serialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct UpsertRealmConfigValidator {
    pub config_type: String,
    #[validate(length(min = 1))]
    pub config_key: String,
    #[validate(length(min = 1))]
    pub config_value: String,
    pub is_secret: Option<bool>,
    pub enabled: Option<bool>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct BatchUpsertRealmConfigValidator {
    #[validate(length(min = 1))]
    pub configs: Vec<UpsertRealmConfigValidator>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RealmConfigResponse {
    pub id: Uuid,
    pub realm_id: String,
    pub config_type: String,
    pub config_key: String,
    pub config_value: Option<String>,
    pub is_secret: bool,
    pub enabled: bool,
    pub metadata: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmailStatusResponse {
    pub configured: bool,
    pub provider: Option<String>,
    pub from_address: Option<String>,
    pub missing_fields: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct EmailTestRequest {
    #[validate(email)]
    pub recipient: String,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmailTestResponse {
    pub success: bool,
    pub message: String,
}

fn to_response(config: RealmConfig) -> RealmConfigResponse {
    // Mask on the server-side classification too, so rows written before the
    // write-path fix (sensitive key stored with is_secret=false) stay masked.
    let sensitive =
        config.is_secret || is_sensitive_config_key(&config.config_type, &config.config_key);
    RealmConfigResponse {
        id: config.id,
        realm_id: config.realm_id,
        config_type: config.config_type.into(),
        config_key: config.config_key,
        config_value: if sensitive {
            None
        } else {
            Some(config.config_value)
        },
        is_secret: config.is_secret,
        enabled: config.enabled,
        metadata: config.metadata,
        created_at: config.created_at.to_rfc3339(),
        updated_at: config.updated_at.to_rfc3339(),
    }
}

/// Get all configs for a given realm
#[utoipa::path(
    get,
    path = "/api/configs/{realmId}",
    tag = "realm_config",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        (status = 200, description = "List of all configs", body = Vec<RealmConfigResponse>),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn list_realm_configs(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
) -> Result<Json<Vec<RealmConfigResponse>>, ApiError> {
    let realm_config_service = state.service.realm_config_service();

    let identity_realm_id = identity.realm_id();
    let current_user_id = identity.user_id();

    tracing::debug!(
        realm_id = %identity_realm_id,
        user_id = %current_user_id,
        "Listing realm configs"
    );

    // Mirror the write handlers' in-handler gate so reads do not depend solely
    // on the service-layer policy wiring (which tests replace with AllowAll).
    AdminIdentity::require(identity.clone(), &realm_id, "realm configs")?
        .require_permission(&state, "settings", "view")
        .await?;

    let configs = realm_config_service
        .get_all_configs(identity, realm_id)
        .await
        .map_err(|e| match e {
            herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                ApiError::forbidden(msg)
            }
            _ => {
                tracing::error!("Failed to list realm configs: {e}");
                ApiError::internal("Failed to list realm configs")
            }
        })?;

    let responses = configs.into_iter().map(to_response).collect();
    Ok(Json(responses))
}

/// Get all configs of a given type
#[utoipa::path(
    get,
    path = "/api/configs/{realmId}/{configType}",
    tag = "realm_config",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("configType" = String, Path, description = "Config type (oauth, turnstile, registration, …)")
    ),
    responses(
        (status = 200, description = "Configs of the given type", body = Vec<RealmConfigResponse>),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn list_realm_configs_by_type(
    Path((realm_id, config_type)): Path<(String, String)>,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
) -> Result<Json<Vec<RealmConfigResponse>>, ApiError> {
    let realm_config_service = state.service.realm_config_service();

    let identity_realm_id = identity.realm_id();
    let current_user_id = identity.user_id();

    tracing::debug!(
        realm_id = %identity_realm_id,
        user_id = %current_user_id,
        "Listing realm configs by type"
    );

    AdminIdentity::require(identity.clone(), &realm_id, "realm configs")?
        .require_permission(&state, "settings", "view")
        .await?;

    let configs = realm_config_service
        .get_configs_by_type(identity, realm_id, config_type)
        .await
        .map_err(|e| match e {
            herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                ApiError::forbidden(msg)
            }
            _ => {
                tracing::error!("Failed to list realm configs by type: {e}");
                ApiError::internal("Failed to list realm configs by type")
            }
        })?;

    let responses = configs.into_iter().map(to_response).collect();
    Ok(Json(responses))
}

/// Get a single config
#[utoipa::path(
    get,
    path = "/api/configs/{realmId}/{configType}/{configKey}",
    tag = "realm_config",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("configType" = String, Path, description = "Config type"),
        ("configKey" = String, Path, description = "Config key")
    ),
    responses(
        (status = 200, description = "Config details", body = RealmConfigResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 404, description = "Not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_realm_config(
    Path((realm_id, config_type, config_key)): Path<(String, String, String)>,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
) -> Result<Json<RealmConfigResponse>, ApiError> {
    let realm_config_service = state.service.realm_config_service();

    let identity_realm_id = identity.realm_id();
    let current_user_id = identity.user_id();

    tracing::debug!(
        realm_id = %identity_realm_id,
        user_id = %current_user_id,
        "Getting realm config"
    );

    AdminIdentity::require(identity.clone(), &realm_id, "realm configs")?
        .require_permission(&state, "settings", "view")
        .await?;

    let config = realm_config_service
        .get_config(identity, realm_id, config_type, config_key)
        .await
        .map_err(|e| match e {
            herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                ApiError::forbidden(msg)
            }
            herald_core::domain::common::entities::app_errors::CoreError::NotFound => {
                ApiError::not_found("Config not found")
            }
            _ => {
                tracing::error!("Failed to get realm config: {e}");
                ApiError::internal("Failed to get realm config")
            }
        })?
        .ok_or_else(|| ApiError::not_found("Config not found"))?;

    Ok(Json(to_response(config)))
}

/// Create or update a config (upsert)
#[utoipa::path(
    put,
    path = "/api/configs/{realmId}",
    tag = "realm_config",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = UpsertRealmConfigValidator,
    responses(
        (status = 200, description = "Config created or updated", body = RealmConfigResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn upsert_realm_config(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(payload): Json<UpsertRealmConfigValidator>,
) -> Result<Json<RealmConfigResponse>, ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::bad_request(format!("验证错误: {}", e)))?;

    let realm_config_service = state.service.realm_config_service();

    let identity_realm_id = identity.realm_id();
    let current_user_id = identity.user_id();

    tracing::debug!(
        realm_id = %identity_realm_id,
        user_id = %current_user_id,
        "Upserting realm config"
    );

    // Authorization must run before the email-configured guard below: the
    // guard's 400 message discloses whether the realm has email configured,
    // which must not reach a caller from another realm (error-shape oracle).
    AdminIdentity::require(identity.clone(), &realm_id, "realm configs")?
        .require_permission(&state, "settings", "manage")
        .await?;

    let config_type = parse_config_type(payload.config_type)?;
    reject_production_provider_base_url(&state.app_env, &config_type, &payload.config_key)?;
    if let Some(provider_type) =
        is_empty_secret_to_preserve(&config_type, &payload.config_key, &payload.config_value)
    {
        let existing = realm_config_service
            .get_config(
                identity,
                realm_id,
                provider_type.to_string(),
                payload.config_key,
            )
            .await
            .map_err(|e| match e {
                herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                    ApiError::forbidden(msg)
                }
                herald_core::domain::common::entities::app_errors::CoreError::NotFound => {
                    ApiError::bad_request("Secret value is required")
                }
                _ => {
                    tracing::error!("Failed to load existing provider secret: {e}");
                    ApiError::internal("Failed to load existing provider secret")
                }
            })?
            .ok_or_else(|| ApiError::bad_request("Secret value is required"))?;
        return Ok(Json(to_response(existing)));
    }

    let is_secret = payload.is_secret.unwrap_or(false)
        || is_sensitive_config_key(&config_type, &payload.config_key);
    let request = UpsertRealmConfigRequest {
        config_type,
        config_key: payload.config_key,
        config_value: payload.config_value,
        is_secret: Some(is_secret),
        enabled: payload.enabled,
        metadata: payload.metadata,
    };

    // Validate: cannot enable email verification without email config
    if request.config_type == ConfigType::Registration
        && request.config_key == "require_email_verification"
        && request.config_value == "true"
    {
        let email_ready = is_email_configured(&state, &realm_id).await?;
        if !email_ready {
            return Err(ApiError::bad_request(
                "Cannot enable email verification without email configuration".to_string(),
            ));
        }
    }

    // LDAP settings row: shape/encryption-channel validation before persisting
    // (admin surface, so field-specific 400 messages are intended).
    validate_ldap_settings_row(
        &request.config_type,
        &request.config_key,
        &request.config_value,
    )?;

    let config = realm_config_service
        .upsert_config(identity.clone(), realm_id.clone(), request)
        .await
        .map_err(|e| match e {
            herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                ApiError::forbidden(msg)
            }
            _ => {
                tracing::error!("Failed to upsert realm config: {e}");
                ApiError::internal("Failed to upsert realm config")
            }
        })?;

    // Payment-provider credential/config writes are security-relevant and
    // audit-logged (PRD wechat-support §4.1); other config types are not.
    if let Some(provider) = provider_string_for_config_type(&config.config_type) {
        let user_agent = user_agent_from_headers(&headers);
        audit_payment_config_change(
            &state,
            &identity,
            &realm_id,
            provider,
            &config.config_key,
            AuditAction::PaymentConfigUpdate,
            ip,
            user_agent,
        )
        .await;
    }

    Ok(Json(to_response(config)))
}

/// Batch create or update configs
#[utoipa::path(
    post,
    path = "/api/configs/{realmId}/batch",
    tag = "realm_config",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = BatchUpsertRealmConfigRequest,
    responses(
        (status = 200, description = "Batch configs created or updated", body = Vec<RealmConfigResponse>),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn batch_upsert_realm_configs(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(payload): Json<BatchUpsertRealmConfigValidator>,
) -> Result<Json<Vec<RealmConfigResponse>>, ApiError> {
    tracing::debug!(
        realm_id = %realm_id,
        config_count = payload.configs.len(),
        "Received batch upsert request"
    );
    payload
        .validate()
        .map_err(|e| ApiError::bad_request(format!("验证错误: {}", e)))?;

    let realm_config_service = state.service.realm_config_service();

    let identity_realm_id = identity.realm_id();
    let current_user_id = identity.user_id();

    tracing::debug!(
        realm_id = %identity_realm_id,
        user_id = %current_user_id,
        "Batch upserting realm configs"
    );

    // Authorization must run before the email-configured guard below: the
    // guard's 400 message discloses whether the realm has email configured,
    // which must not reach a caller from another realm (error-shape oracle).
    AdminIdentity::require(identity.clone(), &realm_id, "realm configs")?
        .require_permission(&state, "settings", "manage")
        .await?;

    let mut skipped_existing = Vec::new();
    let mut requests: Vec<UpsertRealmConfigRequest> = Vec::new();
    for r in payload.configs {
        let config_type = parse_config_type(r.config_type)?;
        reject_production_provider_base_url(&state.app_env, &config_type, &r.config_key)?;
        if let Some(provider_type) =
            is_empty_secret_to_preserve(&config_type, &r.config_key, &r.config_value)
        {
            let existing = realm_config_service
                .get_config(
                    identity.clone(),
                    realm_id.clone(),
                    provider_type.to_string(),
                    r.config_key,
                )
                .await
                .map_err(|e| match e {
                    herald_core::domain::common::entities::app_errors::CoreError::Forbidden(
                        msg,
                    ) => ApiError::forbidden(msg),
                    herald_core::domain::common::entities::app_errors::CoreError::NotFound => {
                        ApiError::bad_request("Secret value is required")
                    }
                    _ => {
                        tracing::error!("Failed to load existing provider secret: {e}");
                        ApiError::internal("Failed to load existing provider secret")
                    }
                })?
                .ok_or_else(|| ApiError::bad_request("Secret value is required"))?;
            skipped_existing.push(existing);
            continue;
        }

        let is_secret =
            r.is_secret.unwrap_or(false) || is_sensitive_config_key(&config_type, &r.config_key);
        requests.push(UpsertRealmConfigRequest {
            config_type,
            config_key: r.config_key,
            config_value: r.config_value,
            is_secret: Some(is_secret),
            enabled: r.enabled,
            metadata: r.metadata,
        });
    }

    // Validate: cannot enable email verification without email config
    if requests.iter().any(|r| {
        r.config_type == ConfigType::Registration
            && r.config_key == "require_email_verification"
            && r.config_value == "true"
    }) {
        let email_ready = is_email_configured(&state, &realm_id).await?;
        if !email_ready {
            return Err(ApiError::bad_request(
                "Cannot enable email verification without email configuration".to_string(),
            ));
        }
    }

    // LDAP settings row: shape/encryption-channel validation before persisting
    // (same rules as the single upsert path).
    for r in &requests {
        validate_ldap_settings_row(&r.config_type, &r.config_key, &r.config_value)?;
    }

    for (index, req) in requests.iter().enumerate() {
        tracing::debug!(
            index = index,
            realm_id = %realm_id,
            config_type = format!("{:?}", req.config_type),
            config_key = %req.config_key,
            enabled = req.enabled,
            is_secret = req.is_secret,
            "Batch upsert request item"
        );
    }

    // Collect payment-provider rows before `requests` is moved into the domain
    // call, so the post-write audit knows what was persisted.
    let payment_rows: Vec<(&'static str, String)> = requests
        .iter()
        .filter_map(|r| {
            provider_string_for_config_type(&r.config_type)
                .map(|provider| (provider, r.config_key.clone()))
        })
        .collect();

    let mut configs = if requests.is_empty() {
        Vec::new()
    } else {
        realm_config_service
            .batch_upsert_configs(
                identity.clone(),
                realm_id.clone(),
                herald_core::domain::realm_config::BatchUpsertRealmConfigRequest {
                    configs: requests,
                },
            )
            .await
            .map_err(|e| match e {
                herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                    ApiError::forbidden(msg)
                }
                _ => {
                    tracing::error!(realm_id = %realm_id, error = %e, "Failed to batch upsert realm configs");
                    ApiError::internal("Failed to batch upsert realm configs")
                }
            })?
    };
    configs.extend(skipped_existing);

    // Payment-provider credential/config writes are security-relevant and
    // audit-logged (PRD wechat-support §4.1); other config types are not.
    if !payment_rows.is_empty() {
        let user_agent = user_agent_from_headers(&headers);
        for (provider, config_key) in payment_rows {
            audit_payment_config_change(
                &state,
                &identity,
                &realm_id,
                provider,
                &config_key,
                AuditAction::PaymentConfigUpdate,
                ip.clone(),
                user_agent.clone(),
            )
            .await;
        }
    }

    tracing::debug!(
        realm_id = %realm_id,
        config_count = configs.len(),
        "Batch upsert completed successfully"
    );

    // Invalidate realm cache after config update (fire-and-forget to avoid blocking response)
    let realm_id_clone = realm_id.clone();
    let permission_checker = state.permission_checker.clone();
    tokio::spawn(async move {
        let start = std::time::Instant::now();
        match permission_checker
            .invalidate_realm_cache(&realm_id_clone)
            .await
        {
            Ok(_) => {
                tracing::debug!(
                    realm_id = %realm_id_clone,
                    duration_ms = start.elapsed().as_millis(),
                    "Realm cache invalidated successfully"
                );
            }
            Err(e) => {
                tracing::warn!(
                    realm_id = %realm_id_clone,
                    error = %e,
                    "Failed to invalidate realm cache (non-critical)"
                );
            }
        }
    });

    let responses = configs.into_iter().map(to_response).collect();
    Ok(Json(responses))
}

/// Delete a config
#[utoipa::path(
    delete,
    path = "/api/configs/{realmId}/{configType}/{configKey}",
    tag = "realm_config",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("configType" = String, Path, description = "Config type"),
        ("configKey" = String, Path, description = "Config key")
    ),
    responses(
        (status = 204, description = "Config deleted"),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 404, description = "Not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn delete_realm_config(
    Path((realm_id, config_type, config_key)): Path<(String, String, String)>,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let realm_config_service = state.service.realm_config_service();

    let identity_realm_id = identity.realm_id();
    let current_user_id = identity.user_id();

    tracing::debug!(
        realm_id = %identity_realm_id,
        user_id = %current_user_id,
        "Deleting realm config"
    );

    // Authorization must run before the provider-deletion guard below: the
    // guard's 400 message discloses per-provider subscription state, which
    // must not reach a caller from another realm (error-shape oracle).
    AdminIdentity::require(identity.clone(), &realm_id, "realm configs")?
        .require_permission(&state, "settings", "manage")
        .await?;

    if let Ok(parsed) = parse_config_type(config_type.clone()) {
        ensure_provider_config_deletable(&state, &realm_id, &parsed).await?;
    }

    // Capture the payment-provider identity of the row before it is consumed
    // by the delete call, so the post-write audit knows what was removed.
    let deleted_payment_row = parse_config_type(config_type.clone())
        .ok()
        .and_then(|parsed| {
            provider_string_for_config_type(&parsed).map(|provider| (provider, config_key.clone()))
        });

    realm_config_service
        .delete_config(identity.clone(), realm_id.clone(), config_type, config_key)
        .await
        .map_err(|e| match e {
            herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                ApiError::forbidden(msg)
            }
            herald_core::domain::common::entities::app_errors::CoreError::NotFound => {
                ApiError::not_found("Config not found")
            }
            _ => {
                tracing::error!("Failed to delete realm config: {e}");
                ApiError::internal("Failed to delete realm config")
            }
        })?;

    // Payment-provider credential/config deletions are security-relevant and
    // audit-logged (PRD wechat-support §4.1); other config types are not.
    if let Some((provider, config_key)) = deleted_payment_row {
        let user_agent = user_agent_from_headers(&headers);
        audit_payment_config_change(
            &state,
            &identity,
            &realm_id,
            provider,
            &config_key,
            AuditAction::PaymentConfigDelete,
            ip,
            user_agent,
        )
        .await;
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Get email configuration status for a realm
#[utoipa::path(
    get,
    path = "/api/configs/{realmId}/email/status",
    tag = "realm_config",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        (status = 200, description = "Email configuration status", body = EmailStatusResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Server error", body = ErrorResponse)
    )
)]
pub async fn email_status(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
) -> Result<Json<EmailStatusResponse>, ApiError> {
    AdminIdentity::require(identity, &realm_id, "email status")?
        .require_permission(&state, "settings", "view")
        .await?;

    let status = EmailService::is_email_configured(&state.pool, &realm_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to check email config: {e}");
            ApiError::internal("Failed to check email config")
        })?;

    Ok(Json(EmailStatusResponse {
        configured: status.configured,
        provider: status.provider,
        from_address: status.from_address,
        missing_fields: status.missing_fields,
    }))
}

/// Send a test email for a realm
#[utoipa::path(
    post,
    path = "/api/configs/{realmId}/email/test",
    tag = "realm_config",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = EmailTestRequest,
    responses(
        (status = 200, description = "Test email result", body = EmailTestResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 429, description = "Too many requests", body = ErrorResponse),
        (status = 500, description = "Server error", body = ErrorResponse)
    )
)]
pub async fn email_test(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Json(payload): Json<EmailTestRequest>,
) -> Result<Json<EmailTestResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "email test")?;
    admin
        .require_permission(&state, "settings", "manage")
        .await?;

    rate_limit_hit_forced(
        &state,
        format!("rl:email:test:{realm_id}:{}", admin.user_id_string()),
        3,
        60,
    )
    .await?;

    payload
        .validate()
        .map_err(|e| ApiError::bad_request(format!("Invalid recipient: {e}")))?;

    let status = EmailService::is_email_configured(&state.pool, &realm_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to check email config: {e}");
            ApiError::internal("Failed to check email config")
        })?;

    if !status.configured {
        return Err(ApiError::bad_request(
            "Email is not configured for this realm".to_string(),
        ));
    }

    match EmailService::send_email(
        &state.pool,
        &realm_id,
        &payload.recipient,
        "Test Email from Herald",
        "Test Email\n\nThis is a test email from your Herald instance.",
        "<h1>Test Email</h1><p>This is a test email from your Herald instance.</p>",
    )
    .await
    {
        Ok(()) => Ok(Json(EmailTestResponse {
            success: true,
            message: "Test email sent successfully".to_string(),
        })),
        Err(e) => {
            // Full error (may embed SMTP host/auth details) goes to logs only;
            // the response stays generic like every other internal error path.
            tracing::error!("Failed to send test email: {e}");
            Ok(Json(EmailTestResponse {
                success: false,
                message: "Failed to send test email; check server logs for details".to_string(),
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(config_type: ConfigType, config_key: &str, is_secret: bool) -> RealmConfig {
        RealmConfig {
            id: Uuid::now_v7(),
            realm_id: "realm-1".to_string(),
            config_type: config_type.clone(),
            config_key: config_key.to_string(),
            config_value: "top-secret-value".to_string(),
            is_secret,
            enabled: true,
            metadata: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    /// WHY: the realm-wide TOTP encryption key is the single most sensitive
    /// config row. Before the fix, a `settings.manage` admin could upsert
    /// `totp_key/version_1` with `isSecret:false` and read the key back in
    /// plaintext — the server-side forced-sensitive list is what guarantees a
    /// mislabeled (or maliciously relabeled) row still masks on read.
    #[test]
    fn totp_key_is_always_masked_even_when_stored_unflagged() {
        assert!(is_sensitive_config_key(&ConfigType::TotpKey, "version_1"));
        assert!(is_sensitive_config_key(
            &ConfigType::TotpKey,
            "anything-else"
        ));

        let response = to_response(config(ConfigType::TotpKey, "version_1", false));
        assert!(
            response.config_value.is_none(),
            "TOTP key stored with is_secret=false must still be masked on read"
        );
        assert_eq!(response.config_key, "version_1");
    }

    #[test]
    fn non_sensitive_config_value_is_returned_verbatim() {
        let response = to_response(config(ConfigType::Registration, "enabled", false));
        assert_eq!(response.config_value.as_deref(), Some("top-secret-value"));
    }

    #[test]
    fn provider_base_url_override_is_test_only() {
        // WHY: this value is consumed by API and worker HTTP clients. A
        // tenant-controlled production value would permit server-side requests
        // to private infrastructure.
        for provider in [
            ConfigType::Stripe,
            ConfigType::Creem,
            ConfigType::Apple,
            ConfigType::Google,
            ConfigType::Wechat,
        ] {
            assert!(
                reject_production_provider_base_url("production", &provider, "base_url").is_err()
            );
            assert!(reject_production_provider_base_url("test", &provider, "base_url").is_ok());
        }
        assert!(
            reject_production_provider_base_url("production", &ConfigType::Stripe, "api_key")
                .is_ok()
        );
    }
}
