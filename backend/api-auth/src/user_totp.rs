use axum::extract::{Extension, Json, State};
use axum_valid::Valid;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::reauth::consume_reauth;
use herald_api_base::application::http::common::auth_utils::SelfIdentity;
use herald_api_base::application::http::common::auth_utils::require_token_scope;
pub use herald_api_base::application::http::server::api_entities::ErrorResponse;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::{
    CredentialScope, Identity, TargetOperation, TokenCredentialContext,
};
use herald_core::domain::user::ports::UserRepository;
use herald_core::domain::user_totp::{
    RealmTotpConfigRepository, UserTotpBackupCode, UserTotpConfig, UserTotpRepository,
    UserTotpService,
};
use herald_core::infrastructure::user::repositories::PostgresUserRepository;
use herald_core::infrastructure::user_totp::{
    PostgresRealmTotpConfigRepository, PostgresUserTotpRepository,
};

/// Router for user TOTP endpoints
pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/totp", axum::routing::post(handle_enable_totp))
        .route("/totp", axum::routing::delete(handle_disable_totp))
        .route(
            "/totp/verify",
            axum::routing::post(handle_verify_totp_setup),
        )
        .route(
            "/totp/regenerate",
            axum::routing::post(handle_regenerate_totp),
        )
        .route("/totp/status", axum::routing::get(handle_get_totp_status))
}

// ============================================================================
// Enable TOTP
// ============================================================================

#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct EnableTotpRequest {
    pub reauth_token: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnableTotpResponse {
    pub secret: String,
    pub qr_code_url: String,
    pub backup_codes: Vec<String>,
    pub temp_token: String,
}

/// Enable TOTP two-factor authentication for the current user
///
/// Initiates TOTP setup by generating a secret key, QR code URL, and backup codes.
/// The user must verify the TOTP code to complete the setup. Requires password verification.
#[utoipa::path(
    post,
    path = "/api/user/totp",
    tag = "user",
    request_body = EnableTotpRequest,
    responses(
        (status = 200, description = "TOTP setup initiated", body = EnableTotpResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "TOTP already enabled", body = ErrorResponse),
        (status = 404, description = "TOTP is not enabled for this realm", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn handle_enable_totp(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    Valid(Json(req)): Valid<Json<EnableTotpRequest>>,
) -> Result<ApiResult<EnableTotpResponse>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::TotpManage)?;
    let self_identity = SelfIdentity::require(identity.clone())?;
    let user_id = self_identity.user_id();

    // 1. Verify current password
    let user_repo = PostgresUserRepository::new(state.db.clone());
    let user = user_repo.get_user_by_id(user_id).await?;

    let realm_repo = PostgresRealmTotpConfigRepository::new(state.db.clone());
    let realm_config = realm_repo.get_realm_totp_config(&user.realm_id).await?;
    if !realm_config.map(|config| config.enabled).unwrap_or(false) {
        return Err(ApiError::not_found(
            "TOTP is not enabled for this realm".to_string(),
        ));
    }

    // 2. Check if user already has TOTP enabled
    let totp_repo = PostgresUserTotpRepository::new(state.db.clone());
    let existing_config = totp_repo.get_config_by_user_id(user_id).await?;

    // If config exists and is enabled, don't allow restart
    if let Some(ref config) = existing_config
        && config.enabled
    {
        return Err(ApiError::conflict("TOTP already enabled".to_string()));
    }

    // Consume the single-use reauth ticket only after all prerequisites have
    // been validated and just before the state-mutating operation.
    consume_reauth(
        &state,
        &identity,
        &context,
        &req.reauth_token,
        TargetOperation::BindAuthenticator,
    )
    .await?;

    // If config exists but is not enabled, delete and recreate
    if existing_config.is_some() {
        totp_repo.delete_config(user_id).await?;
        // Note: delete_config will cascade delete backup codes via ON DELETE CASCADE
    }

    // 3. Generate TOTP secret and backup codes
    let secret = UserTotpService::generate_secret();
    let backup_codes = UserTotpService::generate_backup_codes();

    // 4. Encrypt secret and hash backup codes
    let secret_hash = UserTotpService::encrypt_secret(&secret)?;
    let backup_codes_hashes: Result<Vec<String>, _> = backup_codes
        .iter()
        .map(|code| UserTotpService::hash_backup_code(code))
        .collect();
    let backup_codes_hashes = backup_codes_hashes?;

    // 5. Create TOTP config (disabled until verified)
    // Note: key_version is fixed at 1, reserved for future key rotation
    let totp_config = UserTotpConfig::new(user_id, user.realm_id.clone(), secret_hash, 1);
    let totp_config = totp_repo.create_config(totp_config).await?;

    // 6. Create backup codes
    let backup_code_entities: Vec<UserTotpBackupCode> = backup_codes_hashes
        .iter()
        .map(|hash| UserTotpBackupCode::new(totp_config.id, hash.clone()))
        .collect();
    totp_repo.create_backup_codes(backup_code_entities).await?;

    // 7. Generate temp token for verification step
    let temp_token = format!("totp_setup_{}", Uuid::now_v7());
    let temp_key = format!("totp:setup:temp:{}", temp_token);
    let temp_data = serde_json::json!({
        "user_id": user_id.to_string(),
        "totp_config_id": totp_config.id.to_string(),
    });

    let mut conn = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Internal server error".to_string()))?;

    let _: () = conn
        .set_ex(&temp_key, temp_data.to_string(), 300) // 5 minutes
        .await
        .map_err(|e| {
            tracing::error!("Failed to store temp token: {}", e);
            ApiError::internal("Internal server error".to_string())
        })?;

    // 8. Generate QR code URL
    let qr_code_url = UserTotpService::generate_qr_code_url(&secret, &user.email, &user.realm_id);

    Ok(ApiResult::ok(EnableTotpResponse {
        secret,
        qr_code_url,
        backup_codes,
        temp_token,
    }))
}

