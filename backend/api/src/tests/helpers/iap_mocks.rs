// =============================================================================
// IAP Mock Fixtures + Helpers (Apple JWS + Google Play Developer API wiremock)
// =============================================================================
//
// Test-only fixtures for the IAP scenario tests
// (`backend/api/src/tests/scenarios/billing/iap_*_scenarios.rs`).
//
// Mirrors the wiremock layout of `helpers/creem_mocks.rs` and the RSA-keypair
// pattern of `helpers/google_one_tap_helpers.rs`.
//
// # Trust posture for the Apple verifier
//
// The Herald Apple verifier (`herald_infra_iap::AppleVerifier`) wraps the
// upstream `app-store-server-library` `SignedDataVerifier`, rooted at the
// upstream verifier only accepts a fabricated JWS under its **`LocalTesting`**
// environment (it skips the cryptographic chain check there); under
// `Production` / `Sandbox` it demands a real Apple-signed ES256 x5c chain,
// whose private key Herald does not hold.
//
// The IAP receipt / webhook HTTP handlers in `api-billing/src/iap_handlers.rs`
// read the realm's `environment` config and currently accept only
// `production` / `sandbox` (see `load_apple_credentials`). The verifier unit
// tests in `infra-iap/src/apple/verifier.rs` cover the LocalTesting happy
// path. The HTTP-layer scenario tests here therefore focus on the
// **rejection** paths that the real Apple Root CA anchor guarantees
// (malformed JWS, tampered payload, wrong trust anchor) — these rejections
// hold under any environment because the cryptographic chain is never valid
// for a fabricated JWS. The HTTP path has no LocalTesting injection seam for
// the verifier, so a positive HTTP-layer Apple verification test would need
// one wired first.
//
// =============================================================================

#![allow(dead_code)]
#![allow(clippy::let_underscore_future)]

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

// =============================================================================
// Apple JWS fixture builders
// =============================================================================

/// Encode a 3-segment JWS (`header.payload.signature`) with a minimal ES256
/// header, the supplied payload JSON, and a dummy base64url signature segment.
///
/// Under the upstream `LocalTesting` environment this is sufficient for the
/// verifier to decode the payload (it skips the signature check); under
/// `Production` / `Sandbox` the verifier rejects it because the signature
/// does not match a real Apple-signed x5c chain. This is the same shape the
/// `infra-iap` verifier unit tests use.
pub fn make_apple_jws(payload_json: &Value) -> String {
    let header = json!({ "alg": "ES256", "typ": "JWS" });
    let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
    let payload_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(payload_json).unwrap());
    let signature_b64 = URL_SAFE_NO_PAD.encode(b"dummy-signature");
    format!("{header_b64}.{payload_b64}.{signature_b64}")
}

