use axum::{
    Json,
    extract::{Path, State},
    http::{StatusCode, header::LOCATION},
    response::{IntoResponse, Response},
};
use axum_valid::Valid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use herald_api_base::application::http::auth::util::{
    ClientIp, normalize_email, rate_limit_hit, verify_turnstile_for_client_app,
};
use herald_api_base::application::http::common::public_helper::realm_public_url;
pub use herald_api_base::application::http::server::api_entities::ErrorResponse;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::BrowserTokenService;
use herald_core::domain::security_constants::{
    RESET_PASSWORD_CONFIRM_IP_RATE_LIMIT, RESET_PASSWORD_REQUEST_EMAIL_RATE_LIMIT,
    RESET_PASSWORD_REQUEST_IP_RATE_LIMIT,
};
use herald_core::domain::user::ports::UserService;
use herald_core::infrastructure::authentication::RedisBrowserTokenService;
use herald_core::third::email::{EmailService, EmailTemplateKind};

use crate::mailflow::{self, MailflowType};

#[derive(Serialize, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ResetPasswordRequestRequest {
    #[validate(length(min = 1, max = 255))]
    pub client_id: String,
    #[validate(email)]
    pub email: String,
    pub turnstile_token: Option<String>, // Optional: required only if Turnstile is enabled for realm
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ResetPasswordRequestResponse {
    pub message: String,
}

/// Request password reset
///
/// Initiates a password reset process by sending a verification code to user's email address.
/// Always returns success to prevent email enumeration.
#[utoipa::path(
  post,
  path = "/api/auth/{realmId}/reset_password/request",
  tag = "auth",
  operation_id = "reset_password_request",
  params(
    ("realmId" = String, Path, description = "Realm ID")
  ),
  request_body = ResetPasswordRequestRequest,
  responses(
    (status = 200, description = "Request accepted (always ok).", body = ResetPasswordRequestResponse),
    (status = 400, description = "Bad request", body = ErrorResponse),
    (status = 429, description = "Too many requests", body = ErrorResponse),
    (status = 500, description = "Internal server error", body = ErrorResponse)
  )
)]
pub async fn request(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Valid(Json(payload)): Valid<Json<ResetPasswordRequestRequest>>,
) -> Result<ApiResult<ResetPasswordRequestResponse>, ApiError> {
    let email = normalize_email(&payload.email);

    // Resolve the Client App (validates realm/enabled) before Turnstile so the
    // human-verification check can read its Turnstile config (D-PROTECT-01).
    let client_app =
        mailflow::require_enabled_client(&state, &realm_id, &payload.client_id).await?;

    // turnstile 校验（按 Client App 配置，D-PROTECT-01）
    verify_turnstile_for_client_app(&state, &client_app, payload.turnstile_token.as_deref(), &ip)
        .await?;

    // ip + email 限流
    rate_limit_hit(
        &state,
        format!("rl:reset_password:req:ip:{ip}"),
        RESET_PASSWORD_REQUEST_IP_RATE_LIMIT.0,
        RESET_PASSWORD_REQUEST_IP_RATE_LIMIT.1,
    )
    .await?;
    rate_limit_hit(
        &state,
        format!("rl:reset_password:req:email:{email}"),
        RESET_PASSWORD_REQUEST_EMAIL_RATE_LIMIT.0,
        RESET_PASSWORD_REQUEST_EMAIL_RATE_LIMIT.1,
    )
    .await?;

    // Use UserService to request password reset
    let code = state
        .service
        .user_service()
        .reset_password_request(&realm_id, &email, "reset_password")
        .await
        .map_err(|e| {
            tracing::error!("Failed to create reset password code: {}", e);
            ApiError::internal("Failed to store reset password code".to_string())
        })?;

    mailflow::store(
        &state,
        &code,
        &realm_id,
        &payload.client_id,
        MailflowType::ResetPassword,
    )
    .await?;

    // Send email (best effort: don't expose email failure to caller)
    // Link points to the frontend reset-password page, which reads the code
    // from the query string and POSTs to the confirm endpoint via the API client.
    let link = realm_public_url(
        &state,
        &realm_id,
        &format!("auth/reset-password?code={code}"),
    )
    .await?;
    if let Err(e) = EmailService::send_templated_email(
        &state.pool,
        &realm_id,
        &email,
        EmailTemplateKind::ResetPassword,
        &link,
        None,
    )
    .await
    {
        tracing::error!("Failed to send reset password email: {e}");
    }

    Ok(ApiResult::ok(ResetPasswordRequestResponse {
        message: "ok".to_string(),
    }))
}

