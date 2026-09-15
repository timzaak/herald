//! OIDC discovery endpoint (`.well-known/openid-configuration`).
//!
//! Serves the standard provider metadata so a compliant client needs nothing
//! beyond the issuer URL. Field names follow OIDC Discovery (snake_case), a
//! deliberate exception to the project-wide camelCase convention shared with
//! the other protocol-style OAuth endpoints.

use axum::{
    Json,
    extract::{Path, State},
    http::{StatusCode, header},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use herald_api_base::application::http::auth::util::{ClientIp, rate_limit_hit};
use herald_api_base::application::http::common::public_helper::realm_public_url_parts;
use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::security_constants::{
    OAUTH_DISCOVERY_IP_RATE_LIMIT, OIDC_PUBLIC_CACHE_MAX_AGE_SECONDS,
};

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct OidcDiscoveryResponse {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub userinfo_endpoint: String,
    pub jwks_uri: String,
    pub scopes_supported: Vec<String>,
    pub response_types_supported: Vec<String>,
    pub grant_types_supported: Vec<String>,
    pub subject_type: String,
    pub id_token_signing_alg_values_supported: Vec<String>,
    pub token_endpoint_auth_methods_supported: Vec<String>,
    pub claims_supported: Vec<String>,
    pub code_challenge_methods_supported: Vec<String>,
}

/// Origin the issuer is derived from: an enabled custom domain when present,
/// otherwise the configured public base URL. This is the base part of
/// `realm_public_url_parts` — the API routes already carry the `{realmId}`
/// path segment, so the frontend realm prefix must NOT be appended (the
/// resulting URL would hit the SPA fallback and never route to an API).
pub(crate) async fn build_realm_oauth_origin(
    state: &AppState,
    realm_id: &str,
) -> Result<String, ApiError> {
    let (base, _include_realm_prefix) = realm_public_url_parts(state, realm_id).await?;
    Ok(base)
}

/// `{origin}/api/oauth/{realmId}` — the per-realm path-style issuer. The
/// discovery URL is this value plus `/.well-known/openid-configuration`
/// (OIDC Core append semantics).
pub(crate) fn build_issuer(origin: &str, realm_id: &str) -> String {
    format!("{origin}/api/oauth/{realm_id}")
}

pub(crate) async fn ensure_realm_exists(state: &AppState, realm_id: &str) -> Result<(), ApiError> {
    let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM realm WHERE id = $1")
        .bind(realm_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!(realm_id = %realm_id, error = %e, "Realm lookup failed");
            ApiError::internal("Internal server error")
        })?;
    if exists.is_none() {
        // The realm table has no enabled concept; a missing row is the only
        // failure mode and must not yield partial configuration.
        return Err(ApiError::not_found("Realm not found"));
    }
    Ok(())
}

/// 200 with the shared public-cache policy for the OIDC documents (discovery,
/// JWKS): metadata that only changes on signing-key rotation or custom-domain
/// changes, safe for standard clients to cache for `max-age`.
pub(crate) fn public_cached_json<T: Serialize>(body: T) -> axum::response::Response {
    (
        StatusCode::OK,
        [(
            header::CACHE_CONTROL,
            format!("public, max-age={OIDC_PUBLIC_CACHE_MAX_AGE_SECONDS}"),
        )],
        Json(body),
    )
        .into_response()
}

#[utoipa::path(
    get,
    path = "/api/oauth/{realmId}/.well-known/openid-configuration",
    tag = "oauth",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
    ),
    responses(
        (status = 200, description = "OIDC provider metadata for the realm", body = OidcDiscoveryResponse),
        (status = 404, description = "Realm not found", body = ErrorResponse),
        (status = 429, description = "Rate limit exceeded", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    )
)]
pub async fn oidc_discovery(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
) -> Result<impl IntoResponse, ApiError> {
    rate_limit_hit(
        &state,
        format!("rl:oauth-discovery:ip:{ip}"),
        OAUTH_DISCOVERY_IP_RATE_LIMIT.0,
        OAUTH_DISCOVERY_IP_RATE_LIMIT.1,
    )
    .await?;

    // Realm existence and the origin lookup are independent reads — run them
    // concurrently instead of as sequential DB round-trips.
    let ((), origin) = tokio::try_join!(
        ensure_realm_exists(&state, &realm_id),
        build_realm_oauth_origin(&state, &realm_id),
    )?;
    let issuer = build_issuer(&origin, &realm_id);

    let response = OidcDiscoveryResponse {
        issuer: issuer.clone(),
        authorization_endpoint: format!("{issuer}/authorize"),
        token_endpoint: format!("{issuer}/token"),
        userinfo_endpoint: format!("{issuer}/userinfo"),
        jwks_uri: format!("{issuer}/.well-known/jwks.json"),
        // The only scope with semantics is `openid`; other tokens are carried
        // verbatim and ignored, so the advertised list stays honest.
        scopes_supported: vec!["openid".to_string()],
        response_types_supported: vec!["code".to_string()],
        grant_types_supported: vec!["authorization_code".to_string()],
        subject_type: "public".to_string(),
        id_token_signing_alg_values_supported: vec!["RS256".to_string()],
        // /token is a pure PKCE public client and never validates a client
        // secret — declared as-is so strict clients skip the attempt.
        token_endpoint_auth_methods_supported: vec!["none".to_string()],
        claims_supported: [
            "sub",
            "iss",
            "aud",
            "exp",
            "iat",
            "email",
            "email_verified",
            "nickname",
            "nonce",
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
        code_challenge_methods_supported: vec!["S256".to_string()],
    };

    tracing::debug!(realm_id = %realm_id, "OIDC discovery served");

    Ok(public_cached_json(response))
}
