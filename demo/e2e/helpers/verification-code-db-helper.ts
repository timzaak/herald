/**
 * Verification Code Database Helper for Demo Tests
 *
 * Generic access to the `email_verification_code` table for E2E flows that
 * must read (or clean up) a one-time code the backend persisted there. The
 * demo environment has no readable mailbox (its Resend key is fake, so the
 * send fails AFTER the code is persisted), so tests take the code straight
 * from the database. The `type` column discriminates the flow — e.g.
 * 'change_email' rows store the code under the NEW address,
 * 'reset_password' rows under the account address.
 *
 * reset-password-db-helper.ts predates this module and still carries its own
 * copy of the same queries; migrating it here is a flagged follow-up.
 */

import { Pool, QueryResult } from 'pg'

const DEFAULT_DATABASE_URL =
  process.env.DATABASE_URL || 'postgres://postgres:postgres@127.0.0.1:5432/herald_demo'

const pool = new Pool({
  connectionString: DEFAULT_DATABASE_URL,
})

/**
 * Fetch the most recent verification code of the given type for an email
 * address.
 *
 * Returns `null` when no code exists (e.g. the request was gated or the code
 * was never persisted).
 *
 * @param email The address the code row is stored under (flow-dependent —
 *              change-email codes live under the NEW address).
 * @param type  The `type` column value ('change_email', 'reset_password', ...).
 * @returns The verification code string, or null if none found.
 */
export async function getLatestVerificationCode(
  email: string,
  type: string
): Promise<string | null> {
  const client = await pool.connect()
  try {
    const result: QueryResult<{ verification_code: string }> = await client.query(
      `SELECT verification_code
       FROM email_verification_code
       WHERE email = $1 AND type = $2
       ORDER BY created_at DESC
       LIMIT 1`,
      [email, type]
    )

    if (result.rowCount === 0) {
      console.log(`[VerificationCode DB Helper] No ${type} code found for ${email}`)
      return null
    }

    return result.rows[0].verification_code
  } finally {
    client.release()
  }
}

/**
 * Delete all verification codes of the given type for an email address.
 *
 * Use in test cleanup to avoid leaking codes between test runs.
 *
 * @param email The address to clear codes for.
 * @param type  The `type` column value ('change_email', 'reset_password', ...).
 */
export async function clearVerificationCodes(email: string, type: string): Promise<void> {
  const client = await pool.connect()
  try {
    await client.query(
      `DELETE FROM email_verification_code WHERE email = $1 AND type = $2`,
      [email, type]
    )
    console.log(`[VerificationCode DB Helper] Cleared ${type} codes for ${email}`)
  } finally {
    client.release()
  }
}
