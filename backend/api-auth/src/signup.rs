use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use axum_valid::Valid;
use herald_api_base::application::http::auth::util::{
    ClientIp, is_email_configured, is_email_verification_required, is_platform_signup_enabled,
    normalize_email, rate_limit_hit, user_agent_from_headers, verify_turnstile_for_client_app,
};
pub use herald_api_base::application::http::server::api_entities::ErrorResponse;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::{
    ActorType, AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType,
    NewAuditEvent,
};
use herald_core::domain::authentication::BrowserTokenService;
use herald_core::domain::client::{ADMIN_WEB_CONSOLE_CLIENT_ID, ports::ClientService};
use herald_core::domain::realm::{
    ADMIN_REALM_ID, CreateRealmRequest, InitialAdminUser, RealmService,
};
use herald_core::domain::security_constants::{
    EMAIL_VERIFICATION_CODE_TTL_SECONDS, SIGNUP_CODE_REQUEST_EMAIL_RATE_LIMIT,
    SIGNUP_CODE_REQUEST_IP_RATE_LIMIT, SIGNUP_IP_RATE_LIMIT,
};
use herald_core::domain::user::ports::UserRepository;
use herald_core::infrastructure::authentication::RedisBrowserTokenService;
use herald_core::third::email::EmailService;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use crate::mailflow;

/// Public self-service realm provisioning request.
///
/// `realm_slug` is optional: when omitted the backend assigns a UUID v7 id.
/// `turnstile_token` is only required when the admin realm's `admin-web-console`
/// Client App has Turnstile enabled.
#[derive(Debug, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct SignupRequest {
    #[validate(length(min = 3, max = 50))]
    pub realm_name: String,
    #[validate(length(min = 3, max = 36))]
    pub realm_slug: Option<String>,
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 8, max = 100))]
    pub password: String,
    pub turnstile_token: Option<String>,
    /// 6-digit mailbox verification code, required only when the admin
    /// realm's registration config turns on email verification for signup.
    #[validate(custom(function = "validate_email_verification_code"))]
    pub email_verification_code: Option<String>,
}

/// Signup verification codes are exactly six ASCII digits — the zero-padded
/// format the issuance helper generates; anything else can never match a
/// stored row and is rejected before any code is consumed.
fn validate_email_verification_code(value: &str) -> Result<(), validator::ValidationError> {
    if value.len() == 6 && value.chars().all(|c| c.is_ascii_digit()) {
        Ok(())
    } else {
        Err(validator::ValidationError::new(
            "email verification code must be 6 digits",
        ))
    }
}

/// Tokens for the freshly provisioned realm, plus enough context for the
/// frontend to switch its routing into the new realm's admin console.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SignupResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
    pub refresh_expires_in: u64,
    pub token_type: String,
    pub realm_id: String,
    pub realm_name: String,
}

/// Public visibility of the platform self-service entry.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SignupStatusResponse {
    pub enabled: bool,
    /// Whether the signup flow must present a mailbox verification code
    /// (admin realm registration config; false when no mail channel is
    /// configured, matching the signup-side check).
    pub email_verification_required: bool,
}

/// Request to email a self-service signup verification code.
#[derive(Debug, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct SignupEmailCodeRequest {
    #[validate(email)]
    pub email: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SignupEmailCodeResponse {
    pub message: String,
}

