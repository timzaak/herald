//! Google service-account JWT grant (design §5.6).
//!
//! Implements RFC 7523 JWT bearer authorization grant for Google service
//! accounts: self-sign an RS256 JWT with `iss = client_email`,
//! `aud = token_uri`, exchange it at the OAuth token endpoint for an access
//! token, and cache the token until 60s before its `expires_in`.
//!
//! Reuses the workspace `jsonwebtoken` 9.3 and `reqwest`; no new crypto deps.
//! The private key is the RSA PEM from the downloaded service-account JSON
//! (`private_key` field).

use crate::error::IapError;
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Default token endpoint for Google service-account JWT grants.
pub const GOOGLE_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";

/// Refresh margin: refresh the cached token when it would otherwise expire
/// within this many seconds. Keeps callers from racing the exact expiry.
const REFRESH_MARGIN_SECONDS: i64 = 60;

/// Standard Play Developer API scope (read + write for the 6 endpoints we use).
pub const PLAY_DEV_SCOPE: &str = "https://www.googleapis.com/auth/androidpublisher";

/// Service-account JWT grant authorizer with an in-memory access-token cache.
///
/// Construct once per realm's Google credentials; cloning is cheap (the cached
/// token lives behind an `Arc<Mutex<...>>`). The cache is process-local; if
/// multiple processes serve the same realm they will each maintain their own
/// cache and independently call the token endpoint (acceptable — Google tokens
/// are cheap and the cache only exists to avoid per-request round trips).
#[derive(Clone)]
pub struct GoogleServiceAccountAuth {
    client_email: String,
    private_key: Vec<u8>,
    token_uri: String,
    cached: Arc<Mutex<Option<CachedToken>>>,
}

#[derive(Clone)]
struct CachedToken {
    token: String,
    /// Absolute expiry timestamp (server-supplied `expires_in` projected from
    /// issue time).
    expires_at: DateTime<Utc>,
}

impl GoogleServiceAccountAuth {
    /// Build an authorizer from a parsed service-account credential.
    ///
    /// `private_key_pem` is the raw bytes of the `private_key` field from the
    /// downloaded service-account JSON (PEM-encoded RSA private key).
    pub fn new(client_email: String, private_key_pem: Vec<u8>) -> Self {
        Self::with_token_uri(client_email, private_key_pem, GOOGLE_TOKEN_URI.to_string())
    }

    /// Build an authorizer with a custom token URI (for tests that point at a
    /// mock server).
    pub fn with_token_uri(
        client_email: String,
        private_key_pem: Vec<u8>,
        token_uri: String,
    ) -> Self {
        Self {
            client_email,
            private_key: private_key_pem,
            token_uri,
            cached: Arc::new(Mutex::new(None)),
        }
    }

    /// Obtain a valid access token for the given scope, refreshing the cache
    /// when the cached token is missing or about to expire.
    ///
    /// The `http: &reqwest::Client` argument is passed in (rather than stored)
    /// so callers share Herald's connection pool. `scope` is typically
    /// [`PLAY_DEV_SCOPE`].
    pub async fn access_token(
        &self,
        http: &reqwest::Client,
        scope: &str,
    ) -> Result<String, IapError> {
        // Fast path: cached and not within the refresh margin.
        {
            let cache = self.cached.lock().await;
            if let Some(token) = cache.as_ref()
                && token.expires_at > Utc::now() + Duration::seconds(REFRESH_MARGIN_SECONDS)
            {
                return Ok(token.token.clone());
            }
        }

        // Slow path: mint a fresh JWT, exchange for an access token, cache it.
        let jwt = self.sign_grant_jwt(scope)?;
        let token = self.exchange_grant(http, &jwt).await?;

        let mut cache = self.cached.lock().await;
        *cache = Some(CachedToken {
            token: token.access_token.clone(),
            expires_at: Utc::now() + Duration::seconds(token.expires_in as i64),
        });
        Ok(token.access_token)
    }

    /// Sign the service-account JWT grant (RFC 7523). Exposed as a separate
    /// method so tests can assert the exact `iss` / `aud` / `exp` claims
    /// without going through the HTTP exchange.
    fn sign_grant_jwt(&self, scope: &str) -> Result<String, IapError> {
        let now = Utc::now();
        let claims = ServiceAccountClaims {
            iss: self.client_email.clone(),
            scope: scope.to_string(),
            aud: self.token_uri.clone(),
            iat: now.timestamp(),
            exp: (now + Duration::seconds(3600)).timestamp(),
        };
        let header = Header::new(Algorithm::RS256);
        // Google does not require a `kid` for service-account JWTs (the key is
        // identified by `iss`); leaving it unset.
        let encoding_key = EncodingKey::from_rsa_pem(&self.private_key).map_err(|e| {
            IapError::ServiceAccountAuth(format!("invalid RSA private key PEM: {e}"))
        })?;
        encode(&header, &claims, &encoding_key).map_err(|e| {
            IapError::ServiceAccountAuth(format!("failed to sign service-account JWT: {e}"))
        })
    }

