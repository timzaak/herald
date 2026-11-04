import { z } from 'zod'
import { quotaWindowSchema } from '@/lib/schemas/points-forms'

const fixedPointRuleSchema = z.object({
  id: z.string().nullable().optional(),
  bucketId: z.string().min(1, 'Select a target account'),
  triggerSources: z.array(z.string()).min(1, 'Select at least one trigger'),
  grantMode: z.literal('fixed'),
  pointsAmount: z.number().int().min(1, 'Points must be greater than zero'),
  validityDays: z.number().int().min(0).nullable().optional(),
  grantPeriodType: z.string().nullable().optional(),
  quotaWindows: z.null().optional(),
  enabled: z.boolean().optional(),
  displayOrder: z.number().int().min(0).optional(),
})

const quotaPointRuleSchema = z.object({
  id: z.string().nullable().optional(),
  bucketId: z.string().min(1, 'Select a target account'),
  triggerSources: z.array(z.string()).min(1, 'Select at least one trigger'),
  grantMode: z.literal('quota'),
  pointsAmount: z.null().optional(),
  validityDays: z.null().optional(),
  grantPeriodType: z.string().nullable().optional(),
  quotaWindows: z.array(quotaWindowSchema).min(1, 'Add at least one quota window').max(8),
  enabled: z.boolean().optional(),
  displayOrder: z.number().int().min(0).optional(),
})

export const pointDistributionRuleSchema = z.discriminatedUnion('grantMode', [
  fixedPointRuleSchema,
  quotaPointRuleSchema,
])

export const pointDistributionRulesSchema = z.array(pointDistributionRuleSchema)

export type PointDistributionRuleFormData = z.infer<typeof pointDistributionRuleSchema>

/**
 * Adapt a backend point-distribution rule (an entitlement-mapping `pointRules`
 * entry or a registration `rules` entry) into {@link PointDistributionRuleFormData}.
 *
 * This is the single home for the grant-mode field defaults (`fixed` →
 * `pointsAmount ?? 1`, `validityDays ?? 0`; `quota` → null those and seed
 * windows) so the two consuming pages don't each re-derive them. Quota windows
 * are normalized to `{ windowSeconds, limit }`, dropping any display-only `key`,
 * to match {@link quotaWindowSchema}.
 */
export function toPointDistributionRuleFormData<
  Rule extends {
    grantMode?: string
    pointsAmount?: number | null
    validityDays?: number | null
    quotaWindows?: { windowSeconds: number; limit: number }[] | null
  },
>(rule: Rule): PointDistributionRuleFormData {
  if (rule.grantMode === 'quota') {
    return {
      ...rule,
      grantMode: 'quota',
      pointsAmount: null,
      validityDays: null,
      quotaWindows: (rule.quotaWindows ?? []).map(({ windowSeconds, limit }) => ({
        windowSeconds,
        limit,
      })),
    } as unknown as PointDistributionRuleFormData
  }
  return {
    ...rule,
    grantMode: 'fixed',
    pointsAmount: rule.pointsAmount ?? 1,
    validityDays: rule.validityDays ?? 0,
    quotaWindows: null,
  } as unknown as PointDistributionRuleFormData
}

// Mirrors the generated `PriceMappingUpdate` / `BatchUpdateEntitlementMappingsRequest`.
// One price row per entry; `entitlementKey` is provider-sync owned and not editable here.

export const priceMappingUpdateSchema = z.object({
  mappingId: z.string().min(1),

  billingType: z.enum(['recurring', 'one_time', 'non_renewing']).nullable().optional(),

  billingPeriod: z.string().nullable().optional(),

  enabled: z.boolean().nullable().optional(),

  // LOCAL STATE ONLY — seeded from the GET response and carried in the edit row
  // so the non_renewing input can read/write it. It is NOT part of the batch
  // save payload: `toPriceMappingUpdate` deliberately omits it because the batch
  // duration edits are persisted via a separate single-row PUT
  // (`useUpdateEntitlementMapping`, `UpdateEntitlementMappingRequest` carries
  // `serviceDurationDays` 3-state) triggered on the field's onBlur.
  serviceDurationDays: z.number().int().min(1).nullable().optional(),

  // `PriceMappingUpdate.grantedRoleIds` three-state contract: `null`/undefined
  // ⟺ leave unchanged, `[]` ⟺ clear (no role grant), non-empty ⟺ set. Orthogonal
  // to billing_type and points strategy (empty points + roles = pure entitlement;
  // empty roles + points = pure credit pack; both empty = payment record only).
  grantedRoleIds: z.array(z.string()).nullable().optional(),

  pointRules: pointDistributionRulesSchema.nullable().optional(),
})

export type PriceMappingUpdateFormData = z.infer<typeof priceMappingUpdateSchema>

export const batchEntitlementMappingsSchema = z.object({
  paymentProvider: z.string().min(1),
  externalProductId: z.string().min(1),
  updates: z.array(priceMappingUpdateSchema).min(1),
})

export type BatchEntitlementMappingsFormData = z.infer<typeof batchEntitlementMappingsSchema>

/**
 * Build form defaults for the price-level batch editor from the product's
 * current price rows. Each input row must already carry its `mappingId`.
 */
export function getBatchEntitlementMappingsDefaults(
  config?: Partial<BatchEntitlementMappingsFormData>
): BatchEntitlementMappingsFormData {
  return {
    paymentProvider: '',
    externalProductId: '',
    updates: [],
    ...config,
  } as BatchEntitlementMappingsFormData
}
