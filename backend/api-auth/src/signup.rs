use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use axum_valid::Valid;
use herald_api_base::application::http::auth::util::{
    ClientIp, is_platform_signup_enabled, rate_limit_hit, user_agent_from_headers,
    verify_turnstile_for_client_app,
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
use herald_core::domain::security_constants::SIGNUP_IP_RATE_LIMIT;
use herald_core::domain::user::ports::UserRepository;
use herald_core::infrastructure::authentication::RedisBrowserTokenService;
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
    let email = payload.email.trim().to_string();

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

    // 4. Provision the realm via the policy-free self-service entry. The
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

    // 5. Issue a first-party admin-console session bound to the NEW realm.
    let tokens = RedisBrowserTokenService::new(state.redis_manager.clone())
        .create_first_party_token_family(&admin_user, &console, user_agent, Some(ip.clone()))
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to issue signup session");
            ApiError::internal("Failed to issue session")
        })?;

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
    let enabled = is_platform_signup_enabled(&state).await?;
    Ok(ApiResult::ok(SignupStatusResponse { enabled }))
}
