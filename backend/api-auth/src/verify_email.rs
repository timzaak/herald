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
use herald_api_base::application::http::common::public_helper::realm_public_url_parts;
pub use herald_api_base::application::http::server::api_entities::ErrorResponse;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::security_constants::{
    VERIFY_EMAIL_CONFIRM_IP_RATE_LIMIT, VERIFY_EMAIL_TRIGGER_EMAIL_RATE_LIMIT,
    VERIFY_EMAIL_TRIGGER_IP_RATE_LIMIT,
};
use herald_core::domain::user::ports::UserService;
use herald_core::third::email::{EmailService, EmailTemplateKind};

use crate::mailflow::{self, MailflowType};

#[derive(Serialize, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct VerifyEmailTriggerRequest {
    #[validate(length(min = 1, max = 255))]
    pub client_id: String,
    #[validate(email)]
    pub email: String,
    pub turnstile_token: Option<String>, // Optional: required only if Turnstile is enabled for realm
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct VerifyEmailTriggerResponse {
    pub message: String,
}

/// Trigger email verification
///
/// Sends a verification email to specified email address with a confirmation link.
/// Used to initiate email verification for pending accounts.
#[utoipa::path(
  post,
  path = "/api/auth/{realmId}/verify_email/trigger",
  tag = "auth",
  operation_id = "verify_email_trigger",
  params(
    ("realmId" = String, Path, description = "Realm ID")
  ),
  request_body = VerifyEmailTriggerRequest,
  responses(
    (status = 200, description = "Email verification triggered.", body = VerifyEmailTriggerResponse),
    (status = 400, description = "Bad request", body = ErrorResponse),
    (status = 401, description = "Unauthorized", body = ErrorResponse),
    (status = 429, description = "Too many requests", body = ErrorResponse),
    (status = 500, description = "Internal server error", body = ErrorResponse)
  )
)]
pub async fn trigger(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Valid(Json(payload)): Valid<Json<VerifyEmailTriggerRequest>>,
) -> Result<ApiResult<VerifyEmailTriggerResponse>, ApiError> {
    let email = normalize_email(&payload.email);

    // Resolve the Client App (validates realm/enabled) before Turnstile so the
    // human-verification check can read its Turnstile config (D-PROTECT-01).
    let client_app =
        mailflow::require_enabled_client(&state, &realm_id, &payload.client_id).await?;

    // turnstile 校验（按 Client App 配置，D-PROTECT-01）
    verify_turnstile_for_client_app(&state, &client_app, payload.turnstile_token.as_deref(), &ip)
        .await?;

    // ip + email 限流：每分钟最多 5 次
    rate_limit_hit(
        &state,
        format!("rl:verify_email:ip:{ip}"),
        VERIFY_EMAIL_TRIGGER_IP_RATE_LIMIT.0,
        VERIFY_EMAIL_TRIGGER_IP_RATE_LIMIT.1,
    )
    .await?;
    rate_limit_hit(
        &state,
        format!("rl:verify_email:email:{email}"),
        VERIFY_EMAIL_TRIGGER_EMAIL_RATE_LIMIT.0,
        VERIFY_EMAIL_TRIGGER_EMAIL_RATE_LIMIT.1,
    )
    .await?;

    // Use UserService to trigger email verification
    let code = state
        .service
        .user_service()
        .verify_email_trigger(&realm_id, &email, "verify_email")
        .await
        .map_err(|e| {
            tracing::error!("Failed to store email verification code: {e}");
            ApiError::internal("Failed to store email verification code".to_string())
        })?;

    mailflow::store(
        &state,
        &code,
        &realm_id,
        &payload.client_id,
        MailflowType::VerifyEmail,
    )
    .await?;

    let (public_base, _) = realm_public_url_parts(&state, &realm_id).await?;
    let link = format!("{public_base}/api/auth/{realm_id}/verify_email/confirm/{code}");
    EmailService::send_templated_email(
        &state.pool,
        &realm_id,
        &email,
        EmailTemplateKind::VerifyEmail,
        &link,
        None,
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to send verification email: {e}");
        ApiError::internal("Failed to send verification email".to_string())
    })?;

    Ok(ApiResult::ok(VerifyEmailTriggerResponse {
        message: "ok".to_string(),
    }))
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct VerifyEmailConfirmResponse {
    pub message: String,
}

/// Confirm email verification with code
///
/// Completes email verification process using verification code sent to user's email.
/// Activates user account upon successful verification.
#[utoipa::path(
  get,
  path = "/api/auth/{realmId}/verify_email/confirm/{emailVerificationCode}",
  tag = "auth",
  operation_id = "verify_email_confirm",
  params(
    ("realmId" = String, Path, description = "Realm ID"),
    ("emailVerificationCode" = String, Path, description = "Email verification code")
  ),
  responses(
    (status = 302, description = "Email verified; redirecting to the registered Client App URL."),
    (status = 400, description = "Bad request", body = ErrorResponse),
    (status = 500, description = "Internal server error", body = ErrorResponse)
  )
)]
pub async fn confirm(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Path((realm_id, code)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    rate_limit_hit(
        &state,
        format!("rl:verify_email_confirm:ip:{ip}"),
        VERIFY_EMAIL_CONFIRM_IP_RATE_LIMIT.0,
        VERIFY_EMAIL_CONFIRM_IP_RATE_LIMIT.1,
    )
    .await?;

    let client = mailflow::load_client(&state, &code, &realm_id, MailflowType::VerifyEmail).await?;

    // Use UserService to verify email
    let verified_user = state
        .service
        .user_service()
        .verify_email(&code, &realm_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to verify email: {e}");
            match e {
                herald_core::domain::common::entities::app_errors::CoreError::BadRequest(msg) => {
                    ApiError::bad_request(msg)
                }
                _ => ApiError::internal("Failed to verify email".to_string()),
            }
        })?;

    // Registration points: the no-verification path grants at register time;
    // this activation point is the equivalent moment when the realm requires
    // email verification (points.md: same registration flow must not differ
    // by the verification toggle). Idempotent on `registration:{user_id}`, so
    // a re-verify cannot double-grant; best-effort like the register path.
    if let Err(e) = state
        .registration_service
        .handle_user_registration(verified_user.id, &realm_id)
        .await
    {
        tracing::error!(
            realm_id = %realm_id,
            user_id = %verified_user.id,
            error = %e,
            "Failed to grant registration points at email verification, but verification succeeded"
        );
    }

    let fallback = herald_api_base::application::http::common::public_helper::realm_public_url(
        &state, &realm_id, "",
    )
    .await?;
    let location = mailflow::return_url(client.email_verify_return_url.as_deref(), fallback);
    Ok((StatusCode::FOUND, [(LOCATION, location)]).into_response())
}
