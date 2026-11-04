use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
    response::IntoResponse,
};
use axum_valid::Valid;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use herald_api_base::application::http::auth::util::{
    ClientIp, StoredOtp, claim_sms_otp_code, is_phone_login_enabled, is_sms_configured,
    load_phone_login_settings, map_sms_send_error, mask_phone, normalize_phone, rate_limit_hit,
    sms_otp_failure_error, store_sms_otp_code, user_agent_from_headers,
    verify_turnstile_for_client_app,
};
use herald_api_base::application::http::server::api_entities::ApiError;
pub use herald_api_base::application::http::server::api_entities::ErrorResponse;
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::{
    ActorType, AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType,
    NewAuditEvent,
};
use herald_core::domain::authentication::BrowserTokenService;
use herald_core::domain::security_constants::{
    SMS_OTP_CODE_TTL_SECONDS, SMS_OTP_SEND_IP_RATE_LIMIT, SMS_OTP_SEND_PHONE_RATE_LIMIT,
    SMS_OTP_VERIFY_IP_RATE_LIMIT, SMS_OTP_VERIFY_PHONE_RATE_LIMIT, unified_sms_budget_key,
};
use herald_core::domain::user::ports::UserRepository;
use herald_core::domain::user_totp::UserTotpRepository;
use herald_core::infrastructure::authentication::RedisBrowserTokenService;
use herald_core::infrastructure::user_totp::PostgresUserTotpRepository;
use herald_core::third::sms::SmsService;

use crate::browser_token::BrowserTokenResponse;
use crate::mailflow;

