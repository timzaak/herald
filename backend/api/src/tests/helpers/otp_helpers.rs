// =============================================================================
// Email OTP Test Helpers
// =============================================================================
//
// Test-only utilities for the email-OTP login scenarios
// (`email_otp_send_verify_scenarios.rs`). The production OTP code is stored in
// Redis as plaintext (design email-otp-login §4.5 / §5.4 — amended: plaintext
// rather than hashed, consistent with the password-reset code persisted in the
// `email_verification_code` table and with the session tokens in the same
// Redis; it also lets the Demo/E2E flow read the code back). These helpers
// inject a *known* code into the exact Redis keys the production `verify`
// handler reads, using the same key derivations
// (`emailotp:{realm_id}:{sha256(normalize_email(email))}` for the code and
// `emailotp:attempts:{realm_id}:{digest}` for the INCR attempt counter) and
// the same `StoredOtp` JSON shape (`code` / `max_attempts` /
// `expires_at_ms`).
//
// These helpers MUST stay mechanically in sync with
// `backend/api-auth/src/email_otp.rs` (`otp_redis_key`,
// `otp_attempts_redis_key`, `StoredOtp`, and the `OTP_*` constants). They do
// NOT modify production code.
//
// =============================================================================

#![allow(dead_code)]

use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use redis::AsyncCommands;
use sha2::{Digest, Sha256};

/// Reproduce `normalize_email` (trim + ASCII lowercase) so the injected key
/// matches the production key derivation exactly.
fn normalize_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

/// Reproduce `email_otp::otp_redis_key`:
/// `emailotp:{realm_id}:{sha256(normalize_email(email))}` (hex digest).
pub fn otp_redis_key(realm_id: &str, email: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(normalize_email(email).as_bytes());
    let digest = hex::encode(hasher.finalize());
    format!("emailotp:{realm_id}:{digest}")
}

/// Reproduce `email_otp::otp_attempts_redis_key`:
/// `emailotp:attempts:{realm_id}:{digest}` (plain INCR counter).
pub fn otp_attempts_redis_key(realm_id: &str, email: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(normalize_email(email).as_bytes());
    let digest = hex::encode(hasher.finalize());
    format!("emailotp:attempts:{realm_id}:{digest}")
}

/// Reproduce `StoredOtp` (JSON in Redis). Field names must match the
/// production `#[derive(Serialize, Deserialize)] struct StoredOtp`. The `code`
/// is plaintext (mirrors the production change). `expires_at_ms` is the
/// absolute expiry the verify path needs to restore a claimed-but-mismatched
/// code with its original remaining TTL.
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredOtp {
    code: String,
    max_attempts: i64,
    expires_at_ms: Option<u64>,
}

fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Inject a *known* OTP code into Redis so a subsequent `verify` request can be
/// asserted deterministically.
///
/// Writes the same Redis key + JSON shape the production `send` handler writes,
/// so the production `verify` handler will read it back and compare the
/// plaintext code with constant-time equality. `attempts` seeds the SEPARATE
/// INCR counter key (production keeps the count outside the code JSON) — 0
/// leaves no counter, matching a fresh send. `ttl_secs` should be
/// `OTP_CODE_TTL_SECONDS` in normal cases; tests that need an "expired" key
/// pass a tiny TTL and sleep.
pub async fn inject_otp_code(
    ctx: &TestContext,
    realm_id: &str,
    email: &str,
    code: &str,
    attempts: i64,
    max_attempts: i64,
    ttl_secs: u64,
) {
    let stored = StoredOtp {
        code: code.to_string(),
        max_attempts,
        expires_at_ms: Some(now_epoch_ms() + ttl_secs * 1000),
    };
    let stored_json = serde_json::to_string(&stored).expect("failed to serialize StoredOtp");
    let key = otp_redis_key(realm_id, email);
    let attempts_key = otp_attempts_redis_key(realm_id, email);

    let mut conn = ctx
        ._app_state
        .redis_manager
        .get()
        .await
        .expect("failed to get Redis connection");
    let _: () = conn
        .set_ex(&key, stored_json, ttl_secs)
        .await
        .expect("failed to inject OTP code into Redis");
    if attempts > 0 {
        let _: () = conn
            .set_ex(&attempts_key, attempts, ttl_secs)
            .await
            .expect("failed to seed OTP attempt counter");
    } else {
        // Fresh code → fresh counter (production send deletes leftovers).
        let _: () = conn
            .del(&attempts_key)
            .await
            .expect("failed to reset OTP attempt counter");
    }
}

/// Read back the attempt counter for an email (or `None` if the key is absent
/// — no failed verify yet, or the code was consumed/invalidated, which deletes
/// the counter).
pub async fn read_otp_attempts(ctx: &TestContext, realm_id: &str, email: &str) -> Option<i64> {
    let key = otp_attempts_redis_key(realm_id, email);
    let mut conn = ctx
        ._app_state
        .redis_manager
        .get()
        .await
        .expect("failed to get Redis connection");
    conn.get(&key).await.expect("failed to read OTP counter")
}

/// Delete the stored OTP key for an email (test cleanup / explicit invalidation).
pub async fn delete_otp_code(ctx: &TestContext, realm_id: &str, email: &str) {
    let key = otp_redis_key(realm_id, email);
    let mut conn = ctx
        ._app_state
        .redis_manager
        .get()
        .await
        .expect("failed to get Redis connection");
    let _: () = conn.del(&key).await.expect("failed to delete OTP key");
}
