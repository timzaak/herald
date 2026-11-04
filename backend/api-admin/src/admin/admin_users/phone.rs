// Admin phone disposition (US-PL-005). The rebind flow deliberately does NOT
// require the realm phone-login switch: an account-data operation, not a
// login entrance.

use axum::{
    Extension, Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use axum_valid::Valid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::admin::admin_users::types::ErrorResponse;
use herald_api_base::application::http::auth::util::{
    ClientIp, OtpRecord, SmsOtpFailure, claim_sms_otp_code, is_sms_configured,
    load_phone_login_settings, map_sms_send_error, mask_phone, normalize_phone, now_epoch_ms,
    phone_in_use_conflict, rate_limit_hit, store_otp_record, user_agent_from_headers,
};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::AuditContext;
use herald_core::domain::authentication::Identity;
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::security_constants::{
    SMS_ADMIN_REBIND_CONFIRM_IP_RATE_LIMIT, SMS_ADMIN_REBIND_SEND_IP_RATE_LIMIT,
    SMS_OTP_CODE_TTL_SECONDS, SMS_OTP_MAX_ATTEMPTS, SMS_OTP_SEND_PHONE_RATE_LIMIT,
    unified_sms_budget_key,
};
use herald_core::domain::user::AdminUserService;
use herald_core::domain::user::UserStatus;
use herald_core::domain::user::admin_errors::UserAdminError;
use herald_core::domain::user::ports::UserRepository;
use herald_core::third::sms::SmsService;

#[derive(Debug, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct AdminPhoneRebindSendRequest {
    #[validate(length(min = 1, max = 32))]
    pub phone: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminPhoneRebindSendResponse {
    pub masked_new_phone: String,
    pub expires_in_seconds: u64,
}

#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct AdminPhoneRebindConfirmRequest {
    #[validate(length(min = 6, max = 6))]
    pub code: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminPhoneRebindConfirmResponse {
    pub masked_phone: String,
}

/// Pending rebind record (Redis, TTL ≤ 300s, newest-wins — at most one pending
/// rebind per target user). Transient by design: losing it costs a resend,
/// never data — the old binding is untouched.
#[derive(Serialize, Deserialize)]
pub(crate) struct AdminRebindPending {
    #[serde(rename = "newPhone")]
    pub(crate) new_phone: String,
    code: String,
    #[serde(rename = "maxAttempts")]
    max_attempts: i64,
    #[serde(rename = "expiresAtMs")]
    expires_at_ms: u64,
}

impl OtpRecord for AdminRebindPending {
    fn code(&self) -> &str {
        &self.code
    }
    fn max_attempts(&self) -> i64 {
        self.max_attempts
    }
    fn expires_at_ms(&self) -> u64 {
        self.expires_at_ms
    }
}

pub(crate) fn pending_key(realm_id: &str, user_id: Uuid) -> String {
    format!("smsotp:adminrebind:{realm_id}:{user_id}")
}

fn pending_attempts_key(realm_id: &str, user_id: Uuid) -> String {
    format!("smsotp:adminrebind:attempts:{realm_id}:{user_id}")
}

/// Masked NEW phone of the pending rebind, if any (absent/expired reads as
/// "no pending rebind").
pub(crate) async fn read_pending_rebind_phone(
    state: &AppState,
    realm_id: &str,
    user_id: Uuid,
) -> Option<String> {
    use redis::AsyncCommands;
    let mut conn = state.redis_manager.get().await.ok()?;
    let raw: Option<String> = conn
        .get(pending_key(realm_id, user_id))
        .await
        .ok()
        .flatten();
    raw.and_then(|json| serde_json::from_str::<AdminRebindPending>(&json).ok())
        .and_then(|pending| mask_phone(&pending.new_phone))
}

fn map_user_admin_error(e: UserAdminError) -> ApiError {
    match e {
        UserAdminError::UserNotFound(id) => ApiError::not_found(format!("User not found: {id}")),
        UserAdminError::UserDeleted(id) => ApiError::conflict(format!("User is deleted: {id}")),
        UserAdminError::PermissionDenied(msg) => ApiError::forbidden(msg),
        UserAdminError::InvalidOperation(msg) => ApiError::bad_request(msg),
        UserAdminError::DuplicatePhone(_) => phone_in_use_conflict(),
        other => {
            tracing::error!(error = %other, "Admin phone operation failed");
            ApiError::internal("Unexpected error")
        }
    }
}

/// 404 for cross-realm ids (id-oracle convention) and for Deleted targets —
/// a send against a tombstone would pay for an SMS that can never confirm.
async fn require_target_in_realm(
    state: &AppState,
    realm_id: &str,
    user_id: Uuid,
) -> Result<(), ApiError> {
    match state.user_repository.get_user_by_id(user_id).await {
        Ok(user) if user.realm_id == realm_id && user.status != UserStatus::Deleted => Ok(()),
        Ok(_) => Err(ApiError::not_found("User not found")),
        Err(CoreError::NotFound) => Err(ApiError::not_found("User not found")),
        Err(e) => Err(ApiError::from(e)),
    }
}

/// Remove a user's phone binding (admin disposition; audit recorded).
#[utoipa::path(
    delete,
    path = "/api/users/{userId}/phone",
    operation_id = "admin_user_phone_unbind",
    tag = "users",
    params(
        ("userId" = Uuid, Path, description = "User ID")
    ),
    responses(
        (status = 204, description = "Phone unbound"),
        (status = 400, description = "No phone bound to the account", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions (users.manage)", body = ErrorResponse),
        (status = 404, description = "User not found (including cross-realm targets)", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn admin_user_phone_unbind(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(user_id): Path<Uuid>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let admin = AdminIdentity::require(identity.clone(), "user phone management")?;
    admin.require_permission(&state, "users", "manage").await?;
    let realm_id = admin.realm_id().to_string();

    let ctx = AuditContext::admin(admin.identity(), ip, user_agent_from_headers(&headers));
    state
        .admin_user_service
        .unbind_user_phone(identity, ctx, &realm_id, user_id)
        .await
        .map_err(map_user_admin_error)?;

    Ok(StatusCode::NO_CONTENT)
}

/// The DB write happens only at confirm — until then the old binding is
/// untouched.
#[utoipa::path(
    post,
    path = "/api/users/{userId}/phone/rebind/send",
    operation_id = "admin_user_phone_rebind_send",
    tag = "users",
    params(
        ("userId" = Uuid, Path, description = "User ID")
    ),
    request_body = AdminPhoneRebindSendRequest,
    responses(
        (status = 200, description = "Code sent to the new phone; pending rebind registered", body = AdminPhoneRebindSendResponse),
        (status = 400, description = "SMS channel not configured / invalid phone", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions (users.manage)", body = ErrorResponse),
        (status = 404, description = "User not found (including cross-realm targets)", body = ErrorResponse),
        (status = 409, description = "Phone already in use (or already the target's binding)", body = ErrorResponse),
        (status = 429, description = "Rate limited", body = ErrorResponse),
        (status = 502, description = "SMS upstream send failed", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn admin_user_phone_rebind_send(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(user_id): Path<Uuid>,
    ClientIp(ip): ClientIp,
    Valid(Json(payload)): Valid<Json<AdminPhoneRebindSendRequest>>,
) -> Result<ApiResult<AdminPhoneRebindSendResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, "user phone management")?;
    admin.require_permission(&state, "users", "manage").await?;
    let realm_id = admin.realm_id().to_string();

    require_target_in_realm(&state, &realm_id, user_id).await?;

    // Channel gate (NOT the phone-login switch — account-data operation).
    if !is_sms_configured(&state, &realm_id).await? {
        return Err(ApiError::bad_request(
            "SMS channel is not configured for this realm".to_string(),
        ));
    }

    rate_limit_hit(
        &state,
        format!("rl:sms:adminrebind:send:ip:{ip}"),
        SMS_ADMIN_REBIND_SEND_IP_RATE_LIMIT.0,
        SMS_ADMIN_REBIND_SEND_IP_RATE_LIMIT.1,
    )
    .await?;

    let settings = load_phone_login_settings(&state, &realm_id).await?;
    let e164 = normalize_phone(&payload.phone, &settings.default_country_code)
        .ok_or_else(|| ApiError::bad_request("Invalid phone number"))?;

    // 409 pre-check: conflicts with another account's binding or the target's
    // OWN current one (a no-op rebind is a client error).
    match state
        .user_repository
        .get_user_by_phone(&realm_id, &e164)
        .await
    {
        Ok(Some(_)) => return Err(phone_in_use_conflict()),
        Ok(None) => {}
        Err(e) => return Err(ApiError::from(e)),
    }

    // The unified per-(realm, phone) budget is charged only when a paid SMS
    // will actually be minted — free 409s must not occupy the 1/min slot.
    rate_limit_hit(
        &state,
        unified_sms_budget_key(&realm_id, &e164),
        SMS_OTP_SEND_PHONE_RATE_LIMIT.0,
        SMS_OTP_SEND_PHONE_RATE_LIMIT.1,
    )
    .await?;

    // Newest-wins record + cleared attempt counter; the payload rides the
    // extra `newPhone` field.
    let code = herald_core::third::sms::generate_sms_code();
    {
        let mut conn = state
            .redis_manager
            .get()
            .await
            .map_err(|_| ApiError::internal("Redis connection error"))?;
        let pending = AdminRebindPending {
            new_phone: e164.clone(),
            code: code.clone(),
            max_attempts: SMS_OTP_MAX_ATTEMPTS,
            expires_at_ms: now_epoch_ms() + SMS_OTP_CODE_TTL_SECONDS * 1000,
        };
        store_otp_record(
            &mut conn,
            &pending_key(&realm_id, user_id),
            &pending_attempts_key(&realm_id, user_id),
            &pending,
            SMS_OTP_CODE_TTL_SECONDS,
        )
        .await?;
    }

    SmsService::send_otp(&state.pool, &realm_id, &e164, &code)
        .await
        .map_err(map_sms_send_error)?;

    Ok(ApiResult::ok(AdminPhoneRebindSendResponse {
        masked_new_phone: mask_phone(&e164).unwrap_or_else(|| "****".to_string()),
        expires_in_seconds: SMS_OTP_CODE_TTL_SECONDS,
    }))
}

/// A wrong code costs one attempt from the pending budget; exhausting it
/// invalidates the rebind entirely.
#[utoipa::path(
    post,
    path = "/api/users/{userId}/phone/rebind/confirm",
    operation_id = "admin_user_phone_rebind_confirm",
    tag = "users",
    params(
        ("userId" = Uuid, Path, description = "User ID")
    ),
    request_body = AdminPhoneRebindConfirmRequest,
    responses(
        (status = 200, description = "Rebind applied", body = AdminPhoneRebindConfirmResponse),
        (status = 400, description = "No pending rebind / code wrong, expired or exhausted", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions (users.manage)", body = ErrorResponse),
        (status = 404, description = "User not found (including cross-realm targets)", body = ErrorResponse),
        (status = 409, description = "Phone already in use (concurrent race; resend required)", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn admin_user_phone_rebind_confirm(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(user_id): Path<Uuid>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Valid(Json(payload)): Valid<Json<AdminPhoneRebindConfirmRequest>>,
) -> Result<ApiResult<AdminPhoneRebindConfirmResponse>, ApiError> {
    let admin = AdminIdentity::require(identity.clone(), "user phone management")?;
    admin.require_permission(&state, "users", "manage").await?;
    let realm_id = admin.realm_id().to_string();

    require_target_in_realm(&state, &realm_id, user_id).await?;

    // Per-request throttle; without it code guessing is bounded only by the
    // resend budget.
    rate_limit_hit(
        &state,
        format!("rl:sms:adminrebind:confirm:ip:{ip}"),
        SMS_ADMIN_REBIND_CONFIRM_IP_RATE_LIMIT.0,
        SMS_ADMIN_REBIND_CONFIRM_IP_RATE_LIMIT.1,
    )
    .await?;

    let mut conn = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Redis connection error"))?;

    // Failure wording is family-specific ("rebind"), so the variants map here
    // instead of via `sms_otp_failure_error`.
    let pending: AdminRebindPending = match claim_sms_otp_code(
        &mut conn,
        &pending_key(&realm_id, user_id),
        &pending_attempts_key(&realm_id, user_id),
        &payload.code,
    )
    .await?
    {
        Ok(pending) => pending,
        Err(SmsOtpFailure::Expired) => {
            return Err(ApiError::bad_request(
                "No pending phone rebind for this user; send a new code first",
            ));
        }
        Err(SmsOtpFailure::Exhausted) => {
            return Err(ApiError::bad_request(
                "Too many attempts, please send a new code",
            ));
        }
        Err(SmsOtpFailure::Invalid) => {
            return Err(ApiError::bad_request(
                "Invalid or expired verification code",
            ));
        }
    };

    let ctx = AuditContext::admin(admin.identity(), ip, user_agent_from_headers(&headers));
    state
        .admin_user_service
        .rebind_user_phone(identity, ctx, &realm_id, user_id, &pending.new_phone)
        .await
        .map_err(map_user_admin_error)?;

    Ok(ApiResult::ok(AdminPhoneRebindConfirmResponse {
        masked_phone: mask_phone(&pending.new_phone).unwrap_or_else(|| "****".to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_keys_are_user_scoped() {
        let user = Uuid::nil();
        assert_eq!(
            pending_key("r1", user),
            "smsotp:adminrebind:r1:00000000-0000-0000-0000-000000000000"
        );
        assert!(pending_attempts_key("r1", user).starts_with("smsotp:adminrebind:attempts:r1:"));
    }

    #[test]
    fn pending_record_round_trips_camel_case_fields() {
        // Pin the Redis wire names — a field rename would silently
        // invalidate stored records.
        let pending = AdminRebindPending {
            new_phone: "+8613800138000".to_string(),
            code: "123456".to_string(),
            max_attempts: SMS_OTP_MAX_ATTEMPTS,
            expires_at_ms: 1_700_000_000_000,
        };
        let json = serde_json::to_string(&pending).unwrap();
        assert!(
            json.contains("\"newPhone\":\"+8613800138000\""),
            "got {json}"
        );
        assert!(json.contains("\"maxAttempts\":5"), "got {json}");
        assert!(json.contains("\"expiresAtMs\":1700000000000"), "got {json}");
        let back: AdminRebindPending = serde_json::from_str(&json).unwrap();
        assert_eq!(back.new_phone, "+8613800138000");
    }
}
