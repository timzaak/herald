/**
 * Phone Binding Database Helper for Demo Tests
 *
 * Seeds and inspects account phone bindings directly in the demo database.
 * Neither the self-service side nor the admin side offers a
 * "bind without SMS verification" entry (verification IS the proof of
 * possession, DEC-phone-login-003), so demos that need a pre-bound user
 * (US-PL-002 login) can only seed the binding at the DB level — the same
 * rationale as `totp-db-helper.ts` / `reset-password-db-helper.ts`.
 *
 * The bind-key Redis helper (`sms-otp-redis-helper.injectSmsBindCode`) needs
 * the account id, so `getUserIdByEmail` doubles as the lookup for it.
 *
 * Phone values are stored E.164-normalized (DEC-phone-login-004); callers
 * pass the bare CN national form and this helper derives `+86` + 11 digits
 * (the demo realm's default country code is CN).
 *
 * @see ../../../backend/app/migrations/0006_account_phone.sql (phone column, unique per realm)
 */

import { Pool, QueryResult } from 'pg'
import { toE164 } from './phone-format'

const DEFAULT_DATABASE_URL =
  process.env.DATABASE_URL || 'postgres://postgres:postgres@127.0.0.1:5432/herald_demo'

const pool = new Pool({
  connectionString: DEFAULT_DATABASE_URL,
})

export interface AccountPhoneRow {
  id: string
  email: string
  phone: string | null
}

async function queryAccountByEmail(
  email: string,
  realmId: string
): Promise<AccountPhoneRow | null> {
  const client = await pool.connect()
  try {
    const result: QueryResult<AccountPhoneRow> = await client.query(
      `SELECT id, email, phone FROM account WHERE email = $1 AND realm_id = $2`,
      [email, realmId]
    )
    return result.rowCount === 0 ? null : result.rows[0]
  } finally {
    client.release()
  }
}

/**
 * Look up the account id for an email — the bind-family Redis key embeds it.
 */
export async function getUserIdByEmail(
  email: string,
  realmId: string
): Promise<string | null> {
  const row = await queryAccountByEmail(email, realmId)
  return row?.id ?? null
}

/**
 * Read back the stored (E.164) phone binding for an account, or null when
 * the account has no binding / does not exist.
 */
export async function getUserPhoneByEmail(
  email: string,
  realmId: string
): Promise<string | null> {
  const row = await queryAccountByEmail(email, realmId)
  return row?.phone ?? null
}

/**
 * Seed a phone binding for an existing demo account (idempotent overwrite of
 * that account's own binding; does NOT free the number from another account —
 * demos use dedicated numbers to avoid the per-realm unique index).
 *
 * @returns The account id, or null when the account does not exist.
 */
export async function bindUserPhoneByEmail(
  email: string,
  bareCnPhone: string,
  realmId: string
): Promise<string | null> {
  const userId = await getUserIdByEmail(email, realmId)
  if (!userId) {
    console.log(`[Phone DB Helper] Account ${email} not found in realm ${realmId}; nothing bound`)
    return null
  }
  const e164 = toE164(bareCnPhone)
  const client = await pool.connect()
  try {
    await client.query(`UPDATE account SET phone = $1 WHERE id = $2`, [e164, userId])
    console.log(`[Phone DB Helper] Bound ${e164} to ${email} (${userId})`)
    return userId
  } finally {
    client.release()
  }
}

/**
 * Clear the phone binding for an account (test cleanup — restores the seed
 * user's pristine no-phone state).
 */
export async function clearUserPhoneByEmail(email: string, realmId: string): Promise<void> {
  const client = await pool.connect()
  try {
    await client.query(
      `UPDATE account SET phone = NULL WHERE email = $1 AND realm_id = $2`,
      [email, realmId]
    )
    console.log(`[Phone DB Helper] Cleared phone binding for ${email} in realm ${realmId}`)
  } finally {
    client.release()
  }
}

/** Close the shared pool. Call at the end of a test suite (mirrors totp-db-helper). */
export async function closePhoneDbPool(): Promise<void> {
  await pool.end()
  console.log('[Phone DB Helper] Database connection pool closed')
}
