// =============================================================================
// WeChat Pay v3 callback forging helpers
// =============================================================================
//
// Builds cryptographically-valid WeChat Pay v3 payment-result notifications
// for the `POST /api/third/pay/{realmId}/wechat/webhooks` scenario tests
// (`api-billing/src/wechat_webhook_handlers.rs`).
//
// Trust model in tests: the realm's `platform_public_key` realm_config override
// is seeded with the public half of `platform_key()`, and the callback request
// signature is produced with its private half — so the handler's real
// `verify_callback_signature` path verifies end-to-end without any network /
// real-WeChat-certificate dependency. The `resource` ciphertext is produced with
// the realm's APIv3 Key via the same AES-256-GCM scheme the handler decrypts.

#![allow(dead_code)]

use aes_gcm::Aes256Gcm;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rand::rngs::OsRng;
use rsa::pkcs1v15::Pkcs1v15Sign;
use rsa::pkcs8::EncodePublicKey;
use rsa::{RsaPrivateKey, RsaPublicKey};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::sync::OnceLock;

/// A generated RSA-2048 keypair standing in for the WeChat platform signing
/// key during tests. Held behind a `OnceLock` so keygen runs once per process.
pub struct WechatPlatformKey {
    private: RsaPrivateKey,
    pub public_key_pem: String,
}

impl WechatPlatformKey {
    fn generate() -> Self {
        let mut rng = OsRng;
        let private =
            RsaPrivateKey::new(&mut rng, 2048).expect("generate platform RSA key for tests");
        let public = RsaPublicKey::from(&private);
        let public_key_pem = public
            .to_public_key_pem(rsa::pkcs8::LineEnding::LF)
            .expect("encode platform public key PEM");
        Self {
            private,
            public_key_pem,
        }
    }

    /// Sign the canonical callback message (`{timestamp}\n{nonce}\n{body}\n`)
    /// with the platform private key (RSA-SHA256), base64-encoded — matches
    /// what the handler's `verify_callback_signature` checks.
    pub fn sign_callback(&self, timestamp: &str, nonce: &str, body: &str) -> String {
        let message = format!("{timestamp}\n{nonce}\n{body}\n");
        let digest = Sha256::digest(message.as_bytes());
        let sig = self
            .private
            .sign(Pkcs1v15Sign::new::<Sha256>(), &digest)
            .expect("sign callback");
        STANDARD.encode(sig)
    }
}

static PLATFORM_KEY: OnceLock<WechatPlatformKey> = OnceLock::new();

/// The shared test platform keypair. Its public PEM is the value seeded into
/// the realm's `platform_public_key` override; callbacks are signed with the
/// private half.
pub fn platform_key() -> &'static WechatPlatformKey {
    PLATFORM_KEY.get_or_init(WechatPlatformKey::generate)
}

/// Seed all `config_type='wechat'` rows for a realm. The platform public-key
/// override is set to `platform_key().public_key_pem` so callback verification
/// works without a real WeChat certificate download.
pub async fn insert_wechat_realm_config(pool: &PgPool, realm_id: &str) {
    let notify_url = format!("https://example.com/api/third/pay/{realm_id}/wechat/webhooks");
    let public_key = platform_key().public_key_pem.clone();
    let rows: [(&str, String, bool); 7] = [
        ("app_id", "wxtestappid".to_string(), false),
        ("mch_id", "1230000109".to_string(), false),
        // The webhook path never uses the merchant private key (it verifies
        // with the platform key and decrypts with the APIv3 key), so a
        // non-empty placeholder satisfies `WechatPayClient::new`.
        (
            "private_key",
            "-----BEGIN PRIVATE KEY-----\nunused-in-webhook-path\n-----END PRIVATE KEY-----"
                .to_string(),
            true,
        ),
        ("serial_no", "serial-test".to_string(), false),
        (
            "v3_key",
            "0123456789abcdef0123456789abcdef".to_string(),
            true,
        ),
        ("notify_url", notify_url, false),
        ("platform_public_key", public_key, false),
    ];
    for (key, value, is_secret) in rows {
        sqlx::query(
            "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, is_secret, enabled, metadata)
             VALUES ($1, 'wechat', $2, $3, $4, true, null)
             ON CONFLICT (realm_id, config_type, config_key)
             DO UPDATE SET config_value = EXCLUDED.config_value, is_secret = EXCLUDED.is_secret, enabled = true, updated_at = CURRENT_TIMESTAMP",
        )
        .bind(realm_id)
        .bind(key)
        .bind(value)
        .bind(is_secret)
        .execute(pool)
        .await
        .expect("Failed to upsert WeChat realm_config row");
    }
}

