// Email OTP Test Helpers
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

/// Read back the stored OTP code JSON for an email (or `None` if no code is
/// stored). This is the key the production `send` handler writes and `verify`
/// consumes — unlike the attempt counter, a successful send leaves it
/// present, so absence-proofs of "no code was issued" must assert on this
/// key, not on the counter.
pub async fn read_otp_code(ctx: &TestContext, realm_id: &str, email: &str) -> Option<String> {
    let key = otp_redis_key(realm_id, email);
    let mut conn = ctx
        ._app_state
        .redis_manager
        .get()
        .await
        .expect("failed to get Redis connection");
    conn.get(&key).await.expect("failed to read OTP code key")
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

// SMS OTP test helpers (phone login). Key derivations MUST stay mechanically
// in sync with production (`sms_otp.rs` / `user_phone.rs` / admin `phone.rs`).
// The SMS channel fails loud when unconfigured (no email-style silent no-op),
// so send scenarios must seed channel rows pointing at a wiremock server.

/// Deterministic digits-only CN mobile, bare 11-digit national form (wrap
/// with `+86` for E.164). The pid in the time bucket matters: nextest runs
/// each test in its own process, so the per-process SEQ restarts at 0 —
/// without it two parallel tests can mint the same number and their
/// shared-Redis OTP keys cross-talk.
pub fn unique_cn_phone(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos() as u64;
    let pid = std::process::id() as u64;
    let bucket = ((nanos / 10_000) ^ pid.wrapping_mul(7_919)) % 10_000;
    format!("{prefix}{:04}{:04}", bucket, n % 10_000)
}

/// sha256 hex digest of the E.164 phone (production `phone_digest`).
fn sms_phone_digest(e164: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(e164.as_bytes());
    hex::encode(hasher.finalize())
}

/// Reproduce `sms_otp::login_otp_key`.
pub fn sms_otp_redis_key(realm_id: &str, e164: &str) -> String {
    format!("smsotp:{}:{}", realm_id, sms_phone_digest(e164))
}

/// Reproduce `sms_otp::login_attempts_key`.
pub fn sms_otp_attempts_key(realm_id: &str, e164: &str) -> String {
    format!("smsotp:attempts:{}:{}", realm_id, sms_phone_digest(e164))
}

/// Reproduce `user_phone::bind_otp_key`.
pub fn sms_bind_otp_key(realm_id: &str, user_id: uuid::Uuid, e164: &str) -> String {
    format!(
        "smsotp:bind:{}:{}:{}",
        realm_id,
        user_id,
        sms_phone_digest(e164)
    )
}

/// Reproduce `user_phone::bind_attempts_key`.
pub fn sms_bind_attempts_key(realm_id: &str, user_id: uuid::Uuid, e164: &str) -> String {
    format!(
        "smsotp:bind:attempts:{}:{}:{}",
        realm_id,
        user_id,
        sms_phone_digest(e164)
    )
}

/// Reproduce `phone::pending_key` (admin rebind pending record).
pub fn sms_admin_rebind_key(realm_id: &str, user_id: uuid::Uuid) -> String {
    format!("smsotp:adminrebind:{realm_id}:{user_id}")
}

/// Inject a known SMS login-OTP code (see `inject_otp_code`; attempts 0 = fresh counter).
pub async fn inject_sms_otp_code(
    ctx: &TestContext,
    realm_id: &str,
    e164: &str,
    code: &str,
    attempts: i64,
    max_attempts: i64,
    ttl_secs: u64,
) {
    inject_sms_code_at_key(
        ctx,
        &sms_otp_redis_key(realm_id, e164),
        &sms_otp_attempts_key(realm_id, e164),
        code,
        attempts,
        max_attempts,
        ttl_secs,
    )
    .await;
}

pub async fn inject_sms_bind_code(
    ctx: &TestContext,
    realm_id: &str,
    user_id: uuid::Uuid,
    e164: &str,
    code: &str,
    attempts: i64,
    max_attempts: i64,
    ttl_secs: u64,
) {
    inject_sms_code_at_key(
        ctx,
        &sms_bind_otp_key(realm_id, user_id, e164),
        &sms_bind_attempts_key(realm_id, user_id, e164),
        code,
        attempts,
        max_attempts,
        ttl_secs,
    )
    .await;
}

/// Shared writer matching production `store_sms_otp_code` semantics
/// (`expires_at_ms` is NOT optional in the SMS family).
async fn inject_sms_code_at_key(
    ctx: &TestContext,
    key: &str,
    attempts_key: &str,
    code: &str,
    attempts: i64,
    max_attempts: i64,
    ttl_secs: u64,
) {
    #[derive(serde::Serialize)]
    struct SmsStoredOtp {
        code: String,
        max_attempts: i64,
        expires_at_ms: u64,
    }
    let stored = SmsStoredOtp {
        code: code.to_string(),
        max_attempts,
        expires_at_ms: now_epoch_ms() + ttl_secs * 1000,
    };
    let stored_json = serde_json::to_string(&stored).expect("failed to serialize SmsStoredOtp");
    let mut conn = ctx
        ._app_state
        .redis_manager
        .get()
        .await
        .expect("failed to get Redis connection");
    let _: () = conn
        .set_ex(key, stored_json, ttl_secs)
        .await
        .expect("failed to inject SMS OTP code");
    if attempts > 0 {
        let _: () = conn
            .set_ex(attempts_key, attempts, ttl_secs)
            .await
            .expect("failed to seed SMS attempt counter");
    } else {
        let _: () = conn
            .del(attempts_key)
            .await
            .expect("failed to reset SMS attempt counter");
    }
}

/// `None` = no code stored — the key for "no code was issued" absence proofs.
pub async fn read_sms_otp_code(ctx: &TestContext, realm_id: &str, e164: &str) -> Option<String> {
    let mut conn = ctx
        ._app_state
        .redis_manager
        .get()
        .await
        .expect("failed to get Redis connection");
    conn.get(sms_otp_redis_key(realm_id, e164))
        .await
        .expect("failed to read SMS code key")
}

pub async fn read_sms_otp_attempts(ctx: &TestContext, realm_id: &str, e164: &str) -> Option<i64> {
    let mut conn = ctx
        ._app_state
        .redis_manager
        .get()
        .await
        .expect("failed to get Redis connection");
    conn.get(sms_otp_attempts_key(realm_id, e164))
        .await
        .expect("failed to read SMS attempt counter")
}

pub async fn read_sms_admin_rebind(
    ctx: &TestContext,
    realm_id: &str,
    user_id: uuid::Uuid,
) -> Option<String> {
    let mut conn = ctx
        ._app_state
        .redis_manager
        .get()
        .await
        .expect("failed to get Redis connection");
    conn.get(sms_admin_rebind_key(realm_id, user_id))
        .await
        .expect("failed to read admin rebind pending record")
}

pub async fn seed_sms_channel_aliyun(pool: &sqlx::PgPool, realm_id: &str, base_url: &str) {
    let rows: [(&str, &str); 6] = [
        ("provider", "aliyun"),
        ("aliyun_access_key_id", "test-key-id"),
        ("aliyun_access_key_secret", "test-key-secret"),
        ("aliyun_sign_name", "TestSign"),
        ("aliyun_template_code", "SMS_000000001"),
        ("base_url", base_url),
    ];
    for (key, value) in rows {
        sqlx::query(
            "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, is_secret, enabled, metadata, created_at, updated_at)
             VALUES ($1, 'sms', $2, $3, false, true, '{}'::jsonb, NOW(), NOW())
             ON CONFLICT (realm_id, config_type, config_key)
             DO UPDATE SET config_value = EXCLUDED.config_value, enabled = true, updated_at = NOW()",
        )
        .bind(realm_id)
        .bind(key)
        .bind(value)
        .execute(pool)
        .await
        .expect("failed to seed sms channel row");
    }
}

pub async fn seed_phone_login_config(pool: &sqlx::PgPool, realm_id: &str, enabled: bool) {
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, is_secret, enabled, metadata, created_at, updated_at)
         VALUES ($1, 'phone_login', 'settings', $2, false, true, '{}'::jsonb, NOW(), NOW())
         ON CONFLICT (realm_id, config_type, config_key)
         DO UPDATE SET config_value = EXCLUDED.config_value, enabled = true, updated_at = NOW()",
    )
    .bind(realm_id)
    .bind(format!(r#"{{"enabled":{enabled},"default_country_code":"CN"}}"#))
    .execute(pool)
    .await
    .expect("failed to seed phone_login config");
}

/// Mirrored from `security_constants`; drift breaks the scenario assertions
/// (the pin test in `herald-domain` is the anchor).
pub const SMS_OTP_MAX_ATTEMPTS: i64 = 5;
pub const SMS_OTP_CODE_TTL_SECONDS: u64 = 300;

pub async fn start_aliyun_wiremock() -> wiremock::MockServer {
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"Code": "OK"})))
        .mount(&server)
        .await;
    server
}

pub async fn enable_phone_login(ctx: &TestContext) -> wiremock::MockServer {
    let server = start_aliyun_wiremock().await;
    seed_sms_channel_aliyun(&ctx._app_state.pool, &ctx._realm_id, &server.uri()).await;
    seed_phone_login_config(&ctx._app_state.pool, &ctx._realm_id, true).await;
    server
}
