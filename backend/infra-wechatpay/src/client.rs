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
    /// override; otherwise use (downloading if missing/stale) the cached
    /// certificate matching `serial`.
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
            && let Some(found) = PlatformCertCache::find_fresh(&certs, serial, now)
        {
            return Ok(found.public_key_pem.clone());
        }
        let downloaded = self.download_platform_certs().await?;
        let public_key = downloaded
            .iter()
            .find(|c| c.serial_no == serial)
            .map(|c| c.public_key_pem.clone())
            .ok_or_else(|| WechatPayError::PlatformCertNotFound(serial.to_string()))?;
        self.certs.insert(realm_id, downloaded).await;
        Ok(public_key)
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
}
