/**
 * TOTP Database Helper for Demo Tests
 *
 * Provides database-level functions to disable TOTP configurations directly.
 * This bypasses the UI and login requirements, useful for test cleanup and reset.
 */

import { Pool, PoolClient, QueryResult } from 'pg'

// ============================================================================
// Configuration
// ============================================================================

const DEFAULT_DATABASE_URL =
  process.env.DATABASE_URL || 'postgres://postgres:postgres@127.0.0.1:5432/herald_demo'

const pool = new Pool({
  connectionString: DEFAULT_DATABASE_URL,
})

// ============================================================================
// Types
// ============================================================================

/**
 * TOTP User Configuration
 */
export interface UserTOTPConfig {
  id: string
  user_id: string
  realm_id: string
  enabled: boolean
  verified_at: Date | null
  last_used_at: Date | null
  created_at: Date
  updated_at: Date
}

/**
 * Realm TOTP Settings
 */
export interface RealmTOTPSettings {
  enabled: boolean
  force_enabled: boolean
}

// ============================================================================
// Database Helper Functions
// ============================================================================

/**
 * Disable TOTP for a specific user
 *
 * This function directly updates the database to disable TOTP for a user,
 * bypassing the need for login or UI interaction.
 *
 * @param userId User UUID
 * @param realmId Realm ID (default: 'admin')
 *
 * @example
 * await disableUserTOTP('123e4567-e89b-12d3-a456-426614174000', 'admin')
 */
export async function disableUserTOTP(
  userId: string,
  realmId: string = 'admin'
): Promise<void> {
  const client = await pool.connect()
  try {
    const result: QueryResult = await client.query(
      `UPDATE user_totp_config
       SET enabled = false, updated_at = NOW()
       WHERE user_id = $1 AND realm_id = $2
       RETURNING id, user_id, realm_id, enabled`,
      [userId, realmId]
    )

    if (result.rowCount === 0) {
      console.log(
        `[TOTP DB Helper] No TOTP config found for user ${userId} in realm ${realmId}`
      )
      return
    }

    console.log(
      `[TOTP DB Helper] Disabled TOTP for user ${userId} in realm ${realmId}`
    )
  } finally {
    client.release()
  }
}

/**
 * Disable TOTP for all users in a realm
 *
 * This function disables TOTP for all users in a specific realm.
 * Useful for cleaning up test data.
 *
 * @param realmId Realm ID
 *
 * @example
 * await disableRealmUserTOTP('admin')
 */
export async function disableRealmUserTOTP(realmId: string): Promise<number> {
  const client = await pool.connect()
  try {
    const result: QueryResult = await client.query(
      `UPDATE user_totp_config
       SET enabled = false, updated_at = NOW()
       WHERE realm_id = $1
       RETURNING id, user_id, realm_id, enabled`,
      [realmId]
    )

    console.log(
      `[TOTP DB Helper] Disabled TOTP for ${result.rowCount} users in realm ${realmId}`
    )

    return result.rowCount || 0
  } finally {
    client.release()
  }
}

/**
 * Disable realm-level TOTP configuration
 *
 * This function disables TOTP at the realm level (both enabled and force_enabled).
 *
 * @param realmId Realm ID (default: 'admin')
 *
 * @example
 * await disableRealmTOTP('admin')
 */
export async function disableRealmTOTP(realmId: string = 'admin'): Promise<void> {
  const client = await pool.connect()
  try {
    await client.query(
      `UPDATE realm_config
       SET config_value = jsonb_build_object(
         'enabled', to_jsonb(false),
         'force_enabled', to_jsonb(false)
       ),
       updated_at = NOW()
       WHERE realm_id = $1 AND config_type = 'totp' AND config_key = 'settings'`,
      [realmId]
    )

    console.log(
      `[TOTP DB Helper] Disabled Realm TOTP for realm ${realmId}`
    )
  } finally {
    client.release()
  }
}

/**
 * Get user TOTP configuration
 *
 * @param userId User UUID
 * @param realmId Realm ID (default: 'admin')
 * @returns User TOTP configuration or null if not found
 *
 * @example
 * const config = await getUserTOTPConfig('123e4567-e89b-12d3-a456-426614174000', 'admin')
 * if (config && config.enabled) {
 *   console.log('User has TOTP enabled')
 * }
 */
export async function getUserTOTPConfig(
  userId: string,
  realmId: string = 'admin'
): Promise<UserTOTPConfig | null> {
  const client = await pool.connect()
  try {
    const result: QueryResult = await client.query(
      `SELECT id, user_id, realm_id, enabled, verified_at, last_used_at, created_at, updated_at
       FROM user_totp_config
       WHERE user_id = $1 AND realm_id = $2`,
      [userId, realmId]
    )

    if (result.rowCount === 0) {
      return null
    }

    return result.rows[0] as UserTOTPConfig
  } finally {
    client.release()
  }
}

/**
 * Get realm TOTP settings
 *
 * @param realmId Realm ID (default: 'admin')
 * @returns Realm TOTP settings
 *
 * @example
 * const settings = await getRealmTOTPSettings('admin')
 * console.log(`TOTP enabled: ${settings.enabled}, Force enabled: ${settings.force_enabled}`)
 */
