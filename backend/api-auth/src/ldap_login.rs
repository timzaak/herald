// LDAP enterprise-directory login.
//
// Dedicated public endpoint mirroring the password-login pipeline (DEC-006):
// Client App resolution → Turnstile → shared rl:login:* rate limits →
// directory authentication via the `LdapAuthenticator` port → user matching
// (DN → email → JIT provisioning, DEC-008) → second-factor probe → consent
// gate → OAuth code branch → token family. `login.rs` itself is NOT modified.
//
// Anti-enumeration (DEC-009): every credential-side failure (no unique search
// hit, bind rejected) is a generic 401 "invalid credentials", identical to
// password login. Directory-side failures are a generic 503; the adapter's
// error detail goes to tracing only.

use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
    http::StatusCode,
    response::IntoResponse,
};
use axum_valid::Valid;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utoipa::ToSchema;
use uuid::Uuid;

use herald_api_base::application::http::auth::util::{
    ClientIp, load_ldap_config, normalize_email, rate_limit_hit, user_agent_from_headers,
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
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::ldap::LdapAuthError;
use herald_core::domain::legal::{AgreementType, ConsentSource, LegalAgreementSummary};
use herald_core::domain::oauth::entities::{
    CreateOAuthProviderConfig, OAuthProvider, ProviderType,
};
use herald_core::domain::oauth::ports::OAuthRepository;
use herald_core::domain::security_constants::{
    DEFAULT_OAUTH_CODE_TTL_SECONDS, LOGIN_IDENTIFIER_RATE_LIMIT, LOGIN_IP_RATE_LIMIT,
    OAUTH_STATE_TTL_SECONDS,
};
use herald_core::domain::user::entities::User;
use herald_core::domain::user::ports::{UserRepository, UserService};
use herald_core::domain::user::value_objects::CreateUserRequest;
use herald_core::domain::user_passkey::UserPasskeyRepository;
use herald_core::domain::user_totp::UserTotpRepository;
use herald_core::infrastructure::authentication::RedisBrowserTokenService;
use herald_core::infrastructure::user_passkey::PostgresUserPasskeyRepository;
use herald_core::infrastructure::user_totp::PostgresUserTotpRepository;

use crate::browser_token::BrowserTokenResponse;
use crate::consent_gate::AuthConsentAgreement;
use crate::login::LoginResponse;
use crate::mailflow;
use crate::passkey_rp::resolve_passkey_rp;

const LDAP_PLACEHOLDER_EMAIL_DOMAIN: &str = "ldap.placeholder";

#[derive(Debug, Deserialize, ToSchema, validator::Validate)]
#[serde(rename_all = "camelCase")]
pub struct LdapLoginRequest {
    #[validate(length(min = 1, max = 36))]
    pub client_id: String,
    // Directory login identifier (uid / sAMAccountName / UPN). Wider than the
    // local username caps because the directory owns this namespace; the
    // bound still feeds Redis rate-limit keys, so it must not be unbounded.
    #[validate(length(min = 1, max = 254))]
    pub username: String,
    // Directory password policy belongs to the enterprise directory; this
    // bound only fences DoS, hence wider than the local 8..=36 policy.
    #[validate(length(min = 1, max = 512))]
    pub password: String,
    #[serde(default)]
    #[schema(required = false)]
    pub turnstile_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub oauth_client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub redirect_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub state: Option<String>,
    #[serde(default)]
    #[schema(required = false)]
    pub agreements: Option<Vec<AuthConsentAgreement>>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LdapStatusResponse {
    pub enabled: bool,
}

/// Outcome of the DN → email → JIT matching chain (DEC-008). Matched and
/// JIT-provisioned accounts are treated identically downstream, so they
/// share one variant.
enum LdapUserResolution {
    Resolved(User),
    /// JIT branch: consent must be expressed before any account is created
    /// (US-LD-002 scenario 5). No account row exists when this is returned.
    ConsentRequired(Vec<LegalAgreementSummary>),
}

/// Enterprise (LDAP) directory login.
///
/// Authenticates the submitted username/password against the Realm's
/// configured directory (search-then-bind), matches or JIT-provisions the
/// Herald account, and then runs the full existing login pipeline
/// (2FA / consent / OAuth code / session issuance), auditing
/// `method="ldap"`.
#[utoipa::path(
  post,
  path = "/api/auth/{realmId}/login/ldap",
  tag = "auth",
  params(
    ("realmId" = String, Path, description = "Realm ID")
  ),
  request_body = LdapLoginRequest,
  responses(
    // 200 carries the login-family dual shape: final success is a
    // BrowserTokenResponse, in-flight branches (second factor / consent /
    // OAuth redirect) reuse login.rs's LoginResponse flag form.
    (status = 200, description = "Login succeeded (token) or an in-flight branch flag.", body = BrowserTokenResponse),
    (status = 400, description = "Bad request / LDAP login not enabled for this realm", body = ErrorResponse),
    (status = 401, description = "Invalid credentials (generalized) / Turnstile failed", body = ErrorResponse),
    (status = 403, description = "Account is disabled", body = ErrorResponse),
    (status = 409, description = "JIT provisioning race (retry succeeds)", body = ErrorResponse),
    (status = 429, description = "Too many requests", body = ErrorResponse),
    (status = 500, description = "Internal server error", body = ErrorResponse),
    (status = 503, description = "Directory unavailable", body = ErrorResponse),
  )
)]
#[tracing::instrument(
    // Governance: payload carries the directory password (credential) and
    // username (PII); realm_id is conservatively skipped. Only the
    // low-cardinality operation type is recorded — same shape as login.rs.
    skip(state, headers, payload, realm_id, ip),
    fields(db.system = "postgres", db.operation = "ldap_login")
)]
pub async fn ldap_login(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Valid(Json(payload)): Valid<Json<LdapLoginRequest>>,
) -> Result<impl IntoResponse, ApiError> {
    let user_agent = user_agent_from_headers(&headers);

    // 1. LDAP must be enabled for the realm — checked before the Client App,
    //    Turnstile, rate limits, and the directory, so a disabled realm never
    //    touches the directory nor creates anything (US-LD-001 scenario 5).
    let Some(ldap_config) = load_ldap_config(&state, &realm_id).await? else {
        return Err(ApiError::bad_request(
            "LDAP login is not enabled for this realm".to_string(),
        ));
    };

    // 2. Client App resolution + per-Client-App Turnstile (mirror login.rs).
    let client_app =
        mailflow::require_enabled_client(&state, &realm_id, &payload.client_id).await?;
    verify_turnstile_for_client_app(&state, &client_app, payload.turnstile_token.as_deref(), &ip)
        .await?;

    // 3. Shared login rate-limit budget (same keys and thresholds as password
    //    login — deliberately NOT a separate, weaker budget).
    rate_limit_hit(
        &state,
        format!("rl:login:ip:{ip}"),
        LOGIN_IP_RATE_LIMIT.0,
        LOGIN_IP_RATE_LIMIT.1,
    )
    .await?;
    rate_limit_hit(
        &state,
        format!("rl:login:identifier:{}", payload.username),
        LOGIN_IDENTIFIER_RATE_LIMIT.0,
        LOGIN_IDENTIFIER_RATE_LIMIT.1,
    )
    .await?;

    // 4. Directory authentication.
    let ldap_user = match state
        .ldap_authenticator
        .authenticate(&ldap_config, &payload.username, &payload.password)
        .await
    {
        Ok(user) => user,
        Err(LdapAuthError::InvalidCredentials) => {
            record_ldap_login_failure(
                &state,
                &realm_id,
                &payload.username,
                &ip,
                user_agent.as_deref(),
                &payload.client_id,
                "invalid_credentials",
            )
            .await;
            return Err(ApiError::unauthorized("invalid credentials".to_string()));
        }
        Err(LdapAuthError::Unavailable(detail)) => {
            // Adapter detail stays server-side; the response is generic 503.
            tracing::warn!(
                realm_id = %realm_id,
                detail = %detail,
                "LDAP directory unavailable during login"
            );
            record_ldap_login_failure(
                &state,
                &realm_id,
                &payload.username,
                &ip,
                user_agent.as_deref(),
                &payload.client_id,
                "directory_unavailable",
            )
            .await;
            return Err(ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "Login temporarily unavailable, please try again later",
            ));
        }
    };

    // 5. DN → email → JIT matching chain (DEC-008; realm-scoped throughout).
    // The chain also returns the (realm, "ldap", DN) link row when its level-1
    // lookup already loaded it, so step 6 need not repeat that query.
    let (resolution, known_link) =
        find_or_provision_ldap_user(&state, &realm_id, &ldap_user, payload.agreements.as_deref())
            .await?;

    let user = match resolution {
        LdapUserResolution::ConsentRequired(summaries) => {
            // No account was created; the front-end collects consent and
            // re-submits (directory re-authenticates) with `agreements`.
            return Ok(Json(LoginResponse {
                message: "consent required".to_string(),
                user_id: Uuid::nil(),
                realm_id: realm_id.clone(),
                requires_totp: Some(false),
                second_factors: None,
                temp_token: None,
                expires_in_seconds: 0,
                redirect_to: None,
                consent_required: Some(true),
                agreements: Some(summaries),
            })
            .into_response());
        }
        LdapUserResolution::Resolved(user) => user,
    };

    // 6. Link the directory identity to the resolved account (idempotent).
    ensure_ldap_provider_linked(
        &state,
        &realm_id,
        &ldap_user.dn,
        ldap_user.email.as_deref(),
        user.id,
        known_link,
    )
    .await?;

    // 7. Disabled accounts are rejected even though directory credentials
    //    were valid (account state takes precedence, PRD §4.1).
    if !user.is_active() {
        record_ldap_login_failure(
            &state,
            &realm_id,
            &payload.username,
            &ip,
            user_agent.as_deref(),
            &payload.client_id,
            "disabled_account",
        )
        .await;
        return Err(ApiError::forbidden("账号已被禁用".to_string()));
    }

    // 8. Second-factor probe (mirror login.rs; a JIT account has no factors
    //    yet, so the probe is naturally empty for first logins).
    let totp_repo = PostgresUserTotpRepository::new(state.db.clone());
    let totp_config = totp_repo.get_config_by_user_id(user.id).await?;
    let has_totp = totp_config
        .as_ref()
        .map(|config| config.enabled)
        .unwrap_or(false);

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
            // Tolerant probe: password login must not depend on global
            // passkey RP config; neither must LDAP login (mirror login.rs).
            tracing::debug!(
                user_id = %user.id,
                realm_id = %user.realm_id,
                error = %error,
                "Passkey RP resolution failed during LDAP second-factor probe; passkey will not be offered"
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

        let mut temp_session_data = serde_json::json!({
            "user_id": user.id,
            "realm_id": realm_id,
            "client_id": payload.client_id,
            "client_app_id": client_app.id,
            "client_ip": ip,
            "flow": "custom_user_ui",
        });
        if let Some(ref oauth_client_id) = payload.oauth_client_id {
            temp_session_data["oauth_client_id"] = serde_json::json!(oauth_client_id);
        }
        if let Some(ref redirect_uri) = payload.redirect_uri {
            temp_session_data["redirect_uri"] = serde_json::json!(redirect_uri);
        }
        if let Some(ref oauth_state) = payload.state {
            temp_session_data["state"] = serde_json::json!(oauth_state);
        }

        let mut conn = state
            .redis_manager
            .get()
            .await
            .map_err(|_| ApiError::internal("Internal server error".to_string()))?;
        let _: () = conn
            .set_ex(&temp_key, temp_session_data.to_string(), 300)
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
                    "method": "ldap",
                    "client_id": payload.client_id,
                    "totp_required": has_totp,
                    "passkey_required": has_passkey,
                })),
                ip_address: Some(ip.clone()),
                user_agent: user_agent.clone(),
                trace_id: None,
            })
            .await
        {
            tracing::warn!(error = %audit_err, "Failed to record audit event");
        }

        // The existing /login/verify-totp and /login/passkey/2fa/* endpoints
        // consume this same temp session — zero changes needed there.
        return Ok(Json(LoginResponse {
            message: "ok".to_string(),
            user_id: user.id,
            realm_id: realm_id.clone(),
            requires_totp: Some(has_totp),
            second_factors: Some(
                second_factors
                    .iter()
                    .map(|factor| factor.to_string())
                    .collect(),
            ),
            temp_token: Some(temp_token),
            expires_in_seconds: 300,
            redirect_to: None,
            consent_required: None,
            agreements: None,
        })
        .into_response());
    }

    // 9. Login-as-consent gate for existing users (mirror login.rs order:
    //    credentials → 2FA → consent → session).
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
        return Ok(Json(LoginResponse {
            message: "consent required".to_string(),
            user_id: user.id,
            realm_id: realm_id.clone(),
            requires_totp: Some(false),
            second_factors: None,
            temp_token: None,
            expires_in_seconds: 0,
            redirect_to: None,
            consent_required: Some(true),
            agreements: Some(summaries),
        })
        .into_response());
    }

    // 10. Downstream OAuth authorization-code branch (mirror login.rs).
    let is_oauth_flow = payload.oauth_client_id.is_some()
        && payload.redirect_uri.is_some()
        && payload.state.is_some();

    if is_oauth_flow {
        let oauth_client_id = payload.oauth_client_id.as_deref().ok_or_else(|| {
            ApiError::bad_request("oauth_client_id is required for OAuth flow".to_string())
        })?;
        let redirect_uri = payload.redirect_uri.as_deref().ok_or_else(|| {
            ApiError::bad_request("redirect_uri is required for OAuth flow".to_string())
        })?;
        let state_param = payload
            .state
            .as_deref()
            .ok_or_else(|| ApiError::bad_request("state is required for OAuth flow".to_string()))?;

        let state_key = format!("oauth:state:{}", state_param);
        let mut conn = state
            .redis_manager
            .get()
            .await
            .map_err(|_| ApiError::internal("Internal server error".to_string()))?;

        let state_json: Option<String> = redis::cmd("GETDEL")
            .arg(&state_key)
            .query_async(&mut conn)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Redis GETDEL failed for OAuth state");
                ApiError::internal("Internal server error".to_string())
            })?;

        let state_json = state_json.ok_or_else(|| {
            ApiError::bad_request(
                "OAuth state not found or already used. Please restart the authorization flow."
                    .to_string(),
            )
        })?;

        let state_data: serde_json::Value = serde_json::from_str(&state_json).map_err(|e| {
            tracing::error!(error = %e, "Failed to parse OAuth state JSON");
            ApiError::internal("Internal server error".to_string())
        })?;

        let stored_client_id = state_data["client_id"].as_str().unwrap_or("");
        let stored_realm_id = state_data["realm_id"].as_str().unwrap_or("");
        let stored_redirect_uri = state_data["redirect_uri"].as_str().unwrap_or("");

        if stored_client_id != oauth_client_id {
            return Err(ApiError::bad_request(
                "OAuth state client_id mismatch".to_string(),
            ));
        }
        if stored_realm_id != realm_id {
            return Err(ApiError::bad_request(
                "OAuth state realm_id mismatch".to_string(),
            ));
        }
        if stored_redirect_uri != redirect_uri {
            return Err(ApiError::bad_request(
                "OAuth state redirect_uri mismatch".to_string(),
            ));
        }

        let auth_code = format!("ac_{}", Uuid::now_v7());
        let code_challenge = state_data["code_challenge"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let code_key = format!("oauth:code:{}", auth_code);
        let code_value = serde_json::json!({
            "code_challenge": code_challenge,
            "client_id": oauth_client_id,
            "redirect_uri": redirect_uri,
            "user_id": user.id.to_string(),
            "realm_id": realm_id,
        })
        .to_string();

        let _: () = conn
            .set_ex(&code_key, code_value, OAUTH_STATE_TTL_SECONDS)
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
                    "method": "ldap",
                    "client_id": payload.client_id,
                    "oauth": true,
                })),
                ip_address: Some(ip.clone()),
                user_agent: user_agent.clone(),
                trace_id: None,
            })
            .await
        {
            tracing::warn!(error = %audit_err, "Failed to record audit event");
        }

        let redirect_to = format!("{}?code={}&state={}", redirect_uri, auth_code, state_param);

        return Ok(Json(LoginResponse {
            message: "ok".to_string(),
            user_id: user.id,
            realm_id: realm_id.clone(),
            requires_totp: Some(false),
            second_factors: None,
            temp_token: None,
            expires_in_seconds: DEFAULT_OAUTH_CODE_TTL_SECONDS as i64,
            redirect_to: Some(redirect_to),
            consent_required: None,
            agreements: None,
        })
        .into_response());
    }

    // 11. Session issuance + success audit.
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
            details: Some(serde_json::json!({
                "method": "ldap",
                "client_id": payload.client_id,
            })),
            ip_address: Some(ip.clone()),
            user_agent: user_agent.clone(),
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %audit_err, "Failed to record audit event");
    }

    Ok(Json(BrowserTokenResponse::from(tokens)).into_response())
}

