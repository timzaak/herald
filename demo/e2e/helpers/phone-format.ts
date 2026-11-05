/**
 * Shared E.164 phone derivation for the phone-login demo helpers.
 *
 * Simplified CN-only form: `+86` + bare 11-digit national mobile — the demo
 * suite only exercises CN numbers written in the bare national form, and the
 * demo realm's default country code is CN. The backend authority is
 * `normalize_phone` (api-base auth util): it parses via the `phonenumber`
 * crate using the realm's `defaultCountryCode`; if that changes, this
 * derivation must be revisited (same drift warning as
 * `email-otp-redis-helper.ts`).
 *
 * Single shared copy on purpose: the DB seeding helper (`phone-db-helper.ts`)
 * and the Redis key derivation helper (`sms-otp-redis-helper.ts`) must agree
 * on this derivation — the seeded binding value and the injected OTP keys
 * reference the same E.164 string, and drift between two copies would break
 * the demo's code injection silently.
 */

/** Derive the E.164 storage/key form for a bare 11-digit CN mobile. */
export function toE164(bareCnPhone: string): string {
  const digits = bareCnPhone.replace(/\D/g, '')
  if (digits.length !== 11) {
    throw new Error(`[Phone Format] Expected a bare 11-digit CN mobile, got "${bareCnPhone}"`)
  }
  return `+86${digits}`
}
