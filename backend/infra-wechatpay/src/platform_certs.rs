//! Platform certificate download + moka cache.
//!
//! WeChat platform certificates are used to verify callback signatures. They
//! are downloaded at runtime via `GET /v3/certificates` (response is
//! AES-256-GCM encrypted with the APIv3 Key) and cached in memory keyed by
//! realm — no persistence to `realm_config` (DEC-wechat-support-008). A manual
//! `platform_public_key` override, when configured, is preferred over the cache.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use moka::future::Cache;
use once_cell::sync::Lazy;

use crate::error::WechatPayError;
use crate::models::PlatformCert;
use crate::models::{EncryptedCert, RawPlatformCertEntry};
use crate::signing::decrypt_aes_gcm;

/// Upper bound on how long a cached certificate set is trusted without
/// re-checking expiry (defensive; WeChat certs are typically valid ~12 months).
const CACHE_TTL_SECONDS: u64 = 6 * 3600;
/// Default per-realm throttle on unknown-serial `/v3/certificates` refetches.
const DEFAULT_REFETCH_THROTTLE: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// In-memory cache of downloaded platform certificates, keyed by realm id.
/// Cheap to clone (Arc inner); the process-wide default is shared across all
/// per-request clients constructed by `get_wechat_client_for_realm`.
#[derive(Clone)]
pub struct PlatformCertCache {
    inner: Cache<String, Arc<Vec<PlatformCert>>>,
    /// Per-realm "a refetch ran recently" gates. WeChat's integration
    /// contract requires re-fetching `/v3/certificates` on an unknown serial
    /// (the platform rotates certificates on its own schedule), but an
    /// anonymous webhook flood with made-up serials must not turn that into
    /// one signed outbound download per request — the gate bounds it to one
    /// download per throttle window per realm.
    refetch_gates: Cache<String, ()>,
}

impl PlatformCertCache {
    pub fn new() -> Self {
        Self::with_refetch_throttle(DEFAULT_REFETCH_THROTTLE)
    }

    /// Same cache semantics with a shortened unknown-serial refetch throttle,
    /// so rotation convergence is testable without waiting out the
    /// production window.
    pub(crate) fn with_refetch_throttle(refetch_throttle: std::time::Duration) -> Self {
        Self {
            inner: Cache::builder()
                .time_to_live(std::time::Duration::from_secs(CACHE_TTL_SECONDS))
                .max_capacity(10_000)
                .build(),
            refetch_gates: Cache::builder()
                .time_to_live(refetch_throttle)
                .max_capacity(10_000)
                .build(),
        }
    }

    pub async fn get(&self, realm_id: &str) -> Option<Arc<Vec<PlatformCert>>> {
        self.inner.get(realm_id).await
    }

    pub async fn insert(&self, realm_id: &str, certs: Vec<PlatformCert>) {
        self.inner
            .insert(realm_id.to_string(), Arc::new(certs))
            .await;
    }

    /// Claim the realm's unknown-serial refetch slot: `true` when no refetch
    /// ran within the throttle window (this call claims the slot), `false`
    /// while a recent refetch's gate is still live. Best-effort mutual
    /// exclusion — concurrent callers may both win the race, which only
    /// means a bounded handful of downloads, never one per request.
    pub async fn try_begin_refetch(&self, realm_id: &str) -> bool {
        if self.refetch_gates.get(realm_id).await.is_some() {
            return false;
        }
        self.refetch_gates.insert(realm_id.to_string(), ()).await;
        true
    }

    /// Return the cached certificate matching `serial` if it is still inside
    /// its validity window. A certificate close to expiry REMAINS USABLE for
    /// callback verification: during WeChat's rotation overlap the old
    /// serial's callbacks must still verify against the cached key — only
    /// set-refresh decisions (the refetch throttle) care about freshness,
    /// never the usability of an individual still-valid key (review
    /// 20260919 finding 6).
    pub fn find_valid<'a>(
        certs: &'a [PlatformCert],
        serial: &str,
        now: DateTime<Utc>,
    ) -> Option<&'a PlatformCert> {
        certs
            .iter()
            .find(|c| c.serial_no == serial && c.expire_time > now)
    }
}

impl Default for PlatformCertCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Process-wide shared cache.
static SHARED_CACHE: Lazy<PlatformCertCache> = Lazy::new(PlatformCertCache::new);

impl PlatformCertCache {
    /// The process-wide shared cache used by `WechatPayClient::new`.
    pub fn shared() -> PlatformCertCache {
        SHARED_CACHE.clone()
    }
}

