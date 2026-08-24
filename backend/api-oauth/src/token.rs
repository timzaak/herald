// OAuth token endpoint for authorization code exchange with PKCE validation
//
// Browser clients exchange an authorization code (obtained via the authorize + login flow)
// for a Bearer token family. PKCE ensures the code cannot be intercepted and reused.

use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use herald_api_base::application::http::auth::util::{
    ClientIp, rate_limit_hit, user_agent_from_headers,
};
use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::BrowserTokenService;
use herald_core::domain::client::ports::ClientService;
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::security_constants::OAUTH_TOKEN_IP_RATE_LIMIT;
use herald_core::domain::user::UserRepository;
use herald_core::infrastructure::authentication::RedisBrowserTokenService;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utoipa::ToSchema;

const FIRST_PARTY_CALLBACK_PATH: &str = "/callback";

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

/// OAuth 2.0 token request (RFC 6749)
///
/// Field names use snake_case per OAuth 2.0 specification rather than the
/// project-wide camelCase convention.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct TokenRequest {
    pub grant_type: String,
    pub code: String,
    pub redirect_uri: String,
    pub client_id: String,
    pub code_verifier: String,
}

/// OAuth 2.0 token response (RFC 6749)
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub refresh_expires_in: u64,
}

// ---------------------------------------------------------------------------
// PKCE verification
// ---------------------------------------------------------------------------

