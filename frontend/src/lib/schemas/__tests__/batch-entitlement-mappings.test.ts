import { describe, it, expect } from 'vitest'
import { priceMappingUpdateSchema, batchEntitlementMappingsSchema } from '../billing-forms'
import { makeMapping } from '@/test/fixtures/entitlement-mappings'

// Unit-level Zod contracts for the price-granularity batch editor. Mirrors
// the generated `PriceMappingUpdate` / `BatchUpdateEntitlementMappingsRequest`.
// Asserts required fields, numeric/enum guards, and defaults shape. No MSW, no rendering.

/** Factory for a single valid update payload. */
function validUpdate(overrides: Record<string, unknown> = {}) {
  return {
    mappingId: 'map_pro_monthly',
    ...overrides,
  }
}

/** Factory for a valid batch envelope wrapping one update. */
function validBatch(overrides: Record<string, unknown> = {}) {
  return {
    paymentProvider: 'stripe',
    externalProductId: 'prod_pro',
    updates: [validUpdate()],
    ...overrides,
  }
}

describe('priceMappingUpdateSchema required fields', () => {
  it('fails when mappingId is missing', () => {
    const { mappingId: _omit, ...withoutMappingId } = validUpdate()
    void _omit

    const result = priceMappingUpdateSchema.safeParse(withoutMappingId)

    expect(result.success).toBe(false)
  })

  it('fails when mappingId is an empty string', () => {
    const result = priceMappingUpdateSchema.safeParse(validUpdate({ mappingId: '' }))
    expect(result.success).toBe(false)
  })

  it('accepts a row with only mappingId (optionals omitted)', () => {
    const result = priceMappingUpdateSchema.safeParse({
      mappingId: 'map_1',
    })
    expect(result.success).toBe(true)
  })
})

describe('priceMappingUpdateSchema numeric / enum guards', () => {
  it.each([
    [
      'fixed rule with zero points',
      {
        pointRules: [
          {
            bucketId: 'bucket-1',
            triggerSources: ['topup'],
            grantMode: 'fixed',
            pointsAmount: 0,
          },
        ],
      },
    ],
    [
      'quota rule without windows',
      {
        pointRules: [
          {
            bucketId: 'bucket-1',
            triggerSources: ['subscription_initial'],
            grantMode: 'quota',
            quotaWindows: [],
          },
        ],
      },
    ],
  ])('rejects %s', (_label, overrides) => {
    const result = priceMappingUpdateSchema.safeParse(validUpdate(overrides))
    expect(result.success).toBe(false)
  })

  it.each([
    ['empty rule set', { pointRules: [] }],
    [
      'valid fixed rule',
      {
        pointRules: [
          {
            bucketId: 'bucket-1',
            triggerSources: ['topup'],
            grantMode: 'fixed',
            pointsAmount: 100,
          },
        ],
      },
    ],
  ])('accepts %s', (_label, overrides) => {
    const result = priceMappingUpdateSchema.safeParse(validUpdate(overrides))
    expect(result.success).toBe(true)
  })
})

// Role-grant dimension (design §4.4 / §5.2). `grantedRoleIds` is a three-state
// field mirroring the generated `PriceMappingUpdate.grantedRoleIds`:
// non-empty ⟺ set, [] ⟺ clear, omitted/null ⟺ leave unchanged. Orthogonal to
// billing_type and points (empty points + roles = pure entitlement).
describe('priceMappingUpdateSchema grantedRoleIds three-state', () => {
  it.each([
    ['non-empty array sets roles', { grantedRoleIds: ['role-a'] }],
    ['empty array clears roles', { grantedRoleIds: [] }],
    ['null leaves roles unchanged', { grantedRoleIds: null }],
  ])('accepts %s', (_label, overrides) => {
    expect(priceMappingUpdateSchema.safeParse(validUpdate(overrides)).success).toBe(true)
  })

  it('accepts omission (undefined ⟺ leave unchanged)', () => {
    // validUpdate() produces only mappingId; grantedRoleIds is absent.
    expect(priceMappingUpdateSchema.safeParse(validUpdate()).success).toBe(true)
  })

  it.each([
    ['a string', { grantedRoleIds: 'role-a' }],
    ['a plain object', { grantedRoleIds: {} }],
    ['a number', { grantedRoleIds: 5 }],
  ])('rejects %s', (_label, overrides) => {
    expect(priceMappingUpdateSchema.safeParse(validUpdate(overrides)).success).toBe(false)
  })
})

describe('batchEntitlementMappingsSchema', () => {
  it('wraps the updates array and requires paymentProvider / externalProductId', () => {
    const result = batchEntitlementMappingsSchema.safeParse(validBatch())
    expect(result.success).toBe(true)
    if (result.success) {
      expect(result.data.updates).toHaveLength(1)
      expect(result.data.paymentProvider).toBe('stripe')
      expect(result.data.externalProductId).toBe('prod_pro')
    }
  })

  it('fails when paymentProvider is missing', () => {
    const { paymentProvider: _omit, ...rest } = validBatch()
    void _omit
    expect(batchEntitlementMappingsSchema.safeParse(rest).success).toBe(false)
  })

  it('fails when externalProductId is missing', () => {
    const { externalProductId: _omit, ...rest } = validBatch()
    void _omit
    expect(batchEntitlementMappingsSchema.safeParse(rest).success).toBe(false)
  })

  it('fails when updates is empty (at least one row required)', () => {
    const result = batchEntitlementMappingsSchema.safeParse(validBatch({ updates: [] }))
    expect(result.success).toBe(false)
  })

  it('accepts a batch seeded from a real fixture mapping', () => {
    const mapping = makeMapping()
    const result = batchEntitlementMappingsSchema.safeParse({
      paymentProvider: mapping.paymentProvider,
      externalProductId: mapping.externalProductId,
      updates: [
        {
          mappingId: mapping.id,
          billingType: mapping.billingType,
          billingPeriod: mapping.billingPeriod,
          enabled: mapping.enabled,
        },
      ],
    })
    expect(result.success).toBe(true)
  })
})