/// Public LDAP enablement flag for a Realm (login-page entry visibility).
/// Fail-closed: absent/malformed/disabled/insecure-channel config → false.
#[utoipa::path(
    get,
    path = "/api/auth/{realmId}/ldap/status",
    tag = "auth",
    params(
      ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
      (status = 200, description = "LDAP status", body = LdapStatusResponse),
      (status = 500, description = "Internal server error", body = ErrorResponse),
    )
)]
pub async fn ldap_status(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<LdapStatusResponse>, ApiError> {
    let enabled =
        herald_api_base::application::http::auth::util::is_ldap_enabled(&state, &realm_id).await?;
    Ok(Json(LdapStatusResponse { enabled }))
}

// ---------------------------------------------------------------------------
// matching chain + JIT helpers
// ---------------------------------------------------------------------------

/// DN → email → JIT provisioning. All lookups are realm-scoped. Also returns
/// the (realm, "ldap", DN) link row when the level-1 lookup loaded it, so the
/// caller's link step does not repeat the same query.
async fn find_or_provision_ldap_user(
    state: &AppState,
    realm_id: &str,
    ldap_user: &herald_core::domain::ldap::LdapAuthenticatedUser,
    agreements: Option<&[AuthConsentAgreement]>,
) -> Result<(LdapUserResolution, Option<OAuthProvider>), ApiError> {
    let provider_repo = state.service.oauth_provider_repository();

    // Level 1: existing DN link (repeat logins resolve here — no duplicate
    // accounts, US-LD-002 scenario 1).
    let mut known_link: Option<OAuthProvider> = None;
    match provider_repo
        .find_by_provider_and_open_id(realm_id, ProviderType::Ldap.as_str(), &ldap_user.dn)
        .await
    {
        Ok(link) => {
            if let Some(user_id) = link.user_id {
                let user = state
                    .user_repository
                    .get_user_by_id(user_id)
                    .await
                    .map_err(|e| {
                        tracing::error!(
                            user_id = %user_id,
                            error = %e,
                            "LDAP login: linked account row missing"
                        );
                        ApiError::internal("Internal server error".to_string())
                    })?;
                // The link row is realm-scoped, but tenant isolation must not
                // depend on that row's integrity (same defense as the OAuth
                // callback's user.realm_id guard): a corrupt link pointing at
                // another realm's user must never mint a session here.
                if user.realm_id != realm_id {
                    tracing::error!(
                        realm_id = %realm_id,
                        user_id = %user_id,
                        user_realm_id = %user.realm_id,
                        "LDAP login: DN link references a user of another realm — rejecting"
                    );
                    return Err(ApiError::internal("Internal server error".to_string()));
                }
                return Ok((LdapUserResolution::Resolved(user), Some(link)));
            }
            // Dangling link (no user_id): fall through to email/provision;
            // `ensure_ldap_provider_linked` re-binds it afterwards.
            known_link = Some(link);
        }
        Err(CoreError::NotFound) => {}
        Err(e) => {
            tracing::error!(
                realm_id = %realm_id,
                error = %e,
                "LDAP login: provider link lookup failed"
            );
            return Err(ApiError::internal("Internal server error".to_string()));
        }
    }

    let resolution = resolve_by_email_or_provision(state, realm_id, ldap_user, agreements).await?;
    Ok((resolution, known_link))
}

/// Levels 2–3 of the matching chain: trusted directory email → JIT
/// provisioning. Reached when no DN link exists or the link is dangling.
async fn resolve_by_email_or_provision(
    state: &AppState,
    realm_id: &str,
    ldap_user: &herald_core::domain::ldap::LdapAuthenticatedUser,
    agreements: Option<&[AuthConsentAgreement]>,
) -> Result<LdapUserResolution, ApiError> {
    // Level 2: directory mail is trusted (DEC-008) — matching an existing
    // local account links it instead of creating a duplicate (US-LD-002
    // scenario 2). No `verified` check, unlike OAuth.
    if let Some(raw_email) = ldap_user.email.as_deref() {
        let email = normalize_email(raw_email);
        match state
            .user_repository
            .get_user_by_email(realm_id, &email)
            .await
        {
            Ok(user) => return Ok(LdapUserResolution::Resolved(user)),
            Err(CoreError::NotFound) => {}
            Err(e) => {
                tracing::error!(
                    realm_id = %realm_id,
                    error = %e,
                    "LDAP login: user email lookup failed"
                );
                return Err(ApiError::internal("Internal server error".to_string()));
            }
        }
    }

    // Level 3: JIT provisioning. Consent must be expressed BEFORE any account
    // row is created (US-LD-002 scenario 5); the gate mirrors email_otp's
    // "consent before provisioning" semantics.
    if agreements.is_none_or(|a| a.is_empty()) {
        let summaries = current_effective_summaries(state, realm_id).await;
        return Ok(LdapUserResolution::ConsentRequired(summaries));
    }

    // No registration-policy gate: enabling the directory IS the admin's
    // authorization for its supply (DEC-007 — deliberately different from
    // email-otp/OAuth auto-register).
    let email = match ldap_user.email.as_deref() {
        Some(raw_email) => normalize_email(raw_email),
        // No directory mail: placeholder address, not marked verified
        // (DEC-002, same pattern as the Apple placeholder). The DN hash keeps
        // it unique per directory identity.
        None => format!(
            "{}@{LDAP_PLACEHOLDER_EMAIL_DOMAIN}",
            ldap_dn_digest(&ldap_user.dn)
        ),
    };

    let created = match state
        .service
        .user_service()
        .create_user_without_password(CreateUserRequest {
            realm_id: realm_id.to_string(),
            email: email.clone(),
            password: None,
            provider_ids: None,
        })
        .await
    {
        Ok(created) => created,
        // Two concurrent first logins race on account(realm,email); the
        // unique index makes the loser Conflict — retryable, the next attempt
        // hits the email/DN match instead.
        Err(CoreError::Conflict(msg)) => {
            return Err(ApiError::conflict(msg));
        }
        Err(e) => {
            tracing::error!(
                realm_id = %realm_id,
                error = %e,
                "LDAP JIT: create_user_without_password failed"
            );
            return Err(ApiError::internal("Internal server error".to_string()));
        }
    };

    // WaitVerified → Normal; token families require an active user (same
    // shape as the email_otp auto-register path).
    if let Err(e) = state.service.user_service().activate_user(created.id).await {
        tracing::error!(
            user_id = %created.id,
            error = %e,
            "LDAP JIT: activate_user failed"
        );
        return Err(ApiError::internal("Internal server error".to_string()));
    }

    record_register_consent(state, created.id, realm_id, &email, agreements).await;

    let user = state
        .user_repository
        .get_user_by_id(created.id)
        .await
        .map_err(|e| {
            tracing::error!(
                user_id = %created.id,
                error = %e,
                "LDAP JIT: failed to reload created user"
            );
            ApiError::internal("Internal server error".to_string())
        })?;
    Ok(LdapUserResolution::Resolved(user))
}

/// Idempotently bind the directory identity to the account: create the
/// (realm, "ldap", DN) link row when absent, or re-point a dangling/wrong
/// link at the resolved user. `known_link` is the link row the matching
/// chain's level-1 lookup already loaded (hit or dangling), sparing a second
/// identical query; `None` means that lookup was NotFound, so this function
/// re-fetches — a concurrent login may have created the row in between.
async fn ensure_ldap_provider_linked(
    state: &AppState,
    realm_id: &str,
    dn: &str,
    email: Option<&str>,
    user_id: Uuid,
    known_link: Option<OAuthProvider>,
) -> Result<(), ApiError> {
    let provider_repo = state.service.oauth_provider_repository();

    let existing = match known_link {
        Some(link) => Some(link),
        None => match provider_repo
            .find_by_provider_and_open_id(realm_id, ProviderType::Ldap.as_str(), dn)
            .await
        {
            Ok(link) => Some(link),
            Err(CoreError::NotFound) => None,
            Err(e) => {
                tracing::error!(realm_id = %realm_id, error = %e, "LDAP provider lookup failed");
                return Err(ApiError::internal("Internal server error".to_string()));
            }
        },
    };

    let Some(existing) = existing else {
        let provider = OAuthProvider::new(CreateOAuthProviderConfig {
            realm_id: realm_id.to_string(),
            provider_type: ProviderType::Ldap,
            open_id: dn.to_string(),
            union_id: None,
            email: email.map(str::to_string),
            user_id: Some(user_id),
        });
        return provider_repo
            .create_provider(provider)
            .await
            .map(|_| ())
            .map_err(|e| {
                tracing::error!(user_id = %user_id, error = %e, "LDAP create_provider failed");
                match e {
                    CoreError::Conflict(msg) => ApiError::conflict(msg),
                    _ => ApiError::internal("Internal server error".to_string()),
                }
            });
    };

    if existing.user_id == Some(user_id) {
        Ok(())
    } else {
        provider_repo
            .link_provider_to_user(user_id, existing.id)
            .await
            .map_err(|e| {
                tracing::error!(user_id = %user_id, error = %e, "LDAP link_provider_to_user failed");
                ApiError::internal("Internal server error".to_string())
            })
    }
}

fn ldap_dn_digest(dn: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(dn.as_bytes());
    hex::encode(hasher.finalize())
}

/// Collect current effective ToS + Privacy summaries for the consent branch
/// (same collection as email_otp's helper; missing versions are skipped).
async fn current_effective_summaries(
    state: &AppState,
    realm_id: &str,
) -> Vec<LegalAgreementSummary> {
    let mut summaries = Vec::new();
    for agreement_type in [AgreementType::TermsOfService, AgreementType::PrivacyPolicy] {
        match state
            .legal_service
            .current_effective(realm_id, agreement_type.clone())
            .await
        {
            Ok(Some(version)) => {
                summaries.push(LegalAgreementSummary {
                    agreement_type: agreement_type.as_str().to_string(),
                    version_id: version.id,
                    version_no: version.version_no,
                    effective_at: version.published_at,
                    title: None,
                    summary: None,
                    mode: version.mode,
                    external_url: version.external_url,
                });
            }
            Ok(None) => tracing::warn!(
                realm_id = %realm_id,
                agreement_type = %agreement_type.as_ref(),
                "No effective agreement version deployed; skipping from LDAP consent list"
            ),
            Err(e) => tracing::warn!(
                realm_id = %realm_id,
                agreement_type = %agreement_type.as_ref(),
                error = %e,
                "current_effective lookup failed during LDAP consent gate"
            ),
        }
    }
    summaries
}

/// Best-effort register-as-consent recording (mirrors email_otp.rs; failures
/// log and never block the login).
async fn record_register_consent(
    state: &AppState,
    user_id: Uuid,
    realm_id: &str,
    email: &str,
    agreements: Option<&[AuthConsentAgreement]>,
) {
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
                    "Invalid agreement type in LDAP register-consent payload"
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
                    "No effective agreement version; skipping LDAP register-consent"
                ),
                Err(e) => tracing::warn!(
                    realm_id = %realm_id,
                    agreement_type = %agreement_type.as_ref(),
                    user_id = %user_id,
                    error = %e,
                    "current_effective failed during LDAP register-consent"
                ),
            }
        }
    }

    if items.is_empty() {
        return;
    }

    let actor_meta = herald_core::domain::audit::AuditContext {
        actor_id: user_id.to_string(),
        actor_type: Some(ActorType::User),
        actor_name: Some(email.to_string()),
        ip_address: None,
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
            "record_consent(Register) failed during LDAP JIT; login proceeds"
        );
    }
}

