import { describe, it, expect, vi, beforeEach } from 'vitest'
import { emailSchema, passwordSchema, usernameSchema, changePasswordSchema } from '../common'

/**
 * Verify that Zod schemas use direct Paraglide message function calls
 * via Zod v4's { error: () => m.key() } pattern so that validation
 * messages are resolved from the paraglide i18n layer at validation time
 * (not at module-load time).
 *
 * The schemas use `{ error: () => m.key() }` which calls paraglide
 * message functions lazily when validation runs.
 */

const mockState = vi.hoisted(() => ({ locale: 'en' }))

vi.mock('@/paraglide/messages', () => ({
  m: new Proxy(
    {},
    {
      get: (_target: unknown, prop: string) => {
        return () => `[${mockState.locale}:${String(prop)}]`
      },
    }
  ),
}))

describe('Zod schema i18n -- runtime message resolution', () => {
  beforeEach(() => {
    mockState.locale = 'en'
  })

  describe('emailSchema uses Paraglide messages', () => {
    it('empty string produces error from Paraglide mock', () => {
      const result = emailSchema.safeParse('')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error.issues[0].message).toBe('[en:auth.email_required]')
      }
    })

    it('invalid email format produces error from Paraglide mock', () => {
      const result = emailSchema.safeParse('not-an-email')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error.issues[0].message).toBe('[en:auth.email_invalid]')
      }
    })
  })

  describe('passwordSchema uses Paraglide messages', () => {
    it('short string (below min 8) produces error from Paraglide mock', () => {
      const result = passwordSchema.safeParse('short')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error.issues[0].message).toBe('[en:auth.password_min_length]')
      }
    })
  })

  describe('usernameSchema uses Paraglide messages', () => {
    it('short string (below min 3) produces error from Paraglide mock', () => {
      const result = usernameSchema.safeParse('ab')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error.issues[0].message).toBe('[en:auth.username_min_length]')
      }
    })
  })

  describe('changePasswordSchema cross-field validation', () => {
    it('mismatched passwords produces error from Paraglide mock', () => {
      const result = changePasswordSchema.safeParse({
        oldPass: 'currentPassword1',
        newPass: 'newPassword123',
        confirmPass: 'differentPass1',
      })
      expect(result.success).toBe(false)
      if (!result.success) {
        const mismatchIssue = result.error.issues.find((issue) => issue.path[0] === 'confirmPass')
        expect(mismatchIssue).toBeDefined()
        expect(mismatchIssue!.message).toBe('[en:profile.passwords_dont_match]')
      }
    })

    it('empty oldPass produces error from Paraglide mock', () => {
      const result = changePasswordSchema.safeParse({
        oldPass: '',
        newPass: 'newPassword123',
        confirmPass: 'newPassword123',
      })
      expect(result.success).toBe(false)
      if (!result.success) {
        const oldPassIssue = result.error.issues.find((issue) => issue.path[0] === 'oldPass')
        expect(oldPassIssue).toBeDefined()
        expect(oldPassIssue!.message).toBe('[en:auth.current_password_required]')
      }
    })
  })

  describe('toggling mock produces different messages', () => {
    it('re-importing with different mock locale yields different error strings', async () => {
      const result1 = emailSchema.safeParse('')
      expect(result1.success).toBe(false)
      if (!result1.success) {
        expect(result1.error.issues[0].message).toBe('[en:auth.email_required]')
      }

      mockState.locale = 'zh'

      vi.resetModules()

      const { emailSchema: freshSchema } = await import('../common')

      const result2 = freshSchema.safeParse('')
      expect(result2.success).toBe(false)
      if (!result2.success) {
        expect(result2.error.issues[0].message).toBe('[zh:auth.email_required]')
      }

      mockState.locale = 'en'
    })
  })
})
