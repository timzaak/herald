//! WeChat Pay v3 client: unified order (Native / JSAPI),
//! platform-certificate download, and callback verification + decryption.

use chrono::{DateTime, Utc};
use once_cell::sync::{Lazy, OnceCell};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use rsa::RsaPrivateKey;
use serde::Serialize;
use uuid::Uuid;

use crate::error::WechatPayError;
use crate::models::{
    CreateOrderResult, CreateOrderScene, DecryptedResource, JsapiParams, PlatformCert,
    WechatPayConfig,
};
use crate::platform_certs::PlatformCertCache;
use crate::signing::{
    build_authorization_header, decrypt_aes_gcm, parse_private_key, sign_jsapi_params,
    verify_callback_signature,
};

const DEFAULT_BASE_URL: &str = "https://api.mch.weixin.qq.com";

/// Process-wide HTTP client so the connection pool (TLS session to
/// `api.mch.weixin.qq.com`) is shared across the per-request clients built by
/// `get_wechat_client_for_realm`. `Client` clones are cheap (Arc inner).
static SHARED_HTTP: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        // Bound every outbound call: the webhook path reaches this client
        // BEFORE the inbound signature is verified, so an unresponsive
        // upstream must not pin request handlers indefinitely.
        .timeout(std::time::Duration::from_secs(10))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("default reqwest client construction cannot fail")
});

/// WeChat Pay v3 client for one merchant (realm). Constructed per-request from
/// `realm_config` by `get_wechat_client_for_realm`; the HTTP client and the
/// platform-certificate cache are shared process-wide so repeated webhook
/// verification / order creation does not re-handshake or re-download.
pub struct WechatPayClient {
    config: WechatPayConfig,
    http: reqwest::Client,
    base_url: String,
    certs: PlatformCertCache,
    /// Merchant signing key, parsed from `config.private_key_pem` on first use
    /// (the webhook path never signs, so it must not require a valid key).
    private_key: OnceCell<RsaPrivateKey>,
}

impl WechatPayClient {
    pub fn new(config: WechatPayConfig) -> Result<Self, WechatPayError> {
        if config.app_id.is_empty() {
            return Err(WechatPayError::ConfigMissing("app_id"));
        }
        if config.mch_id.is_empty() {
            return Err(WechatPayError::ConfigMissing("mch_id"));
        }
        if config.private_key_pem.is_empty() {
            return Err(WechatPayError::ConfigMissing("private_key_pem"));
        }
        if config.serial_no.is_empty() {
            return Err(WechatPayError::ConfigMissing("serial_no"));
        }
        if config.api_v3_key.len() != 32 {
            return Err(WechatPayError::ConfigInvalid(format!(
                "api_v3_key must be 32 bytes, got {}",
                config.api_v3_key.len()
            )));
        }
        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        Ok(Self {
            config,
            http: SHARED_HTTP.clone(),
            base_url,
            certs: PlatformCertCache::shared(),
            private_key: OnceCell::new(),
        })
    }

    /// The parsed merchant signing key (lazily parsed once per client).
    fn signing_key(&self) -> Result<&RsaPrivateKey, WechatPayError> {
        self.private_key
            .get_or_try_init(|| parse_private_key(&self.config.private_key_pem))
    }