#[derive(Debug, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct SmsOtpSendRequest {
    /// Phone in any writing form; length ≤ 32. Format authority is the
    /// server: every malformed variant maps to one uniform `invalid_phone`.
    #[validate(length(min = 1, max = 32))]
    pub phone: String,
    #[validate(length(min = 1, max = 36))]
    pub client_id: String,
    #[serde(default)]
    #[schema(required = false)]
    pub turnstile_token: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SmsOtpSendResponse {
    pub message: String,
    pub expires_in_seconds: u64,
    /// Backend-masked form; the frontend never computes or receives the raw
    /// number.
    pub masked_phone: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SmsOtpVerifyRequest {
    pub phone: String,
    pub code: String, // 6 ASCII digits, validated below.
    pub client_id: String,
    #[serde(default)]
    #[schema(required = false)]
    pub turnstile_token: Option<String>,
    #[serde(default)]
    #[schema(required = false)]
    pub agreements: Option<Vec<crate::consent_gate::AuthConsentAgreement>>,
}

impl Validate for SmsOtpVerifyRequest {
    fn validate(&self) -> Result<(), validator::ValidationErrors> {
        let mut errors = validator::ValidationErrors::new();

        if self.client_id.is_empty() || self.client_id.len() > 36 {
            errors.add("client_id", validator::ValidationError::new("length"));
        }
        // Bound the identifier before it reaches Redis keys.
        if self.phone.trim().is_empty() || self.phone.len() > 32 {
            errors.add("phone", validator::ValidationError::new("length"));
        }
        if self.code.len() != 6 || !self.code.bytes().all(|b| b.is_ascii_digit()) {
            errors.add("code", validator::ValidationError::new("invalid_format"));
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// 409 body for `phone_not_registered` — same shape as the email OTP conflict
/// but WITHOUT `agreements` (no auto-register, DEC-phone-login-001).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SmsOtpConflictResponse {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PhoneLoginStatusResponse {
    pub enabled: bool,
}

// Only the login-family key derivation lives here; the claim/store protocol
// is shared (herald_api_base auth util) with the bind/admin paths.

/// sha256 of the E.164 phone — the code key and attempts counter must
/// address the same identity.
pub(crate) fn phone_digest(e164: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(e164.as_bytes());
    hex::encode(hasher.finalize())
}

/// `smsotp:{realm_id}:{sha256(e164)}` — login code (newest code wins).
fn login_otp_key(realm_id: &str, e164: &str) -> String {
    format!("smsotp:{}:{}", realm_id, phone_digest(e164))
}

fn login_attempts_key(realm_id: &str, e164: &str) -> String {
    format!("smsotp:attempts:{}:{}", realm_id, phone_digest(e164))
}

fn phone_not_registered_conflict() -> ApiError {
    ApiError::conflict_json(SmsOtpConflictResponse {
        code: "phone_not_registered".to_string(),
        message: "This phone number is not registered. Please sign up first.".to_string(),
    })
}

/// Send an SMS OTP login code to a BOUND phone.
///
/// Enumeration posture mirrors the email OTP send: an existing-but-inactive
/// account gets a byte-identical 200 with NO code stored or sent; an unknown
/// (or anonymized) phone gets an explicit 409 `phone_not_registered`, never
/// an auto-register branch.
#[utoipa::path(
    post,
    path = "/api/auth/{realmId}/login/sms-otp/send",
    operation_id = "sms_otp_send",
    tag = "auth",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = SmsOtpSendRequest,
    responses(
        (status = 200, description = "Verification code sent (or enumeration-resistant 200).", body = SmsOtpSendResponse),
        (status = 400, description = "Phone login not enabled / invalid phone / SMS channel unusable / bad request", body = ErrorResponse),
        (status = 401, description = "Turnstile verification failed / client app invalid", body = ErrorResponse),
        (status = 409, description = "Phone not registered (no auto-register)", body = SmsOtpConflictResponse),
        (status = 429, description = "Rate limited", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
        (status = 502, description = "SMS upstream send failed", body = ErrorResponse),
    )
)]
pub async fn sms_otp_send(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Valid(Json(payload)): Valid<Json<SmsOtpSendRequest>>,
) -> Result<Json<SmsOtpSendResponse>, ApiError> {
    // Realm switch gate (fail-closed).
    let settings = load_phone_login_settings(&state, &realm_id).await?;
    if !settings.enabled {
        return Err(ApiError::bad_request(
            "Phone login is not enabled for this realm".to_string(),
        ));
    }

    // Validate the Client App before Turnstile (D-PROTECT-01).
    let client_app =
        mailflow::require_enabled_client(&state, &realm_id, &payload.client_id).await?;

    verify_turnstile_for_client_app(&state, &client_app, payload.turnstile_token.as_deref(), &ip)
        .await?;

    // The channel check and the unified budget run only after these gates:
    // unauthenticated callers must not get pre-Turnstile DB reads or
    // channel-status probing for free.
    rate_limit_hit(
        &state,
        format!("rl:sms:login:send:ip:{ip}"),
        SMS_OTP_SEND_IP_RATE_LIMIT.0,
        SMS_OTP_SEND_IP_RATE_LIMIT.1,
    )
    .await?;

    let e164 = normalize_phone(&payload.phone, &settings.default_country_code)
        .ok_or_else(|| ApiError::bad_request("Invalid phone number"))?;

    let masked = mask_phone(&e164).unwrap_or_else(|| "****".to_string());

    let user_opt = match state
        .user_repository
        .get_user_by_phone(&realm_id, &e164)
        .await
    {
        Ok(opt) => opt,
        Err(e) => {
            tracing::error!(realm_id = %realm_id, error = %e, "Failed to look up user for SMS OTP send");
            return Err(ApiError::internal("Internal server error"));
        }
    };

    let should_send = match user_opt.as_ref() {
        Some(user) if user.is_active() => true,
        // Existing but non-active: enumeration-resistant 200, no code stored.
        Some(_) => false,
        // Unknown phone: explicit 409, never a code.
        None => return Err(phone_not_registered_conflict()),
    };

    if !should_send {
        return Ok(Json(SmsOtpSendResponse {
            message: "ok".to_string(),
            expires_in_seconds: SMS_OTP_CODE_TTL_SECONDS,
            masked_phone: masked,
        }));
    }

    // Dangling-state defense: a switch left enabled with a deleted/incomplete
    // channel must reject instead of faking success (PRD §4.2).
    if !is_sms_configured(&state, &realm_id).await? {
        return Err(ApiError::bad_request(
            "SMS channel is not configured for this realm".to_string(),
        ));
    }

    // The unified per-(realm, phone) budget is charged only when a paid SMS
    // will actually be minted — free rejections must not occupy the 1/min slot.
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
            &login_otp_key(&realm_id, &e164),
            &login_attempts_key(&realm_id, &e164),
            &code,
        )
        .await?;
    }
    SmsService::send_otp(&state.pool, &realm_id, &e164, &code)
        .await
        .map_err(map_sms_send_error)?;

    tracing::info!(realm_id = %realm_id, "SMS OTP code sent");

    Ok(Json(SmsOtpSendResponse {
        message: "ok".to_string(),
        expires_in_seconds: SMS_OTP_CODE_TTL_SECONDS,
        masked_phone: masked,
    }))
}