// ============================================================================
// Verify TOTP (Setup)
// ============================================================================

#[derive(Debug, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct VerifyTotpSetupRequest {
    pub temp_token: String,
    pub code: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VerifyTotpSetupResponse {
    pub message: String,
    pub enabled_at: String,
}

/// Verify TOTP setup and enable two-factor authentication
///
/// Completes the TOTP setup process by verifying the TOTP code generated from the secret.
/// On successful verification, TOTP is enabled for the user account.
#[utoipa::path(
    post,
    path = "/api/user/totp/verify",
    tag = "user",
    request_body = VerifyTotpSetupRequest,
    responses(
        (status = 200, description = "TOTP enabled successfully", body = VerifyTotpSetupResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Invalid or expired temp token or TOTP code", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn handle_verify_totp_setup(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    Valid(Json(req)): Valid<Json<VerifyTotpSetupRequest>>,
) -> Result<ApiResult<VerifyTotpSetupResponse>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::TotpManage)?;
    let self_identity = SelfIdentity::require(identity.clone())?;
    let user_id = self_identity.user_id();

    // 1. Retrieve and validate temp token
    let temp_key = format!("totp:setup:temp:{}", req.temp_token);
    let mut conn = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Internal server error".to_string()))?;

    let temp_data_json: Option<String> = conn.get(&temp_key).await.map_err(|e| {
        tracing::error!("Failed to get temp token: {}", e);
        ApiError::internal("Failed to get temp token".to_string())
    })?;

    let temp_data_json = temp_data_json.ok_or(ApiError::unauthorized(
        "Invalid or expired temporary token".to_string(),
    ))?;

    let temp_data: serde_json::Value = serde_json::from_str(&temp_data_json).map_err(|e| {
        tracing::error!("Failed to parse temp token JSON: {}", e);
        ApiError::internal("Failed to parse temp token JSON".to_string())
    })?;

    let temp_user_id = temp_data["user_id"]
        .as_str()
        .ok_or(ApiError::internal("Internal server error".to_string()))?;
    let totp_config_id_str = temp_data["totp_config_id"]
        .as_str()
        .ok_or(ApiError::internal("Internal server error".to_string()))?;

    // 2. Verify user_id matches
    if temp_user_id != user_id.to_string() {
        return Err(ApiError::unauthorized("User ID mismatch".to_string()));
    }

    // 3. Get TOTP config
    let totp_config_id = Uuid::parse_str(totp_config_id_str).map_err(|_| {
        tracing::error!("Invalid totp_config_id format: {}", totp_config_id_str);
        ApiError::internal("Invalid totp_config_id format".to_string())
    })?;

    let totp_repo = PostgresUserTotpRepository::new(state.db.clone());
    let mut totp_config = totp_repo.get_config_by_id(totp_config_id).await?;

    // 4. Verify TOTP code
    let secret = UserTotpService::decrypt_secret(&totp_config.secret_hash)?;
    let verified = UserTotpService::verify_totp(&secret, &req.code)?;

    if !verified {
        // Attempt cap: the temp token is a random UUID handed only to the
        // enrolling user, but within its TTL the 6-digit code must not be
        // guessable without limit. Atomic INCR on a companion key; on
        // exhaustion both keys are deleted so a fresh enrollment must restart.
        const TOTP_SETUP_MAX_ATTEMPTS: i64 = 5;
        let attempts_key = format!("totp:setup:attempts:{}", req.temp_token);
        let attempts: i64 = conn.incr(&attempts_key, 1).await.map_err(|e| {
            tracing::error!("Failed to increment TOTP setup attempt counter: {}", e);
            ApiError::internal("Failed to update attempt counter".to_string())
        })?;
        if attempts == 1 {
            let remaining_ttl: i64 = conn.ttl(&temp_key).await.map_err(|e| {
                tracing::error!("Failed to read TOTP setup token TTL: {}", e);
                ApiError::internal("Failed to read TTL".to_string())
            })?;
            if remaining_ttl > 0 {
                let _: () = conn
                    .expire(&attempts_key, remaining_ttl)
                    .await
                    .map_err(|e| {
                        tracing::error!("Failed to set attempt counter TTL: {}", e);
                        ApiError::internal("Failed to set TTL".to_string())
                    })?;
            }
        }
        if attempts >= TOTP_SETUP_MAX_ATTEMPTS {
            let _: () = conn.del(&temp_key).await.map_err(|e| {
                tracing::error!("Failed to delete exhausted TOTP setup token: {}", e);
                ApiError::internal("Failed to delete temp token".to_string())
            })?;
            let _: () = conn.del(&attempts_key).await.map_err(|e| {
                tracing::error!("Failed to delete exhausted attempt counter: {}", e);
                ApiError::internal("Failed to delete attempt counter".to_string())
            })?;
        }
        return Err(ApiError::unauthorized("Invalid TOTP code".to_string()));
    }

    // 5. Enable TOTP config
    totp_config.enable();
    let totp_config = totp_repo.update_config(totp_config).await?;

    // 6. Delete temp token + attempt counter
    let _: () = conn.del(&temp_key).await.map_err(|e| {
        tracing::error!("Failed to delete temp token: {}", e);
        ApiError::internal("Failed to delete temp token".to_string())
    })?;
    let _: () = conn
        .del(format!("totp:setup:attempts:{}", req.temp_token))
        .await
        .map_err(|e| {
            tracing::error!("Failed to delete TOTP setup attempt counter: {}", e);
            ApiError::internal("Failed to delete attempt counter".to_string())
        })?;

    Ok(ApiResult::ok(VerifyTotpSetupResponse {
        message: "TOTP enabled successfully".to_string(),
        enabled_at: totp_config
            .verified_at
            .ok_or_else(|| ApiError::internal("Verified at not found".to_string()))?
            .to_rfc3339(),
    }))
}

