// Email OTP login.
//
// Two-phase unauthenticated flow: `send` issues a one-time code (or signals the
// consent/email-not-registered branches), `verify` consumes it and either logs
// an existing user in or auto-registers a new account, issuing a Bearer token
// family via `RedisBrowserTokenService`. A public `status` endpoint exposes the
// Realm enablement flag for front-end entry-point visibility.
//
// Reuses `verify_turnstile_for_client_app` and the `ConfigType::EmailOtp`
// helpers + `security_constants` OTP rates. The auto-register consent
// expression is enforced at `send` time (consent before code issuance);
// `verify` only records register-as-consent best-effort.

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
    ClientIp, is_email_otp_enabled, is_registration_enabled, load_email_otp_settings,
    normalize_email, rate_limit_hit, user_agent_from_headers, verify_turnstile_for_client_app,
};
use herald_api_base::application::http::server::api_entities::ApiError;
pub use herald_api_base::application::http::server::api_entities::ErrorResponse;
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::AuditContext;
use herald_core::domain::audit::{
    ActorType, AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType,
    NewAuditEvent,
};
use herald_core::domain::authentication::BrowserTokenService;
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::legal::{AgreementType, ConsentSource};
use herald_core::domain::security_constants::{
    OTP_CODE_TTL_SECONDS, OTP_MAX_ATTEMPTS, OTP_SEND_EMAIL_RATE_LIMIT, OTP_SEND_IP_RATE_LIMIT,
    OTP_VERIFY_EMAIL_RATE_LIMIT, OTP_VERIFY_IP_RATE_LIMIT,
};
use herald_core::domain::user::ports::{UserRepository, UserService};
use herald_core::domain::user::value_objects::CreateUserRequest;
use herald_core::domain::user_passkey::UserPasskeyRepository;
use herald_core::domain::user_totp::UserTotpRepository;
use herald_core::infrastructure::authentication::RedisBrowserTokenService;
use herald_core::infrastructure::user_passkey::PostgresUserPasskeyRepository;
use herald_core::infrastructure::user_totp::PostgresUserTotpRepository;
use herald_core::third::email::EmailService;

