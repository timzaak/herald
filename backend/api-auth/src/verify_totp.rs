use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
    response::IntoResponse,
};
use axum_valid::Valid;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use herald_api_base::application::http::auth::util::{
    ClientIp, epoch_seconds, rate_limit_hit, user_agent_from_headers,
};
use herald_api_base::application::http::server::api_entities::ApiError;
pub use herald_api_base::application::http::server::api_entities::ErrorResponse;
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::BrowserTokenService;
use herald_core::domain::client::ports::ClientService;
use herald_core::domain::security_constants::{
    OAUTH_STATE_TTL_SECONDS, TOTP_LOCKOUT_SECONDS, TOTP_MAX_FAILURES, TOTP_VERIFY_IP_RATE_LIMIT,
    TOTP_VERIFY_USER_RATE_LIMIT,
};
use herald_core::domain::user::ports::UserRepository;
use herald_core::domain::user_totp::{
    TotpVerificationResultWithBackup, UserTotpRepository, UserTotpService,
};
use herald_core::infrastructure::authentication::RedisBrowserTokenService;
use herald_core::infrastructure::user_totp::PostgresUserTotpRepository;

use crate::browser_token::BrowserTokenResponse;
use crate::consent_gate::AuthConsentAgreement;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VerifyTotpRequest {
    pub temp_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub agreements: Option<Vec<AuthConsentAgreement>>,
}

impl Validate for VerifyTotpRequest {
    fn validate(&self) -> Result<(), validator::ValidationErrors> {
        let mut errors = validator::ValidationErrors::new();

        // Validate temp_token
        if self.temp_token.is_empty() {
            errors.add("temp_token", validator::ValidationError::new("required"));
        }

        // Validate that either code or backup_code is provided
        if self.code.is_none() && self.backup_code.is_none() {
            errors.add(
                "",
                validator::ValidationError::new("either_code_or_backup_code_required"),
            );
        }

        // Validate code format if provided
        if let Some(code) = &self.code
            && (code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()))
        {
            errors.add("code", validator::ValidationError::new("invalid_format"));
        }

