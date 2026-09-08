// OAuth callback handler

use axum::{
    Form, Json,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use crate::helper::{
    audit_oauth_login_failure, audit_oauth_login_success, handle_oauth_callback,
    issue_downstream_authorization_code,
};
use herald_api_auth::browser_token::BrowserTokenResponse;
use herald_api_auth::consent_gate::{evaluate_login_consent_gate, mint_consent_restricted_session};
use herald_api_base::application::http::auth::util::{ClientIp, user_agent_from_headers};
use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::{BrowserTokenService, BrowserTokenSet};
use herald_core::domain::client::{entities::ClientApp, ports::ClientService};
use herald_core::domain::legal::LegalAgreementSummary;
use herald_core::domain::user::{User, UserRepository};
use herald_core::infrastructure::authentication::RedisBrowserTokenService;

#[derive(Debug, Deserialize, Serialize, ToSchema, Validate)]
pub struct OAuthCallbackQuery {
    pub code: String,
    pub state: String,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OAuthCallbackResponse {
    pub message: String,
    pub user_id: String,
    #[serde(flatten)]
    #[schema(required = false)]
    pub tokens: Option<BrowserTokenSet>,
    // Consent gate: mirrors the password/OTP/LDAP/passkey entrances — a stale
    // consent record yields 200 + consentRequired=true + current effective
    // summaries and NO full session (the OAuth credential is single-use and
    // cannot be replayed with agreements attached). The restricted family
    // carried by `restrictedSession` lets the client record explicit consent
    // via POST /api/legal/{realmId}/consent; the user then re-triggers login.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub consent_required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub agreements: Option<Vec<LegalAgreementSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub restricted_session: Option<BrowserTokenResponse>,
}

/// Handle OAuth callback from provider for a realm
#[utoipa::path(
    get,
    path = "/api/oauth/{realmId}/{provider}/callback",
    tag = "oauth",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("provider" = String, Path, description = "OAuth provider type"),
        ("code" = String, Query, description = "Authorization code from provider"),
        ("state" = String, Query, description = "State token for CSRF protection")
    ),
    responses(
        (status = 200, description = "OAuth login successful", body = OAuthCallbackResponse),
        (status = 302, description = "Redirect to application"),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
#[tracing::instrument(
    // Governance: query carries provider authorization
    // code + state (CSRF) — both secrets. state holds handles;
    // headers carries User-Agent/cookies, ip may be PII.
    // realm_id/provider are low-cardinality but conservatively skipped.
    // Only http.route is recorded.
    skip(state, query, headers, ip),
    fields(http.route = "/api/oauth/{realmId}/{provider}/callback")
)]
pub async fn oauth_callback(
    State(state): State<AppState>,
    Path((realm_id, provider)): Path<(String, String)>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Query(query): Query<OAuthCallbackQuery>,
) -> Result<Response, ApiError> {
    let user_agent = user_agent_from_headers(&headers);
    oauth_callback_inner(state, realm_id, provider, query, user_agent, ip).await
}

pub async fn oauth_callback_form(
    State(state): State<AppState>,
    Path((realm_id, provider)): Path<(String, String)>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Form(query): Form<OAuthCallbackQuery>,
) -> Result<Response, ApiError> {
    let user_agent = user_agent_from_headers(&headers);
    oauth_callback_inner(state, realm_id, provider, query, user_agent, ip).await
}

async fn oauth_callback_inner(
    state: AppState,
    realm_id: String,
    provider: String,
    query: OAuthCallbackQuery,
    user_agent: Option<String>,
    client_ip: String,
) -> Result<Response, ApiError> {
    // Validate provider
    let provider_type = provider.to_lowercase();
    if !matches!(
        provider_type.as_str(),
        "google" | "github" | "facebook" | "apple"
    ) {
        return Err(ApiError::bad_request(format!(
            "Unsupported OAuth provider: {}",
            provider
        )));
    }

    // Validate query parameters
    query
        .validate()
        .map_err(|e| ApiError::bad_request(format!("Validation error: {}", e)))?;

    // Handle OAuth callback
    let method = format!("oauth.{provider_type}");
    let callback = handle_oauth_callback(
        &state,
        realm_id.clone(),
        provider_type,
        query.code,
        query.state,
    )
    .await?;

    if let Some(downstream_state) = callback.downstream_state {
        return match issue_downstream_authorization(
            &state,
            &realm_id,
            callback.user_id,
            &downstream_state,
            &callback.client_id,
            &method,
            user_agent,
            Some(client_ip),
        )
        .await?
        {
            DownstreamCodeOutcome::Redirect(redirect_uri) => {
                Ok(Redirect::temporary(&redirect_uri).into_response())
            }
            DownstreamCodeOutcome::ConsentRequired(response) => Ok(response),
        };
    }

    let user_id = callback.user_id;
    let client_id = callback.client_id;
    issue_callback_token_response(
        &state,
        &realm_id,
        user_id,
        &client_id,
        &method,
        user_agent,
        Some(client_ip),
    )
    .await
}

