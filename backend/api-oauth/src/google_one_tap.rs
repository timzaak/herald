// Google One Tap login handler (GIS ID Token verification path).
//
// Mirrors `wechat_miniprogram_login` (direct POST, no redirect) and
// `oauth_callback` (direct-session vs downstream-code branching). Reuses
// `verify_google_id_token` for ID Token verification and the existing
// `find_or_create_user` / `issue_callback_token_response` /
// `issue_downstream_authorization_code` helpers so One Tap produces the same
// Herald account as the redirect Google login path.

use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use crate::callback::issue_callback_token_response;
use crate::helper::{find_or_create_user, issue_downstream_authorization_code};
use herald_api_base::application::http::auth::util::{
    ClientIp, rate_limit_hit, user_agent_from_headers,
};
use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::BrowserTokenSet;
use herald_core::domain::oauth::entities::ProviderType;
use herald_core::domain::oauth::value_objects::OAuthUserInfo;
use herald_core::domain::security_constants::OAUTH_UPSTREAM_LOGIN_IP_RATE_LIMIT;
// `StringOrBool` lives on the nested `google` module; the `providers/mod.rs`
// re-export only lifts the free `verify_google_id_token` function, not this
// enum, so reference it via the submodule path.
use herald_core::infrastructure::oauth::google::StringOrBool;
use herald_core::infrastructure::oauth::{ReqwestHttpClient, verify_google_id_token};

#[derive(Debug, Deserialize, Serialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct OneTapRequest {
    /// Google One Tap issued ID Token (JWT), from the GIS `credential` callback.
    #[validate(length(min = 1))]
    pub credential: String,
    /// Herald ClientApp `client_id` that the direct-session token family binds to.
    #[validate(length(min = 1))]
    pub client_id: String,
    /// Optional downstream authorization transaction identifier (OAuth `state`).
    /// Presence selects the downstream-authorization-code branch; absence the
    /// direct-session branch.
    pub downstream_state: Option<String>,
}