// ============================================================================
// Disable TOTP
// ============================================================================

#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct DisableTotpRequest {
    pub reauth_token: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DisableTotpResponse {
    pub message: String,
    pub disabled_at: String,
}

/// Disable TOTP two-factor authentication for the current user
///
/// Disables TOTP and removes the configuration. Requires password verification.
/// Cannot be disabled if the realm has force-enabled TOTP.
#[utoipa::path(
    delete,
    path = "/api/user/totp",
    tag = "user",
    request_body = DisableTotpRequest,
    responses(
        (status = 200, description = "TOTP disabled successfully", body = DisableTotpResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Invalid password", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn handle_disable_totp(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    Valid(Json(req)): Valid<Json<DisableTotpRequest>>,
) -> Result<ApiResult<DisableTotpResponse>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::TotpManage)?;
    let self_identity = SelfIdentity::require(identity.clone())?;
    let user_id = self_identity.user_id();

    // 1. Verify current password
    let user_repo = PostgresUserRepository::new(state.db.clone());
    let user = user_repo.get_user_by_id(user_id).await?;

    // 2. Check Realm TOTP force_enabled setting
    let realm_repo = PostgresRealmTotpConfigRepository::new(state.db.clone());
    let realm_config = realm_repo.get_realm_totp_config(&user.realm_id).await?;

    if let Some(config) = realm_config
        && config.force_enabled
    {
        return Err(ApiError::forbidden("TOTP is enforced by realm".to_string()));
    }

    // 3. Get config and delete it with associated backup codes
    let totp_repo = PostgresUserTotpRepository::new(state.db.clone());
    let config = totp_repo.get_config_by_user_id(user_id).await?;
    if config.is_none() {
        return Err(ApiError::bad_request("TOTP is not enabled".to_string()));
    }

    // Consume the single-use reauth ticket only after validating the user and
    // realm settings, and just before the state-mutating delete.
    consume_reauth(
        &state,
        &identity,
        &context,
        &req.reauth_token,
        TargetOperation::RemoveAuthenticator,
    )
    .await?;

    if let Some(_config) = config {
        totp_repo.delete_config(user_id).await?;
        // Note: delete_config will cascade delete backup codes via ON DELETE CASCADE
    }

    Ok(ApiResult::ok(DisableTotpResponse {
        message: "TOTP disabled successfully".to_string(),
        disabled_at: chrono::Utc::now().to_rfc3339(),
    }))
}