    /// Create a unified order. `amount_fen` is the total in fen (cents); `expire`
    /// sets WeChat's `time_expire` (RFC3339) so the QR / JSAPI session lifetime
    /// matches `payment_attempts.expires_at` (DEC: ≤2h).
    pub async fn create_order(
        &self,
        scene: CreateOrderScene,
        out_trade_no: &str,
        description: &str,
        amount_fen: i64,
        currency: &str,
        expire: DateTime<Utc>,
    ) -> Result<CreateOrderResult, WechatPayError> {
        let payer = match &scene {
            CreateOrderScene::Jsapi { openid } => Some(Payer {
                openid: openid.clone(),
            }),
            CreateOrderScene::Native => None,
        };
        let body = CreateOrderBody {
            appid: &self.config.app_id,
            mchid: &self.config.mch_id,
            description,
            out_trade_no,
            time_expire: &expire.to_rfc3339(),
            notify_url: &self.config.notify_url,
            amount: Amount {
                total: amount_fen,
                currency,
            },
            payer,
        };
        let body_json = serde_json::to_string(&body)?;
        let (path, is_jsapi) = match scene {
            CreateOrderScene::Native => ("/v3/pay/transactions/native", false),
            CreateOrderScene::Jsapi { .. } => ("/v3/pay/transactions/jsapi", true),
        };

        let resp = self.signed_request("POST", path, &body_json).await?;

        if is_jsapi {
            let parsed: PrepayResponse = resp.json().await?;
            let prepay_id = parsed.prepay_id.ok_or(WechatPayError::NoPrepayId)?;
            Ok(CreateOrderResult::Jsapi(
                self.build_jsapi_params(&prepay_id)?,
            ))
        } else {
            let parsed: NativeResponse = resp.json().await?;
            let code_url = parsed.code_url.ok_or(WechatPayError::NoCodeUrl)?;
            Ok(CreateOrderResult::Native { code_url })
        }
    }

    fn build_jsapi_params(&self, prepay_id: &str) -> Result<JsapiParams, WechatPayError> {
        let time_stamp = Utc::now().timestamp().to_string();
        let nonce_str = nonce();
        let package = format!("prepay_id={prepay_id}");
        let pay_sign = sign_jsapi_params(
            self.signing_key()?,
            &self.config.app_id,
            &time_stamp,
            &nonce_str,
            &package,
        )?;
        Ok(JsapiParams {
            app_id: self.config.app_id.clone(),
            time_stamp,
            nonce_str,
            package,
            sign_type: "RSA".to_string(),
            pay_sign,
        })
    }

    /// Download and decrypt all current platform certificates (no caching).
    pub async fn download_platform_certs(&self) -> Result<Vec<PlatformCert>, WechatPayError> {
        let resp = self.signed_request("GET", "/v3/certificates", "").await?;
        let body = resp.text().await?;
        crate::platform_certs::parse_platform_certs(&body, &self.config.api_v3_key)
    }

    /// Resolve the platform public key for a callback: prefer the manual
    /// override; otherwise use the cached certificate matching `serial`,
    /// re-fetching `/v3/certificates` (throttled per realm) when the serial
    /// is unknown to the cache.
    pub async fn get_platform_public_key(
        &self,
        realm_id: &str,
        serial: &str,
    ) -> Result<String, WechatPayError> {
        if let Some(override_key) = &self.config.platform_public_key_override {
            return Ok(override_key.clone());
        }
        let now = Utc::now();
        if let Some(certs) = self.certs.get(realm_id).await
            && let Some(found) = PlatformCertCache::find_valid(&certs, serial, now)
        {
            return Ok(found.public_key_pem.clone());
        }
        // Unknown serial (or a cold cache): WeChat's integration contract is
        // to re-fetch /v3/certificates — the platform rotates its signing
        // certificate on its own schedule, and a rotation landing inside the
        // 6h cache TTL must converge on the first new-serial callback instead
        // of failing every callback until the TTL expires (review 20260919
        // finding 6; the old always-reject negative cache is exactly what
        // broke this). The per-realm refetch throttle keeps the flip side
        // bounded: an anonymous flood of made-up serials cannot force a
        // signed outbound download per request (audit run-1:
        // wechat-callback-platform-cert-download-before-signature-verification-no-negative-cache).
        if !self.certs.try_begin_refetch(realm_id).await {
            return Err(WechatPayError::PlatformCertNotFound(serial.to_string()));
        }
        let downloaded = self.download_platform_certs().await?;
        let public_key = downloaded
            .iter()
            .find(|c| c.serial_no == serial)
            .map(|c| c.public_key_pem.clone());
        // Cache the downloaded set even when the caller's serial missed: it
        // IS the provider's current authoritative set, and it makes later
        // known-serial lookups cache-only.
        self.certs.insert(realm_id, downloaded).await;
        public_key.ok_or_else(|| WechatPayError::PlatformCertNotFound(serial.to_string()))
    }