/// Decode the payload segment (middle) of a JWS without verifying, returning
/// the parsed JSON. Used by tests that need to assert what the verifier would
/// have seen before rejection / after acceptance.
pub fn decode_jws_payload(jws: &str) -> Option<Value> {
    let segs: Vec<&str> = jws.split('.').collect();
    if segs.len() != 3 {
        return None;
    }
    let bytes = URL_SAFE_NO_PAD.decode(segs[1]).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// three-state coverage.
pub struct AppleJwsFixtures {
    /// A well-formed JWS whose decoded payload matches `bundle_id` /
    /// `environment`. Under `LocalTesting` this verifies; under
    /// `Production`/`Sandbox` it is rejected (no real chain).
    pub valid_shape: String,
    /// A JWS whose payload has been tampered with after signing (signature no
    /// longer matches the payload). Rejected under any environment that
    /// actually checks signatures.
    pub tampered_payload: String,
    /// A JWS whose payload claims a different `bundleId` than the verifier
    /// expects. Maps to the upstream `InvalidAppIdentifier` rejection — this
    pub wrong_trust_anchor: String,
}

impl AppleJwsFixtures {
    /// Build the three-fixture set for `bundle_id` / `environment` and a
    /// canonical `original_transaction_id` / `product_id`.
    pub fn for_bundle(bundle_id: &str, environment: &str, txn_id: &str, product_id: &str) -> Self {
        let valid_payload = json!({
            "bundleId": bundle_id,
            "environment": environment,
            "originalTransactionId": txn_id,
            "transactionId": txn_id,
            "productId": product_id,
        });
        let valid_shape = make_apple_jws(&valid_payload);

        // Tampered payload: flip the productId after the (dummy) signature is
        // computed — under signature-checking environments this fails.
        let mut tampered_payload = valid_payload.clone();
        tampered_payload["productId"] = json!(format!("{product_id}.tampered"));
        let tampered_payload_jws = make_apple_jws(&tampered_payload);

        // Wrong trust anchor / app: claim a bundle the verifier did not pin.
        let wrong_payload = json!({
            "bundleId": format!("com.not.{bundle_id}"),
            "environment": environment,
            "originalTransactionId": txn_id,
            "transactionId": txn_id,
            "productId": product_id,
        });
        let wrong_trust_anchor = make_apple_jws(&wrong_payload);

        Self {
            valid_shape,
            tampered_payload: tampered_payload_jws,
            wrong_trust_anchor,
        }
    }
}

/// Build a minimal Apple SSV V2 notification JWS body around a signed
/// transaction info JWS. The notification's `data.bundleId` / `environment`
/// are populated so the verifier's notification-level guards run.
pub fn make_apple_notification_body(
    bundle_id: &str,
    environment: &str,
    notification_type: &str,
    signed_transaction_info: &str,
) -> String {
    let payload = json!({
        "notificationType": notification_type,
        "notificationUUID": Uuid::new_v4().to_string(),
        "data": {
            "bundleId": bundle_id,
            "environment": environment,
            "signedTransactionInfo": signed_transaction_info,
        }
    });
    make_apple_jws(&payload)
}

// =============================================================================
// realm_config insertion helpers (Apple / Google)
// =============================================================================

/// A fixed, throwaway EC P-256 key in PKCS#8 PEM form.
///
/// `AppleServerApiClient` signs its ES256 JWTs via
/// `EncodingKey::from_ec_pem`, which panics on non-EC input — tests that drive
/// the client against a wiremock `base_url` override need a parseable EC key
/// even though the signature is never validated by anything. This key is
/// public test material and authenticates nowhere.
pub fn test_apple_ec_p8_pem() -> &'static str {
    "-----BEGIN PRIVATE KEY-----\nMIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg4PTjHSmxKdgc/3qH\n7psmnyTFvqIUR5q1gzkik/bo2wmhRANCAAQ0R8pYA+QNS30MR4nyoYoBz5NbyJ66\nPUZLrrdOpD/fQtF7xNTQwXV0vpIJhpmfXcu+MnwKdgVcUTWlsXiallSu\n-----END PRIVATE KEY-----"
}

/// Insert Apple IAP credentials into `realm_config` for a realm.
///
/// `environment` is written verbatim; the production handler currently only
/// accepts `production` / `sandbox`.
pub async fn insert_apple_realm_config(
    pool: &PgPool,
    realm_id: &str,
    bundle_id: &str,
    issuer_id: &str,
    key_id: &str,
    private_key_p8: &str,
    environment: &str,
) {
    for (key, value, is_secret) in [
        ("bundle_id", bundle_id.to_string(), false),
        ("issuer_id", issuer_id.to_string(), false),
        ("key_id", key_id.to_string(), false),
        ("private_key_p8", private_key_p8.to_string(), true),
        ("environment", environment.to_string(), false),
    ] {
        sqlx::query(
            "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, is_secret, enabled, metadata)
             VALUES ($1, 'apple', $2, $3, $4, true, null)
             ON CONFLICT (realm_id, config_type, config_key)
             DO UPDATE SET config_value = EXCLUDED.config_value, is_secret = EXCLUDED.is_secret, enabled = true, updated_at = CURRENT_TIMESTAMP",
        )
        .bind(realm_id)
        .bind(key)
        .bind(value)
        .bind(is_secret)
        .execute(pool)
        .await
        .expect("Failed to upsert Apple realm_config row");
    }
}