        // Validate backup_code format if provided
        if let Some(backup_code) = &self.backup_code
            && (backup_code.len() != 6 || !backup_code.chars().all(|c| c.is_ascii_digit()))
        {
            errors.add(
                "backup_code",
                validator::ValidationError::new("invalid_format"),
            );
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VerifyTotpResponse {
    pub message: String,
    pub user_id: String,
    pub token: String,
    pub expires_in_seconds: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consent_required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agreements: Option<Vec<herald_core::domain::legal::LegalAgreementSummary>>,
}

/// Temporary session data for TOTP verification
#[derive(Serialize, Deserialize)]
struct TempSessionData {
    user_id: String,
    realm_id: String,
    client_id: String,
    client_app_id: Uuid,
    client_ip: String,
    flow: String,
    #[serde(default)]
    oauth_client_id: Option<String>,
    #[serde(default)]
    redirect_uri: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

/// Verify TOTP code for two-factor authentication
///
/// Completes the login process for users with TOTP enabled. Accepts either a TOTP code
/// from an authenticator app or a backup code. Creates a permanent session on success.
#[utoipa::path(
    post,
    path = "/api/auth/{realmId}/login/verify-totp",
    tag = "auth",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = VerifyTotpRequest,
    responses(
        (status = 200, description = "TOTP verification successful", body = BrowserTokenResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Invalid TOTP code or backup code", body = ErrorResponse),
        (status = 429, description = "Too many attempts", body = ErrorResponse),
    )
)]
pub async fn handle_verify_totp(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(client_ip): ClientIp,
    headers: HeaderMap,
    Valid(Json(req)): Valid<Json<VerifyTotpRequest>>,
) -> Result<impl IntoResponse, ApiError> {
    let user_agent = user_agent_from_headers(&headers);

    // 1. Validate and retrieve temp session from Redis
    let temp_key = format!("totp:temp:{}", req.temp_token);
    let mut conn = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Redis connection error".to_string()))?;

    let temp_session_json: Option<String> = conn.get(&temp_key).await.map_err(|e| {
        tracing::error!("Failed to get temp session from Redis: {}", e);
        ApiError::internal("Redis operation error".to_string())
    })?;

    let temp_session_json = temp_session_json.ok_or(ApiError::unauthorized(
        "Invalid or expired temporary token".to_string(),
    ))?;

    let temp_session: TempSessionData = serde_json::from_str(&temp_session_json).map_err(|e| {
        tracing::error!("Failed to parse temp session JSON: {}", e);
        ApiError::internal("Redis operation error".to_string())
    })?;

    if temp_session.realm_id != realm_id {
        return Err(ApiError::bad_request(
            "Path realm_id does not match temporary session realm".to_string(),
        ));
    }

    // Resolve the same Client App bound into the temporary login state.
    let client_app = state
        .service
        .client_service()
        .get_client_app_by_client_id(&temp_session.realm_id, &temp_session.client_id)
        .await
        .map_err(|_| ApiError::internal("Client app lookup failed".to_string()))?;
    if !client_app.enabled
        || client_app.id != temp_session.client_app_id
        || temp_session.flow != "custom_user_ui"
    {
        return Err(ApiError::unauthorized("Invalid temporary token"));
    }

    // 2. Check rate limits
    rate_limit_hit(
        &state,
        format!("totp:verify:user:{}", temp_session.user_id),
        TOTP_VERIFY_USER_RATE_LIMIT.0,
        TOTP_VERIFY_USER_RATE_LIMIT.1,
    )
    .await?;
    rate_limit_hit(
        &state,
        format!("totp:verify:ip:{}", client_ip),
        TOTP_VERIFY_IP_RATE_LIMIT.0,
        TOTP_VERIFY_IP_RATE_LIMIT.1,
    )
    .await?;

    // 3. Check failure count (Redis)
    let fail_count_key = format!("totp:fail_count:{}", temp_session.user_id);
    let fail_count: Option<i64> = conn.get(&fail_count_key).await.map_err(|e| {
        tracing::error!("Failed to get fail count from Redis: {}", e);
        ApiError::internal("Redis operation error".to_string())
    })?;

    if let Some(count) = fail_count
        && count >= TOTP_MAX_FAILURES
    {
        return Err(ApiError::too_many_requests(
            "Too many failed attempts. Please try again in 15 minutes.".to_string(),
        ));
    }

    // 4. Get user TOTP configuration
    let user_id = Uuid::parse_str(&temp_session.user_id).map_err(|_| {
        tracing::error!(
            "Invalid user_id format in temp session: {}",
            temp_session.user_id
        );
        ApiError::internal("Redis operation error".to_string())
    })?;

    let totp_repo = PostgresUserTotpRepository::new(state.db.clone());
    let totp_config =
        totp_repo
            .get_config_by_user_id(user_id)
            .await?
            .ok_or(ApiError::unauthorized(
                "TOTP not configured for this user".to_string(),
            ))?;

    if !totp_config.enabled {
        return Err(ApiError::unauthorized("TOTP is not enabled".to_string()));
    }

    // 5. Verify TOTP code or backup code using service layer
    let _using_totp_code = req.code.is_some();

    // For backup codes, we need to fetch them from the database
    let backup_codes = if req.backup_code.is_some() {
        totp_repo.get_backup_codes(totp_config.id).await?
    } else {
        Vec::new()
    };

    // Check for replay attacks: track last used code (only for TOTP codes)
    let last_code_data: Option<String> = if req.code.is_some() {
        let last_code_key = format!("totp:last_code:{}", temp_session.user_id);
        conn.get(&last_code_key).await.map_err(|e| {
            tracing::error!("Failed to get last TOTP code from Redis: {}", e);
            ApiError::internal("Redis operation error".to_string())
        })?
    } else {
        None
    };

    // Call service layer for verification
    let verification_result = UserTotpService::verify_totp_or_backup_code(
        &totp_config,
        req.code.clone(),
        req.backup_code.clone(),
        backup_codes,
        last_code_data.as_deref(),
    )?;

    // Handle verification result
    match verification_result {
        TotpVerificationResultWithBackup::Valid => {
            // Store this code as the last used code with current timestamp
            let last_code_key = format!("totp:last_code:{}", temp_session.user_id);
            let code_data = format!("{}:{}", req.code.unwrap(), epoch_seconds());
            let _: () = conn.set(&last_code_key, code_data).await.map_err(|e| {
                tracing::error!("Failed to store last TOTP code in Redis: {}", e);
                ApiError::internal("Redis operation error".to_string())
            })?;
            // Expire the tracking after 2 minutes (4 time steps)
            let _: () = conn.expire(&last_code_key, 120).await.map_err(|e| {
                tracing::error!("Failed to set expiry on last TOTP code: {}", e);
                ApiError::internal("Redis operation error".to_string())
            })?;
        }
        TotpVerificationResultWithBackup::BackupCodeUsed(code_id) => {
            // Mark backup code as used
            totp_repo.mark_backup_code_used(code_id).await?;
        }
        TotpVerificationResultWithBackup::Expired => {
            tracing::debug!(
                user_id = %temp_session.user_id,
                "TOTP code expired (outside valid time window)"
            );
            // Fall through to error handling
        }
        TotpVerificationResultWithBackup::Replay => {
            tracing::warn!(
                user_id = %temp_session.user_id,
                "TOTP code reuse detected (replay attack)"
            );
            // Fall through to error handling
        }
    }

    // Check if verification failed
    if !matches!(
        verification_result,
        TotpVerificationResultWithBackup::Valid
            | TotpVerificationResultWithBackup::BackupCodeUsed(_)
    ) {
        // Record failure
        let _: () = conn.incr(&fail_count_key, 1).await.map_err(|e| {
            tracing::error!("Failed to increment fail count: {}", e);
            ApiError::internal("Redis operation error".to_string())
        })?;
        let _: () = conn
            .expire(&fail_count_key, TOTP_LOCKOUT_SECONDS as i64)
            .await
            .map_err(|e| {
                tracing::error!("Failed to set expiry on fail count: {}", e);
                ApiError::internal("Redis operation error".to_string())
            })?;

        // Delete temp token on failure (security measure)
        let _: () = conn.del(&temp_key).await.map_err(|e| {
            tracing::error!("Failed to delete temp token: {}", e);
            ApiError::internal("Redis operation error".to_string())
        })?;

        // Return specific error message
        let error_message = "验证码已过期或无效".to_string();

        return Err(ApiError::unauthorized(error_message));
    }

    // 6. Delete temp token and failure count
    let _: () = conn.del(&temp_key).await.map_err(|e| {
        tracing::error!("Failed to delete temp token: {}", e);
        ApiError::internal("Redis operation error".to_string())
    })?;
    let _: () = conn.del(&fail_count_key).await.map_err(|e| {
        tracing::error!("Failed to delete fail count: {}", e);
        ApiError::internal("Redis operation error".to_string())
    })?;

    // 7. Update TOTP last used time
    let mut updated_config = totp_config.clone();
    updated_config.update_last_used();
    totp_repo.update_config(updated_config).await?;

    let user = state.user_repository.get_user_by_id(user_id).await?;
    if let Some(summaries) = crate::consent_gate::evaluate_login_consent_gate(
        &state,
        &user,
        &temp_session.realm_id,
        req.agreements.as_deref(),
        Some(client_ip.clone()),
        None,
    )
    .await
    {
        let response = Json(VerifyTotpResponse {
            message: "consent required".to_string(),
            user_id: temp_session.user_id,
            token: String::new(),
            expires_in_seconds: 0,
            redirect_to: None,
            consent_required: Some(true),
            agreements: Some(summaries),
        })
        .into_response();

        return Ok(response);
    }

    // 8. Check for OAuth context
    let has_oauth = temp_session.oauth_client_id.is_some()
        && temp_session.redirect_uri.is_some()
        && temp_session.state.is_some();

    if has_oauth {
        let oauth_client_id = temp_session
            .oauth_client_id
            .as_ref()
            .expect("checked above");
        let redirect_uri = temp_session.redirect_uri.as_ref().expect("checked above");
        let state_param = temp_session.state.as_ref().expect("checked above");

        // Validate Redis state (atomic GET+DELETE for one-time use)
        let state_key = format!("oauth:state:{}", state_param);
        let state_json: Option<String> = redis::cmd("GETDEL")
            .arg(&state_key)
            .query_async(&mut conn)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Redis GETDEL failed for OAuth state");
                ApiError::internal("Redis operation error".to_string())
            })?;

        let state_json = state_json.ok_or_else(|| {
            ApiError::bad_request(
                "OAuth state not found or already used. Please restart the authorization flow."
                    .to_string(),
            )
        })?;

        let state_data: serde_json::Value = serde_json::from_str(&state_json).map_err(|e| {
            tracing::error!(error = %e, "Failed to parse OAuth state JSON");
            ApiError::internal("Redis operation error".to_string())
        })?;

        // Validate that Redis state fields match the temp session
        let stored_client_id = state_data["client_id"].as_str().unwrap_or("");
        let stored_realm_id = state_data["realm_id"].as_str().unwrap_or("");
        let stored_redirect_uri = state_data["redirect_uri"].as_str().unwrap_or("");

        if stored_client_id != oauth_client_id {
            return Err(ApiError::bad_request(
                "OAuth state client_id mismatch".to_string(),
            ));
        }
        if stored_realm_id != temp_session.realm_id {
            return Err(ApiError::bad_request(
                "OAuth state realm_id mismatch".to_string(),
            ));
        }
        if stored_redirect_uri != redirect_uri {
            return Err(ApiError::bad_request(
                "OAuth state redirect_uri mismatch".to_string(),
            ));
        }

        // Generate authorization code
        let auth_code = format!("ac_{}", Uuid::now_v7());
        let code_challenge = state_data["code_challenge"]
            .as_str()
            .unwrap_or("")
            .to_string();

        // Store authorization code in Redis
        let code_key = format!("oauth:code:{}", auth_code);
        let code_value = serde_json::json!({
            "code_challenge": code_challenge,
            "client_id": oauth_client_id,
            "redirect_uri": redirect_uri,
            "user_id": temp_session.user_id,
            "realm_id": temp_session.realm_id,
        })
        .to_string();

        let _: () = conn
            .set_ex(&code_key, code_value, OAUTH_STATE_TTL_SECONDS)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to store OAuth authorization code");
                ApiError::internal("Redis operation error".to_string())
            })?;

        let redirect_to = format!("{}?code={}&state={}", redirect_uri, auth_code, state_param);

        tracing::debug!("OAuth authorization code generated via TOTP verification");

        let response = Json(VerifyTotpResponse {
            message: "ok".to_string(),
            user_id: temp_session.user_id,
            token: String::new(),
            expires_in_seconds: 0,
            redirect_to: Some(redirect_to),
            consent_required: None,
            agreements: None,
        })
        .into_response();

        return Ok(response);
    }

    let tokens = RedisBrowserTokenService::new(state.redis_manager.clone())
        .create_token_family(&user, &client_app, user_agent, Some(client_ip.clone()))
        .await?;
    Ok(Json(BrowserTokenResponse::from(tokens)).into_response())
}
