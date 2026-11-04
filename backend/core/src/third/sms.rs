// SMS delivery channel for phone login. The `base_url` config override is
// test-only (wiremock); production writes are rejected at the configs API.

use std::future::Future;

use base64::Engine;
use herald_domain::telemetry::external_http::timed_external_http_span;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha1::Sha1;
use sqlx::PgPool;

type HmacSha1 = Hmac<Sha1>;

const SMS_SEND_TIMEOUT_SECS: u64 = 30;

pub trait SmsProvider: Send + Sync {
    fn send(&self, to_e164: &str, code: &str) -> impl Future<Output = Result<(), SmsError>> + Send;
}

#[derive(Debug, thiserror::Error)]
pub enum SmsError {
    #[error("sms channel not configured")]
    NotConfigured,
    #[error("destination not supported by provider")]
    UnsupportedDestination,
    /// Upstream failure, already redacted (no signature strings/credentials).
    #[error("upstream send failed: {0}")]
    Upstream(String),
}

/// 6-digit zero-padded OTP from the random tail of a UUIDv7: 74 random bits
/// far exceed the 6-digit space, and no RNG dependency is needed.
pub fn generate_sms_code() -> String {
    let id = uuid::Uuid::now_v7();
    let bytes = id.as_bytes();
    let tail = u64::from_le_bytes([
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    ]);
    let n = (tail % 1_000_000) as u32;
    format!("{n:06}")
}

#[cfg(test)]
mod code_tests {
    use super::*;

    /// WHY: provider templates pin the code to exactly 6 positions and
    /// verify entrances enforce `^[0-9]{6}$`.
    #[test]
    fn sms_codes_are_six_ascii_digits() {
        for _ in 0..100 {
            let code = generate_sms_code();
            assert_eq!(code.len(), 6);
            assert!(code.bytes().all(|b| b.is_ascii_digit()));
        }
    }
}

const ALIYUN_DEFAULT_BASE_URL: &str = "https://dysmsapi.aliyuncs.com";

pub struct AliyunSmsClient {
    access_key_id: String,
    access_key_secret: String,
    sign_name: String,
    template_code: String,
    base_url: String,
    http: reqwest::Client,
}

/// RFC 3986 percent-encoding as the Aliyun RPC signature requires:
/// form-urlencoded is wrong here (`+` for space, `*` left unencoded).
fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for &byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn canonical_query<'a>(params: impl Iterator<Item = (&'a str, &'a str)>) -> String {
    let mut pairs: Vec<(String, String)> = params
        .map(|(k, v)| (percent_encode(k), percent_encode(v)))
        .collect();
    pairs.sort();
    pairs
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// `GET & percentEncode("/") & percentEncode(sorted query)`.
fn aliyun_string_to_sign(canonical_query: &str) -> String {
    format!(
        "GET&{}&{}",
        percent_encode("/"),
        percent_encode(canonical_query)
    )
}