    /// Verify a callback's request signature.
    pub async fn verify_callback(
        &self,
        realm_id: &str,
        timestamp: &str,
        nonce: &str,
        signature_b64: &str,
        serial: &str,
        body: &str,
    ) -> Result<(), WechatPayError> {
        let public_key = self.get_platform_public_key(realm_id, serial).await?;
        let message = format!("{timestamp}\n{nonce}\n{body}\n");
        verify_callback_signature(&public_key, &message, signature_b64)
    }

    /// Decrypt a callback `resource` payload into the typed result.
    pub fn decrypt_resource(
        &self,
        resource: &EncryptedResource,
    ) -> Result<DecryptedResource, WechatPayError> {
        let plain = decrypt_aes_gcm(
            &resource.ciphertext,
            &resource.associated_data,
            &resource.nonce,
            &self.config.api_v3_key,
        )?;
        serde_json::from_str(&plain).map_err(|e| WechatPayError::Parse(e.to_string()))
    }

    async fn signed_request(
        &self,
        method: &str,
        path_and_query: &str,
        body: &str,
    ) -> Result<reqwest::Response, WechatPayError> {
        let timestamp = Utc::now().timestamp().to_string();
        let nonce = nonce();
        let authorization = build_authorization_header(
            self.signing_key()?,
            &self.config.mch_id,
            &self.config.serial_no,
            method,
            path_and_query,
            body,
            &timestamp,
            &nonce,
        )?;
        let url = format!("{}{}", self.base_url, path_and_query);
        let mut req = self
            .http
            .request(
                reqwest::Method::from_bytes(method.as_bytes()).unwrap(),
                &url,
            )
            .header(AUTHORIZATION, authorization)
            .header(ACCEPT, "application/json");
        if body.is_empty() {
            req = req.header(CONTENT_TYPE, "application/json");
        } else {
            req = req
                .header(CONTENT_TYPE, "application/json")
                .body(body.to_string());
        }
        let resp = req.send().await?;
        let status = resp.status();
        if status.is_success() {
            Ok(resp)
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(WechatPayError::Api {
                status: status.as_u16(),
                body,
            })
        }
    }
}

/// Encrypted `resource` block carried inside a WeChat notification body.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EncryptedResource {
    #[serde(default)]
    pub ciphertext: String,
    #[serde(default)]
    pub associated_data: String,
    #[serde(default)]
    pub nonce: String,
}

fn nonce() -> String {
    Uuid::now_v7().simple().to_string()
}

#[derive(Serialize)]
struct CreateOrderBody<'a> {
    appid: &'a str,
    mchid: &'a str,
    description: &'a str,
    out_trade_no: &'a str,
    time_expire: &'a str,
    notify_url: &'a str,
    amount: Amount<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    payer: Option<Payer>,
}

#[derive(Serialize)]
struct Amount<'a> {
    total: i64,
    currency: &'a str,
}

#[derive(Serialize)]
struct Payer {
    openid: String,
}

#[derive(serde::Deserialize)]
struct NativeResponse {
    code_url: Option<String>,
}

#[derive(serde::Deserialize)]
struct PrepayResponse {
    prepay_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::CreateOrderScene;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config(base_url: String) -> WechatPayConfig {
        WechatPayConfig {
            app_id: "wxappid".into(),
            mch_id: "1234567890".into(),
            private_key_pem: include_str!("../tests/test_private_key.pem").to_string(),
            serial_no: "serial123".into(),
            api_v3_key: "0123456789abcdef0123456789abcdef".into(),
            notify_url: "https://example.com/hook".into(),
            platform_public_key_override: None,
            base_url: Some(base_url),
        }
    }