/// Insert a fully-formed Google service-account credential set into
/// `realm_config` for a realm, pointing the OAuth token endpoint at
/// `token_uri` (typically the wiremock server's `/token`).
///
/// `base_url` (optional) wires the per-realm Developer API + OAuth
/// token-endpoint override the production Google receipt / reconciliation
/// paths read (`realm_config.google.base_url`, mirroring the Stripe/Creem
/// `base_url` pattern). When supplied (typically `google_mock.base_url()`),
/// both the Play Developer API client and the service-account JWT grant hit
/// the wiremock instead of the real Google endpoints. When `None`, the
/// production default endpoints are used.
pub async fn insert_google_realm_config(
    pool: &PgPool,
    realm_id: &str,
    package_name: &str,
    service_account_json: &str,
    base_url: Option<&str>,
) {
    let mut rows: Vec<(&str, String, bool)> = vec![
        ("package_name", package_name.to_string(), false),
        (
            "service_account_json",
            service_account_json.to_string(),
            true,
        ),
    ];
    if let Some(base) = base_url.filter(|b| !b.is_empty()) {
        rows.push(("base_url", base.to_string(), false));
    }
    for (key, value, is_secret) in rows {
        sqlx::query(
            "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, is_secret, enabled, metadata)
             VALUES ($1, 'google', $2, $3, $4, true, null)
             ON CONFLICT (realm_id, config_type, config_key)
             DO UPDATE SET config_value = EXCLUDED.config_value, is_secret = EXCLUDED.is_secret, enabled = true, updated_at = CURRENT_TIMESTAMP",
        )
        .bind(realm_id)
        .bind(key)
        .bind(value)
        .bind(is_secret)
        .execute(pool)
        .await
        .expect("Failed to upsert Google realm_config row");
    }
}

/// Build a Google service-account JSON document (string) embedding the given
/// client email and RSA PEM private key. This is the shape the production
/// `load_google_credentials` parses.
pub fn build_service_account_json(client_email: &str, rsa_pem: &str) -> String {
    json!({
        "type": "service_account",
        "client_email": client_email,
        "private_key": rsa_pem,
        "token_uri": "https://oauth2.googleapis.com/token",
    })
    .to_string()
}

// =============================================================================
// Google Play Developer API mock server
// =============================================================================

/// wiremock-backed Google Play Developer API mock.
///
/// Wraps the 5 endpoints the IAP feature touches
/// (`subscriptionsv2.get`, `subscriptions.acknowledge`, `products.get`,
/// `products.consume`, `voidedpurchases.list`) plus the OAuth `/token` stub
/// the service-account JWT grant hits. Each scenario is mounted on demand by
/// the caller; `reset()` clears registered mocks between tests.
pub struct GooglePlayMockServer {
    pub server: MockServer,
}

impl GooglePlayMockServer {
    pub async fn start() -> Self {
        Self {
            server: MockServer::start().await,
        }
    }

    pub fn base_url(&self) -> String {
        self.server.uri()
    }

    pub fn token_uri(&self) -> String {
        format!("{}/token", self.server.uri())
    }