/// APIv3 Key used by `insert_wechat_realm_config` (tests decrypt with the same).
pub const TEST_V3_KEY: &str = "0123456789abcdef0123456789abcdef";

/// The serial number identifying the test platform cert, sent in the
/// `Wechatpay-Serial` header (ignored by the handler when the override is set).
pub const TEST_SERIAL: &str = "platform-serial-test";

/// Encrypt `plaintext` with AES-256-GCM (APIv3 Key, 12-byte nonce, AAD =
/// `associated_data`) and return the base64 ciphertext — the inverse of the
/// handler's `decrypt_aes_gcm`.
pub fn encrypt_resource(
    plaintext: &str,
    associated_data: &str,
    nonce: &str,
    api_v3_key: &str,
) -> String {
    let cipher = Aes256Gcm::new_from_slice(api_v3_key.as_bytes()).expect("v3 key is 32 bytes");
    let ct = cipher
        .encrypt(
            aes_gcm::Nonce::from_slice(nonce.as_bytes()),
            Payload {
                msg: plaintext.as_bytes(),
                aad: associated_data.as_bytes(),
            },
        )
        .expect("encrypt resource");
    STANDARD.encode(ct)
}

/// Build a WeChat v3 notification body whose decrypted `resource` carries the
/// given payment outcome. `amount_total` is in fen (cents).
pub fn build_notification(
    event_id: &str,
    out_trade_no: &str,
    transaction_id: &str,
    trade_state: &str,
    amount_total: i64,
    api_v3_key: &str,
) -> String {
    let plaintext = serde_json::json!({
        "out_trade_no": out_trade_no,
        "transaction_id": transaction_id,
        "trade_state": trade_state,
        "amount": { "total": amount_total, "currency": "CNY" }
    })
    .to_string();
    let nonce = "nonce1234567"; // 12 bytes
    let associated_data = "transaction";
    let ciphertext = encrypt_resource(&plaintext, associated_data, nonce, api_v3_key);
    serde_json::json!({
        "id": event_id,
        "event_type": "TRANSACTION.SUCCESS",
        "resource_type": "encrypt-resource",
        "resource": {
            "algorithm": "AEAD_AES_256_GCM",
            "ciphertext": ciphertext,
            "associated_data": associated_data,
            "nonce": nonce
        }
    })
    .to_string()
}

/// Build the canonical callback message and sign it with the test platform key.
/// Returns `(timestamp, nonce, signature)`. The timestamp is the current wall
/// clock so the callback passes the handler's replay window.
pub fn sign_signed_headers(body: &str) -> (String, String, String) {
    let timestamp = chrono::Utc::now().timestamp().to_string();
    let nonce = "nonce-abc".to_string();
    let signature = platform_key().sign_callback(&timestamp, &nonce, body);
    (timestamp, nonce, signature)
}

/// Build a `platform_private_key`-signed POST request, or an UNSIGNED one when
/// `signed = false` (to exercise the signature-rejection path).
pub fn wechat_webhook_request(
    realm_id: &str,
    body: String,
    signed: bool,
) -> axum::http::Request<axum::body::Body> {
    use axum::body::Body;
    use axum::http::Request;
    let mut builder = Request::builder()
        .method("POST")
        .uri(format!("/api/third/pay/{realm_id}/wechat/webhooks"))
        .header("Content-Type", "application/json");
    if signed {
        let (timestamp, nonce, signature) = sign_signed_headers(&body);
        builder = builder
            .header("Wechatpay-Timestamp", timestamp)
            .header("Wechatpay-Nonce", nonce)
            .header("Wechatpay-Signature", signature)
            .header("Wechatpay-Serial", TEST_SERIAL);
    }
    builder.body(Body::from(body)).unwrap()
}
