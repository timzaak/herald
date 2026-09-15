//! Platform-level OIDC signing key store.
//!
//! Owns the `oidc_signing_key` table lifecycle: startup bootstrapping of the
//! first key, per-request retrieval of the single active key, JWKS publication
//! of active + unexpired retained keys, and single-transaction rotation.
//!
//! Private keys are stored as AES-256-GCM ciphertext (`nonce(12B) || ct`),
//! with the KEK derived as SHA256(domain-separation prefix + `[jwt] secret`).
//! Deriving the KEK from the existing `[jwt] secret` adds no configuration
//! surface; the cost is that rotating `[jwt] secret` invalidates stored
//! ciphertexts — after such a change one signing-key rotation must be run to
//! restore JWKS/id_token issuance (documented in the docs-web OIDC guide).

use aes_gcm::{
    Aes256Gcm,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use herald_domain::security_constants::OIDC_SIGNING_KEY_RETENTION_SECONDS;
use rsa::{
    RsaPrivateKey, RsaPublicKey,
    pkcs8::{DecodePrivateKey, EncodePrivateKey, LineEnding},
    traits::PublicKeyParts,
};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

/// Domain-separation prefix for the OIDC signing-key KEK. Keeps the derived
/// key independent from any other direct use of the raw `[jwt] secret`.
const OIDC_SIGNING_KEY_KEK_PREFIX: &str = "herald:v1:oidc-signing-key:";

pub struct OidcSigningKeyStore {
    pool: PgPool,
    kek: [u8; 32],
}

/// The decrypted active signing key. Plaintext exists only in memory for the
/// duration of one signing/JWKS operation.
pub struct ActiveSigningKey {
    pub kid: String,
    pub private_key_pem: String,
}

pub struct JwkPublicKey {
    pub kid: String,
    pub n_b64: String,
    pub e_b64: String,
}

pub struct RotationOutcome {
    pub new_kid: String,
    pub retained_until: chrono::DateTime<chrono::Utc>,
    pub retired_kids: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum OidcSigningKeyError {
    #[error("jwt secret is not configured")]
    MissingSecret,
    #[error("no active OIDC signing key")]
    NoActiveKey,
    #[error("concurrent rotation")]
    ConcurrentRotation,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("key crypto error: {0}")]
    Crypto(String),
}

fn derive_kek(jwt_secret: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(OIDC_SIGNING_KEY_KEK_PREFIX.as_bytes());
    hasher.update(jwt_secret.as_bytes());
    hasher.finalize().into()
}

fn encrypt_pem(kek: &[u8; 32], pem: &str) -> Result<Vec<u8>, OidcSigningKeyError> {
    let cipher = Aes256Gcm::new_from_slice(kek)
        .map_err(|e| OidcSigningKeyError::Crypto(format!("KEK invalid: {e}")))?;
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, pem.as_bytes())
        .map_err(|e| OidcSigningKeyError::Crypto(format!("encryption failed: {e}")))?;
    // Stored layout: nonce(12B) || ciphertext.
    let mut stored = nonce.to_vec();
    stored.extend_from_slice(&ciphertext);
    Ok(stored)
}

fn decrypt_pem(kek: &[u8; 32], stored: &[u8]) -> Result<String, OidcSigningKeyError> {
    if stored.len() < 12 {
        return Err(OidcSigningKeyError::Crypto(
            "stored key material too short".to_string(),
        ));
    }
    let (nonce, ciphertext) = stored.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(kek)
        .map_err(|e| OidcSigningKeyError::Crypto(format!("KEK invalid: {e}")))?;
    let plaintext = cipher.decrypt(nonce.into(), ciphertext).map_err(|_| {
        // Most common cause: [jwt] secret changed after this row was
        // written. Recovery is one signing-key rotation.
        OidcSigningKeyError::Crypto(
            "signing-key decryption failed (was [jwt] secret rotated?)".to_string(),
        )
    })?;
    String::from_utf8(plaintext)
        .map_err(|e| OidcSigningKeyError::Crypto(format!("stored PEM not UTF-8: {e}")))
}

fn generate_keypair() -> Result<(uuid::Uuid, String), OidcSigningKeyError> {
    let mut rng = rand::rngs::OsRng;
    let private_key = RsaPrivateKey::new(&mut rng, 2048)
        .map_err(|e| OidcSigningKeyError::Crypto(format!("RSA keygen failed: {e}")))?;
    let pem = private_key
        .to_pkcs8_pem(LineEnding::LF)
        .map_err(|e| OidcSigningKeyError::Crypto(format!("PKCS#8 encoding failed: {e}")))?
        .to_string();
    Ok((uuid::Uuid::now_v7(), pem))
}

fn jwk_from_pem(kid: &str, pem: &str) -> Result<JwkPublicKey, OidcSigningKeyError> {
    let private_key = RsaPrivateKey::from_pkcs8_pem(pem)
        .map_err(|e| OidcSigningKeyError::Crypto(format!("stored key unparsable: {e}")))?;
    let public_key = RsaPublicKey::from(&private_key);
    Ok(JwkPublicKey {
        kid: kid.to_string(),
        n_b64: URL_SAFE_NO_PAD.encode(public_key.n().to_bytes_be()),
        e_b64: URL_SAFE_NO_PAD.encode(public_key.e().to_bytes_be()),
    })
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(
        error,
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23505")
    )
}