    #[tokio::test]
    async fn create_native_order_calls_v3_and_returns_code_url() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v3/pay/transactions/native"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "code_url": "weixin://wx/pay/bizpayurl?pr=abc" }),
            ))
            .mount(&server)
            .await;

        let client = WechatPayClient::new(test_config(server.uri())).unwrap();
        let result = client
            .create_order(
                CreateOrderScene::Native,
                "CAS_ab_x",
                "desc",
                100,
                "CNY",
                Utc::now() + chrono::Duration::hours(1),
            )
            .await
            .expect("native order ok");
        match result {
            CreateOrderResult::Native { code_url } => {
                assert!(code_url.starts_with("weixin://"));
            }
            _ => panic!("expected Native result"),
        }
    }

    #[tokio::test]
    async fn create_jsapi_order_returns_signed_params() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v3/pay/transactions/jsapi"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "prepay_id": "wx2026prep" })),
            )
            .mount(&server)
            .await;

        let client = WechatPayClient::new(test_config(server.uri())).unwrap();
        let result = client
            .create_order(
                CreateOrderScene::Jsapi {
                    openid: "o123".into(),
                },
                "CAS_ab_y",
                "desc",
                100,
                "CNY",
                Utc::now() + chrono::Duration::hours(1),
            )
            .await
            .expect("jsapi order ok");
        match result {
            CreateOrderResult::Jsapi(params) => {
                assert_eq!(params.package, "prepay_id=wx2026prep");
                assert_eq!(params.sign_type, "RSA");
                assert!(!params.pay_sign.is_empty());
            }
            _ => panic!("expected Jsapi result"),
        }
    }

    #[tokio::test]
    async fn api_error_is_propagated_with_status() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v3/certificates"))
            .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
            .mount(&server)
            .await;
        let client = WechatPayClient::new(test_config(server.uri())).unwrap();
        let err = client.download_platform_certs().await.unwrap_err();
        match err {
            WechatPayError::Api { status, .. } => assert_eq!(status, 401),
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    // A /v3/certificates response body: one AEAD_AES_256_GCM-encrypted
    // placeholder certificate entry per serial, decryptable by the client.
    fn cert_body(key: &[u8], serials: &[&str]) -> String {
        use aes_gcm::aead::{Aead, KeyInit, Payload};
        use base64::Engine;
        use base64::engine::general_purpose::STANDARD;

        let cipher = aes_gcm::Aes256Gcm::new_from_slice(key).unwrap();
        let ciphertext = STANDARD.encode(
            cipher
                .encrypt(
                    aes_gcm::Nonce::from_slice(b"nonce1234567"),
                    Payload {
                        msg: b"-----BEGIN CERTIFICATE-----x-----END CERTIFICATE-----",
                        aad: b"cert",
                    },
                )
                .unwrap(),
        );
        let entries: Vec<String> = serials
            .iter()
            .map(|s| {
                format!(
                    "{{\"serial_no\":\"{s}\",\"effective_time\":\"2026-01-01T00:00:00+08:00\",\"expire_time\":\"2099-01-01T00:00:00+08:00\",\"encrypt_certificate\":{{\"algorithm\":\"AEAD_AES_256_GCM\",\"nonce\":\"nonce1234567\",\"associated_data\":\"cert\",\"ciphertext\":\"{ciphertext}\"}}}}"
                )
            })
            .collect();
        format!("[{}]", entries.join(","))
    }

    // Intent (audit run-1:
    // wechat-callback-platform-cert-download-before-signature-verification-no-negative-cache):
    // the anonymous webhook route resolves the platform key for a
    // caller-supplied serial BEFORE the inbound signature is checked, so
    // made-up serials must never force one signed outbound download per
    // request. The first unknown-serial call downloads once (the refetch
    // contract) and consumes the realm's throttle slot; later unknown serials
    // inside the window are rejected by the gate. A download that misses the
    // caller's serial must still populate the cache (the set IS the
    // provider's current one). Old code: every unknown-serial request
    // re-downloaded.
    #[tokio::test]
    async fn unknown_serial_against_fresh_cache_is_rejected_without_download() {
        let key = b"0123456789abcdef0123456789abcdef";
        let body = cert_body(key, &["S1"]);

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v3/certificates"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        let client = WechatPayClient::new(test_config(server.uri())).unwrap();
        let realm = format!(
            "negcache-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );

        // Cold cache: the first unknown-serial call downloads once and — the
        // fix — still caches the authoritative set despite the miss.
        let err = client
            .get_platform_public_key(&realm, "made-up-serial")
            .await
            .unwrap_err();
        assert!(
            matches!(err, WechatPayError::PlatformCertNotFound(_)),
            "got {err:?}"
        );
        assert_eq!(server.received_requests().await.unwrap().len(), 1);

        // Warm + fresh: a second unknown serial is answered from cache —
        // zero further outbound downloads.
        let err = client
            .get_platform_public_key(&realm, "another-made-up-serial")
            .await
            .unwrap_err();
        assert!(
            matches!(err, WechatPayError::PlatformCertNotFound(_)),
            "got {err:?}"
        );
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            1,
            "fresh cached set must reject unknown serials without re-downloading"
        );

        // The known serial still resolves from the warm cache.
        let pem = client.get_platform_public_key(&realm, "S1").await.unwrap();
        assert!(pem.contains("BEGIN CERTIFICATE"));
    }

    // Intent (review 20260919 finding 6): WeChat rotates its platform
    // certificate on its own schedule — a rotation landing inside the 6h
    // cache TTL makes callbacks arrive signed with a serial the cached set
    // does not know. The old always-reject negative cache answered those
    // with PlatformCertNotFound until the TTL expired, failing every
    // callback in the rotation window; now the unknown serial refetches
    // /v3/certificates (throttled), so the rotation converges as soon as the
    // throttle window elapses.
    #[tokio::test]
    async fn rotated_serial_converges_after_refetch_throttle() {
        let key = b"0123456789abcdef0123456789abcdef";
        let server = MockServer::start().await;
        let mock_pre_rotation = Mock::given(method("GET"))
            .and(path("/v3/certificates"))
            .respond_with(ResponseTemplate::new(200).set_body_string(cert_body(key, &["S1"])))
            .mount_as_scoped(&server)
            .await;

        // Same-module literal construction: the shared production cache's
        // 5-minute throttle is not testable; this cache throttles per 100ms.
        let client = WechatPayClient {
            config: test_config(server.uri()),
            http: SHARED_HTTP.clone(),
            base_url: server.uri(),
            certs: crate::platform_certs::PlatformCertCache::with_refetch_throttle(
                std::time::Duration::from_millis(100),
            ),
            private_key: OnceCell::new(),
        };
        let realm = format!(
            "rotation-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );

        // Warm the cache with the pre-rotation set.
        let pem = client.get_platform_public_key(&realm, "S1").await.unwrap();
        assert!(pem.contains("BEGIN CERTIFICATE"));
        assert_eq!(server.received_requests().await.unwrap().len(), 1);

        // WeChat rotates: /v3/certificates now also returns S2.
        drop(mock_pre_rotation);
        Mock::given(method("GET"))
            .and(path("/v3/certificates"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(cert_body(key, &["S1", "S2-rotated"])),
            )
            .mount(&server)
            .await;

        // Inside the throttle window the rotated serial is still rejected
        // without a download (flood bound) ...
        assert!(
            client
                .get_platform_public_key(&realm, "S2-rotated")
                .await
                .is_err()
        );
        assert_eq!(server.received_requests().await.unwrap().len(), 1);

        // ... and converges once the window elapses: the unknown serial
        // refetches and resolves the rotated key.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let pem = client
            .get_platform_public_key(&realm, "S2-rotated")
            .await
            .expect("rotated serial must resolve after the throttle window");
        assert!(pem.contains("BEGIN CERTIFICATE"));
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }

    // Intent (audit run-1: SHARED_HTTP-no-request-timeout-wechat-outbound):
    // every outbound call is deadline-bounded. The webhook path reaches this
    // client BEFORE the inbound signature is verified, so an upstream that
    // accepts the connection and stalls must fail in bounded time instead of
    // pinning request handlers until the environment intervenes. Old code had
    // no timeout at all and would hang this test's 30s bound.
    #[tokio::test]
    async fn stalled_upstream_fails_within_bounded_time() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            // Accept connections forever; never answer.
            loop {
                let _ = listener.accept().await;
            }
        });

        let client = WechatPayClient::new(test_config(format!("http://{addr}"))).unwrap();
        let started = std::time::Instant::now();
        let bounded = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            client.download_platform_certs(),
        )
        .await;
        let elapsed = started.elapsed();
        assert!(
            bounded.is_err() || bounded.unwrap().is_err(),
            "a stalled upstream must produce an error, not a hang"
        );
        assert!(
            elapsed < std::time::Duration::from_secs(30),
            "outbound call must be bounded by the client timeout, took {elapsed:?}"
        );
    }
}
