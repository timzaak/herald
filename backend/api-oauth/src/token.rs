// OAuth token endpoint for authorization code exchange with PKCE validation
//
// Browser clients exchange an authorization code (obtained via the authorize + login flow)
// for a Bearer token family. PKCE ensures the code cannot be intercepted and reused.

use axum::{
    Json,
    body::Bytes,
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
    /// OIDC identity token (RS256 JWT). Present only when the authorization
    /// request carried the `openid` scope; omitted otherwise so non-OIDC
    /// responses stay byte-identical.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
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

/// Parse the token request body.
///
/// Standard OIDC/OAuth clients POST `application/x-www-form-urlencoded` per
/// RFC 6749 §4.1.3 — the discovery document advertises this endpoint to them
/// (Grafana-style "issuer URL only" setups) — while the first-party flow this
/// endpoint was built on uses JSON. Both content types deserialize into the
/// same `TokenRequest` (field names are snake_case, matching the OAuth
/// parameter names), so one struct serves both wire forms.
fn parse_token_request(headers: &HeaderMap, body: &[u8]) -> Result<TokenRequest, ApiError> {
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let is_form = content_type.split(';').next().is_some_and(|mime| {
        mime.trim()
            .eq_ignore_ascii_case("application/x-www-form-urlencoded")
    });
    if is_form {
        serde_urlencoded::from_bytes(body)
            .map_err(|_| ApiError::bad_request("invalid token request body"))
    } else {
        serde_json::from_slice(body)
            .map_err(|_| ApiError::bad_request("invalid token request body"))
    }
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
    // Governance: body carries authorization code, PKCE
    // code_verifier, client_id — all credentials/secrets. state holds handles;
    // realm_id conservatively skipped; headers carries User-Agent/cookies, ip
    // may be PII. Only http.route is recorded.
    skip(state, body, headers, ip),
    fields(http.route = "/api/oauth/{realmId}/token")
)]
pub async fn oauth_token(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    body: Bytes,
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
    let req = parse_token_request(&headers, &body)?;

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

    // OIDC layer: mint an id_token only when the authorization request asked
    // for the `openid` scope. Everything inside this branch is incremental —
    // a flow without `openid` produces the identical response as before.
    //
    // The mint runs BEFORE the token family is written: every step in it can
    // fail (disabled user, profile read, signing-key fetch/decrypt/sign), and
    // once the family lands in Redis the response may no longer fail — a 500
    // after that point would burn the one-time code, strand an orphan token
    // family per retry, and force the user back through login.
    let stored_scope = stored["scope"].as_str();
    let openid_requested = herald_api_auth::oauth_oidc::scope_requests_openid(stored_scope);
    let id_token = if openid_requested {
        // The login-side entrances already reject disabled users, but the
        // authorization code outlives the login by up to its TTL; a user
        // disabled in between must not receive an identity token.
        if user.status.is_disabled() {
            return Err(ApiError::bad_request("authorization code user is invalid"));
        }
        let nonce = stored["nonce"].as_str().map(str::to_string);
        // The profile row, the active signing key, and the realm origin are
        // independent reads — fetch them concurrently instead of paying three
        // serial DB round-trips on the login critical path.
        let (nickname, active_key, origin) = tokio::try_join!(
            crate::oidc_id_token::profile_nickname(&state, &user),
            async {
                state
                    .oidc_signing_key_store
                    .active_signing_key()
                    .await
                    .map_err(|error| {
                        tracing::error!(%error, %realm_id, "OIDC active signing key unavailable");
                        ApiError::internal("Internal server error")
                    })
            },
            crate::oidc_discovery::build_realm_oauth_origin(&state, &realm_id),
        )?;
        Some(crate::oidc_id_token::issue_id_token(
            &realm_id,
            &req.client_id,
            &user,
            nickname,
            nonce,
            &active_key,
            &origin,
        )?)
    } else {
        None
    };

    let token_service = RedisBrowserTokenService::new(state.redis_manager.clone());
    let tokens = token_service
        .create_oauth_token_family(
            &user,
            &client_app,
            user_agent.clone(),
            Some(ip.clone()),
            openid_requested,
        )
        .await
        .map_err(|error| {
            tracing::error!(%error, "OAuth browser token issuance failed");
            ApiError::internal("Internal server error")
        })?;

    // Best-effort audit after the family write — it never fails the exchange.
    if openid_requested {
        crate::oidc_id_token::audit_id_token_issued(&state, &realm_id, &user, &req.client_id, &ip)
            .await;
    }

    Ok(Json(TokenResponse {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        token_type: tokens.token_type,
        expires_in: tokens.expires_in,
        refresh_expires_in: tokens.refresh_expires_in,
        id_token,
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
    use axum::http::header::{CONTENT_TYPE, HeaderMap};

    fn json_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());
        headers
    }

    fn form_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            "application/x-www-form-urlencoded".parse().unwrap(),
        );
        headers
    }

    // WHY: standard OIDC clients (Grafana-style) POST form-encoded per RFC
    // 6749 §4.1.3 and the discovery document points them at this endpoint —
    // the JSON body the first-party flow uses must not be the only wire form.
    #[test]
    fn parse_token_request_accepts_form_and_json_bodies() {
        let expected = TokenRequest {
            grant_type: "authorization_code".into(),
            code: "code-123".into(),
            redirect_uri: "https://app.example.com/cb".into(),
            client_id: "client-a".into(),
            code_verifier: "verifier".into(),
        };

        let from_form = parse_token_request(
            &form_headers(),
            b"grant_type=authorization_code&code=code-123&redirect_uri=https%3A%2F%2Fapp.example.com%2Fcb&client_id=client-a&code_verifier=verifier",
        )
        .expect("form-encoded token request must parse");
        assert_eq!(from_form.code, expected.code);
        assert_eq!(from_form.redirect_uri, expected.redirect_uri);
        assert_eq!(from_form.client_id, expected.client_id);
        assert_eq!(from_form.code_verifier, expected.code_verifier);

        let json_body = serde_json::json!({
            "grant_type": "authorization_code",
            "code": "code-123",
            "redirect_uri": "https://app.example.com/cb",
            "client_id": "client-a",
            "code_verifier": "verifier",
        })
        .to_string()
        .into_bytes();
        let from_json = parse_token_request(&json_headers(), &json_body)
            .expect("JSON token request must keep parsing");
        assert_eq!(from_json.code, expected.code);
    }

    #[test]
    fn parse_token_request_rejects_unparseable_bodies() {
        assert!(parse_token_request(&form_headers(), b"grant_type=&missing=everything").is_err());
        assert!(parse_token_request(&json_headers(), b"not json at all").is_err());
    }

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