/// Best-effort LDAP login-failure audit event (reason distinguishes
/// invalid_credentials / directory_unavailable / disabled_account for
/// administrators; the API response stays generalized).
async fn record_ldap_login_failure(
    state: &AppState,
    realm_id: &str,
    username: &str,
    ip: &str,
    user_agent: Option<&str>,
    client_id: &str,
    reason: &str,
) {
    if let Err(audit_err) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: realm_id.to_string(),
            category: AuditCategory::Auth,
            action: AuditAction::AuthLoginFailed,
            actor_id: username.to_string(),
            actor_type: None,
            actor_name: None,
            target_type: AuditTargetType::User,
            target_id: username.to_string(),
            target_name: None,
            result: AuditResult::Failure,
            details: Some(serde_json::json!({
                "method": "ldap",
                "reason": reason,
                "client_id": client_id,
            })),
            ip_address: Some(ip.to_string()),
            user_agent: user_agent.map(|s| s.to_string()),
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %audit_err, "Failed to record LDAP login-failed audit event");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_email_is_dn_hash_based() {
        // WHY: two directory users without a mail attribute must never
        // collide on the same placeholder account; uniqueness comes from the
        // DN (unique per directory identity), hashed because a raw DN can
        // contain characters that are invalid in an email local part.
        let a = ldap_dn_digest("uid=alice,dc=example,dc=com");
        let b = ldap_dn_digest("uid=bob,dc=example,dc=com");
        assert_ne!(a, b);
        assert_eq!(a.len(), 64, "sha256 hex digest");
        let placeholder = format!("{a}@{LDAP_PLACEHOLDER_EMAIL_DOMAIN}");
        assert!(placeholder.starts_with(&a));
        assert!(placeholder.ends_with(LDAP_PLACEHOLDER_EMAIL_DOMAIN));
    }
}
