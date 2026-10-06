/**
 * TOTP Helper for Demo Tests
 *
 * Provides TOTP code generation functions for E2E testing.
 * Implements RFC 6238 TOTP algorithm with HMAC-SHA256 (backend compatible).
 *
 * @see https://tools.ietf.org/html/rfc6238
 * @see docs/user-stories/auth/totp.md
 */

import crypto from 'crypto'

/**
 * Generate a valid 6-digit TOTP code from a secret
 *
 * @param secret - TOTP secret (base32 encoded)
 * @returns 6-digit TOTP verification code
 *
 * @example
 * const secret = 'JBSWY3DPEHPK3PXP'
 * const code = generateTOTPCodeFromSecret(secret)
 * console.log(code) // e.g., '123456'
 */
export function generateTOTPCodeFromSecret(secret: string): string {
  // 1. Decode base32 secret
  const decodedSecret = base32Decode(secret)

  // 2. Get current time step (30 seconds)
  const timeStep = Math.floor(Date.now() / 1000 / 30)

  // 3. Generate HMAC-SHA256
  const hmac = crypto.createHmac('sha256', decodedSecret)
  const buffer = Buffer.alloc(8)
  buffer.writeBigUInt64BE(BigInt(timeStep))
  hmac.update(buffer)
  const hash = hmac.digest()

  // 4. Dynamic truncation
  const offset = hash[hash.length - 1] & 0x0f
  const truncatedHash = hash.slice(offset, offset + 4)

  // 5. Generate 6-digit code
  const code = truncatedHash.readUInt32BE(0) & 0x7fffffff
  const totpCode = (code % 1000000).toString().padStart(6, '0')

  return totpCode
}

/**
 * Generate a TOTP code for a specific time
 *
 * @param secret - TOTP secret (base32 encoded)
 * @param date - Date object for which to generate the code
 * @returns 6-digit TOTP verification code
 *
 * @example
 * const secret = 'JBSWY3DPEHPK3PXP'
 * const futureDate = new Date(Date.now() + 30000) // 30 seconds in future
 * const code = generateTOTPCodeForDate(secret, futureDate)
 */
export function generateTOTPCodeForDate(secret: string, date: Date): string {
  const decodedSecret = base32Decode(secret)
  const timeStep = Math.floor(date.getTime() / 1000 / 30)

  const hmac = crypto.createHmac('sha256', decodedSecret)
  const buffer = Buffer.alloc(8)
  buffer.writeBigUInt64BE(BigInt(timeStep))
  hmac.update(buffer)
  const hash = hmac.digest()

  const offset = hash[hash.length - 1] & 0x0f
  const truncatedHash = hash.slice(offset, offset + 4)

  const code = truncatedHash.readUInt32BE(0) & 0x7fffffff
  const totpCode = (code % 1000000).toString().padStart(6, '0')

  return totpCode
}

/**
 * Generate a series of TOTP codes for testing time drift scenarios
 *
 * @param secret - TOTP secret (base32 encoded)
 * @param count - Number of codes to generate (default: 3)
 * @param stepSeconds - Time step in seconds (default: 30)
 * @returns Array of TOTP codes
 *
 * @example
 * const secret = 'JBSWY3DPEHPK3PXP'
 * const codes = generateTOTPCodeSequence(secret, 3)
 * console.log(codes) // ['123456', '789012', '345678']
 */
export function generateTOTPCodeSequence(
  secret: string,
  count: number = 3,
  stepSeconds: number = 30
): string[] {
  const codes: string[] = []
  const decodedSecret = base32Decode(secret)
  const currentTime = Math.floor(Date.now() / 1000)

  for (let i = 0; i < count; i++) {
    const timeStep = Math.floor((currentTime + i * stepSeconds) / 30)

    const hmac = crypto.createHmac('sha256', decodedSecret)
    const buffer = Buffer.alloc(8)
    buffer.writeBigUInt64BE(BigInt(timeStep))
    hmac.update(buffer)
    const hash = hmac.digest()

    const offset = hash[hash.length - 1] & 0x0f
    const truncatedHash = hash.slice(offset, offset + 4)

    const code = truncatedHash.readUInt32BE(0) & 0x7fffffff
    const totpCode = (code % 1000000).toString().padStart(6, '0')

    codes.push(totpCode)
  }

  return codes
}

/**
 * Base32 decode
 *
 * Decodes a base32 string to a Buffer.
 * Uses the RFC 4648 base32 alphabet (A-Z, 2-7).
 */
function base32Decode(str: string): Buffer {
  // Base32 alphabet (RFC 4648)
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567'

  // Remove whitespace and padding characters
  str = str.toUpperCase().replace(/[^A-Z2-7]/g, '')

  // Decode to bits
  const bits: number[] = []
  for (const char of str) {
    const val = alphabet.indexOf(char)
    if (val === -1) continue

    // Each base32 character represents 5 bits
    bits.push((val >> 4) & 0x0f, (val >> 3) & 0x01, (val >> 2) & 0x01, (val >> 1) & 0x01, val & 0x01)
  }

  // Convert bits to bytes
  const bytes: number[] = []
  for (let i = 0; i + 8 <= bits.length; i += 8) {
    let byte = 0
    for (let j = 0; j < 8; j++) {
      byte = (byte << 1) | (bits[i + j] || 0)
    }
    bytes.push(byte)
  }

  return Buffer.from(bytes)
}

/**
 * Validate a TOTP code format
 *
 * @param code - TOTP code to validate
 * @returns true if the code is a valid 6-digit number
 */
export function isValidTOTPCode(code: string): boolean {
  return /^\d{6}$/.test(code)
}

/**
 * Test secrets for development and testing
 *
 * These secrets should only be used in demo/test environments.
 * Never use in production.
 */
export const TEST_SECRETS = {
  /**
   * A test TOTP secret for development
   * Base32: JBSWY3DPEHPK3PXP
   *
   * @example
   * const code = generateTOTPCodeFromSecret(TEST_SECRETS.default)
   */
  default: 'JBSWY3DPEHPK3PXP',

  /**
   * Alternative test secrets for testing secret regeneration
   */
  alternative: 'KRSXGZDFNQW4L33A',
  backup: 'M5WGY3DPEHPK3PXP',
}

/**
 * Backup codes generator for testing
 *
 * Generates 10 backup codes as they would be generated by the backend.
 * Each backup code is a 6-digit number.
 *
 * @returns Array of 10 backup codes
 */
export function generateTestBackupCodes(): string[] {
  const codes: string[] = []
  for (let i = 0; i < 10; i++) {
    const code = (Math.floor(Math.random() * 1000000)).toString().padStart(6, '0')
    codes.push(code)
  }
  return codes
}