/// Parse the `GET /v3/certificates` response body (list of encrypted entries)
/// into decrypted `PlatformCert`s. Each `ExpireTime` is an RFC 3339 timestamp;
/// the certificate PEM is the decrypted ciphertext.
pub fn parse_platform_certs(
    body: &str,
    api_v3_key: &str,
) -> Result<Vec<PlatformCert>, WechatPayError> {
    let entries: Vec<RawPlatformCertEntry> =
        serde_json::from_str(body).map_err(|e| WechatPayError::Parse(e.to_string()))?;

    entries
        .into_iter()
        .map(|entry| decrypt_platform_cert(&entry, api_v3_key))
        .collect()
}

fn decrypt_platform_cert(
    entry: &RawPlatformCertEntry,
    api_v3_key: &str,
) -> Result<PlatformCert, WechatPayError> {
    let EncryptedCert {
        nonce,
        associated_data,
        ciphertext,
        ..
    } = &entry.encrypt_certificate;
    let cert_pem = decrypt_aes_gcm(ciphertext, associated_data, nonce, api_v3_key)?;
    let expire_time = parse_rfc3339(&entry.expire_time)?;
    Ok(PlatformCert {
        serial_no: entry.serial_no.clone(),
        public_key_pem: cert_pem,
        expire_time,
    })
}

fn parse_rfc3339(s: &str) -> Result<DateTime<Utc>, WechatPayError> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| WechatPayError::Parse(format!("invalid timestamp {s:?}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    fn encrypt_for_cert(plain: &str, key: &[u8], nonce: &[u8], aad: &str) -> String {
        let cipher = aes_gcm::Aes256Gcm::new_from_slice(key).unwrap();
        let ct = cipher
            .encrypt(
                aes_gcm::Nonce::from_slice(nonce),
                Payload {
                    msg: plain.as_bytes(),
                    aad: aad.as_bytes(),
                },
            )
            .unwrap();
        STANDARD.encode(ct)
    }

    #[test]
    fn parse_platform_certs_decrypts_entries() {
        let key = b"0123456789abcdef0123456789abcdef";
        let ct = encrypt_for_cert(
            "-----BEGIN CERTIFICATE-----x-----END CERTIFICATE-----",
            key,
            b"nonce1234567",
            "cert",
        );
        let body = format!(
            "[{{\"serial_no\":\"S1\",\"effective_time\":\"2026-01-01T00:00:00+08:00\",\"expire_time\":\"2027-01-01T00:00:00+08:00\",\"encrypt_certificate\":{{\"algorithm\":\"AEAD_AES_256_GCM\",\"nonce\":\"nonce1234567\",\"associated_data\":\"cert\",\"ciphertext\":\"{ct}\"}}}}]"
        );
        let certs = parse_platform_certs(&body, std::str::from_utf8(key).unwrap()).unwrap();
        assert_eq!(certs.len(), 1);
        assert_eq!(certs[0].serial_no, "S1");
        assert!(certs[0].public_key_pem.contains("BEGIN CERTIFICATE"));
    }

    // Intent (review 20260919 finding 6): a platform certificate stays USABLE
    // for callback verification until it actually expires — during WeChat's
    // rotation overlap the old serial's callbacks must verify against the
    // cached key. The old find_fresh refused certificates within 30 minutes
    // of expiry, rejecting callbacks whose correct key was sitting in the
    // cache.
    #[test]
    fn find_valid_uses_certificates_until_expiry() {
        let now = Utc::now();
        let close_to_expiry = PlatformCert {
            serial_no: "overlapping".into(),
            public_key_pem: "k".into(),
            expire_time: now + chrono::Duration::minutes(5),
        };
        let expired = PlatformCert {
            serial_no: "expired".into(),
            public_key_pem: "k".into(),
            expire_time: now - chrono::Duration::minutes(1),
        };
        let certs = vec![close_to_expiry, expired];
        assert!(PlatformCertCache::find_valid(&certs, "overlapping", now).is_some());
        assert!(PlatformCertCache::find_valid(&certs, "expired", now).is_none());
        assert!(PlatformCertCache::find_valid(&certs, "absent", now).is_none());
    }

    // Intent: an unknown serial must trigger at most one /v3/certificates
    // refetch per realm per throttle window — WeChat's rotation contract
    // requires the refetch, the throttle keeps made-up-serial floods from
    // turning it into one signed outbound download per request.
    #[tokio::test]
    async fn refetch_throttle_gates_repeat_refetches() {
        let cache = PlatformCertCache::with_refetch_throttle(std::time::Duration::from_millis(50));
        assert!(cache.try_begin_refetch("realm").await);
        assert!(
            !cache.try_begin_refetch("realm").await,
            "within the window a second refetch is rejected from the gate"
        );
        assert!(
            cache.try_begin_refetch("other-realm").await,
            "the gate is per realm"
        );
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        assert!(
            cache.try_begin_refetch("realm").await,
            "after the window the gate clears so a rotation can converge"
        );
    }
}
