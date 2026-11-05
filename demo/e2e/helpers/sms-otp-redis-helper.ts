/**
 * SMS-OTP Redis Helper for Demo Tests
 *
 * The SMS channel FAILS LOUD when unconfigured (unlike email's silent no-op),
 * and the demo suite cannot reach a real SMS upstream. The demos therefore
 * intercept the browser-side SEND requests via `page.route` (synthetic 200)
 * and INJECT the 6-digit code into Redis here, exactly where the production
 * `send` handler would have written it — the production `verify`/`confirm`
 * handlers then consume it for real (real claim protocol, real DB writes).
 *
 * The key derivations and the `StoredOtp` JSON shape are reproduced EXACTLY
 * from the backend — verified against `backend/api-auth/src/sms_otp.rs`
 * (`phone_digest` / `login_otp_key` / `login_attempts_key`),
 * `backend/api-auth/src/user_phone.rs` (`bind_otp_key` /
 * `bind_attempts_key`), `backend/api-base/src/application/http/auth/util.rs`
 * (`StoredOtp`, `store_sms_otp_code`) and the test reference
 * `backend/api/src/tests/helpers/otp_helpers.rs` (`inject_sms_otp_code` /
 * `inject_sms_bind_code`). Any drift here breaks the demo's code injection.
 *
 * E.164 derivation lives in the shared `phone-format.ts` (`toE164`) — see that
 * file for the CN-only simplification and the `normalize_phone` drift warning.
 * The DB seeding helper (`phone-db-helper.ts`) uses the same derivation so the
 * seeded binding value and the keys derived here always agree.
 *
 * @see ../../../backend/api-auth/src/sms_otp.rs (`login_otp_key`, `phone_digest`)
 * @see ../../../backend/api-auth/src/user_phone.rs (`bind_otp_key`)
 * @see ../../../backend/api/src/tests/helpers/otp_helpers.rs (reference derivation)
 * @see ../../../backend/config/demo.toml `[redis] url`
 */

import { createHash } from 'node:crypto'
import Redis from 'ioredis'
import { toE164 } from './phone-format'

const DEFAULT_REDIS_URL = process.env.REDIS_URL || 'redis://127.0.0.1:6379/0'

// Backend constants (backend/domain/src/security_constants.rs): the claim
// protocol reads max_attempts from the stored JSON and the TTL must keep the
// key alive until the UI fills the code.
const SMS_OTP_MAX_ATTEMPTS = 5
const SMS_OTP_CODE_TTL_SECONDS = 300

// A single shared client, mirroring `email-otp-redis-helper.ts`. The demo
// suite reuses this client across tests; `closeSmsOtpRedis()` tears it down.
const redisClient = new Redis(DEFAULT_REDIS_URL)

/** StoredOtp JSON written by production `store_sms_otp_code` (snake_case). */
interface StoredSmsOtp {
  code: string
  max_attempts: number
  expires_at_ms: number
}

/** Reproduce `sms_otp::phone_digest`: sha256 hex of the E.164 phone. */
function phoneDigest(e164: string): string {
  return createHash('sha256').update(e164).digest('hex')
}

/** Reproduce `sms_otp::login_otp_key`: `smsotp:{realm_id}:{sha256(e164)}`. */
function loginOtpKey(realmId: string, e164: string): string {
  return `smsotp:${realmId}:${phoneDigest(e164)}`
}

/** Reproduce `sms_otp::login_attempts_key`. */
function loginAttemptsKey(realmId: string, e164: string): string {
  return `smsotp:attempts:${realmId}:${phoneDigest(e164)}`
}

/** Reproduce `user_phone::bind_otp_key`: `smsotp:bind:{realm}:{user}:{digest}`. */
function bindOtpKey(realmId: string, userId: string, e164: string): string {
  return `smsotp:bind:${realmId}:${userId}:${phoneDigest(e164)}`
}

/** Reproduce `user_phone::bind_attempts_key`. */
function bindAttemptsKey(realmId: string, userId: string, e164: string): string {
  return `smsotp:bind:attempts:${realmId}:${userId}:${phoneDigest(e164)}`
}

