//! Google Play Developer API thin client (design §5.6).
//!
//! Six stable endpoints, one method each, all delegating to a shared
//! `reqwest::Client` and a [`GoogleServiceAccountAuth`] for Bearer auth.
//! Error responses are normalized to [`IapError::GoogleApi`] carrying the
//! HTTP status code so the api layer can map 4xx/5xx uniformly.

use crate::error::IapError;
use crate::google::models::{ProductPurchase, SubscriptionPurchaseV2, VoidedPurchasesList};
use crate::google::service_account::{GoogleServiceAccountAuth, PLAY_DEV_SCOPE};
use serde::Serialize;

/// Base URL for the Play Developer API v3.
const PLAY_DEV_BASE: &str =
    "https://androidpublisher.googleapis.com/androidpublisher/v3/applications";

/// Thin Google Play Developer API client.
///
/// Construct once per realm; cloning is cheap (everything inside is `Clone`).
/// Each method takes the caller-supplied `&GoogleServiceAccountAuth` so a
/// single auth instance can serve multiple clients (e.g. subscription + voided
/// polling) and share the same cached access token.
#[derive(Clone)]
pub struct GoogleDeveloperClient {
    http: reqwest::Client,
    base: String,
}

/// Percent-encode a value interpolated into a single outbound URL path
/// segment. Receipt tokens are free-form caller input: without encoding, a
/// '/' or '..' rewrites the request path (traversal out of the purchases
/// subtree), '?' injects query parameters, and '#' truncates — all against
/// the fixed Google host while carrying the realm's service-account Bearer
/// token.
///
/// A value that IS a lone dot segment ('.' / '..') is rejected instead of
/// encoded: WHATWG URL parsing folds dot segments — including their `%2E`
/// spellings — so escaping cannot prevent the fold into a neighboring
/// resource path, and no real store identifier has that shape (review
/// 20260919, path_segment dot finding).
fn path_segment(raw: &str) -> Result<String, IapError> {
    if raw == "." || raw == ".." {
        return Err(IapError::InvalidPathSegment(raw.to_string()));
    }
    // urlencoding::encode escapes exactly the non-unreserved bytes
    // (everything but alphanumerics and `-._~`) as uppercase `%XX`.
    Ok(urlencoding::encode(raw).into_owned())
}