/// Signature = Base64(HMAC-SHA1(AccessKeySecret + "&", StringToSign)).
fn aliyun_signature(access_key_secret: &str, string_to_sign: &str) -> String {
    let mut mac = HmacSha1::new_from_slice(format!("{access_key_secret}&").as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(string_to_sign.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

/// `+86` becomes the 11-digit national form the domestic API requires;
/// other country codes are unsupported by design.
fn to_aliyun_domestic_number(to_e164: &str) -> Option<&str> {
    to_e164
        .strip_prefix("+86")
        .filter(|rest| rest.len() == 11 && rest.bytes().all(|b| b.is_ascii_digit()))
}

impl AliyunSmsClient {
    fn new(
        access_key_id: String,
        access_key_secret: String,
        sign_name: String,
        template_code: String,
        base_url: Option<String>,
    ) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(SMS_SEND_TIMEOUT_SECS))
            .connect_timeout(std::time::Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| anyhow::anyhow!("failed to build Aliyun SMS client: {e}"))?;
        Ok(Self {
            access_key_id,
            access_key_secret,
            sign_name,
            template_code,
            base_url: base_url.unwrap_or_else(|| ALIYUN_DEFAULT_BASE_URL.to_string()),
            http,
        })
    }

    /// Nonce/timestamp are injected so tests get a reproducible signature.
    fn build_signed_query(&self, to: &str, code: &str, nonce: &str, timestamp: &str) -> String {
        let template_param = format!(r#"{{"code":"{code}"}}"#);
        let params: Vec<(&str, &str)> = vec![
            ("AccessKeyId", self.access_key_id.as_str()),
            ("Action", "SendSms"),
            ("Format", "JSON"),
            ("PhoneNumbers", to),
            ("RegionId", "cn-hangzhou"),
            ("SignName", self.sign_name.as_str()),
            ("SignatureMethod", "HMAC-SHA1"),
            ("SignatureNonce", nonce),
            ("SignatureVersion", "1.0"),
            ("TemplateCode", self.template_code.as_str()),
            ("TemplateParam", template_param.as_str()),
            ("Timestamp", timestamp),
            ("Version", "2017-05-25"),
        ];
        let canonical = canonical_query(params.iter().copied());
        let string_to_sign = aliyun_string_to_sign(&canonical);
        let signature = aliyun_signature(&self.access_key_secret, &string_to_sign);
        format!("Signature={}&{canonical}", percent_encode(&signature))
    }
}

impl SmsProvider for AliyunSmsClient {
    async fn send(&self, to_e164: &str, code: &str) -> Result<(), SmsError> {
        let to = to_aliyun_domestic_number(to_e164).ok_or(SmsError::UnsupportedDestination)?;
        let nonce = uuid::Uuid::now_v7().to_string();
        let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let query = self.build_signed_query(to, code, &nonce, &timestamp);
        let url = format!("{}/?{}", self.base_url.trim_end_matches('/'), query);

        let timing = timed_external_http_span(&self.base_url, "GET");
        let _span_enter = timing.span().enter();

        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| SmsError::Upstream(format!("aliyun request failed: {e}")))?;
        let status = resp.status();
        // Aliyun answers 200 with a JSON body whose Code must be "OK"; body
        // fields carry no credentials, so they are safe in error text.
        let body: AliyunSendResponse = resp
            .json()
            .await
            .map_err(|e| SmsError::Upstream(format!("aliyun response unreadable: {e}")))?;
        if status.is_success() && body.code.as_deref() == Some("OK") {
            Ok(())
        } else {
            Err(SmsError::Upstream(format!(
                "aliyun send failed: {} {}",
                status.as_u16(),
                body.message
                    .unwrap_or_else(|| body.code.unwrap_or_default())
            )))
        }
    }
}

#[derive(Debug, Deserialize)]
struct AliyunSendResponse {
    // Aliyun keys are capitalized; serde is case-sensitive, hence the renames.
    #[serde(rename = "Code")]
    code: Option<String>,
    #[serde(rename = "Message")]
    message: Option<String>,
}

const TWILIO_DEFAULT_BASE_URL: &str = "https://api.twilio.com";

pub struct TwilioSmsClient {
    account_sid: String,
    auth_token: String,
    from: String,
    content_sid: String,
    base_url: String,
    http: reqwest::Client,
}

impl TwilioSmsClient {
    fn new(
        account_sid: String,
        auth_token: String,
        from: String,
        content_sid: String,
        base_url: Option<String>,
    ) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(SMS_SEND_TIMEOUT_SECS))
            .connect_timeout(std::time::Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| anyhow::anyhow!("failed to build Twilio SMS client: {e}"))?;
        Ok(Self {
            account_sid,
            auth_token,
            from,
            content_sid,
            base_url: base_url.unwrap_or_else(|| TWILIO_DEFAULT_BASE_URL.to_string()),
            http,
        })
    }

    fn messages_url(&self) -> String {
        format!(
            "{}/2010-04-01/Accounts/{}/Messages.json",
            self.base_url.trim_end_matches('/'),
            self.account_sid
        )
    }

    fn authorization_header(&self) -> String {
        let raw = format!("{}:{}", self.account_sid, self.auth_token);
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(raw)
        )
    }
}