use crate::browser_token::BrowserTokenResponse;
use crate::consent_gate::AuthConsentAgreement;
use crate::mailflow;
use crate::passkey_rp::resolve_passkey_rp;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct EmailOtpSendRequest {
    #[validate(length(min = 1, max = 36))]
    pub client_id: String,
    #[validate(email)]
    #[validate(length(min = 3, max = 254))]
    pub email: String,
    #[serde(default)]
    #[schema(required = false)]
    pub turnstile_token: Option<String>,
    #[serde(default)]
    #[schema(required = false)]
    pub agreements: Option<Vec<AuthConsentAgreement>>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmailOtpSendResponse {
    pub message: String,
    pub expires_in_seconds: u64,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmailOtpVerifyRequest {
    pub client_id: String,
    pub email: String,
    pub code: String, // 6 ASCII digits, validated below.
    #[serde(default)]
    #[schema(required = false)]
    pub turnstile_token: Option<String>,
    #[serde(default)]
    #[schema(required = false)]
    pub agreements: Option<Vec<AuthConsentAgreement>>,
}

impl Validate for EmailOtpVerifyRequest {
    fn validate(&self) -> Result<(), validator::ValidationErrors> {
        use validator::ValidateEmail;
        let mut errors = validator::ValidationErrors::new();

        if self.client_id.is_empty() || self.client_id.len() > 36 {
            errors.add("client_id", validator::ValidationError::new("length"));
        }
        if !self.email.validate_email() {
            errors.add("email", validator::ValidationError::new("email"));
        }
        // Bound the identifier: it feeds Redis rate-limit keys
        // (`rl:otp:verify:email:{...}`), so an unbounded value is a
        // memory/keyspace DoS vector.
        if self.email.len() > 254 {
            errors.add("email", validator::ValidationError::new("length"));
        }
        // 6 ASCII digits — matches the `code` field regex ^[0-9]{6}$.
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

/// Shared body for both 409 branches. `consent_required` carries the current
/// effective agreement summaries + `consent_required=true`; `email_not_registered`
/// carries a guidance message and no agreements.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmailOtpConflictResponse {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub consent_required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub agreements: Option<Vec<herald_core::domain::legal::LegalAgreementSummary>>,
    pub message: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmailOtpStatusResponse {
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// Redis storage
// ---------------------------------------------------------------------------

/// Stored OTP payload (JSON in Redis). The code is stored as plaintext so the
/// Demo/E2E flow (which has no readable mailbox) can read it back to complete
/// the login/auto-register verification — consistent with how the password-reset
/// code is persisted in plaintext in the `email_verification_code` table. The
/// session tokens live as plaintext in the *same* Redis, so hashing only this
/// 300s one-time code would be inconsistent defense-in-depth. Failed
/// attempts are counted in a SEPARATE Redis key (see
/// [`otp_attempts_redis_key`]) incremented atomically via INCR — a GET/SET
/// counter inside this JSON would let concurrent guesses each read the same
/// snapshot and never reach `max_attempts`. `max_attempts` is snapshotted from
/// constants at write time.
#[derive(Serialize, Deserialize)]
struct StoredOtp {
    code: String,
    max_attempts: i64,
    /// Absolute expiry (epoch ms). The verify path claims the code atomically
    /// (GET+DEL) and restores it on a mismatch; this field is what lets the
    /// restore re-apply the ORIGINAL remaining TTL instead of a fresh one.
    /// `None` only for entries written before the field existed — those are
    /// never restored (a mismatch burns them; fail-closed).
    #[serde(default)]
    expires_at_ms: Option<u64>,
}

/// sha256 digest of the normalized email — shared by the code key and the
/// attempts counter key so they always address the same identity.
fn otp_email_digest(email: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(normalize_email(email).as_bytes());
    hex::encode(hasher.finalize())
}

fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// `emailotp:{realm_id}:{sha256(email trim+lowercase)}`. The email is normalized
/// (trimmed + lowercased) before hashing so casing/whitespace variations collapse
/// to the same key (and thus the same code / attempt counter).
fn otp_redis_key(realm_id: &str, email: &str) -> String {
    format!("emailotp:{realm_id}:{}", otp_email_digest(email))
}

/// Atomic INCR-backed attempt counter companion to [`otp_redis_key`]. Deleted
/// together with the code key on success/exhaustion/re-send.
fn otp_attempts_redis_key(realm_id: &str, email: &str) -> String {
    format!("emailotp:attempts:{realm_id}:{}", otp_email_digest(email))
}

fn rate_key_send(ip: &str) -> String {
    format!("rl:otp:send:ip:{ip}")
}
fn rate_key_send_email(email: &str) -> String {
    format!("rl:otp:send:email:{email}")
}
fn rate_key_verify(ip: &str) -> String {
    format!("rl:otp:verify:ip:{ip}")
}
fn rate_key_verify_email(email: &str) -> String {
    format!("rl:otp:verify:email:{email}")
}

// ---------------------------------------------------------------------------
// send handler
// ---------------------------------------------------------------------------

/// Send an email OTP login code.
///
/// Issues a one-time verification code for an existing active user, or — if the
/// Realm has OTP auto-register enabled — for an unregistered email after consent
/// is expressed. Partially enumeration-resistant: a 200 is returned for a known
/// but non-active user (no code sent). An explicit 409 is still returned for an
/// unregistered email when auto-register is off — the same registration-status
/// bit the register endpoint exposes by design (both paths sit behind Turnstile
/// and per-email/IP rate limits).
#[utoipa::path(
    post,
    path = "/api/auth/{realmId}/login/email-otp/send",
    tag = "auth",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = EmailOtpSendRequest,
    responses(
        (status = 200, description = "Verification code sent (or enumeration-resistant 200).", body = EmailOtpSendResponse),
        (status = 400, description = "OTP login not enabled for realm / bad request", body = ErrorResponse),
        (status = 401, description = "Client App disabled / Turnstile verification failed", body = ErrorResponse),
        (status = 409, description = "Consent required (auto-register) or email not registered (auto-register off)", body = EmailOtpConflictResponse),
        (status = 429, description = "Rate limited", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    )
)]
pub async fn send(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Valid(Json(payload)): Valid<Json<EmailOtpSendRequest>>,
) -> Result<Json<EmailOtpSendResponse>, ApiError> {
    // 1. Realm must have OTP login enabled. Load the settings row once and
    //    reuse the `auto_register` flag below (unregistered branch) instead of
    //    re-reading the same row.
    let otp_settings = load_email_otp_settings(&state, &realm_id).await?;
    if !otp_settings.enabled {
        return Err(ApiError::bad_request(
            "Email OTP login is not enabled for this realm".to_string(),
        ));
    }

    // 2. Resolve + validate the Client App before Turnstile (D-PROTECT-01).
    let client_app =
        mailflow::require_enabled_client(&state, &realm_id, &payload.client_id).await?;

    // 3. Client App-level Turnstile.
    verify_turnstile_for_client_app(&state, &client_app, payload.turnstile_token.as_deref(), &ip)
        .await?;

    let email = normalize_email(&payload.email);

    // 4. IP + email rate limiting.
    rate_limit_hit(
        &state,
        rate_key_send(&ip),
        OTP_SEND_IP_RATE_LIMIT.0,
        OTP_SEND_IP_RATE_LIMIT.1,
    )
    .await?;
    rate_limit_hit(
        &state,
        rate_key_send_email(&email),
        OTP_SEND_EMAIL_RATE_LIMIT.0,
        OTP_SEND_EMAIL_RATE_LIMIT.1,
    )
    .await?;

    // 5. Decide whether a real code should be sent.
    //
    // Lookup tolerates NotFound (→ unregistered branch). Any other error is a
    // genuine failure and must surface as 500 rather than a silent enumeration
    // 200 (which would mask operational problems).
    let user_result = state
        .user_repository
        .get_user_by_email(&realm_id, &email)
        .await;
    let user_opt = match user_result {
        Ok(user) => Some(user),
        Err(CoreError::NotFound) => None,
        Err(e) => {
            tracing::error!(
                realm_id = %realm_id,
                error = %e,
                "Failed to look up user for OTP send"
            );
            return Err(ApiError::internal("Internal server error".to_string()));
        }
    };

    let should_send = match user_opt.as_ref() {
        Some(user) if user.is_active() => true,
        // Existing but non-active: enumeration-resistant 200, no code sent.
        Some(_) => false,
        None => {
            // Unregistered → auto-register path.
            // Realm registration policy takes precedence over the email-otp
            // auto_register flag: auto-register must not bypass the Realm
            // registration policy; when the Realm has registration disabled,
            // an unregistered email is rejected without creating an account.
            if !is_registration_enabled(&state, &realm_id).await? {
                tracing::debug!(
                    realm_id = %realm_id,
                    "Email OTP auto-register blocked: registration not enabled for realm"
                );
                return Err(ApiError::conflict_json(EmailOtpConflictResponse {
                    code: "email_not_registered".to_string(),
                    consent_required: None,
                    agreements: None,
                    message: "This email is not registered. Please sign up first.".to_string(),
                }));
            }
            if !otp_settings.auto_register {
                return Err(ApiError::conflict_json(EmailOtpConflictResponse {
                    code: "email_not_registered".to_string(),
                    consent_required: None,
                    agreements: None,
                    message: "This email is not registered. Please sign up first.".to_string(),
                }));
            }
            // Auto-register requires consent expression BEFORE code issuance
            // (D-CONSENT-01). Missing agreements → 409 with current effective
            // summaries so the front-end can render the consent checkboxes.
            if payload.agreements.as_deref().is_none_or(|a| a.is_empty()) {
                let summaries = current_effective_summaries(&state, &realm_id).await;
                return Err(ApiError::conflict_json(EmailOtpConflictResponse {
                    code: "consent_required".to_string(),
                    consent_required: Some(true),
                    agreements: Some(summaries),
                    message: "Consent to the current agreements is required.".to_string(),
                }));
            }
            true
        }
    };

    if !should_send {
        // Enumeration-resistant success: looks identical to a real send.
        return Ok(Json(EmailOtpSendResponse {
            message: "验证码已发送".to_string(),
            expires_in_seconds: OTP_CODE_TTL_SECONDS,
        }));
    }

    // 6. Generate 6-digit code and store it with TTL.
    let code = generate_otp_code();
    let stored = StoredOtp {
        code: code.clone(),
        max_attempts: OTP_MAX_ATTEMPTS,
        expires_at_ms: Some(now_epoch_ms() + OTP_CODE_TTL_SECONDS * 1000),
    };
    let stored_json = serde_json::to_string(&stored)
        .map_err(|_| ApiError::internal("Failed to serialize OTP state".to_string()))?;
    let key = otp_redis_key(&realm_id, &email);
    let attempts_key = otp_attempts_redis_key(&realm_id, &email);
    {
        let mut conn = state
            .redis_manager
            .get()
            .await
            .map_err(|_| ApiError::internal("Redis connection error".to_string()))?;
        let _: () = conn
            .set_ex(&key, stored_json, OTP_CODE_TTL_SECONDS)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to store OTP code in Redis");
                ApiError::internal("Redis operation error".to_string())
            })?;
        // Fresh code → fresh attempt counter (any leftover counter from the
        // previous code must not count against this one).
        let _: () = conn.del(&attempts_key).await.map_err(|e| {
            tracing::error!(error = %e, "Failed to reset OTP attempt counter");
            ApiError::internal("Redis operation error".to_string())
        })?;
    }

    // 7. Send the email (inline body; no template-engine change).
    let brand = realm_brand_name(&state, &realm_id).await;
    let subject = format!("{brand} 登录验证码");
    let text = format!("您的 {brand} 验证码：{code}");
    let html = format!("<p>您的 <strong>{brand}</strong> 验证码：<strong>{code}</strong></p>");
    // Best-effort: send failure is observable but must not leak code state — the
    // code is already stored in Redis. We surface the error as 500 so it is
    // not silently lost.
    EmailService::send_email(&state.pool, &realm_id, &email, &subject, &text, &html)
        .await
        .map_err(|e| {
            tracing::error!(
                realm_id = %realm_id,
                error = %e,
                "Failed to send OTP email"
            );
            ApiError::internal("Failed to send verification email".to_string())
        })?;

    tracing::info!(
        realm_id = %realm_id,
        is_new_user = user_opt.is_none(),
        "OTP code sent"
    );

    Ok(Json(EmailOtpSendResponse {
        message: "验证码已发送".to_string(),
        expires_in_seconds: OTP_CODE_TTL_SECONDS,
    }))
}

// ---------------------------------------------------------------------------
// verify handler
// ---------------------------------------------------------------------------

/// Verify an email OTP login code and issue a session.
///
/// On a matching code: consumes it (one-time), then either logs the existing
/// user in (login-as-consent gate) or auto-registers a new account
/// (create-without-password → activate → register-as-consent) and issues a
/// Bearer token family via `RedisBrowserTokenService`. Returns `BrowserTokenResponse`.
#[utoipa::path(
    post,
    path = "/api/auth/{realmId}/login/email-otp/verify",
    tag = "auth",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = EmailOtpVerifyRequest,
    responses(
        (status = 200, description = "Verification successful", body = BrowserTokenResponse),
        (status = 400, description = "OTP login not enabled for realm / bad request", body = ErrorResponse),
        (status = 401, description = "Invalid / expired / exhausted code, or disabled account", body = ErrorResponse),
        (status = 429, description = "Rate limited", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    )
)]
pub async fn verify(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Valid(Json(payload)): Valid<Json<EmailOtpVerifyRequest>>,
) -> Result<impl IntoResponse, ApiError> {
    let user_agent = user_agent_from_headers(&headers);

    // 1. Realm must have OTP login enabled. Load the settings row once and
    //    reuse the `auto_register` flag below (auto-register branch) instead
    //    of re-reading the same row — mirrors `send`.
    let otp_settings = load_email_otp_settings(&state, &realm_id).await?;
    if !otp_settings.enabled {
        return Err(ApiError::bad_request(
            "Email OTP login is not enabled for this realm".to_string(),
        ));
    }

    let email = normalize_email(&payload.email);

    // 2. IP + email rate limiting.
    rate_limit_hit(
        &state,
        rate_key_verify(&ip),
        OTP_VERIFY_IP_RATE_LIMIT.0,
        OTP_VERIFY_IP_RATE_LIMIT.1,
    )
    .await?;
    rate_limit_hit(
        &state,
        rate_key_verify_email(&email),
        OTP_VERIFY_EMAIL_RATE_LIMIT.0,
        OTP_VERIFY_EMAIL_RATE_LIMIT.1,
    )
    .await?;

    // 3. Resolve + validate the Client App (carries the session family binding).
    let client_app =
        mailflow::require_enabled_client(&state, &realm_id, &payload.client_id).await?;

    // 4. Client App-level Turnstile.
    verify_turnstile_for_client_app(&state, &client_app, payload.turnstile_token.as_deref(), &ip)
        .await?;

    // 5. Atomically CLAIM the code: GET+DEL inside MULTI/EXEC. The key is
    //    absent for the whole compare, so two concurrent requests presenting
    //    the same (stolen) code cannot both read it — exactly one claim
    //    succeeds. On a mismatch the code is restored below (unless attempts
    //    ran out), preserving the multi-attempt contract.
    let key = otp_redis_key(&realm_id, &email);
    let attempts_key = otp_attempts_redis_key(&realm_id, &email);
    let mut conn = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Redis connection error".to_string()))?;
    let (stored_raw, _deleted): (Option<String>, i64) = redis::pipe()
        .atomic()
        .cmd("GET")
        .arg(&key)
        .cmd("DEL")
        .arg(&key)
        .query_async(&mut conn)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to claim OTP code from Redis");
            ApiError::internal("Redis operation error".to_string())
        })?;
    let stored_json =
        stored_raw.ok_or_else(|| ApiError::unauthorized("验证码已失效，请重新发送".to_string()))?;
    let stored: StoredOtp = serde_json::from_str(&stored_json).map_err(|e| {
        tracing::error!(error = %e, "Failed to parse stored OTP JSON");
        ApiError::internal("Redis operation error".to_string())
    })?;

    // 6. Constant-time compare of the plaintext code.
    let matches = constant_time_eq(&payload.code, &stored.code);

    if !matches {
        // Atomic INCR on the dedicated counter key. A GET/SET rewrite of a
        // counter stored inside the code JSON would let concurrent guesses
        // each read the same snapshot, undercount failures, and effectively
        // multiply the brute-force budget beyond max_attempts.
        let attempts: i64 = conn.incr(&attempts_key, 1).await.map_err(|e| {
            tracing::error!(error = %e, "Failed to increment OTP attempt counter");
            ApiError::internal("Redis operation error".to_string())
        })?;
        if attempts == 1 {
            // First failure: carry over the code's remaining TTL so the
            // counter cannot outlive the code it guards. TTL can return
            // -1 (no expiry) / -2 (no key — the claim already deleted the
            // code, so compute from the stored absolute expiry instead);
            // skip when neither is available.
            let remaining_ttl: i64 = conn.ttl(&key).await.unwrap_or(-2);
            let remaining_secs = if remaining_ttl > 0 {
                Some(remaining_ttl)
            } else {
                stored.expires_at_ms.map(|expires_at_ms| {
                    ((expires_at_ms.saturating_sub(now_epoch_ms())) / 1000) as i64
                })
            };
            if let Some(remaining_secs) = remaining_secs.filter(|secs| *secs > 0) {
                let _: () = conn
                    .expire(&attempts_key, remaining_secs)
                    .await
                    .map_err(|e| {
                        tracing::error!(error = %e, "Failed to set OTP attempt counter TTL");
                        ApiError::internal("Redis operation error".to_string())
                    })?;
            }
        }
        if attempts >= stored.max_attempts {
            // Exhausted → the claim already deleted the code; drop the
            // counter too (force re-send) and refuse to restore.
            let _: () = conn.del(&attempts_key).await.map_err(|e| {
                tracing::error!(error = %e, "Failed to delete exhausted OTP attempt counter");
                ApiError::internal("Redis operation error".to_string())
            })?;
        } else {
            // Not exhausted → restore the claimed code so the remaining
            // attempts still work. NX: if a NEWER code was sent while we held
            // the claim, the old code must not clobber it. Best-effort: a
            // restore failure burns the code (fail-closed), not the security.
            if let Some(expires_at_ms) = stored.expires_at_ms {
                let remaining_ms = expires_at_ms.saturating_sub(now_epoch_ms());
                if remaining_ms > 0 {
                    let restore: Result<(), _> = redis::pipe()
                        .atomic()
                        .cmd("SET")
                        .arg(&key)
                        .arg(&stored_json)
                        .arg("NX")
                        .arg("PX")
                        .arg(remaining_ms)
                        .query_async(&mut conn)
                        .await;
                    if let Err(e) = restore {
                        tracing::warn!(error = %e, "Failed to restore claimed OTP code");
                    }
                }
            }
        }
        record_login_failure(
            &state,
            &realm_id,
            &email,
            &ip,
            user_agent.as_deref(),
            "invalid_code",
        )
        .await;
        return Err(ApiError::unauthorized("验证码错误或已失效".to_string()));
    }

    // 7. Match → the claim already consumed the code; drop the counter and
    //    proceed to user resolution / token issuance.
    let _: () = conn.del(&attempts_key).await.map_err(|e| {
        tracing::error!(error = %e, "Failed to consume OTP attempt counter");
        ApiError::internal("Redis operation error".to_string())
    })?;

    // 8. Resolve the user: existing (login) or auto-register.
    let user_result = state
        .user_repository
        .get_user_by_email(&realm_id, &email)
        .await;
    let user = match user_result {
        Ok(u) => u,
        Err(CoreError::NotFound) => {
            // Auto-register policy gates must hold at consumption time too.
            // The code was issued for an account that existed at `send`; if
            // that account was deleted within the code's TTL, a realm that
            // has since disabled registration (or auto-register) must not
            // mint a fresh account from the stale code. Mirrors the send-side
            // gates and the OAuth find_or_create consumption-time check.
            if !is_registration_enabled(&state, &realm_id).await? || !otp_settings.auto_register {
                tracing::warn!(
                    realm_id = %realm_id,
                    "OTP verify auto-register blocked: registration or auto-register no longer enabled"
                );
                return Err(ApiError::conflict_json(EmailOtpConflictResponse {
                    code: "email_not_registered".to_string(),
                    consent_required: None,
                    agreements: None,
                    message: "This email is not registered. Please sign up first.".to_string(),
                }));
            }
            // Auto-register path. create_user_without_password starts in
            // WaitVerified (status 0); activate_user flips it to Normal before
            // create_token_family (which requires an active user).
            let created = state
                .service
                .user_service()
                .create_user_without_password(CreateUserRequest {
                    realm_id: realm_id.clone(),
                    email: email.clone(),
                    password: None,
                    provider_ids: None,
                })
                .await
                .map_err(|e| {
                    tracing::error!(
                        realm_id = %realm_id,
                        error = %e,
                        "OTP auto-register: create_user_without_password failed"
                    );
                    match e {
                        CoreError::Conflict(msg) => ApiError::conflict(msg),
                        _ => ApiError::internal("Failed to create user".to_string()),
                    }
                })?;
            state
                .service
                .user_service()
                .activate_user(created.id)
                .await
                .map_err(|e| {
                    tracing::error!(
                        user_id = %created.id,
                        error = %e,
                        "OTP auto-register: activate_user failed"
                    );
                    ApiError::internal("Failed to activate user".to_string())
                })?;
            // Register-as-consent (best-effort, mirrors register.rs:224-285).
            record_register_consent(
                &state,
                created.id,
                &realm_id,
                &email,
                payload.agreements.as_deref(),
                &ip,
            )
            .await;

            // JIT account creation is a registration for points purposes
            // (mirrors register.rs; idempotent on `registration:{user_id}`).
            if let Err(e) = state
                .registration_service
                .handle_user_registration(created.id, &realm_id)
                .await
            {
                tracing::error!(
                    realm_id = %realm_id,
                    user_id = %created.id,
                    error = %e,
                    "OTP auto-register: failed to grant registration points"
                );
            }

            state
                .user_repository
                .get_user_by_id(created.id)
                .await
                .map_err(|e| {
                    tracing::error!(
                        user_id = %created.id,
                        error = %e,
                        "OTP auto-register: failed to reload created user"
                    );
                    ApiError::internal("Internal server error".to_string())
                })?
        }
        Err(e) => {
            tracing::error!(
                realm_id = %realm_id,
                error = %e,
                "Failed to look up user for OTP verify"
            );
            return Err(ApiError::internal("Internal server error".to_string()));
        }
    };

    // 9. Disabled accounts cannot log in (auto-register always yields active).
    if !user.is_active() {
        record_login_failure(
            &state,
            &realm_id,
            &email,
            &ip,
            user_agent.as_deref(),
            "disabled_account",
        )
        .await;
        return Err(ApiError::unauthorized("账号已被禁用".to_string()));
    }

    // 9.5 Second-factor gate (PRD email-otp-login.md §4.1: OTP login must not
    //     bypass an existing TOTP/passkey second factor). Mirrors login.rs:
    //     a user with an enabled TOTP config or a passkey for the resolved RP
    //     gets a 5-minute temp session instead of tokens; verify-totp /
    //     verify-passkey completes the login and re-runs the consent gate.
    let totp_repo = PostgresUserTotpRepository::new(state.db.clone());
    let has_totp = totp_repo
        .get_config_by_user_id(user.id)
        .await?
        .map(|config| config.enabled)
        .unwrap_or(false);

    // Best-effort probe identical to the password-login one: a passkey RP
    // resolution failure (e.g. no RP configured for this realm) means "no
    // passkey second factor", never a 500 on the OTP path.
    let passkey_repo = PostgresUserPasskeyRepository::new(state.db.clone());
    let has_passkey = match resolve_passkey_rp(
        &state,
        &user.realm_id,
        &headers,
        Some(client_app.id),
    )
    .await
    {
        Ok(relying_party) => !passkey_repo
            .list_by_user_and_rp(&user.realm_id, user.id, &relying_party.id)
            .await?
            .is_empty(),
        Err(error) => {
            tracing::debug!(
                user_id = %user.id,
                realm_id = %user.realm_id,
                error = %error,
                "Passkey RP resolution failed during OTP login second-factor probe; passkey will not be offered"
            );
            false
        }
    };

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
            .map_err(|_| ApiError::internal("Internal server error".to_string()))?;

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
                    "method": "email_otp",
                    "totp_required": has_totp,
                    "passkey_required": has_passkey,
                })),
                ip_address: Some(ip.clone()),
                user_agent,
                trace_id: None,
            })
            .await
        {
            tracing::warn!(error = %audit_err, "Failed to record OTP login audit event");
        }

        // Same multi-branch 200 contract as password login — build the typed
        // LoginResponse (ldap_login.rs does the same) so the camelCase shape
        // stays locked to the password-login contract; the SDK maps
        // `secondFactors` to its requires-second-factor branch.
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
        })
        .into_response());
    }

    // 10. Login-as-consent gate for existing users. `Some` ⇒ 200 consent_required,
    // no token issued (mirrors login.rs:433 / verify_totp.rs:353).
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
        // Build a consent-required 200 body reusing BrowserTokenResponse-like
        // fields is not appropriate; reuse VerifyTotpResponse-style JSON shape
        // inline to avoid coupling a token DTO to a non-token response.
        let body = serde_json::json!({
            "message": "consent required",
            "consentRequired": true,
            "agreements": summaries,
        });
        return Ok(Json(body).into_response());
    }

    // 11. Issue token family + audit success.
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
            details: Some(serde_json::json!({"method": "email_otp"})),
            ip_address: Some(ip.clone()),
            user_agent,
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %audit_err, "Failed to record OTP login audit event");
    }

    Ok(Json(BrowserTokenResponse::from(tokens)).into_response())
}