impl GoogleDeveloperClient {
    /// Build a client over Herald's shared `reqwest::Client` (rustls).
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            base: PLAY_DEV_BASE.to_string(),
        }
    }

    /// Build a client with a custom base URL (tests pointing at a mock server).
    pub fn with_base_url(http: reqwest::Client, base: String) -> Self {
        Self { http, base }
    }

    /// `purchases.subscriptionsv2.get` — current state of a subscription.
    pub async fn get_subscription(
        &self,
        auth: &GoogleServiceAccountAuth,
        package_name: &str,
        token: &str,
    ) -> Result<SubscriptionPurchaseV2, IapError> {
        let access_token = auth.access_token(&self.http, PLAY_DEV_SCOPE).await?;
        let url = format!(
            "{}/{}/purchases/subscriptionsv2/tokens/{}",
            self.base,
            path_segment(package_name)?,
            path_segment(token)?
        );
        let resp = self.http.get(&url).bearer_auth(access_token).send().await?;
        decode_response(resp).await
    }

    /// `purchases.products.get` — state of a one-time / consumable product.
    pub async fn get_product(
        &self,
        auth: &GoogleServiceAccountAuth,
        package_name: &str,
        product_id: &str,
        token: &str,
    ) -> Result<ProductPurchase, IapError> {
        let access_token = auth.access_token(&self.http, PLAY_DEV_SCOPE).await?;
        let url = format!(
            "{}/{}/purchases/products/{}/tokens/{}",
            self.base,
            path_segment(package_name)?,
            path_segment(product_id)?,
            path_segment(token)?
        );
        let resp = self.http.get(&url).bearer_auth(access_token).send().await?;
        decode_response(resp).await
    }

    /// `purchases.subscriptions.acknowledge` — acknowledge a subscription
    /// (must happen within 3 days or the purchase is auto-refunded).
    pub async fn acknowledge_subscription(
        &self,
        auth: &GoogleServiceAccountAuth,
        package_name: &str,
        token: &str,
    ) -> Result<(), IapError> {
        self.acknowledge_or_consume(
            auth,
            &format!(
                "{}/{}/purchases/subscriptions/tokens/{}:acknowledge",
                self.base,
                path_segment(package_name)?,
                path_segment(token)?
            ),
            &EmptyBody,
        )
        .await
    }

    /// `purchases.products.acknowledge` — acknowledge a non-consumable product.
    pub async fn acknowledge_product(
        &self,
        auth: &GoogleServiceAccountAuth,
        package_name: &str,
        product_id: &str,
        token: &str,
    ) -> Result<(), IapError> {
        self.acknowledge_or_consume(
            auth,
            &format!(
                "{}/{}/purchases/products/{}/tokens/{}:acknowledge",
                self.base,
                path_segment(package_name)?,
                path_segment(product_id)?,
                path_segment(token)?
            ),
            &EmptyBody,
        )
        .await
    }

    /// `purchases.products.consume` — consume a consumable product.
    pub async fn consume_product(
        &self,
        auth: &GoogleServiceAccountAuth,
        package_name: &str,
        product_id: &str,
        token: &str,
    ) -> Result<(), IapError> {
        self.acknowledge_or_consume(
            auth,
            &format!(
                "{}/{}/purchases/products/{}/tokens/{}:consume",
                self.base,
                path_segment(package_name)?,
                path_segment(product_id)?,
                path_segment(token)?
            ),
            &EmptyBody,
        )
        .await
    }

    /// `purchases.voidedpurchases.list` — refund / chargeback polling.
    ///
    /// `page_token` is opaque; pass empty string for the first page. The
    /// caller reads `page_info` / `token_pagination` from the response for the
    /// next page.
    pub async fn list_voided_purchases(
        &self,
        auth: &GoogleServiceAccountAuth,
        package_name: &str,
        page_token: &str,
    ) -> Result<VoidedPurchasesList, IapError> {
        let access_token = auth.access_token(&self.http, PLAY_DEV_SCOPE).await?;
        let url = format!("{}/{package_name}/purchases/voidedpurchases", self.base);
        let mut req = self.http.get(&url).bearer_auth(access_token);
        if !page_token.is_empty() {
            req = req.query(&[("token", page_token)]);
        }
        let resp = req.send().await?;
        decode_response(resp).await
    }

    /// Shared POST-no-body path for acknowledge / consume (both take an empty
    /// `{}` JSON body and return 204 on success).
    async fn acknowledge_or_consume(
        &self,
        auth: &GoogleServiceAccountAuth,
        url: &str,
        body: &EmptyBody,
    ) -> Result<(), IapError> {
        let access_token = auth.access_token(&self.http, PLAY_DEV_SCOPE).await?;
        let resp = self
            .http
            .post(url)
            .bearer_auth(access_token)
            .json(body)
            .send()
            .await?;
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        let body = resp.text().await.unwrap_or_default();
        Err(IapError::GoogleApi {
            status: status.as_u16(),
            body,
        })
    }
}

/// Empty `{}` JSON body for acknowledge / consume endpoints.
#[derive(Debug, Serialize)]
struct EmptyBody;

/// Deserialize a JSON body response, mapping non-success statuses to
/// [`IapError::GoogleApi`].
async fn decode_response<T>(resp: reqwest::Response) -> Result<T, IapError>
where
    T: serde::de::DeserializeOwned,
{
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(IapError::GoogleApi {
            status: status.as_u16(),
            body,
        });
    }
    resp.json::<T>().await.map_err(IapError::Transport)
}

