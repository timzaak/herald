use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use axum_valid::Valid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use crate::reauth::consume_reauth;
use herald_api_base::application::http::auth::identity_middleware::authenticate_bearer;
use herald_api_base::application::http::auth::util::{
    ClientIp, epoch_seconds, normalize_email, rate_limit_hit,
};
use herald_api_base::application::http::common::auth_utils::require_token_scope;
use herald_api_base::application::http::common::public_helper::realm_public_url_parts;
pub use herald_api_base::application::http::server::api_entities::ErrorResponse;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::{CredentialScope, TargetOperation};
use herald_core::domain::security_constants::{
    CHANGE_EMAIL_CONFIRM_IP_RATE_LIMIT, CHANGE_EMAIL_REQUEST_EMAIL_RATE_LIMIT,
    CHANGE_EMAIL_REQUEST_IP_RATE_LIMIT, EMAIL_VERIFICATION_CODE_TTL_SECONDS,
};
use herald_core::third::email::{EmailService, EmailTemplateKind};

#[derive(Serialize, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ChangeEmailRequest {
    #[validate(email)]
    pub new_email: String,

    /// Fresh re-authentication ticket obtained from `/api/user/reauth/verify`.
    pub reauth_token: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ChangeEmailResponse {
    pub message: String,
}

/// Request to change the email address for the authenticated user
///
/// Initiates an email change process by sending a verification code to the new email address.
/// The user must click the confirmation link in the email to complete the change.
#[utoipa::path(
  post,
  path = "/api/auth/{realmId}/change_email/request",
  tag = "auth",
  operation_id = "change_email_request",
  params(
    ("realmId" = String, Path, description = "Realm ID")
  ),
  request_body = ChangeEmailRequest,
  responses(
    (status = 200, description = "Change email request accepted.", body = ChangeEmailResponse),
    (status = 400, description = "Bad request", body = ErrorResponse),
    (status = 401, description = "Unauthorized", body = ErrorResponse),
    (status = 409, description = "Email already in use", body = ErrorResponse),
    (status = 429, description = "Too many requests", body = ErrorResponse),
    (status = 500, description = "Internal server error", body = ErrorResponse)
  )
)]
pub async fn request(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Valid(Json(payload)): Valid<Json<ChangeEmailRequest>>,
) -> Result<ApiResult<ChangeEmailResponse>, ApiError> {
    let (identity, context) = authenticate_bearer(&state, &headers).await?;
    require_token_scope(&identity, &context, CredentialScope::ChangeEmail)?;
    if identity.realm_id() != realm_id {
        return Err(ApiError::forbidden(
            "cannot request email change for a different realm",
        ));
    }

    // Require a fresh re-authentication ticket for this high-assurance operation.
    consume_reauth(
        &state,
        &identity,
        &context,
        &payload.reauth_token,
        TargetOperation::ChangeEmail,
    )
    .await?;

    let new_email = normalize_email(&payload.new_email);

    // Apply rate limiting: 1 request per minute per IP and email
    rate_limit_hit(
        &state,
        format!("rl:change_email:req:ip:{ip}"),
        CHANGE_EMAIL_REQUEST_IP_RATE_LIMIT.0,
        CHANGE_EMAIL_REQUEST_IP_RATE_LIMIT.1,
    )
    .await?;
    rate_limit_hit(
        &state,
        format!("rl:change_email:req:email:{new_email}"),
        CHANGE_EMAIL_REQUEST_EMAIL_RATE_LIMIT.0,
        CHANGE_EMAIL_REQUEST_EMAIL_RATE_LIMIT.1,
    )
    .await?;

    // Generate verification code and create email change request
    let code =
        request_email_change_internal(&state, &realm_id, &identity.user_id(), &new_email).await?;

    // Send confirmation email
    send_confirmation_email(&state, &realm_id, &new_email, &code).await?;

    Ok(ApiResult::ok(ChangeEmailResponse {
        message: "ok".to_string(),
    }))
}

