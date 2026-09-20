//! WeChat Pay client construction from per-realm `realm_config` credentials.
//!
//! Mirrors `get_creem_client_for_realm` / `get_stripe_client_for_realm` in
//! `purchase_service`, but lives here as a free function because both the
//! purchase flow (create order) and the webhook handler (verify callback) need
//! a client.

use herald_domain::common::entities::app_errors::CoreError;
use herald_infra_wechatpay::{WechatPayClient, WechatPayConfig};
use sqlx::PgPool;

/// Required (non-optional) WeChat credential keys in `realm_config`.
const REQUIRED_KEYS: &[&str] = &[
    "app_id",
    "mch_id",
    "private_key",
    "serial_no",
    "v3_key",
    "notify_url",
];

/// Build a `WechatPayClient` for `realm_id` from its `realm_config`
/// (`config_type='wechat'`) rows. Returns an internal-server error when a
/// required key is missing or the private key / APIv3 key is malformed.
pub async fn get_wechat_client_for_realm(
    pool: &PgPool,
    realm_id: &str,
) -> Result<WechatPayClient, CoreError> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT config_key, config_value
         FROM realm_config
         WHERE realm_id = $1 AND config_type = 'wechat' AND enabled = true",
    )
    .bind(realm_id)
    .fetch_all(pool)
    .await
    .map_err(|e| CoreError::InternalServerError(format!("Failed to load WeChat config: {e}")))?;

    let mut map = std::collections::HashMap::<String, String>::new();
    for (key, value) in rows {
        map.insert(key, value);
    }

    for required in REQUIRED_KEYS {
        if !map.contains_key(*required) {
            return Err(CoreError::InternalServerError(format!(
                "WeChat not fully configured for realm: {realm_id} (missing '{required}')"
            )));
        }
    }

    // Label-independent client-side guard (review 20260920): every outbound
    // WeChat request is merchant-key-signed and posted to the configured
    // base — a plaintext non-loopback base must not receive the realm's
    // credentials. Mirrors the Stripe/Creem client-side base_url guard.
    if let Some(base) = map
        .get("base_url")
        .map(String::as_str)
        .filter(|b| !b.is_empty())
        && let Err(error) =
            herald_domain::common::entities::provider_url::validate_provider_base_url(base)
    {
        return Err(CoreError::BadRequest(format!(
            "Invalid WeChat configuration: base_url {error}"
        )));
    }

    let config = WechatPayConfig {
        app_id: map.remove("app_id").unwrap(),
        mch_id: map.remove("mch_id").unwrap(),
        private_key_pem: map.remove("private_key").unwrap(),
        serial_no: map.remove("serial_no").unwrap(),
        api_v3_key: map.remove("v3_key").unwrap(),
        notify_url: map.remove("notify_url").unwrap(),
        platform_public_key_override: map.remove("platform_public_key"),
        base_url: map.remove("base_url"),
    };

    WechatPayClient::new(config).map_err(|e| {
        // Config missing/invalid is a deterministic realm misconfiguration, not
        // a retryable fault — map it the same way `wechat_err_to_core` does in
        // the webhook handler so callers (webhook retry / compensation) do not
        // treat it as transient.
        if e.is_security_rejection() {
            CoreError::BadRequest(format!("Invalid WeChat configuration: {e}"))
        } else {
            CoreError::InternalServerError(format!("Invalid WeChat configuration: {e}"))
        }
    })
}