// `OneTapDirectResponse` mirrors the shape of `OAuthCallbackResponse`:
// `message` + `user_id` + flattened `BrowserTokenSet`. It exists only for the
// OpenAPI schema — the direct-session branch reuses
// `issue_callback_token_response`, whose body is already this shape, so this
// struct is never constructed at runtime (`#[allow(dead_code)]`).
// Consent-gate variant (stale legal consent): the runtime body additionally
// carries `consentRequired: true` + `agreements` + `restrictedSession` and NO
// token fields — not modeled here, same documented single-200-shape limitation
// as the utoipa annotation below.
#[allow(dead_code)]
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OneTapDirectResponse {
    pub message: String,
    pub user_id: String,
    #[serde(flatten)]
    pub tokens: BrowserTokenSet,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OneTapCodeResponse {
    /// Full redirect URI returned by `issue_downstream_authorization_code`,
    /// including `?code=ac_...&state=...` for the downstream Code+PKCE flow.
    pub redirect_uri: String,
}

/// Google One Tap login.
///
/// Verifies a Google One Tap (GIS) ID Token server-side and, depending on
/// `downstreamState`, issues either a direct browser session (Bearer token
/// family bound to `clientId`) or a downstream authorization code for the
/// Code+PKCE flow. Requires the realm to have an enabled Google provider.
#[utoipa::path(
    post,
    path = "/api/oauth/{realmId}/google/one-tap",
    tag = "oauth",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = OneTapRequest,
    responses(
        (status = 200, description = "Direct-session mode: token family issued for the given client_id", body = OneTapDirectResponse),
        (status = 200, description = "Downstream-authorization-code mode: redirect URI returned for Code+PKCE exchange", body = OneTapCodeResponse),
        (status = 400, description = "Bad request (validation error or missing/invalid downstream state)", body = ErrorResponse),
        (status = 401, description = "Unauthorized (ID Token signature/issuer/audience/expiry validation failed, or email not verified)", body = ErrorResponse),
        (status = 404, description = "Google provider not configured or not enabled", body = ErrorResponse),
        (status = 503, description = "Upstream service unavailable (Google JWKS unreachable)", body = ErrorResponse)
    )
)]
pub async fn google_one_tap(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(payload): Json<OneTapRequest>,
) -> Result<Response, ApiError> {
    let user_agent = user_agent_from_headers(&headers);

    // `payload.credential` is a Google ID Token — a secret. Never record it in
    // span fields; log only low-cardinality context (realm_id, provider,
    // failure category, user_id).
    payload
        .validate()
        .map_err(|e| ApiError::bad_request(format!("Validation error: {}", e)))?;

    tracing::info!(
        realm_id = %realm_id,
        provider = "google",
        "Google One Tap login request"
    );

    // Per-IP cap before any upstream call: verification fetches Google JWKS,
    // so unthrottled requests amplify into outbound HTTPS traffic.
    rate_limit_hit(
        &state,
        format!("rl:oauth-one-tap:ip:{ip}"),
        OAUTH_UPSTREAM_LOGIN_IP_RATE_LIMIT.0,
        OAUTH_UPSTREAM_LOGIN_IP_RATE_LIMIT.1,
    )
    .await?;

    let config = state
        .service
        .oauth_config_service()
        .list_enabled_providers(&realm_id)
        .await
        .map_err(|e| {
            tracing::error!(realm_id = %realm_id, error = %e, "Failed to list OAuth providers");
            ApiError::internal("Failed to get provider configuration".to_string())
        })?
        .into_iter()
        .find(|c| c.provider_type.as_str() == "google")
        .ok_or_else(|| {
            ApiError::not_found("Google provider not configured or not enabled".to_string())
        })?;

    // `verify_google_id_token` returns `CoreError::BadRequest` for
    // signature/issuer/audience/expiry failures and `InternalServerError` when
    // Google JWKS is unreachable. Map the former to 401 (validation failure)
    // and the latter to 503 (upstream unavailable) — a 401 here would wrongly
    // blame the caller for an upstream outage.
    let http_client = ReqwestHttpClient::from_client(state.http_client.clone());

    let claims = verify_google_id_token(
        &payload.credential,
        &config.client_id,
        &http_client,
        &state.google_jwks_url,
    )
    .await
    .map_err(|err| {
        use herald_core::domain::common::entities::app_errors::CoreError;
        match err {
            CoreError::BadRequest(msg) => {
                tracing::warn!(
                    realm_id = %realm_id,
                    provider = "google",
                    failure = "id_token_validation",
                    "Google One Tap ID Token rejected"
                );
                ApiError::unauthorized(msg)
            }
            CoreError::InternalServerError(msg) => {
                tracing::error!(
                    realm_id = %realm_id,
                    provider = "google",
                    failure = "jwks_unreachable",
                    error = %msg,
                    "Google JWKS unreachable"
                );
                ApiError::with_error_code(
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    "upstream_error",
                    "Upstream service unavailable",
                )
            }
            other => {
                tracing::error!(
                    realm_id = %realm_id,
                    provider = "google",
                    error = %other,
                    "Unexpected error verifying Google ID Token"
                );
                ApiError::internal("Internal server error".to_string())
            }
        }
    })?;

    // Reject unverified email. `verify_google_id_token` only decodes claims;
    // the "reject unverified" policy lives in the handler. Google may serialize
    // `email_verified` as a bool OR the string "true"/"false" — an `or`-pattern
    // cannot share a single `if` guard when only one arm binds `s`, so the two
    // cases are matched explicitly.
    let email_verified = match claims.email_verified {
        Some(StringOrBool::Bool(true)) => true,
        Some(StringOrBool::Str(ref s)) => s == "true",
        _ => false,
    };
    if !email_verified {
        tracing::warn!(
            realm_id = %realm_id,
            provider = "google",
            failure = "email_not_verified",
            "Google One Tap rejected: email not verified"
        );
        return Err(ApiError::unauthorized(
            "Email not verified by Google".to_string(),
        ));
    }

    let email = claims
        .email
        .as_deref()
        .filter(|email| !email.trim().is_empty())
        .ok_or_else(|| ApiError::unauthorized("Google ID token is missing email"))?
        .to_string();

    // Match/create the Herald user with the same key as the redirect Google
    // login path: `open_id: Some(claims.sub)` ≡ redirect path's
    // `open_id: Some(user_info.id)`.
    let user_info = OAuthUserInfo {
        provider_type: ProviderType::Google,
        provider_user_id: claims.sub.clone(),
        email,
        verified: true,
        avatar: claims.picture,
        name: claims.name,
        union_id: None,
        open_id: Some(claims.sub),
    };

    let user_id = find_or_create_user(&state, &realm_id, &user_info).await?;

    tracing::info!(
        realm_id = %realm_id,
        provider = "google",
        user_id = %user_id,
        "Google One Tap user authenticated"
    );

    match payload.downstream_state {
        Some(ds) => {
            let redirect_uri =
                issue_downstream_authorization_code(&state, &realm_id, user_id, &ds).await?;
            Ok(Json(OneTapCodeResponse { redirect_uri }).into_response())
        }
        None => {
            issue_callback_token_response(
                &state,
                &realm_id,
                user_id,
                &payload.client_id,
                user_agent,
                Some(ip),
            )
            .await
        }
    }
}
