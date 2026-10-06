use crate::application::http::common::auth_utils::{
    TokenClientAppLookup, lookup_token_client_app, require_admin_console_credential,
    require_first_party_credential,
};
use crate::application::http::server::api_entities::ApiError;
use crate::application::http::state::AppState;
use axum::{
    extract::{Request, State},
    http::HeaderMap,
    middleware::Next,
    response::IntoResponse,
};
use herald_core::domain::authentication::{
    BrowserTokenService, CredentialClass, ExpectedTokenAudience, Identity, TokenCredentialContext,
};
use herald_core::domain::user::UserRepository;
use herald_core::infrastructure::authentication::RedisBrowserTokenService;
use uuid::Uuid;

/// Inject a user identity and its browser-token credential context from a Bearer access token.
#[tracing::instrument(
    skip(state, req, next),
    fields(http.route = "inject_token_identity")
)]
pub async fn inject_token_identity(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<impl IntoResponse, ApiError> {
    let (identity, credential_context) = authenticate_bearer(&state, req.headers()).await?;
    let mut req = req;
    req.extensions_mut().insert(identity);
    req.extensions_mut().insert(credential_context);
    Ok(next.run(req).await)
}

/// Shared Bearer validation for the default (browser-only) credential face:
/// every `/api/*` Bearer surface. MCP credentials are rejected here.
pub async fn authenticate_bearer(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(Identity, TokenCredentialContext), ApiError> {
    authenticate_bearer_for(state, headers, &ExpectedTokenAudience::BrowserOnly).await
}

/// Bearer validation parameterized by the credential face the calling surface
/// accepts. `/api/*` routes use [`authenticate_bearer`] (BrowserOnly); the
/// MCP transport passes `McpResource(canonical)` and additionally enforces
/// the token's generation against the live client_app row, so a disabled
/// (generation-bumped) MCP client's credentials die even if the Redis family
/// is still alive.
pub async fn authenticate_bearer_for(
    state: &AppState,
    headers: &HeaderMap,
    expected: &ExpectedTokenAudience,
) -> Result<(Identity, TokenCredentialContext), ApiError> {
    let authorization = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::unauthorized("missing bearer token"))?;
    let (scheme, access_token) = authorization
        .split_once(' ')
        .filter(|(scheme, token)| {
            scheme.eq_ignore_ascii_case("Bearer") && !token.is_empty() && !token.contains(' ')
        })
        .ok_or_else(|| ApiError::unauthorized("invalid bearer token"))?;
    debug_assert!(scheme.eq_ignore_ascii_case("Bearer"));

    let token_service = RedisBrowserTokenService::new(state.redis_manager.clone());
    let token_data = token_service
        .lookup_access_token(access_token)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Browser access token lookup failed");
            ApiError::internal("Internal server error")
        })?
        .ok_or_else(|| ApiError::unauthorized("invalid bearer token"))?;

    let TokenClientAppLookup::Active {
        client_id,
        mcp_token_generation,
    } = lookup_token_client_app(state, token_data.client_app_id, &token_data.realm_id).await?
    else {
        return Err(ApiError::unauthorized("invalid bearer token"));
    };

    // Credential-face gate: audience and class must match the surface. This
    // is the bidirectional isolation point — a browser credential never
    // carries an MCP audience, and an MCP credential is valid ONLY with the
    // exact canonical resource URI and a generation that still matches the
    // live client row (a disable bumped it).
    match expected {
        ExpectedTokenAudience::BrowserOnly => {
            if token_data.credential_class == CredentialClass::Mcp || token_data.audience.is_some()
            {
                return Err(ApiError::unauthorized("invalid bearer token"));
            }
        }
        ExpectedTokenAudience::McpResource(canonical) => {
            if token_data.credential_class != CredentialClass::Mcp
                || token_data.audience.as_deref() != Some(canonical.as_str())
            {
                return Err(ApiError::unauthorized("invalid bearer token"));
            }
            if token_data.mcp_token_generation != Some(mcp_token_generation) {
                tracing::warn!(
                    client_app_id = %token_data.client_app_id,
                    "MCP token rejected: stale generation (client disabled/re-enabled)"
                );
                return Err(ApiError::unauthorized("invalid bearer token"));
            }
        }
    }

    let user_id = Uuid::parse_str(&token_data.user_id)
        .map_err(|_| ApiError::unauthorized("invalid bearer token"))?;
    let user = state
        .user_repository
        .get_user_by_id(user_id)
        .await
        .map_err(|error| match error {
            herald_core::domain::common::entities::app_errors::CoreError::NotFound => {
                ApiError::unauthorized("invalid bearer token")
            }
            error => {
                tracing::error!(%error, %user_id, "Browser token user lookup failed");
                ApiError::internal("Internal server error")
            }
        })?;
    if user.realm_id != token_data.realm_id {
        return Err(ApiError::unauthorized("invalid bearer token"));
    }
    // Defense in depth: status transitions are expected to revoke token
    // families when they happen, but a direct-DB edit or a future code path
    // that flips status without revocation must not leave the account usable.
    // WaitVerified users keep access so they can complete email verification.
    if user.status.is_disabled() {
        return Err(ApiError::unauthorized("invalid bearer token"));
    }

    let credential_context = TokenCredentialContext {
        client_app_id: token_data.client_app_id,
        client_id,
        family_id: token_data.family_id,
        credential_class: token_data.credential_class,
        allowed_scopes: token_data.allowed_scopes,
        audience: token_data.audience,
    };
    Ok((Identity::User(user), credential_context))
}

pub async fn require_admin_console_token(
    req: Request,
    next: Next,
) -> Result<impl IntoResponse, ApiError> {
    let credential_context = req
        .extensions()
        .get::<TokenCredentialContext>()
        .ok_or_else(|| ApiError::unauthorized("missing bearer token context"))?;
    require_admin_console_credential(credential_context)?;
    Ok(next.run(req).await)
}

/// Reject browser credentials that were not issued to Herald's first-party UI.
/// Mount this inside `inject_token_identity` so the credential context already
/// exists when this guard runs.
pub async fn require_first_party_token(
    req: Request,
    next: Next,
) -> Result<impl IntoResponse, ApiError> {
    let credential_context = req
        .extensions()
        .get::<TokenCredentialContext>()
        .ok_or_else(|| ApiError::unauthorized("missing bearer token context"))?;
    require_first_party_credential(credential_context)?;
    Ok(next.run(req).await)
}