impl SmsProvider for TwilioSmsClient {
    async fn send(&self, to_e164: &str, code: &str) -> Result<(), SmsError> {
        let timing = timed_external_http_span(&self.base_url, "POST");
        let _span_enter = timing.span().enter();

        let resp = self
            .http
            .post(self.messages_url())
            .header("Authorization", self.authorization_header())
            // Content stays in the provider-approved template; only the code
            // is passed as a template variable (no local SMS rendering).
            .form(&[
                ("To", to_e164),
                ("From", self.from.as_str()),
                ("ContentSid", self.content_sid.as_str()),
                ("Variables", format!(r#"{{"1": "{code}"}}"#).as_str()),
            ])
            .send()
            .await
            .map_err(|e| SmsError::Upstream(format!("twilio request failed: {e}")))?;

        let status = resp.status();
        if status.is_success() {
            Ok(())
        } else {
            Err(SmsError::Upstream(format!(
                "twilio send failed: {}",
                status.as_u16()
            )))
        }
    }
}

pub enum SmsProviderKind {
    Aliyun(AliyunSmsClient),
    Twilio(TwilioSmsClient),
}

impl SmsProvider for SmsProviderKind {
    async fn send(&self, to_e164: &str, code: &str) -> Result<(), SmsError> {
        match self {
            Self::Aliyun(p) => p.send(to_e164, code).await,
            Self::Twilio(p) => p.send(to_e164, code).await,
        }
    }
}

/// Kept in sync with `is_sensitive_config_key` and the frontend form.
const ALIYUN_REQUIRED_KEYS: [&str; 4] = [
    "aliyun_access_key_id",
    "aliyun_access_key_secret",
    "aliyun_sign_name",
    "aliyun_template_code",
];
const TWILIO_REQUIRED_KEYS: [&str; 3] = [
    "twilio_account_sid",
    "twilio_auth_token",
    "twilio_content_sid",
];
/// E.164 sender or Messaging Service SID; separate from the auth tuple so
/// the missing-field list stays provider-idiomatic.
const TWILIO_FROM_KEY: &str = "twilio_from";

#[derive(Default)]
pub struct SmsConfig {
    pub provider: String,
    pub aliyun_access_key_id: Option<String>,
    pub aliyun_access_key_secret: Option<String>,
    pub aliyun_sign_name: Option<String>,
    pub aliyun_template_code: Option<String>,
    pub twilio_account_sid: Option<String>,
    pub twilio_auth_token: Option<String>,
    pub twilio_from: Option<String>,
    pub twilio_content_sid: Option<String>,
    pub base_url: Option<String>,
}

fn field_of<'a>(config: &'a SmsConfig, key: &str) -> Option<&'a str> {
    match key {
        "aliyun_access_key_id" => config.aliyun_access_key_id.as_deref(),
        "aliyun_access_key_secret" => config.aliyun_access_key_secret.as_deref(),
        "aliyun_sign_name" => config.aliyun_sign_name.as_deref(),
        "aliyun_template_code" => config.aliyun_template_code.as_deref(),
        "twilio_account_sid" => config.twilio_account_sid.as_deref(),
        "twilio_auth_token" => config.twilio_auth_token.as_deref(),
        "twilio_from" => config.twilio_from.as_deref(),
        "twilio_content_sid" => config.twilio_content_sid.as_deref(),
        _ => None,
    }
}

impl SmsConfig {
    /// Errors carry no credentials.
    pub fn build_provider(&self) -> Result<SmsProviderKind, SmsError> {
        match self.provider.as_str() {
            "aliyun" => {
                let get = |key: &str| {
                    field_of(self, key)
                        .filter(|v| !v.is_empty())
                        .map(str::to_string)
                        .ok_or(SmsError::NotConfigured)
                };
                let client = AliyunSmsClient::new(
                    get("aliyun_access_key_id")?,
                    get("aliyun_access_key_secret")?,
                    get("aliyun_sign_name")?,
                    get("aliyun_template_code")?,
                    self.base_url.clone(),
                )
                .map_err(|e| SmsError::Upstream(format!("aliyun client init failed: {e}")))?;
                Ok(SmsProviderKind::Aliyun(client))
            }
            "twilio" => {
                let get = |key: &str| {
                    field_of(self, key)
                        .filter(|v| !v.is_empty())
                        .map(str::to_string)
                        .ok_or(SmsError::NotConfigured)
                };
                let client = TwilioSmsClient::new(
                    get("twilio_account_sid")?,
                    get("twilio_auth_token")?,
                    get("twilio_from")?,
                    get("twilio_content_sid")?,
                    self.base_url.clone(),
                )
                .map_err(|e| SmsError::Upstream(format!("twilio client init failed: {e}")))?;
                Ok(SmsProviderKind::Twilio(client))
            }
            _ => Err(SmsError::NotConfigured),
        }
    }
}