// ============================================================================
// Regenerate TOTP Secret
// ============================================================================

#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct RegenerateTotpRequest {
    pub reauth_token: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegenerateTotpResponse {
    pub secret: String,
    pub qr_code_url: String,
    pub backup_codes: Vec<String>,
    pub temp_token: String,
}

/// Regenerate TOTP secret and backup codes
///
/// Generates a new TOTP secret and backup codes, invalidating the old ones.
/// The user must verify the new TOTP code to complete the regeneration. Requires password verification.
#[utoipa::path(
    post,
    path = "/api/user/totp/regenerate",
    tag = "user",
    request_body = RegenerateTotpRequest,
    responses(
        (status = 200, description = "TOTP secret regenerated", body = RegenerateTotpResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Invalid password", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn handle_regenerate_totp(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    Valid(Json(req)): Valid<Json<RegenerateTotpRequest>>,
) -> Result<ApiResult<RegenerateTotpResponse>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::TotpManage)?;
    consume_reauth(
        &state,
        &identity,
        &context,
        &req.reauth_token,
        TargetOperation::BindAuthenticator,
    )
    .await?;
    let self_identity = SelfIdentity::require(identity.clone())?;
    let user_id = self_identity.user_id();

    // 1. Verify current password
    let user_repo = PostgresUserRepository::new(state.db.clone());
    let user = user_repo.get_user_by_id(user_id).await?;

    // 2. Get existing TOTP config
    let totp_repo = PostgresUserTotpRepository::new(state.db.clone());
    let existing_config = totp_repo
        .get_config_by_user_id(user_id)
        .await?
        .ok_or_else(|| ApiError::not_found("TOTP not configured".to_string()))?;

    // 3. Generate new TOTP secret and backup codes
    let secret = UserTotpService::generate_secret();
    let backup_codes = UserTotpService::generate_backup_codes();

    // 4. Encrypt new secret and hash backup codes
    let secret_hash = UserTotpService::encrypt_secret(&secret)?;
    let backup_codes_hashes: Result<Vec<String>, _> = backup_codes
        .iter()
        .map(|code| UserTotpService::hash_backup_code(code))
        .collect();
    let backup_codes_hashes = backup_codes_hashes?;

    // 5. Update config with new secret (disabled until verified)
    let mut updated_config = existing_config.clone();
    updated_config.regenerate_secret(secret_hash);
    let updated_config = totp_repo.update_config(updated_config).await?;

    // 6. Delete old backup codes and create new ones
    totp_repo.delete_backup_codes(updated_config.id).await?;
    let backup_code_entities: Vec<UserTotpBackupCode> = backup_codes_hashes
        .iter()
        .map(|hash| UserTotpBackupCode::new(updated_config.id, hash.clone()))
        .collect();
    totp_repo.create_backup_codes(backup_code_entities).await?;

    // 7. Generate temp token for verification step
    let temp_token = format!("totp_regen_{}", Uuid::now_v7());
    let temp_key = format!("totp:setup:temp:{}", temp_token);
    let temp_data = serde_json::json!({
        "user_id": user_id.to_string(),
        "totp_config_id": updated_config.id.to_string(),
    });

    let mut conn = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Internal server error".to_string()))?;

    let _: () = conn
        .set_ex(&temp_key, temp_data.to_string(), 300) // 5 minutes
        .await
        .map_err(|e| {
            tracing::error!("Failed to store temp token: {}", e);
            ApiError::internal("Internal server error".to_string())
        })?;

    // 8. Generate QR code URL
    let qr_code_url = UserTotpService::generate_qr_code_url(&secret, &user.email, &user.realm_id);

    Ok(ApiResult::ok(RegenerateTotpResponse {
        secret,
        qr_code_url,
        backup_codes,
        temp_token,
    }))
}