/// Provision a new realm from an unauthenticated visitor and issue an
/// immediate first-party admin-console session for the new realm admin.
///
/// Only the admin realm hosts this entry; any other `realmId` is rejected.
/// Pre-flight defenses run before any realm is created: the platform toggle,
/// human verification bound to the admin realm's `admin-web-console` Client
/// App, and a same-IP 24h quota.
#[utoipa::path(
    post,
    path = "/api/auth/{realmId}/signup",
    tag = "auth",
    params(
        ("realmId" = String, Path, description = "Must be \"admin\"")
    ),
    request_body = SignupRequest,
    responses(
        (status = 200, description = "Realm provisioned and session issued.", body = SignupResponse),
        (status = 400, description = "Validation failed, or realm identifier already exists.", body = ErrorResponse),
        (status = 403, description = "Self-service signup is disabled.", body = ErrorResponse),
        (status = 404, description = "Only the admin realm hosts signup.", body = ErrorResponse),
        (status = 429, description = "Same-IP signup quota reached.", body = ErrorResponse),
        (status = 500, description = "Internal server error.", body = ErrorResponse)
    )
)]
#[tracing::instrument(
    // Governance: payload carries password (credential), turnstile_token, email
    // (PII); realm_id is low-cardinality; ip is client PII. Only the operation
    // type is recorded.
    skip(state, payload, realm_id, ip),
    fields(db.system = "postgres", db.operation = "signup")
)]
pub async fn signup(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Valid(Json(payload)): Valid<Json<SignupRequest>>,
) -> Result<ApiResult<SignupResponse>, ApiError> {
    // The platform entry is hosted exclusively by the admin realm.
    if realm_id != ADMIN_REALM_ID {
        return Err(ApiError::not_found("Not found"));
    }

    let user_agent = user_agent_from_headers(&headers);
    // Same normalization (trim + lowercase) as every other account entrance:
    // the account email unique index is case-sensitive and login normalizes
    // its lookup, so a mixed-case signup email would create an admin that can
    // never sign in again.
    let email = normalize_email(&payload.email);

    // 1. Platform toggle — fail-closed when unset.
    if !is_platform_signup_enabled(&state).await? {
        tracing::info!("Self-service signup rejected: platform toggle disabled");
        return Err(ApiError::forbidden(
            "Self-service signup is disabled".to_string(),
        ));
    }

    // 2. Resolve the admin-web-console Client App and enforce Turnstile per its config.
    let client_app =
        mailflow::require_enabled_client(&state, ADMIN_REALM_ID, ADMIN_WEB_CONSOLE_CLIENT_ID)
            .await?;
    verify_turnstile_for_client_app(&state, &client_app, payload.turnstile_token.as_deref(), &ip)
        .await?;

    // 3. Same-IP 24h quota. Counted here (after validation + human verification,
    //    before create_realm) and not rolled back on provisioning failure.
    rate_limit_hit(
        &state,
        format!("rl:signup:ip:{ip}"),
        SIGNUP_IP_RATE_LIMIT.0,
        SIGNUP_IP_RATE_LIMIT.1,
    )
    .await?;

    // 4. Mailbox verification, when the admin realm's registration config
    //    requires it (reuses the register-facing helper, including its
    //    fail-open-when-no-mail-channel semantics). The code is consumed
    //    here — atomically and single-use — BEFORE the realm is provisioned:
    //    a verified signup then proceeds straight to a Normal admin plus an
    //    immediate session. Consumption must stay below Turnstile and the IP
    //    quota above so their failures never burn the code.
    if is_email_verification_required(&state, ADMIN_REALM_ID).await? {
        let code = payload.email_verification_code.as_deref().ok_or_else(|| {
            ApiError::bad_request("email verification code is required".to_string())
        })?;
        consume_signup_email_code(&state, &email, code).await?;
    }

    // 5. Provision the realm via the policy-free self-service entry. The
    //    admin/ext create_realm paths keep their own permission gates untouched.
    let request = CreateRealmRequest {
        id: payload.realm_slug.filter(|s| !s.trim().is_empty()),
        name: payload.realm_name,
        description: None,
        admin_user: InitialAdminUser {
            email: email.clone(),
            password: payload.password,
        },
    };
    let audit_ctx = herald_core::domain::audit::AuditContext {
        actor_id: "platform-signup".to_string(),
        actor_type: Some(ActorType::System),
        actor_name: Some(email.clone()),
        ip_address: Some(ip.clone()),
        user_agent: user_agent.clone(),
        trace_id: None,
    };
    let realm = state
        .service
        .realm_service()
        .create_realm_self_service(request, ADMIN_REALM_ID.to_string(), audit_ctx)
        .await?;

    // The provisioning service creates the admin user but does not return it
    // (the repository's Realm.admin_user is always None). Look the new admin up
    // by email within the freshly created realm to issue their session.
    let admin_user = state
        .user_repository
        .get_user_by_email(&realm.id, &email)
        .await
        .map_err(|_| ApiError::internal("New realm admin user not found after provisioning"))?;
    let console = state
        .service
        .client_service()
        .get_client_app_by_client_id(&realm.id, ADMIN_WEB_CONSOLE_CLIENT_ID)
        .await
        .map_err(|_| ApiError::internal("New realm admin-web-console client missing"))?;

    // 6. Issue a first-party admin-console session bound to the NEW realm.
    let tokens = RedisBrowserTokenService::new(state.redis_manager.clone())
        .create_first_party_token_family(&admin_user, &console, user_agent, Some(ip.clone()))
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to issue signup session");
            ApiError::internal("Failed to issue session")
        })?;

    // 7. Record consent to the effective ToS + Privacy for the new realm
    //    admin ("signup = consent"), mirroring the register entrance.
    crate::consent_gate::record_register_consent(
        &state,
        admin_user.id,
        &realm.id,
        &email,
        Some(&ip),
    )
    .await;

    // Best-effort platform audit record (does not block the response).
    if let Err(e) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: ADMIN_REALM_ID.to_string(),
            category: AuditCategory::RealmManagement,
            action: AuditAction::RealmCreate,
            actor_id: admin_user.id.to_string(),
            actor_type: Some(ActorType::System),
            actor_name: Some(email),
            target_type: AuditTargetType::Realm,
            target_id: realm.id.clone(),
            target_name: Some(realm.name.clone()),
            result: AuditResult::Success,
            details: Some(serde_json::json!({
                "source": "platform_signup",
                "realm_id": realm.id,
                "realm_name": realm.name,
            })),
            ip_address: Some(ip),
            user_agent: None,
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %e, "Failed to record platform signup audit event");
    }

    Ok(ApiResult::ok(SignupResponse {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        expires_in: tokens.expires_in,
        refresh_expires_in: tokens.refresh_expires_in,
        token_type: tokens.token_type,
        realm_id: realm.id.clone(),
        realm_name: realm.name.clone(),
    }))
}

