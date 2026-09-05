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

use crate::helper::handle_oauth_callback;
use herald_api_base::application::http::auth::util::{ClientIp, user_agent_from_headers};
use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::{BrowserTokenService, BrowserTokenSet};
use herald_core::domain::client::ports::ClientService;
use herald_core::domain::user::UserRepository;
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
    pub tokens: BrowserTokenSet,
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
    let callback = handle_oauth_callback(
        &state,
        realm_id.clone(),
        provider_type,
        query.code,
        query.state,
    )
    .await?;

    if let Some(redirect_uri) = callback.downstream_redirect_uri {
        return Ok(Redirect::temporary(&redirect_uri).into_response());
    }

    let user_id = callback.user_id;
    let client_id = callback.client_id;
    issue_callback_token_response(
        &state,
        &realm_id,
        user_id,
        &client_id,
        user_agent,
        Some(client_ip),
    )
    .await
}

pub async fn issue_callback_token_response(
    state: &AppState,
    realm_id: &str,
    user_id: uuid::Uuid,
    client_id: &str,
    user_agent: Option<String>,
    client_ip: Option<String>,
) -> Result<Response, ApiError> {
    let client_app = state
        .service
        .client_service()
        .get_client_app_by_client_id(realm_id, client_id)
        .await
        .map_err(|_| ApiError::bad_request("OAuth client app is not enabled"))?;
    if !client_app.enabled {
        return Err(ApiError::bad_request("OAuth client app is not enabled"));
    }
    if !client_app.is_first_party {
        return Err(ApiError::bad_request(
            "Third-party OAuth clients must use the authorization-code flow with PKCE",
        ));
    }
    let user = state
        .user_repository
        .get_user_by_id(user_id)
        .await
        .map_err(|_| ApiError::unauthorized("OAuth user no longer exists"))?;
    if user.realm_id != realm_id {
        return Err(ApiError::bad_request("OAuth user realm mismatch"));
    }
    // Defense in depth: disabled/deleted accounts must not receive new token
    // families on the OAuth direct-session path (inject_token_identity also
    // rejects them downstream, but tokens should not be issued at all).
    // WaitVerified users keep access so they can complete email verification,
    // matching the identity middleware.
    if user.status.is_disabled() {
        return Err(ApiError::unauthorized("Account is disabled"));
    }
    let token_service = RedisBrowserTokenService::new(state.redis_manager.clone());
    let tokens = token_service
        .create_first_party_token_family(&user, &client_app, user_agent, client_ip)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Failed to issue browser token family after OAuth callback");
            ApiError::internal("Internal server error")
        })?;
    Ok(Json(OAuthCallbackResponse {
        message: "OAuth login successful".to_string(),
        user_id: user_id.to_string(),
        tokens,
    })
    .into_response())
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