/// Verify an SMS OTP login code and issue a session.
///
/// Reuses the password-login completion chain: second-factor gate (TOTP/passkey
/// → temp token), consent gate, `BrowserTokenResponse`.
#[utoipa::path(
    post,
    path = "/api/auth/{realmId}/login/sms-otp/verify",
    operation_id = "sms_otp_verify",
    tag = "auth",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = SmsOtpVerifyRequest,
    responses(
        (status = 200, description = "Verification successful", body = BrowserTokenResponse),
        (status = 400, description = "Phone login not enabled / invalid phone / code wrong, expired, consumed or exhausted", body = ErrorResponse),
        (status = 401, description = "Client App invalid", body = ErrorResponse),
        (status = 403, description = "Account is disabled", body = ErrorResponse),
        (status = 409, description = "Phone no longer registered (anonymized between send and verify)", body = SmsOtpConflictResponse),
        (status = 429, description = "Rate limited", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    )
)]
pub async fn sms_otp_verify(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Valid(Json(payload)): Valid<Json<SmsOtpVerifyRequest>>,
) -> Result<impl IntoResponse, ApiError> {
    let user_agent = user_agent_from_headers(&headers);

    // Realm switch gate (fail-closed).
    let settings = load_phone_login_settings(&state, &realm_id).await?;
    if !settings.enabled {
        return Err(ApiError::bad_request(
            "Phone login is not enabled for this realm".to_string(),
        ));
    }

    let e164 = normalize_phone(&payload.phone, &settings.default_country_code)
        .ok_or_else(|| ApiError::bad_request("Invalid phone number"))?;

    // Dual rate limiting (no monetary cost; the attempts counter bounds
    // brute force).
    rate_limit_hit(
        &state,
        format!("rl:sms:login:verify:ip:{ip}"),
        SMS_OTP_VERIFY_IP_RATE_LIMIT.0,
        SMS_OTP_VERIFY_IP_RATE_LIMIT.1,
    )
    .await?;
    rate_limit_hit(
        &state,
        format!("rl:sms:login:verify:realm:{realm_id}:phone:{e164}"),
        SMS_OTP_VERIFY_PHONE_RATE_LIMIT.0,
        SMS_OTP_VERIFY_PHONE_RATE_LIMIT.1,
    )
    .await?;

    // The Client App carries the session family binding.
    let client_app =
        mailflow::require_enabled_client(&state, &realm_id, &payload.client_id).await?;
    verify_turnstile_for_client_app(&state, &client_app, payload.turnstile_token.as_deref(), &ip)
        .await?;

    let mut conn = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Redis connection error"))?;
    let key = login_otp_key(&realm_id, &e164);
    let attempts_key = login_attempts_key(&realm_id, &e164);
    if let Err(failure) =
        claim_sms_otp_code::<StoredOtp>(&mut conn, &key, &attempts_key, &payload.code).await?
    {
        record_login_failure(
            &state,
            &realm_id,
            &e164,
            &ip,
            user_agent.as_deref(),
            "invalid_code",
        )
        .await;
        return Err(sms_otp_failure_error(failure));
    }

    let user = match state
        .user_repository
        .get_user_by_phone(&realm_id, &e164)
        .await
    {
        // Anonymized between send and verify (phone = NULL): same
        // not-registered branch as the send side.
        Ok(None) => return Err(phone_not_registered_conflict()),
        Ok(Some(user)) => user,
        Err(e) => {
            tracing::error!(realm_id = %realm_id, error = %e, "Failed to look up user for SMS OTP verify");
            return Err(ApiError::internal("Internal server error"));
        }
    };

    // Disabled accounts cannot log in even with a correct, claimed code.
    if !user.is_active() {
        record_login_failure(
            &state,
            &realm_id,
            &e164,
            &ip,
            user_agent.as_deref(),
            "disabled_account",
        )
        .await;
        return Err(ApiError::forbidden("Account is disabled"));
    }

    // Second-factor gate — SMS OTP must never bypass an existing TOTP/passkey
    // factor (same gates as password login).
    let totp_repo = PostgresUserTotpRepository::new(state.db.clone());
    let has_totp = totp_repo
        .get_config_by_user_id(user.id)
        .await?
        .map(|config| config.enabled)
        .unwrap_or(false);
    let has_passkey = crate::passkey_rp::login_second_factor_has_passkey(
        &state,
        &user.realm_id,
        user.id,
        &headers,
        Some(client_app.id),
    )
    .await?;

    let mut second_factors = Vec::new();
    if has_totp {
        second_factors.push("totp");
    }
    if has_passkey {
        second_factors.push("passkey");
    }

    if !second_factors.is_empty() {
        let temp_token = format!("totp_login_{}", Uuid::now_v7());
        let temp_key = format!("totp:temp:{}", temp_token);
        let temp_session_data = serde_json::json!({
            "user_id": user.id,
            "realm_id": realm_id.clone(),
            "client_id": payload.client_id,
            "client_app_id": client_app.id,
            "client_ip": ip.clone(),
            "flow": "custom_user_ui",
        });
        let _: () = conn
            .set_ex(temp_key, temp_session_data.to_string(), 300) // 5 minutes
            .await
            .map_err(|_| ApiError::internal("Internal server error"))?;

        if let Err(audit_err) = state
            .audit_event_repository
            .create(NewAuditEvent {
                realm_id: realm_id.clone(),
                category: AuditCategory::Auth,
                action: AuditAction::AuthLogin,
                actor_id: user.id.to_string(),
                actor_type: Some(ActorType::User),
                actor_name: Some(user.email.clone()),
                target_type: AuditTargetType::User,
                target_id: user.id.to_string(),
                target_name: Some(user.email.clone()),
                result: AuditResult::Success,
                details: Some(serde_json::json!({
                    "method": "sms_otp",
                    "totp_required": has_totp,
                    "passkey_required": has_passkey,
                })),
                ip_address: Some(ip.clone()),
                user_agent,
                trace_id: None,
            })
            .await
        {
            tracing::warn!(error = %audit_err, "Failed to record SMS OTP login audit event");
        }

        return Ok(Json(crate::login::LoginResponse {
            message: "second factor required".to_string(),
            user_id: user.id,
            realm_id: realm_id.clone(),
            requires_totp: Some(has_totp),
            second_factors: Some(second_factors.iter().map(|f| f.to_string()).collect()),
            temp_token: Some(temp_token),
            expires_in_seconds: 300,
            redirect_to: None,
            consent_required: None,
            agreements: None,
            restricted_session: None,
        })
        .into_response());
    }

    if let Some(summaries) = crate::consent_gate::evaluate_login_consent_gate(
        &state,
        &user,
        &realm_id,
        payload.agreements.as_deref(),
        Some(ip.clone()),
        user_agent.clone(),
    )
    .await
    {
        let restricted_session = crate::consent_gate::mint_consent_restricted_session(
            &state,
            &user,
            &client_app,
            user_agent.clone(),
            Some(ip.clone()),
        )
        .await?;
        let body = serde_json::json!({
            "message": "consent required",
            "consentRequired": true,
            "agreements": summaries,
            "restrictedSession": BrowserTokenResponse::from(restricted_session),
        });
        return Ok(Json(body).into_response());
    }

    let tokens = RedisBrowserTokenService::new(state.redis_manager.clone())
        .create_token_family(&user, &client_app, user_agent.clone(), Some(ip.clone()))
        .await?;

    if let Err(audit_err) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: realm_id.clone(),
            category: AuditCategory::Auth,
            action: AuditAction::AuthLogin,
            actor_id: user.id.to_string(),
            actor_type: Some(ActorType::User),
            actor_name: Some(user.email.clone()),
            target_type: AuditTargetType::User,
            target_id: user.id.to_string(),
            target_name: Some(user.email.clone()),
            result: AuditResult::Success,
            details: Some(serde_json::json!({"method": "sms_otp"})),
            ip_address: Some(ip.clone()),
            user_agent,
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %audit_err, "Failed to record SMS OTP login audit event");
    }

    Ok(Json(BrowserTokenResponse::from(tokens)).into_response())
}