#[derive(Serialize, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ResetPasswordConfirmRequest {
    #[validate(length(min = 8, max = 100))]
    pub new_pass: String,
    pub turnstile_token: Option<String>, // Optional: required only if Turnstile is enabled for realm
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ResetPasswordConfirmResponse {
    pub message: String,
}

/// Confirm password reset with verification code
///
/// Completes password reset process using verification code sent to user's email.
#[utoipa::path(
  post,
  path = "/api/auth/{realmId}/reset_password/confirm/{resetCode}",
  tag = "auth",
  operation_id = "reset_password_confirm",
  params(
    ("realmId" = String, Path, description = "Realm ID"),
    ("resetCode" = String, Path, description = "Reset password code")
  ),
  request_body = ResetPasswordConfirmRequest,
  responses(
    (status = 302, description = "Password reset successful; redirecting to the registered Client App URL."),
    (status = 400, description = "Bad request", body = ErrorResponse),
    (status = 429, description = "Too many requests", body = ErrorResponse),
    (status = 500, description = "Internal server error", body = ErrorResponse)
  )
)]
pub async fn confirm(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Path((realm_id, code)): Path<(String, String)>,
    Valid(Json(payload)): Valid<Json<ResetPasswordConfirmRequest>>,
) -> Result<Response, ApiError> {
    // Resolve the Client App via the mailflow code before Turnstile so the
    // human-verification check can read its Turnstile config (D-PROTECT-01).
    let client =
        mailflow::load_client(&state, &code, &realm_id, MailflowType::ResetPassword).await?;

    // turnstile 校验（按 Client App 配置，D-PROTECT-01）
    verify_turnstile_for_client_app(&state, &client, payload.turnstile_token.as_deref(), &ip)
        .await?;

    rate_limit_hit(
        &state,
        format!("rl:reset_password:confirm:ip:{ip}"),
        RESET_PASSWORD_CONFIRM_IP_RATE_LIMIT.0,
        RESET_PASSWORD_CONFIRM_IP_RATE_LIMIT.1,
    )
    .await?;

    // Use UserService to confirm password reset
    let user_id = state
        .service
        .user_service()
        .reset_password_confirm(&code, payload.new_pass, &realm_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to confirm password reset: {}", e);
            match e {
                herald_core::domain::common::entities::app_errors::CoreError::BadRequest(msg) => {
                    ApiError::bad_request(msg)
                }
                _ => ApiError::internal("Failed to reset password".to_string()),
            }
        })?;

    // Revoke all active sessions for the user after a successful password
    // reset, so any potentially compromised session is invalidated. This is a
    // best-effort post-reset security action: the password has already been
    // changed, so a revocation failure is logged but does not undo the reset.
    let token_service = RedisBrowserTokenService::new(state.redis_manager.clone());
    if let Err(e) = token_service
        .revoke_user_families(&user_id.to_string())
        .await
    {
        tracing::error!(error = %e, %user_id, "Failed to revoke sessions after password reset");
    }

    let fallback = realm_public_url(&state, &realm_id, "").await?;
    let location = mailflow::return_url(client.password_reset_return_url.as_deref(), fallback);
    Ok((StatusCode::FOUND, [(LOCATION, location)]).into_response())
}
