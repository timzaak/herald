//! OIDC JWKS endpoint (`.well-known/jwks.json`).
//!
//! Publishes the verification public keys (active key plus retained keys
//! still inside their rotation overlap window) so clients verify id_tokens
//! locally.

use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use herald_api_base::application::http::auth::util::{ClientIp, rate_limit_hit};
use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::security_constants::OAUTH_JWKS_IP_RATE_LIMIT;
use herald_core::infrastructure::oidc_signing_key::OidcSigningKeyError;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct OidcJwk {
    pub kty: String,
    #[serde(rename = "use")]
    pub use_: String,
    pub alg: String,
    pub kid: String,
    pub n: String,
    pub e: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct OidcJwksResponse {
    pub keys: Vec<OidcJwk>,
}

fn map_key_error(error: OidcSigningKeyError) -> ApiError {
    // Fail closed: an empty keyset would look like a healthy provider whose
    // tokens simply never verify.
    tracing::error!(%error, "OIDC signing key store failure serving JWKS");
    ApiError::internal("Internal server error")
}

#[utoipa::path(
    get,
    path = "/api/oauth/{realmId}/.well-known/jwks.json",
    tag = "oauth",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
    ),
    responses(
        (status = 200, description = "Verification public keys (active + unexpired retained)", body = OidcJwksResponse),
        (status = 404, description = "Realm not found", body = ErrorResponse),
        (status = 429, description = "Rate limit exceeded", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    )
)]
pub async fn oidc_jwks(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
) -> Result<impl IntoResponse, ApiError> {
    rate_limit_hit(
        &state,
        format!("rl:oauth-jwks:ip:{ip}"),
        OAUTH_JWKS_IP_RATE_LIMIT.0,
        OAUTH_JWKS_IP_RATE_LIMIT.1,
    )
    .await?;

    // Realm existence and the keyset read are independent — run them
    // concurrently instead of as sequential DB round-trips.
    let ((), keys) = tokio::try_join!(
        crate::oidc_discovery::ensure_realm_exists(&state, &realm_id),
        async {
            state
                .oidc_signing_key_store
                .jwks_public_keys()
                .await
                .map_err(map_key_error)
        },
    )?;

    let response = OidcJwksResponse {
        keys: keys
            .into_iter()
            .map(|key| OidcJwk {
                kty: "RSA".to_string(),
                use_: "sig".to_string(),
                alg: "RS256".to_string(),
                kid: key.kid,
                n: key.n_b64,
                e: key.e_b64,
            })
            .collect(),
    };

    tracing::debug!(realm_id = %realm_id, key_count = response.keys.len(), "OIDC JWKS served");

    Ok(crate::oidc_discovery::public_cached_json(response))
}