    async fn exchange_grant(
        &self,
        http: &reqwest::Client,
        jwt: &str,
    ) -> Result<TokenResponse, IapError> {
        let resp = http
            .post(&self.token_uri)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", jwt),
            ])
            .send()
            .await
            .map_err(IapError::Transport)?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(IapError::ServiceAccountAuth(format!(
                "google token endpoint returned status={status} body={body}"
            )));
        }

        let token: TokenResponse = resp.json().await.map_err(IapError::Transport)?;
        Ok(token)
    }
}

/// Claims for the service-account JWT bearer grant (RFC 7523 §2.1).
#[derive(Debug, Serialize)]
struct ServiceAccountClaims {
    iss: String,
    scope: String,
    aud: String,
    iat: i64,
    exp: i64,
}

/// Google OAuth token endpoint response.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
}

#[cfg(test)]
mod tests {
    //! Google service-account JWT grant tests (design §6.1).
    //!
    //! Covers:
    //! - JWT grant claims correctness (`iss`, `aud`, `exp`, `iat`, `scope`) by
    //!   intercepting the `assertion` form field and decoding the JWT.
    //! - access token caching: a second `access_token` call within the expiry
    //!   window reuses the cached token (no second HTTP hit).
    //! - access token expiry refresh: after simulating expiry, a new HTTP call
    //!   is made.
    //! - error status mapping: a non-2xx from the token endpoint surfaces as
    //!   `ServiceAccountAuth`.
    //!
    //! The RSA private key is generated at runtime via the `rsa` crate
    //! (dev-only dependency) so each test has a cryptographically valid key
    //! pair without shipping a static PEM fixture.

    use super::*;
    use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
    use rand::rngs::OsRng;
    use rsa::pkcs1::EncodeRsaPrivateKey;
    use rsa::{RsaPrivateKey, pkcs1::LineEnding};
    use serde::Deserialize;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Generate a fresh 2048-bit RSA keypair and return the private key as a
    /// PKCS#1 PEM string. Slow (~tens of ms) but runs once per test; avoids
    /// shipping a static key fixture.
    fn fresh_rsa_pem() -> Vec<u8> {
        let mut rng = OsRng;
        let key = RsaPrivateKey::new(&mut rng, 2048).expect("generate RSA keypair");
        key.to_pkcs1_pem(LineEnding::LF)
            .expect("encode PKCS#1 PEM")
            .as_bytes()
            .to_vec()
    }

    /// Decoded JWT grant claims (subset of [`ServiceAccountClaims`] asserted on).
    #[derive(Debug, Deserialize, PartialEq)]
    struct GrantClaims {
        iss: String,
        aud: String,
        scope: String,
        iat: i64,
        exp: i64,
    }

    fn http_client() -> reqwest::Client {
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("test http client")
    }

    /// Decode the assertion JWT from a token-endpoint request body to inspect
    /// the grant claims. The JWT is signed by the same test key the decoder
    /// loads, so signature verification succeeds and we assert claim contents.
    ///
    /// `signing_pem` is the PKCS#1 *private* key PEM; we derive the public
    /// components (n, e) from it to build a `DecodingKey` —
    /// `jsonwebtoken`'s `from_rsa_pem` only accepts *public* key PEMs.
    async fn capture_assertion_claims(server: &MockServer, signing_pem: &[u8]) -> GrantClaims {
        use base64::Engine;
        use rsa::pkcs1::DecodeRsaPrivateKey;
        use rsa::traits::PublicKeyParts;

        let requests = server
            .received_requests()
            .await
            .expect("request recording enabled");
        let token_req = requests
            .iter()
            .find(|r| r.url.path() == "/token")
            .expect("at least one token request recorded");
        // Body is form-urlencoded: grant_type=...&assertion=<jwt>
        let form: std::collections::HashMap<String, String> =
            url::form_urlencoded::parse(&token_req.body)
                .into_owned()
                .collect();
        let assertion = form
            .get("assertion")
            .expect("assertion form field present")
            .clone();
        let grant_type = form.get("grant_type").map(String::as_str).unwrap_or("");
        assert_eq!(
            grant_type, "urn:ietf:params:oauth:grant-type:jwt-bearer",
            "grant_type form field"
        );

        let private_key =
            rsa::RsaPrivateKey::from_pkcs1_pem(std::str::from_utf8(signing_pem).unwrap())
                .expect("parse signing private key PEM");
        // jsonwebtoken 9.3 `from_rsa_components` takes base64url strings of
        // the modulus / exponent.
        let n_b64 =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(private_key.n().to_bytes_be());
        let e_b64 =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(private_key.e().to_bytes_be());
        let decoding_key = DecodingKey::from_rsa_components(&n_b64, &e_b64)
            .expect("build RSA decoding key from public components");
        // The grant JWT carries an `aud` + `exp` claim; we assert their values
        // directly on the decoded claims below, so disable jsonwebtoken's own
        // `aud` validation (no expected audience is configured here).
        let mut validation = Validation::new(Algorithm::RS256);
        validation.validate_aud = false;
        let token_data = decode::<GrantClaims>(&assertion, &decoding_key, &validation)
            .expect("decode + verify assertion JWT");
        token_data.claims
    }