// ---------------------------------------------------------------------------
// status handler
// ---------------------------------------------------------------------------

/// Public OTP-login enablement flag for a Realm.
///
/// Reads the `email_otp` / `settings` config row. Returns `{ enabled: false }`
/// when the config is absent or malformed (opt-in per Realm).
#[utoipa::path(
    get,
    path = "/api/auth/{realmId}/email-otp/status",
    tag = "auth",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        (status = 200, description = "OTP status", body = EmailOtpStatusResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    )
)]
pub async fn status(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<EmailOtpStatusResponse>, ApiError> {
    let enabled = is_email_otp_enabled(&state, &realm_id).await?;
    Ok(Json(EmailOtpStatusResponse { enabled }))
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn generate_otp_code() -> String {
    // 6-digit zero-padded code derived from the random tail of a UUIDv7 (the
    // 74 bits of UUIDv7 randomness are more than enough to sample a 6-digit
    // space). This avoids adding a new RNG dependency to the crate while still
    // being unpredictable.
    let id = Uuid::now_v7();
    let bytes = id.as_bytes();
    // Use the trailing 8 bytes (the random node-id segment of UUIDv7) as a
    // little-endian u64, then fold down to 0..1_000_000.
    let tail = u64::from_le_bytes([
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    ]);
    let n = (tail % 1_000_000) as u32;
    format!("{n:06}")
}

/// Constant-time byte comparison so timing does not leak hash prefix info.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a_bytes, b_bytes) = (a.as_bytes(), b.as_bytes());
    if a_bytes.len() != b_bytes.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a_bytes.iter().zip(b_bytes.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Collect current effective ToS + Privacy summaries for the consent-required
/// 409 body. Missing effective versions (seed anomaly) are skipped.
async fn current_effective_summaries(
    state: &AppState,
    realm_id: &str,
) -> Vec<herald_core::domain::legal::LegalAgreementSummary> {
    let mut summaries = Vec::new();
    for agreement_type in [AgreementType::TermsOfService, AgreementType::PrivacyPolicy] {
        match state
            .legal_service
            .current_effective(realm_id, agreement_type.clone())
            .await
        {
            Ok(Some(version)) => {
                summaries.push(herald_core::domain::legal::LegalAgreementSummary {
                    agreement_type: agreement_type.as_str().to_string(),
                    version_id: version.id,
                    version_no: version.version_no,
                    effective_at: version.published_at,
                    title: None,
                    summary: None,
                    mode: version.mode,
                    external_url: version.external_url,
                })
            }
            Ok(None) => tracing::warn!(
                realm_id = %realm_id,
                agreement_type = %agreement_type.as_ref(),
                "No effective agreement version deployed; skipping from OTP consent list"
            ),
            Err(e) => tracing::warn!(
                realm_id = %realm_id,
                agreement_type = %agreement_type.as_ref(),
                error = %e,
                "current_effective lookup failed during OTP send"
            ),
        }
    }
    summaries
}

/// Best-effort register-as-consent recording (mirrors register.rs:224-285).
/// Failures are logged and do NOT block account creation / login.
async fn record_register_consent(
    state: &AppState,
    user_id: Uuid,
    realm_id: &str,
    email: &str,
    agreements: Option<&[AuthConsentAgreement]>,
    ip: &str,
) {
    // Prefer explicitly expressed agreements if provided; otherwise record the
    // current effective ToS + Privacy (the send-phase consent gate guarantees
    // expression happened, but the verifier payload may omit them on retry).
    let mut items: Vec<(AgreementType, Uuid)> = Vec::new();
    if let Some(agreements) = agreements
        && !agreements.is_empty()
    {
        for item in agreements {
            let Ok(agreement_type) = AgreementType::try_from(item.agreement_type.as_str()) else {
                tracing::warn!(
                    user_id = %user_id,
                    realm_id = %realm_id,
                    agreement_type = %item.agreement_type,
                    "Invalid agreement type in OTP register-consent payload"
                );
                continue;
            };
            items.push((agreement_type, item.version_id));
        }
    } else {
        for agreement_type in [AgreementType::TermsOfService, AgreementType::PrivacyPolicy] {
            match state
                .legal_service
                .current_effective(realm_id, agreement_type.clone())
                .await
            {
                Ok(Some(version)) => items.push((agreement_type, version.id)),
                Ok(None) => tracing::warn!(
                    realm_id = %realm_id,
                    agreement_type = %agreement_type.as_ref(),
                    user_id = %user_id,
                    "No effective agreement version; skipping OTP register-consent"
                ),
                Err(e) => tracing::warn!(
                    realm_id = %realm_id,
                    agreement_type = %agreement_type.as_ref(),
                    user_id = %user_id,
                    error = %e,
                    "current_effective failed during OTP register-consent"
                ),
            }
        }
    }

    if items.is_empty() {
        return;
    }

    let actor_meta = AuditContext {
        actor_id: user_id.to_string(),
        actor_type: Some(ActorType::User),
        actor_name: Some(email.to_string()),
        ip_address: Some(ip.to_string()),
        user_agent: None,
        trace_id: None,
    };
    if let Err(e) = state
        .legal_service
        .record_consent(
            user_id,
            realm_id,
            items,
            ConsentSource::Register,
            actor_meta,
        )
        .await
    {
        tracing::warn!(
            realm_id = %realm_id,
            user_id = %user_id,
            error = %e,
            "record_consent(Register) failed during OTP auto-register; login proceeds"
        );
    }
}

/// Best-effort login-failure audit event.
async fn record_login_failure(
    state: &AppState,
    realm_id: &str,
    email: &str,
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
            actor_id: email.to_string(),
            actor_type: None,
            actor_name: None,
            target_type: AuditTargetType::User,
            target_id: email.to_string(),
            target_name: None,
            result: AuditResult::Failure,
            details: Some(serde_json::json!({
                "method": "email_otp",
                "reason": reason
            })),
            ip_address: Some(ip.to_string()),
            user_agent: user_agent.map(|s| s.to_string()),
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %audit_err, "Failed to record OTP login-failed audit event");
    }
}

/// Resolve the realm brand name for the OTP email body via the shared helper
/// in `core::third::email`. Failures degrade to the default rather than
/// failing the request.
async fn realm_brand_name(state: &AppState, realm_id: &str) -> String {
    herald_core::third::email::resolve_realm_brand(&state.pool, realm_id)
        .await
        .unwrap_or_else(|_| "Herald".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn otp_redis_key_normalizes_case_and_whitespace() {
        // Same logical email must collapse to one key so casing/whitespace
        // variations cannot fragment the code store or the attempt counter.
        let upper = otp_redis_key("realm-1", "  Alice@Example.COM ");
        let lower = otp_redis_key("realm-1", "alice@example.com");
        assert_eq!(upper, lower);
        assert!(
            upper.starts_with("emailotp:realm-1:"),
            "key must keep the realm prefix; got {upper}"
        );
        // SHA-256 hex digest is 64 chars after the prefix.
        let digest = upper.strip_prefix("emailotp:realm-1:").unwrap();
        assert_eq!(digest.len(), 64, "expected sha256 hex digest");
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn otp_redis_key_is_realm_scoped() {
        let a = otp_redis_key("realm-a", "alice@example.com");
        let b = otp_redis_key("realm-b", "alice@example.com");
        assert_ne!(a, b, "different realms must not share OTP keys");
    }

    #[test]
    fn otp_attempts_key_shares_digest_with_code_key() {
        // The counter key must address the same (realm, normalized email) as
        // the code key so send/verify keep them in lockstep.
        let code_key = otp_redis_key("realm-1", "  Alice@Example.COM ");
        let attempts_key = otp_attempts_redis_key("realm-1", "alice@example.com");
        assert_eq!(
            code_key.trim_start_matches("emailotp:realm-1:"),
            attempts_key.trim_start_matches("emailotp:attempts:realm-1:")
        );
    }

    #[test]
    fn stored_otp_round_trips_plaintext_code() {
        // The code is persisted as plaintext (not hashed) so the Demo/E2E flow
        // can read it back from Redis. Serialization must preserve the exact
        // code and the attempt limit unchanged (attempt counting itself lives
        // in the dedicated INCR key).
        let stored = StoredOtp {
            code: "123456".to_string(),
            max_attempts: OTP_MAX_ATTEMPTS,
            expires_at_ms: Some(1_700_000_000_000),
        };
        let json = serde_json::to_string(&stored).unwrap();
        assert!(
            json.contains("\"code\":\"123456\""),
            "stored JSON must carry the plaintext code; got {json}"
        );
        let back: StoredOtp = serde_json::from_str(&json).unwrap();
        assert_eq!(back.code, "123456");
        assert_eq!(back.max_attempts, OTP_MAX_ATTEMPTS);
        assert_eq!(back.expires_at_ms, Some(1_700_000_000_000));
        // Entries written before the field existed must still deserialize
        // (restore is skipped for them — fail-closed, never a parse error).
        let legacy: StoredOtp =
            serde_json::from_str(r#"{"code":"654321","max_attempts":5}"#).unwrap();
        assert_eq!(legacy.expires_at_ms, None);
    }

    #[test]
    fn constant_time_eq_matches_equality() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
        assert!(constant_time_eq("", ""));
    }
}