#[cfg(test)]
mod tests {
    //! Google Developer API client tests (design §6.1).
    //!
    //! Covers the six endpoints' serde tolerance (`#[serde(default)]` swallows
    //! unknown fields and tolerates missing fields) and the HTTP error-status →
    //! [`IapError::GoogleApi`] mapping. The service-account JWT grant is
    //! stubbed by mounting a `/token` mock returning a long-TTL access token,
    //! so the client tests focus on the Developer API surface.

    use super::*;
    use crate::google::service_account::GoogleServiceAccountAuth;
    use rand::rngs::OsRng;
    use rsa::pkcs1::EncodeRsaPrivateKey;
    use rsa::{RsaPrivateKey, pkcs1::LineEnding};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Generate a throwaway 2048-bit RSA private key in PKCS#1 PEM form for the
    /// service-account JWT grant stub.
    fn fresh_rsa_pem() -> Vec<u8> {
        let mut rng = OsRng;
        let key = RsaPrivateKey::new(&mut rng, 2048).expect("generate RSA keypair");
        key.to_pkcs1_pem(LineEnding::LF)
            .expect("encode PKCS#1 PEM")
            .as_bytes()
            .to_vec()
    }

    /// Mount a permissive `/token` stub returning a long-TTL access token so
    /// the developer client's bearer-auth flow succeeds against the mock.
    async fn mount_token_stub(server: &MockServer) {
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "ya29.dev-token",
                "expires_in": 3600,
                "token_type": "Bearer",
            })))
            .mount(server)
            .await;
    }

    fn http_client() -> reqwest::Client {
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("test http client")
    }

    #[tokio::test]
    async fn get_subscription_decodes_response_and_tolerates_unknown_fields() {
        let server = MockServer::start().await;
        mount_token_stub(&server).await;

        // Response carries several unknown / future fields
        // (`unknownFutureField`, `pausedStateContext`) and omits some declared
        // optional fields. `#[serde(default)]` must keep the parse succeeding
        // and surface the declared fields we care about.
        Mock::given(method("GET"))
            .and(path(
                "/com.herald.app/purchases/subscriptionsv2/tokens/purchase-token-1",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "subscriptionState": "SUBSCRIPTION_STATE_ACTIVE",
                "acknowledgementState": "ACKNOWLEDGEMENT_STATE_ACKNOWLEDGED",
                "obfuscatedExternalAccountId": "user-uuid-here",
                "lineItems": [{
                    "productId": "pro_monthly",
                    "state": "PURCHASED",
                    "expiryTime": "2026-12-31T00:00:00Z",
                    "autoRenewingPlan": { "autoRenewalEnabled": true },
                    "unknownFutureLineItemField": "ignored"
                }],
                "unknownFutureField": { "whatever": 123 },
                "pausedStateContext": "ignored-too"
            })))
            .mount(&server)
            .await;

        let http = http_client();
        let client = GoogleDeveloperClient::with_base_url(http.clone(), server.uri());
        let auth = GoogleServiceAccountAuth::with_token_uri(
            "svc@herald-test.iam.gserviceaccount.com".to_string(),
            fresh_rsa_pem(),
            format!("{}/token", server.uri()),
        );

        let sub = client
            .get_subscription(&auth, "com.herald.app", "purchase-token-1")
            .await
            .expect("subscription decode");

        assert_eq!(
            sub.subscription_state.as_deref(),
            Some("SUBSCRIPTION_STATE_ACTIVE")
        );
        assert_eq!(
            sub.obfuscated_external_account_id.as_deref(),
            Some("user-uuid-here")
        );
        assert_eq!(sub.line_items.len(), 1);
        assert_eq!(sub.line_items[0].product_id.as_deref(), Some("pro_monthly"));
        assert!(
            sub.line_items[0]
                .auto_renewing_plan
                .as_ref()
                .map(|p| p.auto_renewal_enabled)
                .unwrap_or(false)
        );
    }

    #[tokio::test]
    async fn get_product_decodes_consumption_and_acknowledgement_state() {
        let server = MockServer::start().await;
        mount_token_stub(&server).await;

        Mock::given(method("GET"))
            .and(path(
                "/com.herald.app/purchases/products/credits_100/tokens/purchase-token-2",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "consumptionState": 0,
                "acknowledgementState": 1,
                "productId": "credits_100",
                "purchaseState": 0,
                "obfuscatedExternalAccountId": "user-uuid-2",
                "purchaseTimeMillis": "1700000000000",
                "unknownProductFutureField": "ignored"
            })))
            .mount(&server)
            .await;

        let http = http_client();
        let client = GoogleDeveloperClient::with_base_url(http.clone(), server.uri());
        let auth = GoogleServiceAccountAuth::with_token_uri(
            "svc@herald-test.iam.gserviceaccount.com".to_string(),
            fresh_rsa_pem(),
            format!("{}/token", server.uri()),
        );

        let product = client
            .get_product(&auth, "com.herald.app", "credits_100", "purchase-token-2")
            .await
            .expect("product decode");

        assert_eq!(product.consumption_state, Some(0));
        assert_eq!(product.acknowledgement_state, Some(1));
        assert_eq!(product.product_id.as_deref(), Some("credits_100"));
        assert_eq!(
            product.obfuscated_external_account_id.as_deref(),
            Some("user-uuid-2")
        );
    }

    #[tokio::test]
    async fn acknowledge_subscription_posts_and_succeeds_on_204() {
        let server = MockServer::start().await;
        mount_token_stub(&server).await;

        Mock::given(method("POST"))
            .and(path(
                "/com.herald.app/purchases/subscriptions/tokens/token-ack:acknowledge",
            ))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let http = http_client();
        let client = GoogleDeveloperClient::with_base_url(http.clone(), server.uri());
        let auth = GoogleServiceAccountAuth::with_token_uri(
            "svc@herald-test.iam.gserviceaccount.com".to_string(),
            fresh_rsa_pem(),
            format!("{}/token", server.uri()),
        );

        client
            .acknowledge_subscription(&auth, "com.herald.app", "token-ack")
            .await
            .expect("acknowledge ok");
    }

    #[tokio::test]
    async fn acknowledge_product_posts_and_succeeds_on_204() {
        let server = MockServer::start().await;
        mount_token_stub(&server).await;

        Mock::given(method("POST"))
            .and(path(
                "/com.herald.app/purchases/products/non_consumable/tokens/token-prod-ack:acknowledge",
            ))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let http = http_client();
        let client = GoogleDeveloperClient::with_base_url(http.clone(), server.uri());
        let auth = GoogleServiceAccountAuth::with_token_uri(
            "svc@herald-test.iam.gserviceaccount.com".to_string(),
            fresh_rsa_pem(),
            format!("{}/token", server.uri()),
        );

        client
            .acknowledge_product(&auth, "com.herald.app", "non_consumable", "token-prod-ack")
            .await
            .expect("acknowledge product ok");
    }

    #[tokio::test]
    async fn consume_product_posts_and_succeeds_on_204() {
        let server = MockServer::start().await;
        mount_token_stub(&server).await;

        Mock::given(method("POST"))
            .and(path(
                "/com.herald.app/purchases/products/credits_100/tokens/token-consume:consume",
            ))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let http = http_client();
        let client = GoogleDeveloperClient::with_base_url(http.clone(), server.uri());
        let auth = GoogleServiceAccountAuth::with_token_uri(
            "svc@herald-test.iam.gserviceaccount.com".to_string(),
            fresh_rsa_pem(),
            format!("{}/token", server.uri()),
        );

        client
            .consume_product(&auth, "com.herald.app", "credits_100", "token-consume")
            .await
            .expect("consume ok");
    }

    #[tokio::test]
    async fn list_voided_purchases_decodes_response_and_passes_page_token() {
        let server = MockServer::start().await;
        mount_token_stub(&server).await;

        // The mock matches the `token=page-2` query param, proving the client
        // forwards the page token; the response carries unknown fields that
        // `#[serde(default)]` must swallow.
        Mock::given(method("GET"))
            .and(path("/com.herald.app/purchases/voidedpurchases"))
            .and(query_param("token", "page-2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "voidedPurchases": [{
                    "purchaseToken": "orig-token-1",
                    "purchaseType": 0,
                    "voidedTimeMillis": "1700000001000",
                    "orderId": "GPA.3333-1111-2222-33333",
                    "unknownVoidedField": "ignored"
                }],
                "tokenPagination": { "nextPageToken": "page-3", "previousPageToken": "page-1" },
                "pageInfo": { "totalResultCount": 42 }
            })))
            .mount(&server)
            .await;

        let http = http_client();
        let client = GoogleDeveloperClient::with_base_url(http.clone(), server.uri());
        let auth = GoogleServiceAccountAuth::with_token_uri(
            "svc@herald-test.iam.gserviceaccount.com".to_string(),
            fresh_rsa_pem(),
            format!("{}/token", server.uri()),
        );

        let list = client
            .list_voided_purchases(&auth, "com.herald.app", "page-2")
            .await
            .expect("voided list decode");

        assert_eq!(list.voided_purchases.len(), 1);
        assert_eq!(
            list.voided_purchases[0].purchase_token.as_deref(),
            Some("orig-token-1")
        );
        assert_eq!(
            list.voided_purchases[0].order_id.as_deref(),
            Some("GPA.3333-1111-2222-33333")
        );
        assert_eq!(
            list.token_pagination
                .as_ref()
                .and_then(|p| p.next_page_token.clone()),
            Some("page-3".to_string())
        );
        assert_eq!(
            list.page_info.as_ref().and_then(|p| p.total_result_count),
            Some(42)
        );
    }

    #[tokio::test]
    async fn developer_api_maps_4xx_error_to_google_api_with_status_and_body() {
        // Error mapping: a 400 from the Developer API must surface as
        // IapError::GoogleApi { status: 400, body } so the api layer can map
        // 4xx uniformly. Covers the "invalid purchase token" failure mode.
        let server = MockServer::start().await;
        mount_token_stub(&server).await;

        Mock::given(method("GET"))
            .and(path(
                "/com.herald.app/purchases/subscriptionsv2/tokens/invalid",
            ))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": {
                    "code": 400,
                    "message": "The purchase token was not found.",
                    "errors": [{ "reason": "invalidPurchaseToken" }],
                }
            })))
            .mount(&server)
            .await;

        let http = http_client();
        let client = GoogleDeveloperClient::with_base_url(http.clone(), server.uri());
        let auth = GoogleServiceAccountAuth::with_token_uri(
            "svc@herald-test.iam.gserviceaccount.com".to_string(),
            fresh_rsa_pem(),
            format!("{}/token", server.uri()),
        );

        let result = client
            .get_subscription(&auth, "com.herald.app", "invalid")
            .await;
        assert!(
            matches!(result, Err(IapError::GoogleApi { status: 400, ref body })
                if body.contains("invalidPurchaseToken") || body.contains("purchase token")),
            "4xx from Developer API must map to GoogleApi{{status=400, body}}, got {result:?}"
        );
    }

    #[tokio::test]
    async fn developer_api_maps_5xx_error_to_google_api_with_status() {
        // Error mapping: a 503 (transient backend) must surface as
        // IapError::GoogleApi { status: 503, .. } so callers can retry.
        let server = MockServer::start().await;
        mount_token_stub(&server).await;

        Mock::given(method("GET"))
            .and(path(
                "/com.herald.app/purchases/products/credits_100/tokens/token-x",
            ))
            .respond_with(ResponseTemplate::new(503).set_body_string("backend unavailable"))
            .mount(&server)
            .await;

        let http = http_client();
        let client = GoogleDeveloperClient::with_base_url(http.clone(), server.uri());
        let auth = GoogleServiceAccountAuth::with_token_uri(
            "svc@herald-test.iam.gserviceaccount.com".to_string(),
            fresh_rsa_pem(),
            format!("{}/token", server.uri()),
        );

        let result = client
            .get_product(&auth, "com.herald.app", "credits_100", "token-x")
            .await;
        assert!(
            matches!(result, Err(IapError::GoogleApi { status: 503, .. })),
            "5xx from Developer API must map to GoogleApi{{status=503}}, got {result:?}"
        );
    }

    // Intent (audit run-1: unencoded-purchase-token-interpolation-into-
    // google-api-url-path): the receipt token is free-form caller input sent
    // to this client before any provider validation. A token carrying
    // '/', '..', '?' or '#' must not rewrite the outbound request path/query
    // — it must travel percent-encoded inside ONE path segment. The mock
    // asserts the exact encoded path is hit; old code produced
    // /com.herald.app/purchases/products/p/tokens/a/b (path traversal), and
    // wiremock's mock (registered on the encoded path) would not match.
    #[tokio::test]
    async fn hostile_receipt_token_stays_encoded_in_one_path_segment() {
        let server = MockServer::start().await;
        mount_token_stub(&server).await;

        // Permissive stub: assert on the recorded wire path below rather than
        // a fixed matcher (the url crate normalizes %-encodings of unreserved
        // chars like '.' back to raw, so an exact-path matcher is brittle).
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "consumptionState": 0, "acknowledgementState": 0 }),
            ))
            .mount(&server)
            .await;

        let http = http_client();
        let client = GoogleDeveloperClient::with_base_url(http, server.uri());
        let auth = GoogleServiceAccountAuth::with_token_uri(
            "svc@herald-test.iam.gserviceaccount.com".to_string(),
            fresh_rsa_pem(),
            format!("{}/token", server.uri()),
        );

        let product = client
            .get_product(&auth, "com.herald.app", "p/rod.uct", "a/b?c#d..")
            .await
            .expect("request must succeed against the permissive stub");
        assert_eq!(product.consumption_state, Some(0));

        // The wire path carries the hostile token percent-encoded inside ONE
        // segment: every '/' stays %2F (no traversal out of the purchases
        // subtree), '?' stays %3F, '#' stays %23. Old code sent /a/b?c#d..
        // raw: extra path segments plus an injected (empty) query.
        let requests = server.received_requests().await.unwrap();
        let api_request = requests
            .iter()
            .find(|r| r.url.path().contains("/purchases/products/"))
            .expect("developer API request recorded");
        let path = api_request.url.path();
        assert!(
            path.contains("a%2Fb%3Fc%23d"),
            "token must stay percent-encoded in a single segment, wire path: {path}"
        );
        assert!(
            !path.contains("/a/b"),
            "raw '/' from the token must never reach the wire path, got: {path}"
        );
        assert_eq!(
            path.matches('/').count(),
            6,
            "exactly the six structural segments, wire path: {path}"
        );
        assert!(
            api_request.url.query().is_none(),
            "no query may be injected through the token"
        );
    }

    // Intent (review 20260919, path_segment dot finding): a value that is a
    // lone dot segment cannot be ADDRESSED safely at all — WHATWG URL
    // parsing folds '.', '..' and even their '%2E' spellings into a
    // neighboring resource path, so the old pass-through '.' let a crafted
    // token hit e.g. /purchases/products/{product}/tokens (the collection)
    // instead of the token resource. No real store identifier has that
    // shape: reject.
    #[test]
    fn lone_dot_segments_are_rejected_not_encoded() {
        assert!(matches!(
            path_segment("."),
            Err(IapError::InvalidPathSegment(v)) if v == "."
        ));
        assert!(matches!(
            path_segment(".."),
            Err(IapError::InvalidPathSegment(v)) if v == ".."
        ));
        // Dots INSIDE a longer identifier are fine (package/product names).
        assert_eq!(path_segment("com.herald.app").unwrap(), "com.herald.app");
        // A %2e-looking input is double-escaped and stays one literal
        // segment (not a dot segment on the wire).
        assert_eq!(path_segment("%2e%2e").unwrap(), "%252e%252e");
    }
}
