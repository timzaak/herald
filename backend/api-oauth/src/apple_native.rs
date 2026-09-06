// Apple native (Sign in with Apple) login handler — in-app, no redirect.
//
// Mirrors `google_one_tap` (direct POST, no redirect) and the OAuth callback
// direct-session vs downstream-code branching. Reuses `verify_apple_id_token`
// for ID Token verification and the existing `find_or_create_user` /
// `issue_callback_token_response` / `issue_downstream_authorization_code`
// helpers so Apple native login produces the same Herald account as the Apple
// web redirect path (same match key: `open_id: Some(claims.sub)`).
//
// Unlike Google One Tap, the Apple native path never calls Apple's token
// endpoint and never uses `client_secret`: the iOS app obtains the
// `identityToken` directly via `ASAuthorizationAppleIDProvider` and submits it
// here, where Herald validates signature/issuer/audience/expiry against Apple
// JWKS.

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
use herald_core::infrastructure::oauth::{ReqwestHttpClient, verify_apple_id_token};

#[derive(Debug, Deserialize, Serialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct AppleNativeRequest {
    /// Apple-issued identity token (JWT) from `ASAuthorizationAppleIDProvider`.
    #[validate(length(min = 1))]
    pub identity_token: String,
    /// Herald ClientApp `client_id` that the direct-session token family binds to.
    #[validate(length(min = 1))]
    pub client_id: String,
    /// Optional downstream authorization transaction identifier (OAuth `state`).
    /// Presence selects the downstream-authorization-code branch; absence the
    /// direct-session branch.
    pub downstream_state: Option<String>,
}