export async function getRealmTOTPSettings(
  realmId: string = 'admin'
): Promise<RealmTOTPSettings | null> {
  const client = await pool.connect()
  try {
    const result: QueryResult = await client.query(
      `SELECT config_value
       FROM realm_config
       WHERE realm_id = $1 AND config_type = 'totp' AND config_key = 'settings'`,
      [realmId]
    )

    if (result.rowCount === 0) {
      return {
        enabled: false,
        force_enabled: false,
      }
    }

    const configValue = result.rows[0].config_value
    return {
      enabled: configValue.enabled || false,
      force_enabled: configValue.force_enabled || false,
    }
  } finally {
    client.release()
  }
}

/**
 * List all users with TOTP enabled in a realm
 *
 * @param realmId Realm ID
 * @returns Array of user IDs with TOTP enabled
 *
 * @example
 * const userIds = await listUsersWithTOTPEnabled('admin')
 * console.log(`Found ${userIds.length} users with TOTP enabled`)
 */
export async function listUsersWithTOTPEnabled(
  realmId: string
): Promise<string[]> {
  const client = await pool.connect()
  try {
    const result: QueryResult = await client.query(
      `SELECT user_id
       FROM user_totp_config
       WHERE realm_id = $1 AND enabled = true`,
      [realmId]
    )

    return result.rows.map((row) => row.user_id)
  } finally {
    client.release()
  }
}

/**
 * Delete TOTP configuration for a user
 *
 * This completely removes the TOTP configuration (including secret and backup codes).
 * Use with caution - this cannot be undone.
 *
 * @param userId User UUID
 * @param realmId Realm ID (default: 'admin')
 *
 * @example
 * await deleteUserTOTP('123e4567-e89b-12d3-a456-426614174000', 'admin')
 */
export async function deleteUserTOTP(
  userId: string,
  realmId: string = 'admin'
): Promise<void> {
  const client = await pool.connect()
  try {
    // Start transaction
    await client.query('BEGIN')

    // Delete user TOTP config (cascade will delete backup codes)
    const result: QueryResult = await client.query(
      `DELETE FROM user_totp_config
       WHERE user_id = $1 AND realm_id = $2
       RETURNING id`,
      [userId, realmId]
    )

    // Commit transaction
    await client.query('COMMIT')

    if (result.rowCount === 0) {
      console.log(
        `[TOTP DB Helper] No TOTP config found to delete for user ${userId} in realm ${realmId}`
      )
      return
    }

    console.log(
      `[TOTP DB Helper] Deleted TOTP config for user ${userId} in realm ${realmId}`
    )
  } catch (error) {
    // Rollback on error
    await client.query('ROLLBACK')
    throw error
  } finally {
    client.release()
  }
}

/**
 * Reset TOTP for a realm (disable all users and realm settings)
 *
 * This is a comprehensive reset that:
 * 1. Disables TOTP for all users in the realm
 * 2. Disables realm-level TOTP configuration
 *
 * Useful for cleaning up after tests that enable TOTP.
 *
 * @param realmId Realm ID
 *
 * @example
 * await resetRealmTOTP('admin')
 */
export async function resetRealmTOTP(realmId: string): Promise<void> {
  console.log(`[TOTP DB Helper] Resetting TOTP for realm ${realmId}`)

  // Step 1: Disable all user TOTP configurations
  const userCount = await disableRealmUserTOTP(realmId)

  // Step 2: Disable realm-level TOTP settings
  await disableRealmTOTP(realmId)

  console.log(
    `[TOTP DB Helper] Realm ${realmId} TOTP reset complete (${userCount} users disabled)`
  )
}

// ============================================================================
// Utility Functions
// ============================================================================

/**
 * Close the database connection pool
 *
 * Call this when done with all database operations.
 *
 * @example
 * await closePool()
 */
export async function closePool(): Promise<void> {
  await pool.end()
  console.log('[TOTP DB Helper] Database connection pool closed')
}

/**
 * Execute a custom SQL query
 *
 * @param sql SQL query string
 * @param params Query parameters
 * @returns Query result
 *
 * @example
 * const result = await executeQuery('SELECT * FROM user_totp_config WHERE realm_id = $1', ['admin'])
 */
export async function executeQuery(
  sql: string,
  params: any[] = []
): Promise<QueryResult> {
  const client = await pool.connect()
  try {
    return await client.query(sql, params)
  } finally {
    client.release()
  }
}

/**
 * Get database pool for advanced operations
 *
 * @returns PostgreSQL pool instance
 */
export function getPool(): Pool {
  return pool
}

// ============================================================================
// Convenience Functions for Common Use Cases
// ============================================================================