/// Load the OAuth client app and reject anything not enabled. Shared by the
/// direct-session branch (which additionally requires first-party) and the
/// downstream consent-gate branch.
async fn load_enabled_client_app(
    state: &AppState,
    realm_id: &str,
    client_id: &str,
) -> Result<ClientApp, ApiError> {
    let client_app = state
        .service
        .client_service()
        .get_client_app_by_client_id(realm_id, client_id)
        .await
        .map_err(|_| ApiError::bad_request("OAuth client app is not enabled"))?;
    if !client_app.enabled {
        return Err(ApiError::bad_request("OAuth client app is not enabled"));
    }
    Ok(client_app)
}

/// Load the OAuth login user and apply the shared pre-issuance login policy:
/// the user must exist, belong to `realm_id`, and not be disabled/deleted.
/// Defense in depth — the identity middleware also rejects disabled accounts
/// downstream, but tokens/codes should not be issued at all. WaitVerified
/// users keep access so they can complete email verification, matching the
/// identity middleware. Every rejection is audited as an `auth.login_failed`
/// OAuth event before the error surfaces.
async fn load_gated_oauth_user(
    state: &AppState,
    realm_id: &str,
    user_id: uuid::Uuid,
    method: &str,
    user_agent: Option<&str>,
    client_ip: Option<&str>,
) -> Result<User, ApiError> {
    let user = match state.user_repository.get_user_by_id(user_id).await {
        Ok(user) => user,
        Err(_) => {
            audit_oauth_login_failure(
                state,
                realm_id,
                &user_id.to_string(),
                method,
                "user_not_found",
                client_ip.map(str::to_string),
                user_agent.map(str::to_string),
            )
            .await;
            return Err(ApiError::unauthorized("OAuth user no longer exists"));
        }
    };
    if user.realm_id != realm_id {
        audit_oauth_login_failure(
            state,
            realm_id,
            &user.id.to_string(),
            method,
            "realm_mismatch",
            client_ip.map(str::to_string),
            user_agent.map(str::to_string),
        )
        .await;
        return Err(ApiError::bad_request("OAuth user realm mismatch"));
    }
    if user.status.is_disabled() {
        audit_oauth_login_failure(
            state,
            realm_id,
            &user.id.to_string(),
            method,
            "account_disabled",
            client_ip.map(str::to_string),
            user_agent.map(str::to_string),
        )
        .await;
        return Err(ApiError::unauthorized("Account is disabled"));
    }
    Ok(user)
}

/// Build the consentRequired response shared by both callback branches: a
/// consent-restricted browser family for `client_app` plus the current
/// agreement summaries, in the same 200 body shape as the credential
/// entrances. The OAuth credential is single-use and cannot be replayed with
/// agreements attached, so recovery flows through the restricted family.
async fn consent_required_response(
    state: &AppState,
    user: &User,
    client_app: &ClientApp,
    summaries: Vec<LegalAgreementSummary>,
    user_agent: Option<String>,
    client_ip: Option<String>,
) -> Result<Response, ApiError> {
    let restricted_session =
        mint_consent_restricted_session(state, user, client_app, user_agent, client_ip).await?;
    Ok(Json(OAuthCallbackResponse {
        message: "consent required".to_string(),
        user_id: user.id.to_string(),
        tokens: None,
        consent_required: Some(true),
        agreements: Some(summaries),
        restricted_session: Some(restricted_session.into()),
    })
    .into_response())
}