/**
 * Inject a known code at the given (code, attempts) key pair, matching
 * production `store_sms_otp_code` semantics: `expires_at_ms` is absolute
 * epoch ms, the attempts counter lives OUTSIDE the JSON, and a fresh code
 * carries no leftover counter (production send deletes leftovers).
 */
async function injectCodeAtKeys(
  key: string,
  attemptsKey: string,
  code: string,
  ttlSecs: number
): Promise<void> {
  const stored: StoredSmsOtp = {
    code,
    max_attempts: SMS_OTP_MAX_ATTEMPTS,
    expires_at_ms: Date.now() + ttlSecs * 1000,
  }
  await redisClient.set(key, JSON.stringify(stored), 'EX', ttlSecs)
  await redisClient.del(attemptsKey)
}

/** Parse the StoredOtp JSON at `key`, or null when absent/unparseable. */
async function readCodeAtKey(key: string): Promise<string | null> {
  const raw = await redisClient.get(key)
  if (!raw) return null
  try {
    const stored = JSON.parse(raw) as StoredSmsOtp
    return stored.code
  } catch {
    return null
  }
}

/**
 * Inject a known SMS LOGIN code (the login-family key the production
 * `sms_otp_send` writes and `sms_otp_verify` claims).
 *
 * @param realmId   Realm id, inserted verbatim into the Redis key.
 * @param phone     CN mobile in any bare national form (normalized to +86 E.164).
 * @param code      The 6-digit code the demo will type into the UI.
 * @param ttlSecs   Key TTL; defaults to the production code TTL (300s).
 */
export async function injectSmsLoginCode(
  realmId: string,
  phone: string,
  code: string,
  ttlSecs: number = SMS_OTP_CODE_TTL_SECONDS
): Promise<void> {
  const e164 = toE164(phone)
  await injectCodeAtKeys(
    loginOtpKey(realmId, e164),
    loginAttemptsKey(realmId, e164),
    code,
    ttlSecs
  )
  console.log(`[SmsOtp Redis Helper] Injected login code for ${e164} in realm ${realmId}`)
}

/**
 * Inject a known SMS BIND/REBIND code (the bind-family key the production
 * `user_phone_bind_send` writes and `user_phone_bind_confirm` claims).
 *
 * @param userId  Account id — the bind key is per-user, not per-phone-only.
 */
export async function injectSmsBindCode(
  realmId: string,
  userId: string,
  phone: string,
  code: string,
  ttlSecs: number = SMS_OTP_CODE_TTL_SECONDS
): Promise<void> {
  const e164 = toE164(phone)
  await injectCodeAtKeys(
    bindOtpKey(realmId, userId, e164),
    bindAttemptsKey(realmId, userId, e164),
    code,
    ttlSecs
  )
  console.log(
    `[SmsOtp Redis Helper] Injected bind code for ${e164} (user ${userId}) in realm ${realmId}`
  )
}

/**
 * Read back the stored login code (also used to assert "no code was issued"
 * for the unregistered-phone branch, which must leave the key absent).
 */
export async function getSmsLoginCode(
  realmId: string,
  phone: string
): Promise<string | null> {
  return readCodeAtKey(loginOtpKey(realmId, toE164(phone)))
}

/** Read back the stored bind code (verify the injection landed). */
export async function getSmsBindCode(
  realmId: string,
  userId: string,
  phone: string
): Promise<string | null> {
  return readCodeAtKey(bindOtpKey(realmId, userId, toE164(phone)))
}

/** Delete the login-family code + counter keys (test cleanup). */
export async function clearSmsLoginCode(realmId: string, phone: string): Promise<void> {
  const e164 = toE164(phone)
  await redisClient.del(loginOtpKey(realmId, e164), loginAttemptsKey(realmId, e164))
}

/** Delete the bind-family code + counter keys (test cleanup). */
export async function clearSmsBindCode(
  realmId: string,
  userId: string,
  phone: string
): Promise<void> {
  const e164 = toE164(phone)
  await redisClient.del(bindOtpKey(realmId, userId, e164), bindAttemptsKey(realmId, userId, e164))
}

/**
 * Close the shared Redis connection. Call at the end of a test suite (mirrors
 * `closeOtpRedis`).
 */
export async function closeSmsOtpRedis(): Promise<void> {
  await redisClient.quit()
  console.log('[SmsOtp Redis Helper] Redis connection closed')
}