/// Public phone-login enablement flag for a Realm (login-page entry
/// visibility). Fail-closed: absent or malformed config ⇒ `{enabled:false}`.
#[utoipa::path(
    get,
    path = "/api/auth/{realmId}/phone-login/status",
    operation_id = "phone_login_status",
    tag = "auth",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        (status = 200, description = "Phone login status", body = PhoneLoginStatusResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    )
)]
pub async fn phone_login_status(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<PhoneLoginStatusResponse>, ApiError> {
    let enabled = is_phone_login_enabled(&state, &realm_id).await?;
    Ok(Json(PhoneLoginStatusResponse { enabled }))
}

/// Best-effort SMS login-failure audit event (masked identifier only).
async fn record_login_failure(
    state: &AppState,
    realm_id: &str,
    e164: &str,
    ip: &str,
    user_agent: Option<&str>,
    reason: &str,
) {
    if let Err(audit_err) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: realm_id.to_string(),
            category: AuditCategory::Auth,
            action: AuditAction::AuthLoginFailed,
            actor_id: mask_phone(e164).unwrap_or_else(|| "****".to_string()),
            actor_type: None,
            actor_name: None,
            target_type: AuditTargetType::User,
            target_id: mask_phone(e164).unwrap_or_else(|| "****".to_string()),
            target_name: None,
            result: AuditResult::Failure,
            details: Some(serde_json::json!({
                "method": "sms_otp",
                "reason": reason
            })),
            ip_address: Some(ip.to_string()),
            user_agent: user_agent.map(|s| s.to_string()),
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %audit_err, "Failed to record SMS OTP login-failed audit event");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_keys_share_the_realm_scoped_digest() {
        let code_key = login_otp_key("realm-1", "+8613800138000");
        let attempts_key = login_attempts_key("realm-1", "+8613800138000");
        assert!(code_key.starts_with("smsotp:realm-1:"));
        assert!(attempts_key.starts_with("smsotp:attempts:realm-1:"));
        let digest = phone_digest("+8613800138000");
        assert!(code_key.ends_with(&digest));
        assert_eq!(digest.len(), 64, "sha256 hex digest");
        // Realm scoping: same phone, different realm ⇒ different keys.
        assert_ne!(code_key, login_otp_key("realm-2", "+8613800138000"));
    }
}