pub async fn issue_callback_token_response(
    state: &AppState,
    realm_id: &str,
    user_id: uuid::Uuid,
    client_id: &str,
    method: &str,
    user_agent: Option<String>,
    client_ip: Option<String>,
) -> Result<Response, ApiError> {
    let client_app = load_enabled_client_app(state, realm_id, client_id).await?;
    if !client_app.is_first_party {
        return Err(ApiError::bad_request(
            "Third-party OAuth clients must use the authorization-code flow with PKCE",
        ));
    }
    let user = load_gated_oauth_user(
        state,
        realm_id,
        user_id,
        method,
        user_agent.as_deref(),
        client_ip.as_deref(),
    )
    .await?;
    // Consent gate: the OAuth direct-login entrance is NOT exempt from the
    // login-consent rule (legal-consent PRD) — a user whose recorded consent
    // is stale must accept the current agreements before receiving a full
    // session, exactly like the password/OTP/LDAP/passkey entrances. Without
    // this check the "update → re-consent" rule could be bypassed by picking
    // a social entrance.
    if let Some(summaries) = evaluate_login_consent_gate(
        state,
        &user,
        realm_id,
        None,
        client_ip.clone(),
        user_agent.clone(),
    )
    .await
    {
        return consent_required_response(
            state,
            &user,
            &client_app,
            summaries,
            user_agent,
            client_ip,
        )
        .await;
    }
    let token_service = RedisBrowserTokenService::new(state.redis_manager.clone());
    let tokens = token_service
        .create_first_party_token_family(&user, &client_app, user_agent.clone(), client_ip.clone())
        .await
        .map_err(|error| {
            tracing::error!(%error, "Failed to issue browser token family after OAuth callback");
            ApiError::internal("Internal server error")
        })?;
    audit_oauth_login_success(
        state,
        realm_id,
        &user,
        method,
        Some(client_id),
        client_ip,
        user_agent,
    )
    .await;
    Ok(Json(OAuthCallbackResponse {
        message: "OAuth login successful".to_string(),
        user_id: user_id.to_string(),
        tokens: Some(tokens),
        consent_required: None,
        agreements: None,
        restricted_session: None,
    })
    .into_response())
}

/// Outcome of the downstream authorization-code branch shared by every OAuth
/// login entrance (provider callback, WeChat continuation, One Tap, Apple
/// native).
pub enum DownstreamCodeOutcome {
    /// Consent current: the one-time downstream state was consumed and a
    /// downstream authorization code was issued; carry the full redirect URI
    /// (with `code` + `state` query params) back to the caller.
    Redirect(String),
    /// Stale consent: no downstream code was issued. The response body is the
    /// same consentRequired shape as the direct-session branch.
    ConsentRequired(Response),
}

/// Issue a downstream authorization code for the Code+PKCE flow. Runs the
/// login policy shared with [`issue_callback_token_response`] (via
/// [`load_gated_oauth_user`]) BEFORE the one-time downstream state is
/// consumed: disabled-account rejection and the login consent gate. The
/// downstream code path is a login entrance — its `/token` first-party
/// exchange mints the same full browser family the direct branch would, so
/// skipping the gate here would let a stale-consent (or disabled) user reach
/// a complete session through the social entrances while every credential
/// entrance is gated.
///
/// Gated users receive the restricted family + current agreement summaries and
/// recover by recording consent via POST /api/legal/{realmId}/consent and
/// re-triggering the entrance; the downstream state is deliberately NOT
/// consumed so a fresh pass through the flow can complete the authorization.
#[allow(clippy::too_many_arguments)]
pub async fn issue_downstream_authorization(
    state: &AppState,
    realm_id: &str,
    user_id: uuid::Uuid,
    downstream_state: &str,
    client_id: &str,
    method: &str,
    user_agent: Option<String>,
    client_ip: Option<String>,
) -> Result<DownstreamCodeOutcome, ApiError> {
    let user = load_gated_oauth_user(
        state,
        realm_id,
        user_id,
        method,
        user_agent.as_deref(),
        client_ip.as_deref(),
    )
    .await?;
    if let Some(summaries) = evaluate_login_consent_gate(
        state,
        &user,
        realm_id,
        None,
        client_ip.clone(),
        user_agent.clone(),
    )
    .await
    {
        let client_app = load_enabled_client_app(state, realm_id, client_id).await?;
        return Ok(DownstreamCodeOutcome::ConsentRequired(
            consent_required_response(state, &user, &client_app, summaries, user_agent, client_ip)
                .await?,
        ));
    }
    let redirect_uri =
        issue_downstream_authorization_code(state, realm_id, user_id, downstream_state).await?;
    audit_oauth_login_success(
        state,
        realm_id,
        &user,
        method,
        Some(client_id),
        client_ip,
        user_agent,
    )
    .await;
    Ok(DownstreamCodeOutcome::Redirect(redirect_uri))
}

