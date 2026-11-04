// Self-service phone bind/rebind/unbind (US-PL-001), behind the first-party-only
// `CredentialScope::PhoneManage`. Bind-confirm needs no reauth: possession of
// the SMS code is the proof; until it verifies, the old binding is untouched.

use axum::{
    Json,
    extract::{Extension, State},
};
use axum_valid::Valid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use herald_api_base::application::http::auth::util::{
    ClientIp, StoredOtp, claim_sms_otp_code, is_phone_login_enabled, is_sms_configured,
    load_phone_login_settings, map_sms_send_error, mask_phone, normalize_phone,
    phone_in_use_conflict, rate_limit_hit, sms_otp_failure_error, store_sms_otp_code,
};
use herald_api_base::application::http::common::auth_utils::require_token_scope;
pub use herald_api_base::application::http::server::api_entities::ErrorResponse;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::{
    AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType, NewAuditEvent,
};
use herald_core::domain::authentication::{
    CredentialScope, Identity, TargetOperation, TokenCredentialContext,
};
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::security_constants::{
    SMS_BIND_CONFIRM_IP_RATE_LIMIT, SMS_BIND_SEND_IP_RATE_LIMIT, SMS_OTP_CODE_TTL_SECONDS,
    SMS_OTP_SEND_PHONE_RATE_LIMIT, unified_sms_budget_key,
};
use herald_core::domain::user::ports::UserRepository;
use herald_core::third::sms::SmsService;

use crate::reauth::consume_reauth;
use crate::sms_otp::phone_digest;

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/phone", axum::routing::get(user_phone_status))
        .route(
            "/phone/bind/send",
            axum::routing::post(user_phone_bind_send),
        )
        .route(
            "/phone/bind/confirm",
            axum::routing::post(user_phone_bind_confirm),
        )
        .route("/phone/unbind", axum::routing::post(user_phone_unbind))
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserPhoneStatusResponse {
    /// Bound ⇒ possession was verified (no separate flag — phones only ever
    /// land via a verified SMS code).
    pub bound: bool,
    pub masked_phone: Option<String>,
    /// Realm switch state (fail-closed).
    pub phone_login_enabled: bool,
    /// SMS channel state, shown next to a disabled entry.
    pub sms_configured: bool,
}

#[derive(Debug, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct PhoneBindSendRequest {
    #[validate(length(min = 1, max = 32))]
    pub phone: String,
    /// Fresh reauth ticket from `/api/user/reauth/verify` (ChangePhone).
    pub reauth_token: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PhoneBindSendResponse {
    pub message: String,
    pub expires_in_seconds: u64,
    pub masked_phone: String,
}

