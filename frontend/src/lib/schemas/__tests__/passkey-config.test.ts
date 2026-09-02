import { describe, it, expect } from 'vitest'
import { passkeyConfigSchema } from '../realm-config'

/**
 * Factory: a minimal valid passkey config. Overrides merge on top of the
 * baseline so individual tests can focus on one field at a time without
 * re-declaring the full shape.
 */
function makePasskeyConfig(
  overrides: Partial<
    Record<'enabled' | 'userVerification' | 'crossPlatformAuthenticator', unknown>
  > = {}
): Record<string, unknown> {
  return {
    enabled: true,
    ...overrides,
  }
}

describe('passkeyConfigSchema', () => {
  describe('defaults encode business decisions', () => {
    it('should default userVerification to preferred when omitted', () => {
      const result = passkeyConfigSchema.safeParse(makePasskeyConfig())

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.data.userVerification).toBe('preferred')
      }
    })

    it('should default crossPlatformAuthenticator to true when omitted', () => {
      const result = passkeyConfigSchema.safeParse(makePasskeyConfig())

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.data.crossPlatformAuthenticator).toBe(true)
      }
    })
  })

  describe('required fields', () => {
    it('should reject when enabled is missing (no default)', () => {
      const result = passkeyConfigSchema.safeParse({
        userVerification: 'preferred',
      })

      expect(result.success).toBe(false)
    })
  })

  describe('userVerification enum enforcement', () => {
    it.each(['preferred', 'required'] as const)(
      'should accept userVerification=%s',
      (userVerification) => {
        const result = passkeyConfigSchema.safeParse(makePasskeyConfig({ userVerification }))

        expect(result.success).toBe(true)
        if (result.success) {
          expect(result.data.userVerification).toBe(userVerification)
        }
      }
    )

    // The WebAuthn spec also defines 'discouraged', but this realm config
    // intentionally narrows the enum to preferred/required — it must be rejected.
    it('should reject discouraged (not part of realm enum)', () => {
      const result = passkeyConfigSchema.safeParse(
        makePasskeyConfig({ userVerification: 'discouraged' })
      )

      expect(result.success).toBe(false)
    })

    it('should reject invalid userVerification value', () => {
      const result = passkeyConfigSchema.safeParse(makePasskeyConfig({ userVerification: 'maybe' }))

      expect(result.success).toBe(false)
    })
  })

  describe('boolean field type enforcement', () => {
    it.each([
      ['enabled', 'true'],
      ['crossPlatformAuthenticator', 'true'],
    ])('should reject non-boolean %s (string instead of boolean)', (field, value) => {
      const result = passkeyConfigSchema.safeParse(makePasskeyConfig({ [field]: value }))

      expect(result.success).toBe(false)
    })
  })
})