/// Email the 6-digit verification code the signup flow will later consume.
///
/// Only the admin realm hosts this entry; any other `realmId` is rejected.
/// No Turnstile here (same posture as the other code-only senders — the
/// per-IP + per-mailbox rate limits carry the anti-bombing defense), and the
/// platform toggle gates it fail-closed so the code sender can never outlive
/// the signup entry it serves.
#[utoipa::path(
    post,
    path = "/api/auth/{realmId}/signup/email_code",
    tag = "auth",
    params(
        ("realmId" = String, Path, description = "Must be \"admin\"")
    ),
    request_body = SignupEmailCodeRequest,
    responses(
        (status = 200, description = "Verification code emailed.", body = SignupEmailCodeResponse),
        (status = 400, description = "Email channel not configured for the admin realm.", body = ErrorResponse),
        (status = 403, description = "Self-service signup is disabled.", body = ErrorResponse),
        (status = 404, description = "Only the admin realm hosts signup.", body = ErrorResponse),
        (status = 429, description = "Code request rate limit reached.", body = ErrorResponse),
        (status = 500, description = "Internal server error.", body = ErrorResponse)
    )
)]
#[tracing::instrument(
    // Governance: payload carries email (PII); realm_id is low-cardinality;
    // ip is client PII. Only the operation type is recorded.
    skip(state, payload, realm_id, ip),
    fields(db.system = "postgres", db.operation = "signup_email_code")
)]
pub async fn send_email_code(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Valid(Json(payload)): Valid<Json<SignupEmailCodeRequest>>,
) -> Result<ApiResult<SignupEmailCodeResponse>, ApiError> {
    // The platform entry is hosted exclusively by the admin realm.
    if realm_id != ADMIN_REALM_ID {
        return Err(ApiError::not_found("Not found"));
    }

    // 1. Platform toggle — fail-closed when unset, same gate as signup itself.
    if !is_platform_signup_enabled(&state).await? {
        tracing::info!("Signup code request rejected: platform toggle disabled");
        return Err(ApiError::forbidden(
            "Self-service signup is disabled".to_string(),
        ));
    }

    // Same normalization (trim + lowercase) as signup: the code row is keyed
    // on the normalized address and the consume step re-normalizes the
    // submitted email the same way.
    let email = normalize_email(&payload.email);

    // 2. Per-IP + per-mailbox quota. Runs ahead of the cheaper 400 exit below
    //    so a realm without a mail channel cannot bypass the limiter by
    //    spamming the rejection path.
    rate_limit_hit(
        &state,
        format!("rl:signup:code:ip:{ip}"),
        SIGNUP_CODE_REQUEST_IP_RATE_LIMIT.0,
        SIGNUP_CODE_REQUEST_IP_RATE_LIMIT.1,
    )
    .await?;
    rate_limit_hit(
        &state,
        format!("rl:signup:code:email:{email}"),
        SIGNUP_CODE_REQUEST_EMAIL_RATE_LIMIT.0,
        SIGNUP_CODE_REQUEST_EMAIL_RATE_LIMIT.1,
    )
    .await?;

    // 3. A code without a mail channel is a dead end: the mail would never
    //    arrive and the send would be silently skipped into a fake success.
    //    Reject before any code is issued (same precedence as change_email).
    if !is_email_configured(&state, ADMIN_REALM_ID).await? {
        return Err(ApiError::bad_request(
            "Email is not configured for this realm".to_string(),
        ));
    }

    // 4. Issue the code (newest-wins) and mail it through the template
    //    system's {{code}} family, like the OTP login mail.
    let code = issue_signup_email_code(&state, &email).await?;
    EmailService::send_signup_verify_email(&state.pool, ADMIN_REALM_ID, &email, &code, None)
        .await
        .map_err(|e| {
            tracing::error!("Failed to send signup verification email: {}", e);
            ApiError::internal("Failed to send verification email")
        })?;

    Ok(ApiResult::ok(SignupEmailCodeResponse {
        message: "ok".to_string(),
    }))
}