#[derive(Debug, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct PhoneBindConfirmRequest {
    #[validate(length(min = 1, max = 32))]
    pub phone: String,
    #[validate(length(min = 6, max = 6))]
    pub code: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PhoneBindConfirmResponse {
    pub masked_phone: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PhoneUnbindRequest {
    pub reauth_token: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PhoneUnbindResponse {
    pub message: String,
}

fn bind_otp_key(realm_id: &str, user_id: uuid::Uuid, e164: &str) -> String {
    format!(
        "smsotp:bind:{}:{}:{}",
        realm_id,
        user_id,
        phone_digest(e164)
    )
}

fn bind_attempts_key(realm_id: &str, user_id: uuid::Uuid, e164: &str) -> String {
    format!(
        "smsotp:bind:attempts:{}:{}:{}",
        realm_id,
        user_id,
        phone_digest(e164)
    )
}

/// Phone binding status + entry gating info for the Security page.
///
/// Deliberately NOT behind the realm switch: with the switch off the retained
/// binding stays visible (read-only) — smooth degradation (US-PL-004).
#[utoipa::path(
    get,
    path = "/api/user/phone",
    operation_id = "user_phone_status",
    tag = "user",
    responses(
        (status = 200, description = "Phone binding status", body = UserPhoneStatusResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden (scope)", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn user_phone_status(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
) -> Result<ApiResult<UserPhoneStatusResponse>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::ProfileRead)?;
    let user = identity
        .as_user()
        .ok_or_else(|| ApiError::forbidden("authenticated user token required"))?;

    // Read fresh: the identity snapshot predates any bind in this session.
    let current = state
        .user_repository
        .get_user_by_id(user.id)
        .await
        .map_err(ApiError::from)?;

    let phone_login_enabled = is_phone_login_enabled(&state, &current.realm_id).await?;
    let sms_configured = is_sms_configured(&state, &current.realm_id).await?;

    Ok(ApiResult::ok(UserPhoneStatusResponse {
        bound: current.phone.is_some(),
        masked_phone: current.phone.as_deref().and_then(mask_phone),
        phone_login_enabled,
        sms_configured,
    }))
}

/// Sends the code to a NEW phone (bind or rebind); the old binding stays
/// until the confirm step succeeds.
#[utoipa::path(
    post,
    path = "/api/user/phone/bind/send",
    operation_id = "user_phone_bind_send",
    tag = "user",
    request_body = PhoneBindSendRequest,
    responses(
        (status = 200, description = "Code sent to the new phone", body = PhoneBindSendResponse),
        (status = 400, description = "Switch off / invalid phone / SMS channel unusable", body = ErrorResponse),
        (status = 401, description = "Unauthorized / invalid reauth ticket", body = ErrorResponse),
        (status = 403, description = "Forbidden (scope)", body = ErrorResponse),
        (status = 409, description = "Phone already in use / reauth ticket consumed or mismatched", body = ErrorResponse),
        (status = 429, description = "Rate limited", body = ErrorResponse),
        (status = 502, description = "SMS upstream send failed", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn user_phone_bind_send(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    ClientIp(ip): ClientIp,
    Valid(Json(payload)): Valid<Json<PhoneBindSendRequest>>,
) -> Result<ApiResult<PhoneBindSendResponse>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::PhoneManage)?;
    let user = identity
        .as_user()
        .ok_or_else(|| ApiError::forbidden("authenticated user token required"))?;
    let realm_id = user.realm_id.clone();

    // Switch gate: a realm without phone login must refuse self-service
    // binding outright (US-PL-001).
    let settings = load_phone_login_settings(&state, &realm_id).await?;
    if !settings.enabled {
        return Err(ApiError::bad_request(
            "Phone login is not enabled for this realm".to_string(),
        ));
    }

    rate_limit_hit(
        &state,
        format!("rl:sms:bind:send:ip:{ip}"),
        SMS_BIND_SEND_IP_RATE_LIMIT.0,
        SMS_BIND_SEND_IP_RATE_LIMIT.1,
    )
    .await?;

    let e164 = normalize_phone(&payload.phone, &settings.default_country_code)
        .ok_or_else(|| ApiError::bad_request("Invalid phone number"))?;

    // Channel check BEFORE the single-use reauth ticket is consumed (same
    // precedence as change_email): a dead end must not burn the ticket.
    if !is_sms_configured(&state, &realm_id).await? {
        return Err(ApiError::bad_request(
            "SMS channel is not configured for this realm".to_string(),
        ));
    }

    consume_reauth(
        &state,
        &identity,
        &context,
        &payload.reauth_token,
        TargetOperation::ChangePhone,
    )
    .await?;

    // Uniqueness pre-check AFTER the reauth ticket (same position as
    // change_email's occupancy check): the 409 vs 401 difference would
    // otherwise let any session probe which numbers are bound — without a
    // valid ticket — and it must not burn the unified budget either. The DB
    // partial index remains the final guard.
    match state
        .user_repository
        .get_user_by_phone(&realm_id, &e164)
        .await
    {
        Ok(Some(other)) if other.id != user.id => return Err(phone_in_use_conflict()),
        Ok(_) => {}
        Err(e) => return Err(ApiError::from(e)),
    }

    // The unified per-(realm, phone) budget is charged only once the request
    // will actually mint a paid SMS: free rejections (401/409) must not let
    // an attacker occupy a victim's 1/min slot at zero cost.
    rate_limit_hit(
        &state,
        unified_sms_budget_key(&realm_id, &e164),
        SMS_OTP_SEND_PHONE_RATE_LIMIT.0,
        SMS_OTP_SEND_PHONE_RATE_LIMIT.1,
    )
    .await?;

    let code = herald_core::third::sms::generate_sms_code();
    {
        let mut conn = state
            .redis_manager
            .get()
            .await
            .map_err(|_| ApiError::internal("Redis connection error"))?;
        store_sms_otp_code(
            &mut conn,
            &bind_otp_key(&realm_id, user.id, &e164),
            &bind_attempts_key(&realm_id, user.id, &e164),
            &code,
        )
        .await?;
    }
    SmsService::send_otp(&state.pool, &realm_id, &e164, &code)
        .await
        .map_err(map_sms_send_error)?;

    Ok(ApiResult::ok(PhoneBindSendResponse {
        message: "ok".to_string(),
        expires_in_seconds: SMS_OTP_CODE_TTL_SECONDS,
        masked_phone: mask_phone(&e164).unwrap_or_else(|| "****".to_string()),
    }))
}

/// Complete the bind/rebind: verify the SMS code, then write the phone in a
/// single UPDATE. The rebind contract — "验证完成前旧绑定不变" — holds because
/// the DB write happens ONLY here. No reauth is consumed: possession of the
/// code IS the proof (mirrors change_email confirm).
#[utoipa::path(
    post,
    path = "/api/user/phone/bind/confirm",
    operation_id = "user_phone_bind_confirm",
    tag = "user",
    request_body = PhoneBindConfirmRequest,
    responses(
        (status = 200, description = "Phone bound/rebound", body = PhoneBindConfirmResponse),
        (status = 400, description = "Switch off / invalid phone / code wrong, expired, consumed or exhausted", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden (scope)", body = ErrorResponse),
        (status = 409, description = "Phone already in use (concurrent bind race)", body = ErrorResponse),
        (status = 429, description = "Rate limited", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn user_phone_bind_confirm(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    ClientIp(ip): ClientIp,
    Valid(Json(payload)): Valid<Json<PhoneBindConfirmRequest>>,
) -> Result<ApiResult<PhoneBindConfirmResponse>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::PhoneManage)?;
    let user = identity
        .as_user()
        .ok_or_else(|| ApiError::forbidden("authenticated user token required"))?;
    let realm_id = user.realm_id.clone();

    let settings = load_phone_login_settings(&state, &realm_id).await?;
    if !settings.enabled {
        return Err(ApiError::bad_request(
            "Phone login is not enabled for this realm".to_string(),
        ));
    }

    rate_limit_hit(
        &state,
        format!("rl:sms:bind:confirm:ip:{ip}"),
        SMS_BIND_CONFIRM_IP_RATE_LIMIT.0,
        SMS_BIND_CONFIRM_IP_RATE_LIMIT.1,
    )
    .await?;

    let e164 = normalize_phone(&payload.phone, &settings.default_country_code)
        .ok_or_else(|| ApiError::bad_request("Invalid phone number"))?;

    // Atomic claim + attempt accounting, same semantics as the login code.
    let mut conn = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Redis connection error"))?;
    let key = bind_otp_key(&realm_id, user.id, &e164);
    let attempts_key = bind_attempts_key(&realm_id, user.id, &e164);
    if let Err(failure) =
        claim_sms_otp_code::<StoredOtp>(&mut conn, &key, &attempts_key, &payload.code).await?
    {
        return Err(sms_otp_failure_error(failure));
    }

    // Single UPDATE; a concurrent same-phone bind loses here (409) via the
    // partial unique index — the final-consistency guard behind the pre-check.
    let had_previous = state
        .user_repository
        .get_user_by_id(user.id)
        .await
        .ok()
        .and_then(|u| u.phone)
        .is_some();

    state
        .user_repository
        .set_user_phone(&realm_id, user.id, Some(e164.clone()))
        .await
        .map_err(|e| match e {
            CoreError::Conflict(_) => phone_in_use_conflict(),
            other => ApiError::from(other),
        })?;

    let masked = mask_phone(&e164).unwrap_or_else(|| "****".to_string());

    // Audit (masked only). Best-effort: never fails the committed bind.
    if let Err(audit_err) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: realm_id.clone(),
            category: AuditCategory::UserManagement,
            action: AuditAction::PhoneBind,
            actor_id: user.id.to_string(),
            actor_type: Some(herald_core::domain::audit::ActorType::User),
            actor_name: Some(user.email.clone()),
            target_type: AuditTargetType::User,
            target_id: user.id.to_string(),
            target_name: Some(user.email.clone()),
            result: AuditResult::Success,
            details: Some(serde_json::json!({
                "maskedPhone": masked,
                "had_previous": had_previous,
            })),
            ip_address: Some(ip),
            user_agent: None,
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %audit_err, "Failed to record phone bind audit event");
    }

    Ok(ApiResult::ok(PhoneBindConfirmResponse {
        masked_phone: masked,
    }))
}