// `AppleNativeDirectResponse` mirrors the shape of `OAuthCallbackResponse`:
// `message` + `user_id` + flattened `BrowserTokenSet`. It exists only for the
// OpenAPI schema — the direct-session branch reuses
// `issue_callback_token_response`, whose body is already this shape, so this
// struct is never constructed at runtime (`#[allow(dead_code)]`).
// Consent-gate variant (stale legal consent): the runtime body additionally
// carries `consentRequired: true` + `agreements` + `restrictedSession` and NO
// token fields — not modeled here.
#[allow(dead_code)]
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppleNativeDirectResponse {
    pub message: String,
    pub user_id: String,
    #[serde(flatten)]
    pub tokens: BrowserTokenSet,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppleNativeCodeResponse {
    /// Full redirect URI returned by `issue_downstream_authorization_code`,
    /// including `?code=ac_...&state=...` for the downstream Code+PKCE flow.
    pub redirect_uri: String,
}

/// Apple native login.
///
/// Verifies an Apple `identityToken` (from `ASAuthorizationAppleIDProvider`)
/// server-side and, depending on `downstreamState`, issues either a direct
/// browser session (Bearer token family bound to `clientId`) or a downstream
/// authorization code for the Code+PKCE flow. Requires the realm to have an
/// enabled Apple provider.
#[utoipa::path(
    post,
    path = "/api/oauth/{realmId}/apple/native-login",
    tag = "oauth",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = AppleNativeRequest,
    responses(
        (status = 200, description = "Direct-session mode: token family issued for the given client_id", body = AppleNativeDirectResponse),
        (status = 200, description = "Downstream-authorization-code mode: redirect URI returned for Code+PKCE exchange", body = AppleNativeCodeResponse),
        (status = 400, description = "Bad request (validation error or missing/invalid downstream state)", body = ErrorResponse),
        (status = 401, description = "Unauthorized (identityToken signature/issuer/audience/expiry validation failed)", body = ErrorResponse),
        (status = 404, description = "Apple provider not configured or not enabled", body = ErrorResponse),
        (status = 503, description = "Upstream service unavailable (Apple JWKS unreachable)", body = ErrorResponse)
    )
)]
pub async fn apple_native_login(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(payload): Json<AppleNativeRequest>,
) -> Result<Response, ApiError> {
    let user_agent = user_agent_from_headers(&headers);

    // `payload.identityToken` is a secret. Never record it in span fields; log
    // only low-cardinality context (realm_id, provider, failure category,
    // user_id).
    payload
        .validate()
        .map_err(|e| ApiError::bad_request(format!("Validation error: {}", e)))?;

    tracing::info!(
        realm_id = %realm_id,
        provider = "apple",
        "Apple native login request"
    );

    // Per-IP cap before any upstream call: verification fetches Apple JWKS,
    // so unthrottled requests amplify into outbound HTTPS traffic.
    rate_limit_hit(
        &state,
        format!("rl:oauth-apple-native:ip:{ip}"),
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
        .find(|c| c.provider_type.as_str() == "apple")
        .ok_or_else(|| {
            ApiError::not_found("Apple provider not configured or not enabled".to_string())
        })?;

    // `verify_apple_id_token` returns `CoreError::BadRequest` for
    // signature/issuer/audience/expiry failures and `InternalServerError` when
    // Apple JWKS is unreachable. Map the former to 401 (validation failure)
    // and the latter to 503 (upstream unavailable) — a 401 here would wrongly
    // blame the caller for an upstream outage.
    let http_client = ReqwestHttpClient::from_client(state.http_client.clone());

    let claims = verify_apple_id_token(
        &payload.identity_token,
        &config.client_id,
        &http_client,
        &state.apple_jwks_url,
    )
    .await
    .map_err(|err| {
        use herald_core::domain::common::entities::app_errors::CoreError;
        match err {
            CoreError::BadRequest(msg) => {
                tracing::warn!(
                    realm_id = %realm_id,
                    provider = "apple",
                    failure = "id_token_validation",
                    "Apple identity token rejected"
                );
                ApiError::unauthorized(msg)
            }
            CoreError::InternalServerError(msg) => {
                tracing::error!(
                    realm_id = %realm_id,
                    provider = "apple",
                    failure = "jwks_unreachable",
                    error = %msg,
                    "Apple JWKS unreachable"
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
                    provider = "apple",
                    error = %other,
                    "Unexpected error verifying Apple identity token"
                );
                ApiError::internal("Internal server error".to_string())
            }
        }
    })?;

    // DEC-005: email handling. Apple's relay address
    // (`@privaterelay.appleid.apple.com`) is a real, deliverable mailbox and is
    // stored as the user's real email. When the identity token carries no email
    // (non-first authorization or hidden email) AND no existing provider record
    // matches this `sub`, `find_or_create_user` would have nothing to put in
    // the NOT NULL `account.email` — so synthesize a placeholder
    // `{sub}@apple.placeholder` with `verified=false`, mirroring the WeChat
    // placeholder pattern. Returning users (existing provider record) are
    // matched by `open_id` before email is consulted, so an empty email does
    // not block them. This intentionally differs from the Apple web redirect
    // path, which rejects an empty email.
    let email = match claims.email {
        Some(e) if !e.is_empty() => e,
        _ => format!("{}@apple.placeholder", claims.sub),
    };
    let verified = claims.email_verified.as_deref() == Some("true");

    // Match/create the Herald user with the same key as the Apple web redirect
    // path: `open_id: Some(claims.sub)` ≡ redirect path's open_id.
    let user_info = OAuthUserInfo {
        provider_type: ProviderType::Apple,
        provider_user_id: claims.sub.clone(),
        email,
        verified,
        avatar: None,   // Apple doesn't provide an avatar
        name: None,     // name only on first login; native id_token doesn't carry it
        union_id: None, // Apple doesn't provide a union id
        open_id: Some(claims.sub),
    };

    let user_id = find_or_create_user(&state, &realm_id, &user_info).await?;

    tracing::info!(
        realm_id = %realm_id,
        provider = "apple",
        user_id = %user_id,
        "Apple native login user authenticated"
    );

    match payload.downstream_state {
        Some(ds) => {
            let redirect_uri =
                issue_downstream_authorization_code(&state, &realm_id, user_id, &ds).await?;
            Ok(Json(AppleNativeCodeResponse { redirect_uri }).into_response())
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