// Governance tests.
//
// Covers: oauth `oauth_callback` (callback.rs), `oauth_token`
// (token.rs), and `handle_oauth_callback` (helper.rs) instrument skip
// correctness.
//
// WHY: the oauth callback/token paths carry the provider authorization `code`,
// the CSRF `state`/`state_token`, PKCE `code_verifier`, and `client_id` — all
// secrets. If the `#[instrument]` macro ever stops skipping those, the secret
// leaks into a span field. Source-scan baseline, anchored per
// function to the immediately-preceding `#[tracing::instrument(...)]`.
#[cfg(test)]
mod instrument_skip_tests {
    const CALLBACK_SRC: &str = include_str!("callback.rs");
    const TOKEN_SRC: &str = include_str!("token.rs");
    const HELPER_SRC: &str = include_str!("helper.rs");

    fn instrument_body_preceding(src: &str, fn_name: &str) -> String {
        let needle = format!("fn {fn_name}");
        let fn_pos = src
            .find(&needle)
            .unwrap_or_else(|| panic!("fn {fn_name} not found in source"));
        let attr_start = src[..fn_pos]
            .rfind("#[tracing::instrument(")
            .unwrap_or_else(|| panic!("no #[tracing::instrument( preceding fn {fn_name}"));
        let body_start = attr_start + "#[tracing::instrument(".len();
        // Find the attribute close: the first line at/after body_start whose
        // trimmed content is exactly `)]`. This handles indented closes (e.g.
        // inside an `impl` block) and ignores inline `))]` sequences such as
        // `#[validate(length(...))]` that appear on struct fields.
        let tail = &src[body_start..];
        let mut consumed = 0usize;
        for line in tail.lines() {
            let prev = consumed;
            consumed += line.len() + 1; // +1 for the line separator
            if line.trim() == ")]" {
                return tail[..prev].to_string();
            }
        }
        panic!("unterminated #[tracing::instrument( for fn {fn_name}")
    }

    #[test]
    fn instrument_skip_oauth_callback_excludes_code_state() {
        let body = instrument_body_preceding(CALLBACK_SRC, "oauth_callback");
        // `query` carries the provider authorization `code` + CSRF `state`.
        for required in ["query", "state"] {
            assert!(
                body.contains(required),
                "oauth_callback must skip `{required}`; body was:\n{body}"
            );
        }
        for banned in ["code", "token", "secret", "email", "password"] {
            assert!(
                !body.contains(&format!("{banned} ="))
                    && !body.contains(&format!("fields({banned}")),
                "oauth_callback span must not record a `{banned}` field; body was:\n{body}"
            );
        }
    }

    #[test]
    fn instrument_skip_oauth_token_excludes_code_and_verifier() {
        let body = instrument_body_preceding(TOKEN_SRC, "oauth_token");
        // `req` carries authorization code, PKCE code_verifier, client_id.
        assert!(
            body.contains("req"),
            "oauth_token must skip `req` (carries auth code / code_verifier / client_id); body was:\n{body}"
        );
        for banned in ["code", "token", "verifier", "secret", "client_id"] {
            assert!(
                !body.contains(&format!("{banned} ="))
                    && !body.contains(&format!("fields({banned}")),
                "oauth_token span must not record a `{banned}` field; body was:\n{body}"
            );
        }
    }

    #[test]
    fn instrument_skip_oauth_helper_excludes_code_and_state_token() {
        let body = instrument_body_preceding(HELPER_SRC, "handle_oauth_callback");
        for required in ["code", "state_token", "realm_id", "state"] {
            assert!(
                body.contains(required),
                "handle_oauth_callback must skip `{required}`; body was:\n{body}"
            );
        }
        for banned in ["token", "secret", "email", "password"] {
            assert!(
                !body.contains(&format!("{banned} ="))
                    && !body.contains(&format!("fields({banned}")),
                "handle_oauth_callback span must not record a `{banned}` field; body was:\n{body}"
            );
        }
    }
}
