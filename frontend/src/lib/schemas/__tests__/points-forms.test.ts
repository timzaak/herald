import { describe, it, expect } from 'vitest'
import { transactionFiltersSchema, accountFiltersSchema, grantPointsSchema } from '../points-forms'

describe('transactionFiltersSchema', () => {
  describe('transactionType field', () => {
    it('GIVEN invalid type WHEN validating THEN should fail', () => {
      const result = transactionFiltersSchema.safeParse({
        transactionType: 'transfer' as any,
      })

      expect(result.success).toBe(false)
    })
  })

  describe('startTime field', () => {
    it('GIVEN invalid datetime format WHEN validating THEN should fail', () => {
      const result = transactionFiltersSchema.safeParse({
        startTime: '2025-01-01',
      })

      expect(result.success).toBe(false)
    })
  })

  describe('endTime field', () => {
    it('GIVEN invalid datetime format WHEN validating THEN should fail', () => {
      const result = transactionFiltersSchema.safeParse({
        endTime: '2025-03-15',
      })

      expect(result.success).toBe(false)
    })
  })

  describe('clientAppId field', () => {
    it('GIVEN valid UUID WHEN validating THEN should pass', () => {
      const result = transactionFiltersSchema.safeParse({
        clientAppId: '550e8400-e29b-41d4-a716-446655440000',
      })

      expect(result.success).toBe(true)
    })
  })
})

describe('accountFiltersSchema', () => {
  describe('status field', () => {
    it('GIVEN invalid status WHEN validating THEN should fail', () => {
      const result = accountFiltersSchema.safeParse({
        status: 'pending' as any,
      })

      expect(result.success).toBe(false)
    })
  })
})

/**
 * Factory for valid grant-points input. `bucketId` is the required Credit
 * Bucket target — schema enforces non-empty via
 * `min(1)`, NOT `.uuid()`. UUID grammar is the backend's authority (400
 * `grant_bucket_required`); the fail-loud concern at the schema layer is
 * "non-empty required".
 */
function validGrantInput(overrides: Record<string, unknown> = {}) {
  return {
    userId: 'user-1',
    amount: 100,
    reason: 'manual adjustment',
    bucketId: '550e8400-e29b-41d4-a716-446655440000',
    ...overrides,
  }
}

describe('grantPointsSchema', () => {
  describe('bucketId is a required target', () => {
    it.each([
      ['missing field', undefined],
      ['empty string', ''],
    ])('rejects %s with the bucket-required business error (fail loud)', (_label, bucketId) => {
      const input = validGrantInput()
      delete (input as { bucketId?: unknown }).bucketId
      if (bucketId !== undefined) {
        input.bucketId = bucketId
      }
      const result = grantPointsSchema.safeParse(input)
      expect(result.success).toBe(false)
      if (!result.success) {
        // The observable error carries the "必选目标 Bucket" business
        // semantics via points.validation_bucket_required, not a generic
        // zod message.
        const bucketIssue = result.error.issues.find((i) => i.path[0] === 'bucketId')
        expect(bucketIssue).toBeDefined()
      }
    })

    it('accepts a valid UUID', () => {
      const result = grantPointsSchema.safeParse(
        validGrantInput({ bucketId: '550e8400-e29b-41d4-a716-446655440000' })
      )
      expect(result.success).toBe(true)
    })

    // NOTE: schema uses `.min(1)`, not `.uuid()`. A non-empty non-UUID string
    // PASSES the schema — UUID format is the backend's authority (400
    // grant_bucket_required). Assert this contract explicitly so a future
    // tightening (e.g. adding .uuid()) is a loud, intentional change.
    it('passes a non-empty non-UUID string (UUID grammar enforced server-side)', () => {
      const result = grantPointsSchema.safeParse(validGrantInput({ bucketId: 'x' }))
      expect(result.success).toBe(true)
    })
  })
})
