//! OIDC userinfo endpoint.
//!
//! Returns the standard identity claims for the bearer of a browser access
//! token — the same claim set (and values) the id_token carries. Accepts both
//! GET and POST per OIDC Core §5.3.1; POST ignores the body entirely and reads
//! the token only from the Authorization header.

use axum::{
    Extension, Json,
    extract::{Path, State},
    http::header::CACHE_CONTROL,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use herald_api_base::application::http::auth::util::{ClientIp, rate_limit_hit};
use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::{CredentialScope, Identity, TokenCredentialContext};
use herald_core::domain::security_constants::OAUTH_USERINFO_IP_RATE_LIMIT;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct OidcUserInfoResponse {
    pub sub: String,
    pub email: String,
    pub email_verified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
}

#[utoipa::path(
    get,
    post,
    path = "/api/oauth/{realmId}/userinfo",
    tag = "oauth",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
    ),
    responses(
        (status = 200, description = "Standard identity claims for the token's user", body = OidcUserInfoResponse),
        (status = 401, description = "Missing, invalid, or realm-mismatched bearer token", body = ErrorResponse),
        (status = 403, description = "Token's flow did not request the openid scope", body = ErrorResponse),
        (status = 429, description = "Rate limit exceeded", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    )
)]
pub async fn oidc_userinfo(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Extension(identity): Extension<Identity>,
    Extension(credential): Extension<TokenCredentialContext>,
) -> Result<Response, ApiError> {
    rate_limit_hit(
        &state,
        format!("rl:oauth-userinfo:ip:{ip}"),
        OAUTH_USERINFO_IP_RATE_LIMIT.0,
        OAUTH_USERINFO_IP_RATE_LIMIT.1,
    )
    .await?;

    // OIDC Core §5.3.1: identity claims are served only for access tokens
    // whose flow requested the `openid` scope. A plain OAuth token (or a
    // first-party console session) authenticates fine but must not read the
    // identity surface — 403, not 401, so clients can tell an invalid token
    // apart from a scope gate. Checked directly against the scope set (not
    // require_token_scope) because FirstParty credentials pass that gate
    // unconditionally and must NOT pass this one.
    if !credential.allowed_scopes.contains(&CredentialScope::Openid) {
        return Err(ApiError::forbidden("openid scope required"));
    }

    let user = identity
        .as_user()
        .ok_or_else(|| ApiError::unauthorized("invalid bearer token"))?;
    // The middleware already guarantees the token's user realm matches the
    // token; the path realm must match too, or one realm's endpoint must not
    // answer for another realm's tokens.
    if user.realm_id != realm_id {
        return Err(ApiError::unauthorized("invalid bearer token"));
    }

    // nickname comes from the same shared lookup the id_token uses, so the
    // two identity surfaces cannot drift.
    let nickname = crate::oidc_id_token::profile_nickname(&state, user).await?;

    tracing::debug!(realm_id = %realm_id, user_id = %user.id, "OIDC userinfo served");

    // OIDC Core §5.3.2: userinfo responses carry PII and must never be
    // stored by shared caches along the way.
    Ok((
        [(CACHE_CONTROL, "no-store")],
        Json(OidcUserInfoResponse {
            sub: user.id.to_string(),
            email: user.email.clone(),
            email_verified: crate::oidc_id_token::email_verified_from_status(&user.status),
            nickname,
        }),
    )
        .into_response())
}