/// Insert a fresh active-key row. The partial unique index turns a lost
/// bootstrap/rotation race into a unique-violation error the caller maps.
async fn insert_active_key<'e, E>(
    executor: E,
    kid: uuid::Uuid,
    encrypted: &[u8],
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(
        "INSERT INTO oidc_signing_key (id, status, private_key_pem_encrypted)
         VALUES ($1, 'active', $2)",
    )
    .bind(kid)
    .bind(encrypted)
    .execute(executor)
    .await
    .map(|_| ())
}

impl OidcSigningKeyStore {
    pub fn new(pool: PgPool, jwt_secret: &str) -> Result<Self, OidcSigningKeyError> {
        if jwt_secret.is_empty() {
            return Err(OidcSigningKeyError::MissingSecret);
        }
        Ok(Self {
            pool,
            kek: derive_kek(jwt_secret),
        })
    }

    /// Ensure an active key exists; generate one when absent. Safe to call
    /// concurrently from multiple instances: the partial unique index makes the
    /// second INSERT fail with 23505, which means another instance won the
    /// race and is treated as success.
    pub async fn ensure_active_key(&self) -> Result<(), OidcSigningKeyError> {
        let existing: Option<(uuid::Uuid,)> =
            sqlx::query_as("SELECT id FROM oidc_signing_key WHERE status = 'active'")
                .fetch_optional(&self.pool)
                .await?;
        if existing.is_some() {
            return Ok(());
        }
        let (kid, pem) = generate_keypair()?;
        let encrypted = encrypt_pem(&self.kek, &pem)?;
        match insert_active_key(&self.pool, kid, &encrypted).await {
            Ok(()) => {
                tracing::info!(kid = %kid, "Bootstrapped first OIDC signing key");
                Ok(())
            }
            Err(error) if is_unique_violation(&error) => {
                tracing::debug!("Another instance bootstrapped the OIDC signing key first");
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Fetch and decrypt the single active key. Called per token issuance /
    /// JWKS request; the table holds single-digit row counts, so caching is
    /// deliberately avoided (rotation visibility beats the lookup cost).
    pub async fn active_signing_key(&self) -> Result<ActiveSigningKey, OidcSigningKeyError> {
        let row: Option<(uuid::Uuid, Vec<u8>)> = sqlx::query_as(
            "SELECT id, private_key_pem_encrypted FROM oidc_signing_key WHERE status = 'active'",
        )
        .fetch_optional(&self.pool)
        .await?;
        let Some((kid, encrypted)) = row else {
            return Err(OidcSigningKeyError::NoActiveKey);
        };
        Ok(ActiveSigningKey {
            kid: kid.to_string(),
            private_key_pem: decrypt_pem(&self.kek, &encrypted)?,
        })
    }

    /// The JWKS publication set: active key plus retained keys whose overlap
    /// window is still open. n/e are derived from the decrypted private key so
    /// no public-key column can drift out of sync. A retained row that no
    /// longer decrypts (typically after a `[jwt] secret` change followed by a
    /// rotation) is skipped with an error log — its public half is
    /// unrecoverable either way, and failing the whole response would also
    /// break verification of tokens signed by the new key. An undecryptable
    /// ACTIVE key stays fail-closed.
    pub async fn jwks_public_keys(&self) -> Result<Vec<JwkPublicKey>, OidcSigningKeyError> {
        let rows: Vec<(uuid::Uuid, String, Vec<u8>)> = sqlx::query_as(
            "SELECT id, status, private_key_pem_encrypted FROM oidc_signing_key
             WHERE status = 'active' OR (status = 'retained' AND retained_until > NOW())
             ORDER BY created_at",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut keys = Vec::with_capacity(rows.len());
        for (kid, status, encrypted) in rows {
            let pem = match decrypt_pem(&self.kek, &encrypted) {
                Ok(pem) => pem,
                Err(error) if status == "retained" => {
                    tracing::error!(
                        kid = %kid,
                        error = %error,
                        "Skipping undecryptable retained OIDC signing key in JWKS"
                    );
                    continue;
                }
                Err(error) => return Err(error),
            };
            keys.push(jwk_from_pem(&kid.to_string(), &pem)?);
        }
        Ok(keys)
    }

    /// Rotate in a single transaction: retire expired retained keys
    /// (housekeeping), demote the active key to retained with a fresh overlap
    /// window, then insert the new active key. The partial unique index turns
    /// a concurrent rotation into `ConcurrentRotation` for the loser.
    pub async fn rotate(&self) -> Result<RotationOutcome, OidcSigningKeyError> {
        let retained_until =
            chrono::Utc::now() + chrono::Duration::seconds(OIDC_SIGNING_KEY_RETENTION_SECONDS);
        let retained_until_rfc3339 = retained_until.to_rfc3339();

        // RSA keygen is comparatively slow and independent of any DB state —
        // run it before opening the transaction so neither the pooled
        // connection nor the row locks below are held for the keygen duration.
        // A lost race discards the pre-generated key, exactly as before.
        let (new_kid, pem) = generate_keypair()?;
        let encrypted = encrypt_pem(&self.kek, &pem)?;

        let mut tx = self.pool.begin().await?;

        let retired: Vec<(uuid::Uuid,)> = sqlx::query_as(
            "UPDATE oidc_signing_key SET status = 'retired', updated_at = NOW()
             WHERE status = 'retained' AND retained_until <= NOW()
             RETURNING id",
        )
        .fetch_all(&mut *tx)
        .await?;

        // Rows-affected 0 is legal: the very first rotation of a bootstrapped
        // key demotes nothing only when no active row exists, which the
        // bootstrap guarantee makes unreachable in practice.
        sqlx::query(
            "UPDATE oidc_signing_key SET status = 'retained',
                    retained_until = $1::timestamptz, updated_at = NOW()
             WHERE status = 'active'",
        )
        .bind(&retained_until_rfc3339)
        .execute(&mut *tx)
        .await?;

        match insert_active_key(&mut *tx, new_kid, &encrypted).await {
            Ok(()) => {}
            Err(error) if is_unique_violation(&error) => {
                return Err(OidcSigningKeyError::ConcurrentRotation);
            }
            Err(error) => return Err(error.into()),
        }

        tx.commit().await?;

        tracing::info!(
            new_kid = %new_kid,
            retained_until = %retained_until_rfc3339,
            retired_kids = ?retired.iter().map(|(id,)| id.to_string()).collect::<Vec<_>>(),
            "Rotated OIDC signing key"
        );

        Ok(RotationOutcome {
            new_kid: new_kid.to_string(),
            retained_until,
            retired_kids: retired.into_iter().map(|(id,)| id.to_string()).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // WHY: the KEK must be a pure function of the configured secret — two
    // store instances built from the same secret must decrypt each other's
    // rows, or a process restart would lock every stored key.
    #[test]
    fn kek_derivation_is_deterministic_and_secret_sensitive() {
        assert_eq!(derive_kek("secret-a"), derive_kek("secret-a"));
        assert_ne!(derive_kek("secret-a"), derive_kek("secret-b"));
    }

    #[test]
    fn encrypt_decrypt_roundtrip_recovers_pem() {
        let kek = derive_kek("test-secret");
        let stored = encrypt_pem(
            &kek,
            "-----BEGIN PRIVATE KEY-----\nabc\n-----END PRIVATE KEY-----\n",
        )
        .expect("encryption must not fail for valid input");
        let recovered = decrypt_pem(&kek, &stored).expect("roundtrip must recover the PEM");
        assert!(recovered.starts_with("-----BEGIN PRIVATE KEY-----"));
    }

    // WHY: the whole point of sealing stored keys is that material encrypted
    // under one secret is unreadable under another ([jwt] secret rotation
    // invalidates ciphertexts; callers surface that as a decryption error).
    #[test]
    fn decrypt_under_wrong_secret_fails() {
        let stored = encrypt_pem(&derive_kek("secret-a"), "private-key-pem").unwrap();
        assert!(decrypt_pem(&derive_kek("secret-b"), &stored).is_err());
    }

    #[test]
    fn truncated_ciphertext_is_rejected() {
        let kek = derive_kek("test-secret");
        assert!(decrypt_pem(&kek, &[0u8; 8]).is_err());
    }

    #[tokio::test]
    async fn empty_secret_is_rejected() {
        // connect_lazy still needs a Tokio context for sqlx's internal
        // reaper task; validity of the URL is irrelevant to this test.
        let pool = PgPool::connect_lazy("postgres://invalid").unwrap();
        assert!(matches!(
            OidcSigningKeyStore::new(pool, ""),
            Err(OidcSigningKeyError::MissingSecret)
        ));
    }

    // WHY: JWKS n/e must be the base64url of the same modulus/exponent the
    // stored private key signs with — a mismatched pair would make every
    // client-side verification fail while local signing succeeds.
    #[test]
    fn jwk_components_match_generated_private_key() {
        let mut rng = rand::rngs::OsRng;
        let private_key = RsaPrivateKey::new(&mut rng, 2048).expect("keygen");
        let pem = private_key
            .to_pkcs8_pem(LineEnding::LF)
            .unwrap()
            .to_string();
        let jwk = jwk_from_pem("kid-test", &pem).expect("valid PEM must derive a JWK");
        let public_key = RsaPublicKey::from(&private_key);
        assert_eq!(jwk.kid, "kid-test");
        assert_eq!(
            jwk.n_b64,
            URL_SAFE_NO_PAD.encode(public_key.n().to_bytes_be())
        );
        assert_eq!(
            jwk.e_b64,
            URL_SAFE_NO_PAD.encode(public_key.e().to_bytes_be())
        );
        // RSA-2048 public exponent is 65537 -> base64url "AQAB".
        assert_eq!(jwk.e_b64, "AQAB");
    }
}
