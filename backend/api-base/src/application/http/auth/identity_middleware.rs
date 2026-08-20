use crate::application::http::common::auth_utils::{
    require_admin_console_credential, require_first_party_credential,
};
use crate::application::http::server::api_entities::ApiError;
use crate::application::http::state::AppState;
use axum::{
    extract::{Request, State},
    http::HeaderMap,
    middleware::Next,
    response::IntoResponse,
};
use herald_core::domain::authentication::{BrowserTokenService, Identity, TokenCredentialContext};
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

pub async fn authenticate_bearer(
    state: &AppState,
    headers: &HeaderMap,
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

    let client = sqlx::query_as::<_, (bool, String)>(
        "SELECT enabled, client_id FROM client_app WHERE id = $1 AND realm_id = $2",
    )
    .bind(token_data.client_app_id)
    .bind(&token_data.realm_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| {
        tracing::error!(%error, "Browser token Client App lookup failed");
        ApiError::internal("Internal server error")
    })?
    .ok_or_else(|| ApiError::unauthorized("invalid bearer token"))?;
    if !client.0 {
        return Err(ApiError::unauthorized("invalid bearer token"));
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
    if matches!(
        user.status,
        herald_core::domain::user::entities::UserStatus::Forbidden
            | herald_core::domain::user::entities::UserStatus::Deleted
    ) {
        return Err(ApiError::unauthorized("invalid bearer token"));
    }

    let credential_context = TokenCredentialContext {
        client_app_id: token_data.client_app_id,
        client_id: client.1,
        family_id: token_data.family_id,
        credential_class: token_data.credential_class,
        allowed_scopes: token_data.allowed_scopes,
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
