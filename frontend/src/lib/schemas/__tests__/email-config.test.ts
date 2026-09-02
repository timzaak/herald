import { describe, it, expect } from 'vitest'
import { emailConfigSchema } from '../realm-config'

describe('emailConfigSchema', () => {
  describe('default values encode business decisions', () => {
    it('should default smtpPort to 587 when omitted', () => {
      const result = emailConfigSchema.safeParse({
        provider: 'resend',
        fromAddress: '',
      })

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.data.smtpPort).toBe('587')
      }
    })

    it('should default smtpEncryption to starttls when omitted', () => {
      const result = emailConfigSchema.safeParse({
        provider: 'resend',
        fromAddress: '',
      })

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.data.smtpEncryption).toBe('starttls')
      }
    })
  })

  describe('fromAddress allows empty string for unconfigured realm', () => {
    it('should accept empty string (unconfigured realm)', () => {
      const result = emailConfigSchema.safeParse({
        provider: 'resend',
        fromAddress: '',
      })

      expect(result.success).toBe(true)
    })

    it('should accept valid email address', () => {
      const result = emailConfigSchema.safeParse({
        provider: 'resend',
        fromAddress: 'noreply@example.com',
      })

      expect(result.success).toBe(true)
    })

    it('should reject invalid email that is not empty string', () => {
      const result = emailConfigSchema.safeParse({
        provider: 'resend',
        fromAddress: 'not-an-email',
      })

      expect(result.success).toBe(false)
    })
  })

  describe('provider enum enforcement', () => {
    it.each(['resend', 'smtp'] as const)('should accept provider=%s', (provider) => {
      const result = emailConfigSchema.safeParse({
        provider,
        fromAddress: '',
      })

      expect(result.success).toBe(true)
    })

    it('should reject invalid provider value', () => {
      const result = emailConfigSchema.safeParse({
        provider: 'sendgrid',
        fromAddress: '',
      })

      expect(result.success).toBe(false)
    })
  })

  describe('smtpEncryption enum enforcement', () => {
    it.each(['starttls', 'ssl'] as const)('should accept smtpEncryption=%s', (encryption) => {
      const result = emailConfigSchema.safeParse({
        provider: 'smtp',
        fromAddress: '',
        smtpEncryption: encryption,
      })

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.data.smtpEncryption).toBe(encryption)
      }
    })

    it('should reject invalid encryption value', () => {
      const result = emailConfigSchema.safeParse({
        provider: 'smtp',
        fromAddress: '',
        smtpEncryption: 'tls',
      })

      expect(result.success).toBe(false)
    })
  })
})