/// Internal function to create email change request
/// This contains the business logic for generating and storing verification codes
async fn request_email_change_internal(
    state: &AppState,
    realm_id: &str,
    user_id: &str,
    new_email: &str,
) -> Result<String, ApiError> {
    let code = ChangeEmailCode::generate(realm_id, user_id);

    // Newest code wins (mirrors the verification-code repository): the confirm
    // path reads the latest row, so older unconsumed change-email codes are
    // invalidated instead of staying usable for the full TTL.
    sqlx::query(
        "DELETE FROM email_verification_code WHERE realm_id = $1 AND email = $2 AND type = 'change_email'",
    )
    .bind(realm_id)
    .bind(new_email)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to invalidate previous change-email code: {}", e);
        ApiError::internal("Failed to create verification code")
    })?;

    sqlx::query(
        "INSERT INTO email_verification_code (realm_id, email, type, verification_code) VALUES ($1, $2, $3, $4)",
    )
    .bind(realm_id)
    .bind(new_email)
    .bind("change_email")
    .bind(&code)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to create email verification code: {}", e);
        ApiError::internal("Failed to create verification code")
    })?;

    Ok(code)
}

/// Internal function to send confirmation email
async fn send_confirmation_email(
    state: &AppState,
    realm_id: &str,
    new_email: &str,
    code: &str,
) -> Result<(), ApiError> {
    let (public_base, _) = realm_public_url_parts(state, realm_id).await?;
    let link = format!("{public_base}/api/auth/{realm_id}/change_email/confirm/{code}");
    EmailService::send_templated_email(
        &state.pool,
        realm_id,
        new_email,
        EmailTemplateKind::ChangeEmail,
        &link,
        None,
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to send confirmation email: {}", e);
        ApiError::internal("Failed to send confirmation email")
    })?;
    Ok(())
}

/// Confirm email change with verification code
///
/// Completes the email change process using the verification code sent to the new email address.
#[utoipa::path(
  get,
  path = "/api/auth/{realmId}/change_email/confirm/{changeCode}",
  tag = "auth",
  operation_id = "change_email_confirm",
  params(
    ("realmId" = String, Path, description = "Realm ID"),
    ("changeCode" = String, Path, description = "Change email confirmation code")
  ),
  responses(
    (status = 200, description = "Email changed.", body = ChangeEmailResponse),
    (status = 400, description = "Bad request", body = ErrorResponse),
    (status = 401, description = "Unauthorized", body = ErrorResponse),
    (status = 403, description = "Forbidden", body = ErrorResponse),
    (status = 409, description = "Email already in use", body = ErrorResponse),
    (status = 500, description = "Internal server error", body = ErrorResponse)
  )
)]
pub async fn confirm(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Path((realm_id, code)): Path<(String, String)>,
) -> Result<ApiResult<ChangeEmailResponse>, ApiError> {
    let (identity, context) = authenticate_bearer(&state, &headers).await?;
    require_token_scope(&identity, &context, CredentialScope::ChangeEmail)?;

    rate_limit_hit(
        &state,
        format!("rl:change_email:confirm:ip:{ip}"),
        CHANGE_EMAIL_CONFIRM_IP_RATE_LIMIT.0,
        CHANGE_EMAIL_CONFIRM_IP_RATE_LIMIT.1,
    )
    .await?;

    if identity.realm_id() != realm_id {
        return Err(ApiError::forbidden(
            "cannot confirm email change for a different realm",
        ));
    }

    // Use the current authenticated session as the source of truth so a stolen
    // confirmation code cannot be replayed across users or sessions.
    confirm_email_change_internal(&state, &realm_id, &identity.user_id(), &code).await?;

    Ok(ApiResult::ok(ChangeEmailResponse {
        message: "ok".to_string(),
    }))
}