    #[tokio::test]
    async fn jwt_grant_carries_correct_claims_and_exchanges_for_access_token() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "ya29.test-token",
                "expires_in": 3600,
                "token_type": "Bearer",
            })))
            .mount(&server)
            .await;

        let pem = fresh_rsa_pem();
        let auth = GoogleServiceAccountAuth::with_token_uri(
            "svc-account@herald-test.iam.gserviceaccount.com".to_string(),
            pem.clone(),
            format!("{}/token", server.uri()),
        );

        let token = auth
            .access_token(&http_client(), PLAY_DEV_SCOPE)
            .await
            .expect("access token");
        assert_eq!(token, "ya29.test-token");

        let claims = capture_assertion_claims(&server, &pem).await;
        assert_eq!(
            claims.iss, "svc-account@herald-test.iam.gserviceaccount.com",
            "iss must be the service account email"
        );
        assert_eq!(
            claims.aud,
            format!("{}/token", server.uri()),
            "aud must be the token endpoint"
        );
        assert_eq!(
            claims.scope, PLAY_DEV_SCOPE,
            "scope must be requested scope"
        );
        assert!(
            claims.exp > claims.iat,
            "exp must be after iat (typically iat + 3600s)"
        );
        assert!(
            claims.exp - claims.iat >= 3500 && claims.exp - claims.iat <= 3600,
            "exp-iat should be ~3600s (1h), got {}",
            claims.exp - claims.iat
        );
    }

    #[tokio::test]
    async fn access_token_cache_reuses_token_within_expiry_window() {
        // Cache hit: a second call within the expiry window must NOT trigger a
        // second HTTP request to the token endpoint.
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "ya29.cached",
                "expires_in": 3600,
                "token_type": "Bearer",
            })))
            // Expect exactly 1 call: the second access_token() must hit cache.
            .expect(1)
            .mount(&server)
            .await;

        let auth = GoogleServiceAccountAuth::with_token_uri(
            "svc@herald-test.iam.gserviceaccount.com".to_string(),
            fresh_rsa_pem(),
            format!("{}/token", server.uri()),
        );

        let http = http_client();
        let first = auth
            .access_token(&http, PLAY_DEV_SCOPE)
            .await
            .expect("first access token");
        let second = auth
            .access_token(&http, PLAY_DEV_SCOPE)
            .await
            .expect("cached access token");

        assert_eq!(first, "ya29.cached");
        assert_eq!(
            second, first,
            "second call within expiry must return the cached token"
        );
    }

    #[tokio::test]
    async fn access_token_cache_refreshes_after_expiry() {
        // Refresh: simulate expiry with a tiny expires_in and sleep past the
        // refresh margin. The second call must trigger a second HTTP request.
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "ya29.first",
                // Tiny TTL so the cache entry is past the 60s refresh margin
                // immediately.
                "expires_in": 1,
                "token_type": "Bearer",
            })))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "ya29.refreshed",
                "expires_in": 3600,
                "token_type": "Bearer",
            })))
            .mount(&server)
            .await;

        let auth = GoogleServiceAccountAuth::with_token_uri(
            "svc@herald-test.iam.gserviceaccount.com".to_string(),
            fresh_rsa_pem(),
            format!("{}/token", server.uri()),
        );

        let http = http_client();
        let first = auth
            .access_token(&http, PLAY_DEV_SCOPE)
            .await
            .expect("first token");
        // expires_in=1 -> expires_at ~= now+1s; after a short sleep the next
        // call is past the 60s refresh margin and must hit the endpoint again.
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        let second = auth
            .access_token(&http, PLAY_DEV_SCOPE)
            .await
            .expect("refreshed token");

        assert_eq!(first, "ya29.first");
        assert_eq!(
            second, "ya29.refreshed",
            "after expiry the access token must be refreshed from the endpoint"
        );
    }

    #[tokio::test]
    async fn token_endpoint_error_surfaces_as_service_account_auth_error() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "invalid_grant",
                "error_description": "Invalid JWT signature.",
            })))
            .mount(&server)
            .await;

        let auth = GoogleServiceAccountAuth::with_token_uri(
            "svc@herald-test.iam.gserviceaccount.com".to_string(),
            fresh_rsa_pem(),
            format!("{}/token", server.uri()),
        );

        let result = auth.access_token(&http_client(), PLAY_DEV_SCOPE).await;
        assert!(
            matches!(result, Err(IapError::ServiceAccountAuth(ref msg))
                if msg.contains("status=400") && msg.contains("invalid_grant")),
            "token endpoint error must surface as ServiceAccountAuth carrying status, got {result:?}"
        );
    }
}