    /// Mount the OAuth `/token` stub returning a long-TTL access token. Must
    /// be mounted before any Developer API call so the bearer-auth flow
    /// succeeds against the mock.
    pub async fn mount_token_stub(&self) {
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "ya29.test-token",
                "expires_in": 3600,
                "token_type": "Bearer",
            })))
            .mount(&self.server)
            .await;
    }

    // ---- subscriptionsv2.get ----

    /// Mount a successful `subscriptionsv2.get` response for `token`,
    /// returning an ACTIVE subscription owned by `obfuscated_account_id` with
    /// a single line item for `product_id`.
    pub async fn mount_subscription_get_success(
        &self,
        package_name: &str,
        token: &str,
        product_id: &str,
        obfuscated_account_id: &str,
    ) {
        Mock::given(method("GET"))
            .and(path(format!(
                "/{package_name}/purchases/subscriptionsv2/tokens/{token}"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "subscriptionState": "SUBSCRIPTION_STATE_ACTIVE",
                "acknowledgementState": "ACKNOWLEDGEMENT_STATE_ACKNOWLEDGED",
                "obfuscatedExternalAccountId": obfuscated_account_id,
                "lineItems": [{
                    "productId": product_id,
                    "state": "PURCHASED",
                    "expiryTime": "2026-12-31T00:00:00Z",
                    "autoRenewingPlan": { "autoRenewalEnabled": true },
                }],
            })))
            .mount(&self.server)
            .await;
    }

    /// Mount a 404 for `subscriptionsv2.get` (purchase-token lookup fails).
    /// Drives the receipt `verification_failed` (422) path.
    pub async fn mount_subscription_get_not_found(&self, package_name: &str, token: &str) {
        Mock::given(method("GET"))
            .and(path(format!(
                "/{package_name}/purchases/subscriptionsv2/tokens/{token}"
            )))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "error": { "code": 404, "message": "No subscription was found for the token." }
            })))
            .mount(&self.server)
            .await;
    }

    // ---- subscriptions.acknowledge ----

    /// Mount a successful 204 for `subscriptions.acknowledge`.
    pub async fn mount_subscription_acknowledge_success(&self, package_name: &str, token: &str) {
        Mock::given(method("POST"))
            .and(path(format!(
                "/{package_name}/purchases/subscriptions/tokens/{token}:acknowledge"
            )))
            .respond_with(ResponseTemplate::new(204))
            .mount(&self.server)
            .await;
    }

    /// Mount a 500 for `subscriptions.acknowledge` to drive the
    pub async fn mount_subscription_acknowledge_failure(&self, package_name: &str, token: &str) {
        Mock::given(method("POST"))
            .and(path(format!(
                "/{package_name}/purchases/subscriptions/tokens/{token}:acknowledge"
            )))
            .respond_with(ResponseTemplate::new(500).set_body_string("backend unavailable"))
            .mount(&self.server)
            .await;
    }

    // ---- products.get ----

    /// Mount a successful `products.get` for a consumable one_time product.
    /// `consumption_state` 0 == not yet consumed.
    pub async fn mount_product_get_success(
        &self,
        package_name: &str,
        product_id: &str,
        token: &str,
        obfuscated_account_id: &str,
    ) {
        Mock::given(method("GET"))
            .and(path(format!(
                "/{package_name}/purchases/products/{product_id}/tokens/{token}"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "consumptionState": 0,
                "acknowledgementState": 1,
                "productId": product_id,
                "purchaseState": 0,
                "obfuscatedExternalAccountId": obfuscated_account_id,
                "purchaseTimeMillis": "1700000000000",
            })))
            .mount(&self.server)
            .await;
    }

    /// Mount a 404 for `products.get` (lookup fails → 422 verification_failed).
    pub async fn mount_product_get_not_found(
        &self,
        package_name: &str,
        product_id: &str,
        token: &str,
    ) {
        Mock::given(method("GET"))
            .and(path(format!(
                "/{package_name}/purchases/products/{product_id}/tokens/{token}"
            )))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "error": { "code": 404, "message": "The purchase token was not found." }
            })))
            .mount(&self.server)
            .await;
    }

    // ---- products.consume ----

    /// Mount a successful 204 for `products.consume`.
    pub async fn mount_product_consume_success(
        &self,
        package_name: &str,
        product_id: &str,
        token: &str,
    ) {
        Mock::given(method("POST"))
            .and(path(format!(
                "/{package_name}/purchases/products/{product_id}/tokens/{token}:consume"
            )))
            .respond_with(ResponseTemplate::new(204))
            .mount(&self.server)
            .await;
    }

    /// Mount a 500 for `products.consume` to drive the consume-failure
    /// rollback regression (one_time equivalent of ack-failure).
    pub async fn mount_product_consume_failure(
        &self,
        package_name: &str,
        product_id: &str,
        token: &str,
    ) {
        Mock::given(method("POST"))
            .and(path(format!(
                "/{package_name}/purchases/products/{product_id}/tokens/{token}:consume"
            )))
            .respond_with(ResponseTemplate::new(500).set_body_string("backend unavailable"))
            .mount(&self.server)
            .await;
    }

    /// Mount a successful 204 for `products.acknowledge` — the endpoint
    /// `google_ack_or_consume_in_tx` selects for an `OneTime` mapping whose
    /// `points_per_period` is 0/NULL (i.e. a non-consumable / buyout product
    /// that must NOT be consumed). The URI mirrors
    /// `GoogleDeveloperClient::acknowledge_product`
    /// (`infra-iap/src/google/developer_api_client.rs:96`):
    /// `/{package_name}/purchases/products/{product_id}/tokens/{token}:acknowledge`.
    pub async fn mount_product_acknowledge_success(
        &self,
        package_name: &str,
        product_id: &str,
        token: &str,
    ) {
        Mock::given(method("POST"))
            .and(path(format!(
                "/{package_name}/purchases/products/{product_id}/tokens/{token}:acknowledge"
            )))
            .respond_with(ResponseTemplate::new(204))
            .mount(&self.server)
            .await;
    }

    /// Mount a 500 for `products.acknowledge` to drive the ack-failure
    /// rollback regression for the buyout (non-consumable) path. Mirrors the
    /// subscription ack-failure stub but for the product acknowledge endpoint.
    pub async fn mount_product_acknowledge_failure(
        &self,
        package_name: &str,
        product_id: &str,
        token: &str,
    ) {
        Mock::given(method("POST"))
            .and(path(format!(
                "/{package_name}/purchases/products/{product_id}/tokens/{token}:acknowledge"
            )))
            .respond_with(ResponseTemplate::new(500).set_body_string("backend unavailable"))
            .mount(&self.server)
            .await;
    }

    // ---- voidedpurchases.list ----

    /// Mount a successful `voidedpurchases.list` page for `package_name`,
    /// optionally matching a `token` query param for page-advance assertions.
    pub async fn mount_voided_list_success(
        &self,
        package_name: &str,
        page_token: Option<&str>,
        voided_token: &str,
        order_id: &str,
    ) {
        let mut mock = Mock::given(method("GET"))
            .and(path(format!("/{package_name}/purchases/voidedpurchases")));
        if let Some(tok) = page_token {
            mock = mock.and(query_param("token", tok));
        }
        mock.respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "voidedPurchases": [{
                "purchaseToken": voided_token,
                "purchaseType": 0,
                "voidedTimeMillis": "1700000001000",
                "orderId": order_id,
            }],
        })))
        .mount(&self.server)
        .await;
    }

    /// Reset all mocks (clears registered stubs).
    pub async fn reset(&self) {
        self.server.reset().await;
    }
}

/// Generate a throwaway 2048-bit RSA private key in PKCS#1 PEM form for the
/// Google service-account JWT grant stub. Mirrors the `infra-iap` developer
/// client unit tests.
pub fn fresh_rsa_pem() -> Vec<u8> {
    use rand::rngs::OsRng;
    use rsa::pkcs1::EncodeRsaPrivateKey;
    use rsa::{RsaPrivateKey, pkcs1::LineEnding};
    let mut rng = OsRng;
    let key = RsaPrivateKey::new(&mut rng, 2048).expect("generate RSA keypair");
    key.to_pkcs1_pem(LineEnding::LF)
        .expect("encode PKCS#1 PEM")
        .as_bytes()
        .to_vec()
}