/// Internal function to handle email change confirmation with transaction management
/// This function contains the business logic that should ideally be in a service layer
async fn confirm_email_change_internal(
    state: &AppState,
    current_realm_id: &str,
    current_user_id: &str,
    code: &str,
) -> Result<(), ApiError> {
    let parsed = ChangeEmailCode::parse(code)?;

    let user_id = uuid::Uuid::parse_str(parsed.user_id)
        .map_err(|_| ApiError::bad_request("invalid user_id in change code".to_string()))?;

    if parsed.realm_id != current_realm_id || parsed.user_id != current_user_id {
        return Err(ApiError::forbidden(
            "cannot confirm email change for a different user",
        ));
    }

    // Begin transaction for atomic code lookup + email update
    let mut tx = state.pool.begin().await.map_err(|e| {
        tracing::error!("Failed to begin transaction: {}", e);
        ApiError::internal("Failed to begin transaction")
    })?;

    // Retrieve the new email from verification code (inside tx for atomicity).
    // Codes older than the TTL are rejected — the emailed link must not stay
    // usable forever.
    let code_cutoff =
        chrono::Utc::now() - chrono::Duration::seconds(EMAIL_VERIFICATION_CODE_TTL_SECONDS as i64);
    let new_email: Option<String> = sqlx::query_scalar(
        "SELECT email FROM email_verification_code WHERE realm_id = $1 AND verification_code = $2 AND type = 'change_email' AND created_at >= $3 ORDER BY id DESC LIMIT 1 FOR UPDATE",
    )
    .bind(current_realm_id)
    .bind(code)
    .bind(code_cutoff)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| {
        tracing::error!("Failed to retrieve email verification code: {}", e);
        ApiError::internal("Failed to retrieve verification code")
    })?;

    let new_email =
        new_email.ok_or_else(|| ApiError::bad_request("change code not found".to_string()))?;

    // Update user email
    let update_result = sqlx::query(
        "UPDATE account SET email = $1, updated_at = NOW() WHERE realm_id = $2 AND id = $3",
    )
    .bind(&new_email)
    .bind(current_realm_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await;

    match update_result {
        Ok(_) => {
            // Delete the verification code after successful update
            sqlx::query("DELETE FROM email_verification_code WHERE realm_id = $1 AND verification_code = $2 AND type = 'change_email'")
                .bind(current_realm_id)
                .bind(code)
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to delete verification code: {}", e);
                    ApiError::internal("Failed to delete verification code")
                })?;

            // Commit transaction
            tx.commit().await.map_err(|e| {
                tracing::error!("Failed to commit transaction: {}", e);
                ApiError::internal("Failed to commit transaction")
            })?;
            Ok(())
        }
        Err(e) => {
            // Handle unique constraint violation (email already exists)
            if let sqlx::Error::Database(db_err) = &e
                && db_err.code().as_deref() == Some("23505")
            {
                return Err(ApiError::conflict("email already in use".to_string()));
            }
            tracing::error!("Failed to change email: {}", e);
            Err(ApiError::internal("Failed to change email".to_string()))
        }
    }
}

/// Verification code for email change: `realmId_userId_uuid_ts`
///
/// realm_id may contain underscores; user_id is a UUID (hyphens, no underscores);
/// uuid and timestamp are always underscore-free.
struct ChangeEmailCode<'a> {
    realm_id: &'a str,
    user_id: &'a str,
    _uuid: &'a str,
    _ts: &'a str,
}

impl<'a> ChangeEmailCode<'a> {
    fn generate(realm_id: &str, user_id: &str) -> String {
        let ts = epoch_seconds();
        format!("{}_{}_{}_{}", realm_id, user_id, uuid::Uuid::now_v7(), ts)
    }

    fn parse(code: &'a str) -> Result<Self, ApiError> {
        let mut parts = code.rsplitn(3, '_');
        let _ts = parts
            .next()
            .ok_or_else(|| ApiError::bad_request("invalid change code".to_string()))?;
        let _uuid = parts
            .next()
            .ok_or_else(|| ApiError::bad_request("invalid change code".to_string()))?;
        let remainder = parts
            .next()
            .ok_or_else(|| ApiError::bad_request("invalid change code".to_string()))?;

        let mut rp = remainder.rsplitn(2, '_');
        let user_id = rp
            .next()
            .ok_or_else(|| ApiError::bad_request("invalid change code".to_string()))?;
        let realm_id = rp
            .next()
            .ok_or_else(|| ApiError::bad_request("invalid change code".to_string()))?;

        Ok(Self {
            realm_id,
            user_id,
            _uuid,
            _ts,
        })
    }
}
