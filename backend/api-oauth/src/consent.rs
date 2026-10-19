//! OAuth consent endpoints for third-party Authorization Code + PKCE flows.
//!
//! `/authorize` redirects authenticated and anonymous browsers alike to the
//! SPA consent route — the navigation itself carries no Herald credential
//! (sessions are Bearer tokens held by the SPA, not cookies), so the SPA is
//! the only place that can see whether a session exists. These two endpoints
//! are the SPA's backend half:
//!
//! * `GET /consent` probes a pending authorize transaction without consuming
//!   it: does this user still need to consent, and which client is asking?
//! * `POST /consent` is the user's decision. Approve mints the authorization
//!   code (same state-validation and code-record semantics as the login
//!   entrances); deny redirects back with the RFC 6749 §4.1.2.1
//!   `error=access_denied` propagation. The decision consumes the one-time
//!   state either way.
//!
//! Grants are remembered at the `/token` exchange (see `token.rs`), the single
//! choke point every entrance's codes pass through, so a completed
//! authorization — via password login, a second factor, a social entrance, or
//! this consent page — makes the next authorize silent.

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use herald_api_auth::consent_gate::evaluate_login_consent_gate;
use herald_api_base::application::http::rate_limit::rate_limit_hit;
use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::{
    ActorType, AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType,
    NewAuditEvent,
};
use herald_core::domain::authentication::Identity;
use herald_core::domain::client::ports::ClientService;
use herald_core::domain::legal::LegalAgreementSummary;
use herald_core::domain::security_constants::DEFAULT_OAUTH_CODE_TTL_SECONDS;
use herald_core::domain::user::ports::UserRepository;
use redis::AsyncCommands;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema)]
pub struct ConsentProbeQueryParams {
    pub oauth_client_id: String,
    pub redirect_uri: String,
    pub state: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ConsentProbeResponse {
    /// True when the user must see the consent UI before a code is minted:
    /// no remembered grant for this client (and not a first-party client,
    /// which the login itself authorizes).
    pub consent_required: bool,
    pub client_app_name: String,
    pub client_app_icon_url: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ConsentDecisionRequest {
    pub oauth_client_id: String,
    pub redirect_uri: String,
    pub state: String,
    pub approved: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ConsentDecisionResponse {
    /// "authorized" (code issued, `redirect_to` carries it), "denied"
    /// (`redirect_to` carries `error=access_denied`), or "consent_required"
    /// (the legal consent gate blocked the approval; record consent for the
    /// listed agreements and re-submit — the state was NOT consumed).
    pub status: String,
    pub redirect_to: Option<String>,
    pub consent_required: Option<bool>,
    pub agreements: Option<Vec<LegalAgreementSummary>>,
}

// ---------------------------------------------------------------------------
// Grant persistence (shared with the /token exchange)
// ---------------------------------------------------------------------------

/// Remember that `user_id` authorized `client_app_id`, best-effort: the grant
/// only decides whether the NEXT authorize shows the consent page, so a failed
/// write degrades to one extra confirmation, never to a skipped one.
pub async fn record_user_client_grant(
    state: &AppState,
    realm_id: &str,
    user_id: Uuid,
    client_app_id: Uuid,
) {
    let result = sqlx::query(
        "INSERT INTO user_client_grant (realm_id, user_id, client_app_id)
         VALUES ($1, $2, $3)
         ON CONFLICT (user_id, client_app_id) DO NOTHING",
    )
    .bind(realm_id)
    .bind(user_id)
    .bind(client_app_id)
    .execute(&state.pool)
    .await;
    if let Err(error) = result {
        tracing::warn!(
            %error,
            realm_id,
            %user_id,
            %client_app_id,
            "Failed to record OAuth client grant; next authorize will re-ask consent"
        );
    }
}

async fn user_has_client_grant(
    state: &AppState,
    user_id: Uuid,
    client_app_id: Uuid,
) -> Result<bool, ApiError> {
    let granted: Option<i32> = sqlx::query_scalar(
        "SELECT 1 FROM user_client_grant WHERE user_id = $1 AND client_app_id = $2",
    )
    .bind(user_id)
    .bind(client_app_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Database query failed: user_client_grant lookup");
        ApiError::internal("Database query failed".to_string())
    })?;
    Ok(granted.is_some())
}

// ---------------------------------------------------------------------------
// Shared validation
// ---------------------------------------------------------------------------

fn parse_identity_user(identity: &Identity) -> Result<Uuid, ApiError> {
    Uuid::parse_str(&identity.user_id())
        .map_err(|_| ApiError::unauthorized("Invalid session identity".to_string()))
}

/// Load the pending authorize transaction's stored fields and check them
/// against the request, mirroring the login entrances' state validation. When
/// `consume` is false the state stays pending (probe); when true the GETDEL
/// makes the load one-shot (decision).
async fn load_and_match_state(
    conn: &mut redis::aio::ConnectionManager,
    oauth_client_id: &str,
    redirect_uri: &str,
    state_param: &str,
    realm_id: &str,
    consume: bool,
) -> Result<serde_json::Value, ApiError> {
    let state_key = format!("oauth:state:{}", state_param);
    let state_json: Option<String> = if consume {
        redis::cmd("GETDEL").arg(&state_key).query_async(conn).await
    } else {
        redis::cmd("GET").arg(&state_key).query_async(conn).await
    }
    .map_err(|e| {
        tracing::error!(error = %e, "Redis state lookup failed for OAuth consent");
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

    if state_data["client_id"].as_str().unwrap_or("") != oauth_client_id {
        return Err(ApiError::bad_request(
            "OAuth state client_id mismatch".to_string(),
        ));
    }
    if state_data["realm_id"].as_str().unwrap_or("") != realm_id {
        return Err(ApiError::bad_request(
            "OAuth state realm_id mismatch".to_string(),
        ));
    }
    if state_data["redirect_uri"].as_str().unwrap_or("") != redirect_uri {
        return Err(ApiError::bad_request(
            "OAuth state redirect_uri mismatch".to_string(),
        ));
    }
    Ok(state_data)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Probe a pending third-party authorize transaction for the session user.
///
/// Returns whether consent is still required (no remembered grant) plus the
/// client app's display fields for the consent UI. Does not consume the
/// one-time state.
#[utoipa::path(
    get,
    path = "/api/oauth/{realmId}/consent",
    tag = "oauth",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("oauth_client_id" = String, Query, description = "OAuth Client ID the authorize flow is for"),
        ("redirect_uri" = String, Query, description = "Redirect URI of the pending authorize flow (must match the stored state)"),
        ("state" = String, Query, description = "State token of the pending authorize flow"),
    ),
    responses(
        (status = 200, description = "Consent status for this user and client", body = ConsentProbeResponse),
        (status = 400, description = "State missing/expired or field mismatch", body = ErrorResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 403, description = "Session belongs to a different realm / client disabled", body = ErrorResponse),
    )
)]
pub async fn oauth_consent_probe(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Query(params): Query<ConsentProbeQueryParams>,
) -> Result<Json<ConsentProbeResponse>, ApiError> {
    if identity.realm_id() != realm_id {
        return Err(ApiError::forbidden(
            "Access denied: cannot consent for a different realm".to_string(),
        ));
    }
    rate_limit_hit(
        &state,
        format!("rl:oauth-consent:user:{}", identity.user_id()),
        20,
        300,
    )
    .await?;

    let user_id = parse_identity_user(&identity)?;

    let mut conn = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Internal server error".to_string()))?;
    load_and_match_state(
        &mut conn,
        &params.oauth_client_id,
        &params.redirect_uri,
        &params.state,
        &realm_id,
        false,
    )
    .await?;

    let client_app = state
        .service
        .client_service()
        .get_client_app_by_client_id(&realm_id, &params.oauth_client_id)
        .await
        .map_err(|e| match e {
            herald_core::domain::common::entities::app_errors::CoreError::NotFound => {
                ApiError::not_found(format!(
                    "Client app with client_id '{}' not found in realm '{}'",
                    params.oauth_client_id, realm_id
                ))
            }
            error => {
                tracing::error!(error = %error, "Client app lookup failed: OAuth consent probe");
                ApiError::internal("Internal server error".to_string())
            }
        })?;
    if !client_app.enabled {
        return Err(ApiError::forbidden("Client app is disabled".to_string()));
    }

    // First-party clients are Herald's own browser UIs — logging in IS the
    // authorization, so they never see the consent UI.
    let has_grant =
        !client_app.is_first_party && user_has_client_grant(&state, user_id, client_app.id).await?;

    Ok(Json(ConsentProbeResponse {
        consent_required: !has_grant,
        client_app_name: client_app.name,
        client_app_icon_url: client_app.icon_url,
    }))
}

/// Record the user's approve/deny decision for a pending third-party
/// authorize transaction and consume its one-time state.
///
/// Approve mints the authorization code (identical state-validation and
/// code-record semantics to the login entrances) and returns the redirect URL
/// carrying `code` + `state`; deny returns the redirect URL carrying
/// `error=access_denied` + `state` (RFC 6749 §4.1.2.1).
#[utoipa::path(
    post,
    path = "/api/oauth/{realmId}/consent",
    tag = "oauth",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
    ),
    request_body = ConsentDecisionRequest,
    responses(
        (status = 200, description = "Decision recorded; redirect_to carries the outcome", body = ConsentDecisionResponse),
        (status = 400, description = "State missing/expired or field mismatch", body = ErrorResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 403, description = "Session belongs to a different realm", body = ErrorResponse),
    )
)]
pub async fn oauth_consent_decision(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Json(payload): Json<ConsentDecisionRequest>,
) -> Result<Json<ConsentDecisionResponse>, ApiError> {
    if identity.realm_id() != realm_id {
        return Err(ApiError::forbidden(
            "Access denied: cannot consent for a different realm".to_string(),
        ));
    }
    rate_limit_hit(
        &state,
        format!("rl:oauth-consent:user:{}", identity.user_id()),
        20,
        300,
    )
    .await?;

    let user_id = parse_identity_user(&identity)?;

    // The legal consent gate runs BEFORE the state is consumed: a blocked
    // approval must be retryable after the user records consent, mirroring
    // device_confirm's "stays verified" contract.
    if payload.approved {
        let user = state
            .user_repository
            .get_user_by_id(user_id)
            .await
            .map_err(|_| ApiError::unauthorized("Session user no longer exists".to_string()))?;
        if let Some(summaries) =
            evaluate_login_consent_gate(&state, &user, &realm_id, None, None, None).await
        {
            tracing::info!(
                realm_id = %realm_id,
                %user_id,
                client_id = %payload.oauth_client_id,
                "OAuth consent approval blocked at legal consent gate; state left pending"
            );
            return Ok(Json(ConsentDecisionResponse {
                status: "consent_required".to_string(),
                redirect_to: None,
                consent_required: Some(true),
                agreements: Some(summaries),
            }));
        }
    }

    let mut conn = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Internal server error".to_string()))?;

    let state_data = load_and_match_state(
        &mut conn,
        &payload.oauth_client_id,
        &payload.redirect_uri,
        &payload.state,
        &realm_id,
        true,
    )
    .await?;

    // The authorize endpoint checked the client when the flow started; the
    // pending state can outlive a disable, so re-check at completion before
    // any code is minted (same guard as the login entrances).
    herald_api_auth::oauth_oidc::ensure_mcp_client_still_enabled(
        &state,
        &realm_id,
        &payload.oauth_client_id,
    )
    .await?;

    if !payload.approved {
        let redirect_to = match url::Url::parse(&payload.redirect_uri) {
            Ok(mut url) => {
                url.query_pairs_mut()
                    .append_pair("error", "access_denied")
                    .append_pair("state", &payload.state);
                url.into()
            }
            // The redirect_uri was validated at /authorize time, so this arm
            // is unreachable in practice; keep a usable fallback rather than
            // failing the denial on a defensive path.
            Err(_) => format!(
                "{}?error=access_denied&state={}",
                payload.redirect_uri, payload.state
            ),
        };
        tracing::info!(
            realm_id = %realm_id,
            %user_id,
            client_id = %payload.oauth_client_id,
            "OAuth consent denied by user"
        );
        return Ok(Json(ConsentDecisionResponse {
            status: "denied".to_string(),
            redirect_to: Some(redirect_to),
            consent_required: None,
            agreements: None,
        }));
    }

    // Mint the authorization code with the exact semantics of the login
    // entrances' completion block.
    let auth_code = format!("ac_{}", Uuid::now_v7());
    let code_challenge = state_data["code_challenge"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let code_value = herald_api_auth::oauth_oidc::build_oauth_code_record_from_state(
        &code_challenge,
        &payload.oauth_client_id,
        &payload.redirect_uri,
        &user_id.to_string(),
        &realm_id,
        &state_data,
    );
    let code_key = format!("oauth:code:{}", auth_code);
    let _: () = conn
        .set_ex(&code_key, code_value, DEFAULT_OAUTH_CODE_TTL_SECONDS)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to store OAuth authorization code");
            ApiError::internal("Internal server error".to_string())
        })?;

    let redirect_to = herald_api_auth::oauth_oidc::append_code_and_state(
        &payload.redirect_uri,
        &auth_code,
        &payload.state,
    );

    if let Err(audit_err) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: realm_id.clone(),
            category: AuditCategory::Auth,
            action: AuditAction::AuthLogin,
            actor_id: user_id.to_string(),
            actor_type: Some(ActorType::User),
            actor_name: identity_email(&identity),
            target_type: AuditTargetType::User,
            target_id: user_id.to_string(),
            target_name: identity_email(&identity),
            result: AuditResult::Success,
            details: Some(serde_json::json!({
                "method": "oauth-consent",
                "client_id": payload.oauth_client_id,
                "oauth": true,
            })),
            ip_address: None,
            user_agent: None,
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %audit_err, "Failed to record audit event");
    }

    tracing::info!(
        realm_id = %realm_id,
        %user_id,
        client_id = %payload.oauth_client_id,
        "OAuth authorization code generated via consent approval"
    );

    Ok(Json(ConsentDecisionResponse {
        status: "authorized".to_string(),
        redirect_to: Some(redirect_to),
        consent_required: None,
        agreements: None,
    }))
}

/// Display name for audit rows; the user identity carries the account email.
fn identity_email(identity: &Identity) -> Option<String> {
    identity.as_user().map(|user| user.email.clone())
}