// ============================================================================
// Get TOTP Status
// ============================================================================

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TotpStatusResponse {
    pub enabled: bool,
    pub enabled_at: Option<String>,
    pub last_verified_at: Option<String>,
    pub backup_codes: BackupCodeStatsResponse,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct BackupCodeStatsResponse {
    pub total: i32,
    pub remaining: i32,
    pub used: i32,
}

/// Get TOTP configuration status for the current user
///
/// Returns the TOTP status including whether it is enabled, when it was enabled,
/// and backup code usage statistics.
#[utoipa::path(
    get,
    path = "/api/user/totp/status",
    tag = "user",
    responses(
        (status = 200, description = "TOTP status retrieved", body = TotpStatusResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn handle_get_totp_status(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
) -> Result<ApiResult<TotpStatusResponse>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::TotpManage)?;
    let self_identity = SelfIdentity::require(identity.clone())?;
    let user_id = self_identity.user_id();

    let totp_repo = PostgresUserTotpRepository::new(state.db.clone());
    let totp_config = totp_repo.get_config_by_user_id(user_id).await?;

    if totp_config.is_none() {
        return Ok(ApiResult::ok(TotpStatusResponse {
            enabled: false,
            enabled_at: None,
            last_verified_at: None,
            backup_codes: BackupCodeStatsResponse {
                total: 0,
                remaining: 0,
                used: 0,
            },
        }));
    }

    let config = totp_config.ok_or_else(|| {
        tracing::error!("totp_config should be Some after is_none() check");
        ApiError::internal("TOTP config not found".to_string())
    })?;
    let backup_stats = totp_repo.get_backup_code_stats(config.id).await?;

    Ok(ApiResult::ok(TotpStatusResponse {
        enabled: config.enabled,
        enabled_at: config.verified_at.map(|dt| dt.to_rfc3339()),
        last_verified_at: config.last_used_at.map(|dt| dt.to_rfc3339()),
        backup_codes: BackupCodeStatsResponse {
            total: backup_stats.total,
            remaining: backup_stats.remaining,
            used: backup_stats.used,
        },
    }))
}
