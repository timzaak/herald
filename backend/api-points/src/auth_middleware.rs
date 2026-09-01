// Flexible Authentication Middleware for Points API
//
// This middleware supports Bearer and API key authentication for points endpoints.
//
// Priority:
// 1. First tries API key authentication (X-API-Key header)
// 2. Falls back to Bearer authentication
// 3. Returns 401 if neither authentication method succeeds

use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use herald_api_base::application::http::common::api_key_utils::validate_api_key;
use herald_api_base::application::http::common::error_codes::ErrorCode;
use herald_api_base::application::http::server::api_entities::ApiError;
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::{CredentialClass, Identity, TokenCredentialContext};
use std::collections::HashSet;
use tracing::debug;
use uuid::Uuid;

/// Try API key authentication
///
/// The validation orchestration (cache → DB → status → Client App cascade →
/// usage stats) lives in api-base `api_key_utils::validate_api_key`; this
/// wrapper only extracts the header and maps the outcome: `Ok(None)` means
/// no API key is present (fall through to Bearer), `Err` means a key was
/// present but failed validation.
#[tracing::instrument(
    // Governance: `headers` carry the raw X-API-Key credential.
    skip(state, headers)
)]
async fn try_api_key_auth(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<Identity>, ApiError> {
    // Extract X-API-Key header
    let api_key_header = match headers.get("X-API-Key").and_then(|v| v.to_str().ok()) {
        Some(key) => key,
        None => return Ok(None), // No API key, try session auth
    };

    let api_key = api_key_header.trim();
    if api_key.is_empty() {
        return Ok(None); // Empty API key, try session auth
    }

    match validate_api_key(state, api_key).await {
        Ok(entity) => Ok(Some(Identity::ThirdParty(entity))),
        Err(e) => Err(ApiError::with_error_code(
            e.status_code(),
            e.error_code().as_str(),
            e.error_code().to_string(),
        )),
    }
}

/// Flexible authentication middleware that supports both API key and Bearer auth.
///
/// Authentication flow:
/// 1. Check for X-API-Key header → authenticate as ThirdParty
/// 2. If no API key, check for a Bearer token → authenticate as User
/// 3. Return 401 if neither authentication method succeeds
///
/// This allows:
/// - Third-party clients to use API keys (consume points, webhooks)
/// - Browser users to access points endpoints with scoped Bearer tokens
#[tracing::instrument(
    // Governance: request headers carry API-key or Bearer credentials.
    skip(state, req, next),
    fields(http.route = "points_flexible_auth")
)]
pub async fn flexible_auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    // Try API key authentication first
    let api_key_auth_result = try_api_key_auth(&state, req.headers()).await;

    match api_key_auth_result {
        Ok(Some(identity)) => {
            // Insert a synthetic TokenCredentialContext so downstream handlers that
            // extract it do not fail at the axum extractor layer. API keys are not
            // browser-token users, so scopes are empty and user-scoped endpoints
            // will still be rejected by require_token_scope/require_authenticated_user.
            req.extensions_mut().insert(identity.clone());
            req.extensions_mut().insert(TokenCredentialContext {
                client_app_id: Uuid::nil(),
                client_id: String::new(),
                family_id: Uuid::nil(),
                credential_class: CredentialClass::CustomUserUi,
                allowed_scopes: HashSet::new(),
            });
            return next.run(req).await;
        }
        Ok(None) => {}
        Err(e) => {
            debug!("API key auth failed: {}", e);
        }
    }

    match herald_api_base::application::http::auth::identity_middleware::authenticate_bearer(
        &state,
        req.headers(),
    )
    .await
    {
        Ok((identity, credential_context)) => {
            req.extensions_mut().insert(identity);
            req.extensions_mut().insert(credential_context);
            next.run(req).await
        }
        Err(_) => {
            // Both authentication methods failed
            ApiError::with_error_code(
                StatusCode::UNAUTHORIZED,
                ErrorCode::Unauthorized.as_str(),
                "Authentication required (API key or Bearer token)",
            )
            .into_response()
        }
    }
}

// Governance tests.
//
// Covers: points `flexible_auth_middleware` + `try_api_key_auth`
// (auth_middleware.rs), `grant_points` (grant.rs), `list_transactions`
// (transactions.rs) instrument skip correctness.
//
// WHY: the auth middleware reads API-key or Bearer credentials from
// request headers — credentials. The grant/transactions handlers carry
// `identity` (user_id/realm_id) and the request body/query (target user_id).
// If the `#[instrument]` macro ever stops skipping those, the credential/PII
// leaks into a span field. Source-scan baseline, anchored per
// function to the immediately-preceding `#[tracing::instrument(...)]`.
#[cfg(test)]
mod instrument_skip_tests {
    const AUTH_SRC: &str = include_str!("auth_middleware.rs");
    const GRANT_SRC: &str = include_str!("grant.rs");
    const TX_SRC: &str = include_str!("transactions.rs");

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
    fn instrument_skip_points_flexible_auth_excludes_api_key_and_token() {
        let body = instrument_body_preceding(AUTH_SRC, "flexible_auth_middleware");
        // `req` headers carry API-key or Bearer credentials.
        for required in ["req", "state", "next"] {
            assert!(
                body.contains(required),
                "flexible_auth_middleware must skip `{required}`; body was:\n{body}"
            );
        }
        for banned in ["token", "api_key", "apikey", "secret", "password", "auth"] {
            assert!(
                !body.contains(&format!("{banned} ="))
                    && !body.contains(&format!("fields({banned}")),
                "flexible_auth_middleware span must not record a `{banned}` field; body was:\n{body}"
            );
        }
    }

    #[test]
    fn instrument_skip_points_try_api_key_auth_excludes_headers() {
        let body = instrument_body_preceding(AUTH_SRC, "try_api_key_auth");
        // `headers` carry the raw X-API-Key credential.
        assert!(
            body.contains("headers"),
            "try_api_key_auth must skip `headers` (raw X-API-Key); body was:\n{body}"
        );
        for banned in ["token", "api_key", "apikey", "secret"] {
            assert!(
                !body.contains(&format!("{banned} ="))
                    && !body.contains(&format!("fields({banned}")),
                "try_api_key_auth span must not record a `{banned}` field; body was:\n{body}"
            );
        }
    }

    #[test]
    fn instrument_skip_points_grant_excludes_identity_and_body() {
        let body = instrument_body_preceding(GRANT_SRC, "grant_points");
        // Uses `skip_all` — assert that's still the case (covers identity,
        // realm_id, request body which carries target user_id).
        assert!(
            body.contains("skip_all"),
            "grant_points must use skip_all (identity carries user_id/realm_id; body carries target user_id); body was:\n{body}"
        );
        for banned in ["token", "password", "email", "secret", "user_id"] {
            assert!(
                !body.contains(&format!("{banned} ="))
                    && !body.contains(&format!("fields({banned}")),
                "grant_points span must not record a `{banned}` field; body was:\n{body}"
            );
        }
    }

    #[test]
    fn instrument_skip_points_list_transactions_excludes_identity_and_filters() {
        let body = instrument_body_preceding(TX_SRC, "list_transactions");
        assert!(
            body.contains("skip_all"),
            "list_transactions must use skip_all (identity + query filters carry user_id/bucket_id); body was:\n{body}"
        );
        for banned in ["token", "password", "email", "secret", "user_id"] {
            assert!(
                !body.contains(&format!("{banned} ="))
                    && !body.contains(&format!("fields({banned}")),
                "list_transactions span must not record a `{banned}` field; body was:\n{body}"
            );
        }
    }
}
