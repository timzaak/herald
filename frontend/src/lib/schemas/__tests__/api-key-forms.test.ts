import { describe, it, expect } from 'vitest'
import { createApiKeySchema, updateApiKeySchema } from '../api-key-forms'

const CLIENT_APP_ID = '018f6f3a-7f25-7c00-9a2f-000000000001'

describe('createApiKeySchema', () => {
  describe('valid inputs', () => {
    it('should accept minimal valid input (name + clientAppId)', () => {
      const result = createApiKeySchema.safeParse({
        name: 'My API Key',
        clientAppId: CLIENT_APP_ID,
      })

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.data.name).toBe('My API Key')
        expect(result.data.expiresAt).toBeUndefined()
      }
    })

    it('should accept name at boundary length 1', () => {
      const result = createApiKeySchema.safeParse({ name: 'x', clientAppId: CLIENT_APP_ID })

      expect(result.success).toBe(true)
    })

    it('should accept name at boundary length 100', () => {
      const result = createApiKeySchema.safeParse({
        name: 'a'.repeat(100),
        clientAppId: CLIENT_APP_ID,
      })

      expect(result.success).toBe(true)
    })

    it('should accept optional expiresAt as a date string', () => {
      const result = createApiKeySchema.safeParse({
        name: 'Expiring Key',
        clientAppId: CLIENT_APP_ID,
        expiresAt: '2099-12-31T23:59:59Z',
      })

      expect(result.success).toBe(true)
      if (result.success) {
        // transform converts to full ISO string via Date.toISOString()
        expect(result.data.expiresAt).toBe('2099-12-31T23:59:59.000Z')
      }
    })

    it('should accept required clientAppId as UUID', () => {
      const result = createApiKeySchema.safeParse({
        name: 'Scoped Key',
        clientAppId: CLIENT_APP_ID,
      })

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.data.clientAppId).toBe(CLIENT_APP_ID)
      }
    })

    it('should strip empty string expiresAt to undefined', () => {
      const result = createApiKeySchema.safeParse({
        name: 'No Expiry Key',
        clientAppId: CLIENT_APP_ID,
        expiresAt: '',
      })

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.data.expiresAt).toBeUndefined()
      }
    })

    it('should convert datetime-local value to RFC 3339 format', () => {
      // datetime-local input produces values like "2026-12-31T23:59"
      const result = createApiKeySchema.safeParse({
        name: 'Local DateTime Key',
        clientAppId: CLIENT_APP_ID,
        expiresAt: '2026-12-31T23:59',
      })

      expect(result.success).toBe(true)
      if (result.success) {
        // Should be a valid ISO 8601 / RFC 3339 string
        expect(result.data.expiresAt).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/)
      }
    })
  })

  describe('invalid inputs', () => {
    it('should reject empty name', () => {
      const result = createApiKeySchema.safeParse({ name: '', clientAppId: CLIENT_APP_ID })

      expect(result.success).toBe(false)
    })

    it('should reject name exceeding 100 characters', () => {
      const result = createApiKeySchema.safeParse({
        name: 'a'.repeat(101),
        clientAppId: CLIENT_APP_ID,
      })

      expect(result.success).toBe(false)
    })

    it('should accept whitespace-only name (no .trim() in schema)', () => {
      // Schema uses .min(1) without .trim(), so whitespace-only passes.
      // This test documents current behavior.
      const result = createApiKeySchema.safeParse({ name: '   ', clientAppId: CLIENT_APP_ID })

      expect(result.success).toBe(true)
    })

    it('should reject non-string expiresAt', () => {
      const result = createApiKeySchema.safeParse({
        name: 'Valid Name',
        clientAppId: CLIENT_APP_ID,
        expiresAt: 12345,
      })

      expect(result.success).toBe(false)
    })

    it('should reject missing clientAppId', () => {
      // clientAppId is required: the bound app is the auth gate (a disabled
      // app kills the key), so every key must name its gate explicitly.
      const result = createApiKeySchema.safeParse({ name: 'Valid Name' })

      expect(result.success).toBe(false)
    })

    it('should reject invalid clientAppId', () => {
      const result = createApiKeySchema.safeParse({
        name: 'Valid Name',
        clientAppId: 'not-a-uuid',
      })

      expect(result.success).toBe(false)
    })
  })
})

describe('updateApiKeySchema', () => {
  describe('valid inputs', () => {
    it('should accept update with name, enabled, and expiresAt', () => {
      const result = updateApiKeySchema.safeParse({
        name: 'Updated Key',
        enabled: true,
        expiresAt: '2099-12-31T23:59:59Z',
      })

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.data.name).toBe('Updated Key')
        expect(result.data.enabled).toBe(true)
        // transform converts to full ISO string via Date.toISOString()
        expect(result.data.expiresAt).toBe('2099-12-31T23:59:59.000Z')
      }
    })

    it('should convert datetime-local value to RFC 3339 in update', () => {
      const result = updateApiKeySchema.safeParse({
        name: 'Updated Key',
        enabled: true,
        expiresAt: '2026-12-31T23:59',
      })

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.data.expiresAt).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/)
      }
    })

    it('should accept enabled as boolean false', () => {
      const result = updateApiKeySchema.safeParse({
        name: 'Key',
        enabled: false,
        expiresAt: null,
      })

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.data.enabled).toBe(false)
      }
    })

    it('should accept expiresAt set to null (clear expiry)', () => {
      const result = updateApiKeySchema.safeParse({
        name: 'Key',
        enabled: true,
        expiresAt: null,
      })

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.data.expiresAt).toBeNull()
      }
    })
  })

  describe('invalid inputs', () => {
    it('should reject name exceeding 100 characters', () => {
      const result = updateApiKeySchema.safeParse({
        name: 'a'.repeat(101),
        enabled: true,
        expiresAt: null,
      })

      expect(result.success).toBe(false)
    })

    it('should reject empty name', () => {
      const result = updateApiKeySchema.safeParse({
        name: '',
        enabled: true,
        expiresAt: null,
      })

      expect(result.success).toBe(false)
    })

    it('should reject non-boolean enabled value', () => {
      const result = updateApiKeySchema.safeParse({
        name: 'Key',
        enabled: 'yes',
        expiresAt: null,
      })

      expect(result.success).toBe(false)
    })

    it('should reject non-string, non-null expiresAt', () => {
      const result = updateApiKeySchema.safeParse({
        name: 'Key',
        enabled: true,
        expiresAt: 12345,
      })

      expect(result.success).toBe(false)
    })

    it('should reject missing expiresAt (field is required, not optional)', () => {
      // Schema uses .nullable(), not .optional(), so the field must be present
      const result = updateApiKeySchema.safeParse({
        name: 'Key',
        enabled: true,
      })

      expect(result.success).toBe(false)
    })
  })
})