/// An unknown/missing provider counts as unconfigured and is listed in
/// `missing_fields` (mirrors `EmailConfigStatus`).
#[derive(Debug, Clone, serde::Serialize)]
pub struct SmsConfigStatus {
    pub configured: bool,
    pub provider: Option<String>,
    pub missing_fields: Vec<String>,
}

pub struct SmsService;

impl SmsService {
    async fn read_sms_config(pool: &PgPool, realm_id: &str) -> anyhow::Result<Option<SmsConfig>> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT config_key, config_value FROM realm_config
             WHERE realm_id = $1 AND config_type = 'sms'",
        )
        .bind(realm_id)
        .fetch_all(pool)
        .await?;

        if rows.is_empty() {
            return Ok(None);
        }

        let mut cfg = SmsConfig::default();
        for (key, value) in rows {
            match key.as_str() {
                "provider" => cfg.provider = value,
                "aliyun_access_key_id" => cfg.aliyun_access_key_id = Some(value),
                "aliyun_access_key_secret" => cfg.aliyun_access_key_secret = Some(value),
                "aliyun_sign_name" => cfg.aliyun_sign_name = Some(value),
                "aliyun_template_code" => cfg.aliyun_template_code = Some(value),
                "twilio_account_sid" => cfg.twilio_account_sid = Some(value),
                "twilio_auth_token" => cfg.twilio_auth_token = Some(value),
                "twilio_from" => cfg.twilio_from = Some(value),
                "twilio_content_sid" => cfg.twilio_content_sid = Some(value),
                "base_url" => cfg.base_url = Some(value),
                _ => {}
            }
        }
        Ok(Some(cfg))
    }

    pub async fn config_status(pool: &PgPool, realm_id: &str) -> anyhow::Result<SmsConfigStatus> {
        let cfg = Self::read_sms_config(pool, realm_id).await?;
        let Some(cfg) = cfg else {
            return Ok(SmsConfigStatus {
                configured: false,
                provider: None,
                missing_fields: vec!["provider".to_string()],
            });
        };

        let provider = cfg.provider.clone();
        let mut missing = Vec::new();
        if provider.is_empty() {
            missing.push("provider".to_string());
        } else {
            let required: &[&str] = match provider.as_str() {
                "aliyun" => &ALIYUN_REQUIRED_KEYS,
                "twilio" => &TWILIO_REQUIRED_KEYS,
                // A typo'd provider can never send; don't report configured.
                _ => &["provider"],
            };
            missing.extend(
                required
                    .iter()
                    .filter(|key| field_of(&cfg, key).unwrap_or("").is_empty())
                    .map(|key| key.to_string()),
            );
            if provider == "twilio" && field_of(&cfg, TWILIO_FROM_KEY).unwrap_or("").is_empty() {
                missing.push(TWILIO_FROM_KEY.to_string());
            }
        }

        let configured = missing.is_empty();
        Ok(SmsConfigStatus {
            configured,
            provider: if provider.is_empty() {
                None
            } else {
                Some(provider)
            },
            missing_fields: missing,
        })
    }

    pub async fn is_sms_configured(pool: &PgPool, realm_id: &str) -> anyhow::Result<bool> {
        Ok(Self::config_status(pool, realm_id).await?.configured)
    }

    /// Never silently no-ops like the email channel: an unconfigured channel
    /// is `Err(NotConfigured)` — callers must reject, not fake success.
    pub async fn send_otp(
        pool: &PgPool,
        realm_id: &str,
        to_e164: &str,
        code: &str,
    ) -> Result<(), SmsError> {
        let cfg = Self::read_sms_config(pool, realm_id)
            .await
            .map_err(|e| SmsError::Upstream(format!("failed to read sms config: {e}")))?;
        let cfg = cfg.ok_or(SmsError::NotConfigured)?;
        let provider = cfg.build_provider()?;
        provider.send(to_e164, code).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encode_encodes_space_as_percent20_not_plus() {
        // form-urlencoded emits `+` for space but Aliyun requires `%20`; a
        // regression invalidates every signature with a spaced value.
        assert_eq!(percent_encode(" "), "%20");
        assert_eq!(percent_encode("a b"), "a%20b");
    }

    #[test]
    fn percent_encode_encodes_star_and_slash_but_keeps_tilde() {
        // `*` must be %2A (form encoders leave it raw), `/` must be %2F, and
        // `~` is an RFC3986 unreserved character that must NOT be encoded
        // (encoding it is the other classic Aliyun signature failure).
        assert_eq!(percent_encode("*"), "%2A");
        assert_eq!(percent_encode("/"), "%2F");
        assert_eq!(percent_encode("~"), "~");
        assert_eq!(percent_encode("a+b"), "a%2Bb");
        // Uppercase hex, non-ASCII (UTF-8 bytes) encoded per byte.
        assert_eq!(percent_encode("签名"), "%E7%AD%BE%E5%90%8D");
    }

    #[test]
    fn canonical_query_sorts_keys_before_encoding() {
        // Aliyun spec: sort by (encoded) parameter name.
        let canonical = canonical_query([("Format", "JSON"), ("Action", "SendSms")].into_iter());
        assert_eq!(canonical, "Action=SendSms&Format=JSON");
    }

    #[test]
    fn string_to_sign_wraps_get_and_encoded_slide() {
        let canonical = canonical_query([("Action", "SendSms"), ("Format", "JSON")].into_iter());
        let sts = aliyun_string_to_sign(&canonical);
        assert_eq!(sts, "GET&%2F&Action%3DSendSms%26Format%3DJSON");
    }

    /// Fixed vector computed independently (python hmac/sha1/base64), key
    /// `testsecret&`; anchors the signature composition end-to-end.
    #[test]
    fn aliyun_signature_matches_independent_fixed_vector() {
        let sts = "GET&%2F&Action%3DSendSms%26Format%3DJSON";
        assert_eq!(
            aliyun_signature("testsecret", sts),
            "780EkZgUyhWEJx0pp/JJZyKHZNo="
        );
    }

    #[test]
    fn aliyun_signed_query_places_signature_first_and_signs_rest() {
        let client = AliyunSmsClient::new(
            "testid".to_string(),
            "testsecret".to_string(),
            "测试签名".to_string(),
            "SMS_123".to_string(),
            None,
        )
        .unwrap();
        let query =
            client.build_signed_query("13800138000", "123456", "nonce-1", "2016-02-23T12:46:24Z");
        assert!(query.starts_with("Signature="));
        let rest = &query["Signature=".len()..];
        let amp = rest
            .find('&')
            .expect("canonical query follows the signature");
        let (sig_value, canonical) = rest.split_at(amp);
        let canonical = &canonical[1..];
        // The signature must cover exactly the remaining sorted pairs.
        let sts = aliyun_string_to_sign(canonical);
        let expected = percent_encode(&aliyun_signature("testsecret", &sts));
        assert_eq!(sig_value, expected);
        let names: Vec<&str> = canonical
            .split('&')
            .map(|p| p.split('=').next().unwrap())
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "query pairs must stay canonically sorted");
        assert!(query.contains("TemplateParam="));
        assert!(canonical.contains("123456"));
    }

    #[test]
    fn aliyun_number_adaptation_strips_country_code_or_rejects() {
        assert_eq!(
            to_aliyun_domestic_number("+8613800138000"),
            Some("13800138000")
        );
        // International numbers are the Twilio channel's job.
        assert_eq!(to_aliyun_domestic_number("+12125551234"), None);
        assert_eq!(to_aliyun_domestic_number("+8612345"), None);
        assert_eq!(to_aliyun_domestic_number("13800138000"), None);
    }

    #[test]
    fn twilio_basic_auth_header_is_sid_colon_token() {
        let client = TwilioSmsClient::new(
            "ACsid".to_string(),
            "tok".to_string(),
            "+15550001111".to_string(),
            "HXabc".to_string(),
            None,
        )
        .unwrap();
        assert_eq!(client.authorization_header(), "Basic QUNzaWQ6dG9r");
    }

    #[test]
    fn twilio_messages_url_honors_base_url_override() {
        let client = TwilioSmsClient::new(
            "ACsid".to_string(),
            "tok".to_string(),
            "+15550001111".to_string(),
            "HXabc".to_string(),
            Some("http://127.0.0.1:9999".to_string()),
        )
        .unwrap();
        assert_eq!(
            client.messages_url(),
            "http://127.0.0.1:9999/2010-04-01/Accounts/ACsid/Messages.json"
        );
    }

    fn full_aliyun_config() -> SmsConfig {
        SmsConfig {
            provider: "aliyun".to_string(),
            aliyun_access_key_id: Some("id".to_string()),
            aliyun_access_key_secret: Some("secret".to_string()),
            aliyun_sign_name: Some("sign".to_string()),
            aliyun_template_code: Some("SMS_1".to_string()),
            ..SmsConfig::default()
        }
    }

    #[test]
    fn complete_aliyun_config_builds_provider() {
        assert!(full_aliyun_config().build_provider().is_ok());
    }

    #[test]
    fn missing_aliyun_secret_cannot_build_provider() {
        let mut cfg = full_aliyun_config();
        cfg.aliyun_access_key_secret = None;
        assert!(matches!(cfg.build_provider(), Err(SmsError::NotConfigured)));
        // Empty string counts as missing (the masked-echo write path).
        cfg.aliyun_access_key_secret = Some(String::new());
        assert!(matches!(cfg.build_provider(), Err(SmsError::NotConfigured)));
    }

    #[test]
    fn unknown_provider_cannot_build() {
        let mut cfg = full_aliyun_config();
        cfg.provider = "tencent".to_string();
        assert!(matches!(cfg.build_provider(), Err(SmsError::NotConfigured)));
    }

    #[tokio::test]
    async fn sms_status_counts_missing_fields_per_provider() {
        let (handle, pool, _sea) = herald_test_db::create_isolated_schema_database(2).await;
        let realm = "sms-status-realm";
        sqlx::query("INSERT INTO realm (id, name) VALUES ($1, 'SMS Realm')")
            .bind(realm)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
             VALUES ($1, 'sms', 'provider', 'aliyun', true)",
        )
        .bind(realm)
        .execute(&pool)
        .await
        .unwrap();

        let status = SmsService::config_status(&pool, realm).await.unwrap();
        assert!(!status.configured);
        assert_eq!(status.provider.as_deref(), Some("aliyun"));
        assert!(
            status
                .missing_fields
                .contains(&"aliyun_access_key_id".to_string())
        );
        assert!(
            status
                .missing_fields
                .contains(&"aliyun_template_code".to_string())
        );

        for (key, value) in [
            ("aliyun_access_key_id", "id"),
            ("aliyun_access_key_secret", "secret"),
            ("aliyun_sign_name", "sign"),
            ("aliyun_template_code", "SMS_1"),
        ] {
            sqlx::query(
                "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
                 VALUES ($1, 'sms', $2, $3, true)",
            )
            .bind(realm)
            .bind(key)
            .bind(value)
            .execute(&pool)
            .await
            .unwrap();
        }
        let status = SmsService::config_status(&pool, realm).await.unwrap();
        assert!(status.configured, "missing: {:?}", status.missing_fields);
        assert!(SmsService::is_sms_configured(&pool, realm).await.unwrap());

        sqlx::query("UPDATE realm_config SET config_value = 'pigeon' WHERE realm_id = $1 AND config_key = 'provider'")
            .bind(realm)
            .execute(&pool)
            .await
            .unwrap();
        let status = SmsService::config_status(&pool, realm).await.unwrap();
        assert!(!status.configured);

        handle.teardown().await;
    }
}