/// Remove the binding. Consumes a reauth ticket (RemovePhone); the account
/// keeps email/username + password login afterwards (US-PL-001 scenario 6).
#[utoipa::path(
    post,
    path = "/api/user/phone/unbind",
    operation_id = "user_phone_unbind",
    tag = "user",
    request_body = PhoneUnbindRequest,
    responses(
        (status = 200, description = "Phone unbound", body = PhoneUnbindResponse),
        (status = 400, description = "Switch off / no binding to remove", body = ErrorResponse),
        (status = 401, description = "Unauthorized / invalid reauth ticket", body = ErrorResponse),
        (status = 403, description = "Forbidden (scope)", body = ErrorResponse),
        (status = 409, description = "Reauth ticket consumed or target mismatch", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn user_phone_unbind(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    ClientIp(ip): ClientIp,
    Json(payload): Json<PhoneUnbindRequest>,
) -> Result<ApiResult<PhoneUnbindResponse>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::PhoneManage)?;
    let user = identity
        .as_user()
        .ok_or_else(|| ApiError::forbidden("authenticated user token required"))?;
    let realm_id = user.realm_id.clone();

    // Switch gate: a disabled realm keeps the retained binding manageable
    // only via an admin, not self-service (US-PL-004 degradation semantics).
    if !is_phone_login_enabled(&state, &realm_id).await? {
        return Err(ApiError::bad_request(
            "Phone login is not enabled for this realm".to_string(),
        ));
    }

    // Dead-end check BEFORE the single-use reauth ticket is consumed (same
    // rule as bind_send): a "no binding" rejection must not burn the ticket.
    let current = state
        .user_repository
        .get_user_by_id(user.id)
        .await
        .map_err(ApiError::from)?;
    let Some(old_phone) = current.phone else {
        return Err(ApiError::bad_request("No phone is bound to this account"));
    };

    consume_reauth(
        &state,
        &identity,
        &context,
        &payload.reauth_token,
        TargetOperation::RemovePhone,
    )
    .await?;

    state
        .user_repository
        .set_user_phone(&realm_id, user.id, None)
        .await
        .map_err(ApiError::from)?;

    if let Err(audit_err) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: realm_id.clone(),
            category: AuditCategory::UserManagement,
            action: AuditAction::PhoneUnbind,
            actor_id: user.id.to_string(),
            actor_type: Some(herald_core::domain::audit::ActorType::User),
            actor_name: Some(user.email.clone()),
            target_type: AuditTargetType::User,
            target_id: user.id.to_string(),
            target_name: Some(user.email.clone()),
            result: AuditResult::Success,
            details: Some(serde_json::json!({
                "maskedPhone": mask_phone(&old_phone),
            })),
            ip_address: Some(ip),
            user_agent: None,
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %audit_err, "Failed to record phone unbind audit event");
    }

    Ok(ApiResult::ok(PhoneUnbindResponse {
        message: "ok".to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_keys_are_user_and_phone_scoped() {
        // Two users binding the SAME phone in one realm must not share code
        // storage; one user binding two phones must not either.
        let a = bind_otp_key("r1", uuid::Uuid::nil(), "+8613800138000");
        let b = bind_attempts_key("r1", uuid::Uuid::nil(), "+8613800138000");
        assert!(a.starts_with("smsotp:bind:r1:"));
        assert!(b.starts_with("smsotp:bind:attempts:r1:"));
        assert!(a.ends_with(&phone_digest("+8613800138000")));
        assert!(b.ends_with(&phone_digest("+8613800138000")));
    }
}