/// Verify PKCE code_verifier against stored code_challenge.
///
/// Computes BASE64URL(SHA256(code_verifier)) and compares to the stored challenge.
fn verify_pkce(code_verifier: &str, code_challenge: &str) -> bool {
    let mut hasher = Sha256::new();
    hasher.update(code_verifier.as_bytes());
    let hash = hasher.finalize();
    let computed = URL_SAFE_NO_PAD.encode(hash);
    computed == code_challenge
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/api/oauth/{realmId}/token",
    tag = "oauth",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
    ),
    request_body = TokenRequest,
    responses(
        (status = 200, description = "Access token issued", body = TokenResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
    )
)]
#[tracing::instrument(
    // Governance: req carries authorization code, PKCE
    // code_verifier, client_id — all credentials/secrets. state holds handles;
    // realm_id conservatively skipped; headers carries User-Agent/cookies, ip
    // may be PII. Only http.route is recorded.
    skip(state, req, headers, ip),
    fields(http.route = "/api/oauth/{realmId}/token")
)]
pub async fn oauth_token(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(req): Json<TokenRequest>,
) -> Result<Json<TokenResponse>, ApiError> {
    // Per-IP cap mirroring /authorize: each request costs a Redis GETDEL plus
    // client_app/user DB reads, so an unauthenticated code flood must not hit
    // Redis/DB at network speed.
    rate_limit_hit(
        &state,
        format!("rl:oauth-token:ip:{ip}"),
        OAUTH_TOKEN_IP_RATE_LIMIT.0,
        OAUTH_TOKEN_IP_RATE_LIMIT.1,
    )
    .await?;

    let user_agent = user_agent_from_headers(&headers);

    if req.grant_type != "authorization_code" {
        return Err(ApiError::bad_request(
            "grant_type must be 'authorization_code'",
        ));
    }

    // Atomically get-and-delete authorization code (one-time use)
    let mut conn = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Internal server error".to_string()))?;

    let key = format!("oauth:code:{}", req.code);
    let code_json: Option<String> = redis::cmd("GETDEL")
        .arg(&key)
        .query_async(&mut conn)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Redis GETDEL failed for OAuth authorization code");
            ApiError::internal("Internal server error".to_string())
        })?;

    let code_json = code_json.ok_or_else(|| {
        ApiError::bad_request("Invalid or expired authorization code".to_string())
    })?;

    let stored: serde_json::Value = serde_json::from_str(&code_json).map_err(|e| {
        tracing::error!(error = %e, "Failed to parse authorization code data");
        ApiError::internal("Internal server error".to_string())
    })?;

    let stored_client_id = stored["client_id"].as_str().unwrap_or("");
    let stored_redirect_uri = stored["redirect_uri"].as_str().unwrap_or("");
    let stored_realm_id = stored["realm_id"].as_str().unwrap_or("");
    let stored_user_id = stored["user_id"].as_str().unwrap_or("");
    let stored_code_challenge = stored["code_challenge"].as_str().unwrap_or("");

    validate_code_bindings(
        stored_client_id,
        stored_redirect_uri,
        stored_realm_id,
        stored_code_challenge,
        &realm_id,
        &req,
    )?;

    let client_app = state
        .service
        .client_service()
        .get_client_app_by_client_id(&realm_id, &req.client_id)
        .await
        .map_err(map_client_error)?;
    if !client_app.enabled {
        return Err(ApiError::bad_request("OAuth client app is not enabled"));
    }
    if client_app.is_first_party {
        validate_first_party_redirect(&state.public_base_url, &req.redirect_uri)?;
    }

    let user_id = uuid::Uuid::parse_str(stored_user_id)
        .map_err(|_| ApiError::bad_request("authorization code user is invalid"))?;
    let user = state
        .user_repository
        .get_user_by_id(user_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, %user_id, "OAuth token user lookup failed");
            ApiError::bad_request("authorization code user is invalid")
        })?;
    if user.realm_id != realm_id {
        return Err(ApiError::bad_request("authorization code user is invalid"));
    }

    let token_service = RedisBrowserTokenService::new(state.redis_manager.clone());
    let tokens = if client_app.is_first_party {
        token_service
            .create_first_party_token_family(
                &user,
                &client_app,
                user_agent.clone(),
                Some(ip.clone()),
            )
            .await
    } else {
        token_service
            .create_token_family(&user, &client_app, user_agent.clone(), Some(ip.clone()))
            .await
    }
    .map_err(|error| {
        tracing::error!(%error, "OAuth browser token issuance failed");
        ApiError::internal("Internal server error")
    })?;

    Ok(Json(TokenResponse {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        token_type: tokens.token_type,
        expires_in: tokens.expires_in,
        refresh_expires_in: tokens.refresh_expires_in,
    }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn map_client_error(error: CoreError) -> ApiError {
    match error {
        CoreError::NotFound => ApiError::bad_request("OAuth client app is not enabled"),
        error => {
            tracing::error!(%error, "OAuth Client App lookup failed");
            ApiError::internal("Internal server error")
        }
    }
}

fn validate_code_bindings(
    stored_client_id: &str,
    stored_redirect_uri: &str,
    stored_realm_id: &str,
    stored_code_challenge: &str,
    realm_id: &str,
    request: &TokenRequest,
) -> Result<(), ApiError> {
    if stored_client_id != request.client_id {
        return Err(ApiError::bad_request("client_id mismatch"));
    }
    if stored_redirect_uri != request.redirect_uri {
        return Err(ApiError::bad_request("redirect_uri mismatch"));
    }
    if stored_realm_id != realm_id {
        return Err(ApiError::bad_request("realm_id mismatch"));
    }
    if !verify_pkce(&request.code_verifier, stored_code_challenge) {
        return Err(ApiError::bad_request("PKCE verification failed"));
    }
    Ok(())
}

pub(crate) fn validate_first_party_redirect(
    frontend_url: &str,
    redirect_uri: &str,
) -> Result<(), ApiError> {
    let expected = format!(
        "{}{}",
        frontend_url.trim_end_matches('/'),
        FIRST_PARTY_CALLBACK_PATH
    );
    url::Url::parse(&expected).map_err(|_| ApiError::internal("Frontend URL is invalid"))?;
    if redirect_uri != expected {
        return Err(ApiError::bad_request("redirect_uri mismatch"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_pkce_correct_challenge() {
        // Known test vector: code_verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"
        // SHA256 + BASE64URL(no pad) = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert!(verify_pkce(verifier, challenge));
    }

    #[test]
    fn verify_pkce_wrong_verifier_fails() {
        let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert!(!verify_pkce("wrong_verifier", challenge));
    }

    #[test]
    fn verify_pkce_empty_inputs() {
        // Empty verifier with empty challenge: SHA256("") base64url'd
        let hash = {
            let mut h = Sha256::new();
            h.update(b"");
            URL_SAFE_NO_PAD.encode(h.finalize())
        };
        assert!(verify_pkce("", &hash));
    }

    #[test]
    fn first_party_redirect_must_exactly_match_server_frontend_callback() {
        assert!(
            validate_first_party_redirect("https://herald.test/", "https://herald.test/callback")
                .is_ok()
        );
        assert!(
            validate_first_party_redirect("https://herald.test", "https://evil.test/callback")
                .is_err()
        );
        assert!(
            validate_first_party_redirect(
                "https://herald.test",
                "https://herald.test/callback/extra"
            )
            .is_err()
        );
    }

    #[test]
    fn first_party_wrong_client_app_is_rejected_by_code_binding() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let request = TokenRequest {
            grant_type: "authorization_code".into(),
            code: "one-time-code".into(),
            redirect_uri: "https://herald.test/callback".into(),
            client_id: "attacker-client".into(),
            code_verifier: verifier.into(),
        };
        assert!(
            validate_code_bindings(
                "admin-web-console",
                "https://herald.test/callback",
                "admin",
                "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
                "admin",
                &request,
            )
            .is_err(),
            "an authorization code must remain bound to its original Client App"
        );
    }
}
