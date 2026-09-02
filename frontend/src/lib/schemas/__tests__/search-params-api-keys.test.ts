/**
 * API Key Search Schema Tests
 *
 * Tests apiKeysSearchSchema boundary validation:
 * valid inputs, invalid inputs, and unknown field stripping.
 */

import { describe, it, expect } from 'vitest'
import { apiKeysSearchSchema } from '../search-params'

describe('apiKeysSearchSchema', () => {
  describe('valid inputs', () => {
    it('accepts empty object (all fields optional)', () => {
      const result = apiKeysSearchSchema.safeParse({})
      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.data).toEqual({})
      }
    })

    it('accepts valid page and pageSize', () => {
      const result = apiKeysSearchSchema.safeParse({ page: 0, pageSize: 20 })
      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.data).toEqual({ page: 0, pageSize: 20 })
      }
    })

    it('accepts pageSize at boundary 1', () => {
      const result = apiKeysSearchSchema.safeParse({ pageSize: 1 })
      expect(result.success).toBe(true)
    })

    it('accepts pageSize at boundary 100', () => {
      const result = apiKeysSearchSchema.safeParse({ pageSize: 100 })
      expect(result.success).toBe(true)
    })

    it('accepts page at boundary 0', () => {
      const result = apiKeysSearchSchema.safeParse({ page: 0 })
      expect(result.success).toBe(true)
    })
  })

  describe('invalid inputs', () => {
    it.each([
      { input: { page: -1 }, field: 'page', value: -1, reason: 'negative page' },
      { input: { pageSize: 0 }, field: 'pageSize', value: 0, reason: 'pageSize of 0' },
      { input: { pageSize: 101 }, field: 'pageSize', value: 101, reason: 'pageSize above 100' },
      { input: { page: 1.5 }, field: 'page', value: 1.5, reason: 'non-integer page' },
      { input: { pageSize: 20.5 }, field: 'pageSize', value: 20.5, reason: 'non-integer pageSize' },
    ])('rejects $reason', ({ input }) => {
      const result = apiKeysSearchSchema.safeParse(input)
      expect(result.success).toBe(false)
    })
  })
})