/// Generate and persist the signup verification code (newest-wins: the
/// previous unconsumed 'signup' row for the mailbox is deleted first,
/// mirroring the change-email request path, so only the latest issued code
/// can be consumed within the TTL).
async fn issue_signup_email_code(state: &AppState, email: &str) -> Result<String, ApiError> {
    let code = crate::email_otp::generate_otp_code();

    sqlx::query(
        "DELETE FROM email_verification_code WHERE realm_id = $1 AND email = $2 AND type = 'signup'",
    )
    .bind(ADMIN_REALM_ID)
    .bind(email)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to invalidate previous signup code: {}", e);
        ApiError::internal("Failed to create verification code")
    })?;

    sqlx::query(
        "INSERT INTO email_verification_code (realm_id, email, type, verification_code) VALUES ($1, $2, $3, $4)",
    )
    .bind(ADMIN_REALM_ID)
    .bind(email)
    .bind("signup")
    .bind(&code)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to create signup verification code: {}", e);
        ApiError::internal("Failed to create verification code")
    })?;

    Ok(code)
}

/// Atomically consume the single-use signup verification code bound to the
/// submitted mailbox. `DELETE ... RETURNING` makes check-and-burn one
/// statement, so exactly one racing request can succeed per issued code.
/// Zero rows means no unexpired 'signup' code matches the (code, email)
/// pair — one message for wrong code, wrong mailbox and expired alike, so
/// the response cannot act as an oracle for which part was off.
async fn consume_signup_email_code(
    state: &AppState,
    email: &str,
    code: &str,
) -> Result<(), ApiError> {
    // Codes older than the TTL are rejected — an emailed code must not stay
    // usable forever (shared email_verification_code TTL).
    let cutoff =
        chrono::Utc::now() - chrono::Duration::seconds(EMAIL_VERIFICATION_CODE_TTL_SECONDS as i64);
    let consumed: Option<String> = sqlx::query_scalar(
        "DELETE FROM email_verification_code
         WHERE realm_id = $1 AND type = 'signup' AND verification_code = $2
           AND email = $3 AND created_at >= $4
         RETURNING email",
    )
    .bind(ADMIN_REALM_ID)
    .bind(code)
    .bind(email)
    .bind(cutoff)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to consume signup verification code: {}", e);
        ApiError::internal("Failed to verify email code")
    })?;
    if consumed.is_none() {
        return Err(ApiError::bad_request(
            "invalid email verification code".to_string(),
        ));
    }
    Ok(())
}

/// Public visibility of the self-service entry. Fail-closed: a missing toggle
/// row resolves to `enabled: false`.
#[utoipa::path(
    get,
    path = "/api/auth/{realmId}/signup/status",
    tag = "auth",
    params(
        ("realmId" = String, Path, description = "Must be \"admin\"")
    ),
    responses(
        (status = 200, description = "Signup toggle status.", body = SignupStatusResponse),
        (status = 404, description = "Only the admin realm hosts signup.", body = ErrorResponse)
    )
)]
pub async fn get_signup_status(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
) -> Result<ApiResult<SignupStatusResponse>, ApiError> {
    if realm_id != ADMIN_REALM_ID {
        return Err(ApiError::not_found("Not found"));
    }
    // Both flags are independent reads — run them concurrently instead of
    // paying two serial DB round trips on this unauthenticated endpoint.
    // `?` order keeps enabled's error precedence when both fail. The email
    // flag uses the same helper the signup handler gates on, so the flag the
    // frontend reads can never disagree with the check that enforces it (the
    // helper's fail-open-when-no-mail-channel semantics included).
    let (enabled, email_verification_required) = tokio::join!(
        is_platform_signup_enabled(&state),
        is_email_verification_required(&state, ADMIN_REALM_ID)
    );
    let enabled = enabled?;
    let email_verification_required = email_verification_required?;
    Ok(ApiResult::ok(SignupStatusResponse {
        enabled,
        email_verification_required,
    }))
}