/**
 * Disable admin user TOTP
 *
 * Convenience function for the most common use case: disabling TOTP for the admin user.
 *
 * @param adminUserId Admin user UUID (optional, will query if not provided)
 * @param realmId Realm ID (default: 'admin')
 *
 * @example
 * // Disable admin TOTP by user ID
 * await disableAdminTOTP('123e4567-e89b-12d3-a456-426614174000', 'admin')
 *
 * // Disable admin TOTP by email (will query user ID first)
 * await disableAdminTOTPByEmail('admin@cas.com', 'admin')
 */
export async function disableAdminTOTP(
  adminUserId: string,
  realmId: string = 'admin'
): Promise<void> {
  return disableUserTOTP(adminUserId, realmId)
}

/**
 * Disable admin user TOTP by email
 *
 * This function first queries the user ID from the email, then disables TOTP.
 *
 * @param adminEmail Admin email (default: 'admin@cas.com')
 * @param realmId Realm ID (default: 'admin')
 *
 * @example
 * await disableAdminTOTPByEmail('admin@cas.com', 'admin')
 */
export async function disableAdminTOTPByEmail(
  adminEmail: string = 'admin@cas.com',
  realmId: string = 'admin'
): Promise<void> {
  const client = await pool.connect()
  try {
    // Query user ID from email
    const result: QueryResult = await client.query(
      `SELECT id FROM account WHERE email = $1 AND realm_id = $2`,
      [adminEmail, realmId]
    )

    if (result.rowCount === 0) {
      console.log(
        `[TOTP DB Helper] User ${adminEmail} not found in realm ${realmId}`
      )
      return
    }

    const userId = result.rows[0].id
    await disableUserTOTP(userId, realmId)
  } finally {
    client.release()
  }
}

/**
 * Ensure admin user does not have TOTP enabled
 *
 * This function checks if admin has TOTP enabled and disables it if necessary.
 *
 * @param adminEmail Admin email (default: 'admin@cas.com')
 * @param realmId Realm ID (default: 'admin')
 *
 * @example
 * await ensureAdminNoTOTP()
 */
export async function ensureAdminNoTOTP(
  adminEmail: string = 'admin@cas.com',
  realmId: string = 'admin'
): Promise<boolean> {
  // Query user ID from email
  const client = await pool.connect()
  let userId: string | null = null

  try {
    const result: QueryResult = await client.query(
      `SELECT id FROM account WHERE email = $1 AND realm_id = $2`,
      [adminEmail, realmId]
    )

    if (result.rowCount === 0) {
      console.log(
        `[TOTP DB Helper] User ${adminEmail} not found in realm ${realmId}`
      )
      return false
    }

    userId = result.rows[0].id
  } finally {
    client.release()
  }

  // Check if TOTP is enabled
  const config = await getUserTOTPConfig(userId, realmId)

  if (!config || !config.enabled) {
    console.log(`[TOTP DB Helper] Admin ${adminEmail} does not have TOTP enabled`)
    return false
  }

  // Disable TOTP
  await disableUserTOTP(userId, realmId)
  console.log(`[TOTP DB Helper] Admin ${adminEmail} TOTP disabled`)

  return true
}

// ============================================================================
// Test Script Example
// ============================================================================

/**
 * Example test script that demonstrates common usage
 *
 * To run this example:
 * 1. Save as a separate file (e.g., test-totp-helper.ts)
 * 2. Run with: tsx test-totp-helper.ts
 */
export async function exampleUsage(): Promise<void> {
  console.log('=== TOTP Database Helper Example ===\n')

  // Example 1: Check admin TOTP status
  console.log('1. Checking admin TOTP status...')
  const adminEmail = 'admin@cas.com'
  const realmId = 'admin'

  try {
    const adminHasTOTP = await ensureAdminNoTOTP(adminEmail, realmId)
    console.log(`   Admin TOTP disabled: ${adminHasTOTP}\n`)
  } catch (error) {
    console.log(`   Error: ${error}\n`)
  }

  // Example 2: List users with TOTP enabled
  console.log('2. Listing users with TOTP enabled...')
  try {
    const userIds = await listUsersWithTOTPEnabled(realmId)
    console.log(`   Found ${userIds.length} users with TOTP enabled`)
    if (userIds.length > 0) {
      console.log(`   User IDs: ${userIds.join(', ')}`)
    }
    console.log()
  } catch (error) {
    console.log(`   Error: ${error}\n`)
  }

  // Example 3: Get realm TOTP settings
  console.log('3. Getting realm TOTP settings...')
  try {
    const settings = await getRealmTOTPSettings(realmId)
    console.log(`   TOTP enabled: ${settings?.enabled}`)
    console.log(`   Force enabled: ${settings?.force_enabled}`)
    console.log()
  } catch (error) {
    console.log(`   Error: ${error}\n`)
  }

  // Example 4: Reset realm TOTP
  console.log('4. Resetting realm TOTP...')
  try {
    await resetRealmTOTP(realmId)
    console.log('   Realm TOTP reset complete\n')
  } catch (error) {
    console.log(`   Error: ${error}\n`)
  }

  // Close connection pool
  await closePool()
  console.log('=== Example Complete ===')
}

// Export for direct usage (optional)
if (import.meta.url === `file://${process.argv[1]}`) {
  exampleUsage().catch(console.error)
}
