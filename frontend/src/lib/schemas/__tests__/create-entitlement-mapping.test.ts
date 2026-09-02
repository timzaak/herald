import { describe, it, expect } from 'vitest'
import {
  createEntitlementMappingSchema,
  getCreateEntitlementMappingDefaults,
  majorUnitsToMinor,
  minorToMajorUnits,
} from '../create-entitlement-mapping'

/**
 * Base factory for a fully-valid create-mapping form. Individual tests override
 * the fields they want to invalidate so the schema boundary under test is the
 * only moving part.
 */
function makeValidForm(overrides: Record<string, unknown> = {}) {
  return {
    paymentProvider: 'apple',
    externalProductId: 'com.example.app.premium',
    externalPriceId: null,
    entitlementKey: 'premium',
    billingType: 'recurring',
    billingPeriod: 'monthly',
    pointRules: [],
    serviceDurationDays: null,
    grantedRoleIds: [],
    enabled: true,
    ...overrides,
  }
}

describe('createEntitlementMappingSchema', () => {
  it('accepts a valid recurring mapping with billingPeriod', () => {
    const result = createEntitlementMappingSchema.safeParse(makeValidForm())

    expect(result.success).toBe(true)
  })

  it('rejects recurring without billingPeriod (cross-field refinement)', () => {
    // Core cross-field constraint (support-iap §4.4.2 / §4.2.2):
    // billingType === 'recurring' ⇒ billingPeriod is mandatory. A null/missing
    // billingPeriod on a recurring row must be flagged at the billingPeriod
    // path — Demo never reaches this branch because the form hides submit until
    // the field is filled, so this Vitest is the only coverage.
    const result = createEntitlementMappingSchema.safeParse(makeValidForm({ billingPeriod: null }))

    expect(result.success).toBe(false)
    if (!result.success) {
      const paths = result.error.issues.map((issue) => String(issue.path[0]))
      expect(paths).toContain('billingPeriod')
    }
  })

  it('rejects recurring with a missing billingPeriod field entirely', () => {
    const { billingPeriod: _omit, ...withoutPeriod } = makeValidForm()

    const result = createEntitlementMappingSchema.safeParse(withoutPeriod)

    expect(result.success).toBe(false)
    if (!result.success) {
      const paths = result.error.issues.map((issue) => String(issue.path[0]))
      expect(paths).toContain('billingPeriod')
    }
  })

  it('accepts one_time without billingPeriod', () => {
    const result = createEntitlementMappingSchema.safeParse(
      makeValidForm({
        billingType: 'one_time',
        billingPeriod: null,
        // one_time rows have no recurring points; validityDays is the relevant field.
        validityDays: 30,
      })
    )

    expect(result.success).toBe(true)
  })

  describe('non_renewing (DEC-pay_model-005)', () => {
    // §4.2.2): serviceDurationDays is required (>=1) and billingPeriod is
    // mutually exclusive. These are schema-only checks the Demo cannot reach
    // (the dialog hides submit / the conflicting field), so Vitest is the
    // coverage.

    it('accepts non_renewing with a valid serviceDurationDays (>= 1)', () => {
      const result = createEntitlementMappingSchema.safeParse(
        makeValidForm({
          billingType: 'non_renewing',
          billingPeriod: null,
          serviceDurationDays: 90,
        })
      )

      expect(result.success).toBe(true)
    })

    it('rejects non_renewing when serviceDurationDays is missing', () => {
      const result = createEntitlementMappingSchema.safeParse(
        makeValidForm({
          billingType: 'non_renewing',
          billingPeriod: null,
          serviceDurationDays: null,
        })
      )

      expect(result.success).toBe(false)
      if (!result.success) {
        const paths = result.error.issues.map((issue) => String(issue.path[0]))
        expect(paths).toContain('serviceDurationDays')
      }
    })

    it.each([0, -1, 0.5])(
      'rejects non_renewing with an out-of-range serviceDurationDays (%s)',
      (serviceDurationDays) => {
        const result = createEntitlementMappingSchema.safeParse(
          makeValidForm({
            billingType: 'non_renewing',
            billingPeriod: null,
            serviceDurationDays,
          })
        )

        expect(result.success).toBe(false)
        if (!result.success) {
          const paths = result.error.issues.map((issue) => String(issue.path[0]))
          // 0/-1 are falsy so the required-refinement fires first at the
          // serviceDurationDays path; 0.5 passes required but fails `.int()`.
          expect(paths).toContain('serviceDurationDays')
        }
      }
    )

    it.each([0, -1, 1.5, '7'])(
      'rejects non_renewing with non-positive or non-integer serviceDurationDays (%s)',
      (serviceDurationDays) => {
        // Type/precision boundary beyond the existing out-of-range row: covers
        // a non-integer (1.5) and a string-coerced value ('7') that must fail
        // the `.int()` / numeric checks, plus the non-positive cases for
        // parity. `as unknown` bypasses the TS layer; the runtime schema is the
        // authority (the form input always arrives as a number or null, but the
        // schema must reject malformed shapes defensively).
        const result = createEntitlementMappingSchema.safeParse(
          makeValidForm({
            billingType: 'non_renewing',
            billingPeriod: null,
            serviceDurationDays: serviceDurationDays as unknown,
          })
        )

        expect(result.success).toBe(false)
        if (!result.success) {
          const paths = result.error.issues.map((issue) => String(issue.path[0]))
          expect(paths).toContain('serviceDurationDays')
        }
      }
    )

    it('rejects non_renewing when billingPeriod is also set (mutually exclusive)', () => {
      const result = createEntitlementMappingSchema.safeParse(
        makeValidForm({
          billingType: 'non_renewing',
          billingPeriod: 'monthly',
          serviceDurationDays: 90,
        })
      )

      expect(result.success).toBe(false)
      if (!result.success) {
        const paths = result.error.issues.map((issue) => String(issue.path[0]))
        // The mutually-exclusive refinement flags billingPeriod (not the
        // recurring-required wording, which is a different i18n key).
        expect(paths).toContain('billingPeriod')
      }
    })
  })

  describe('non_renewing cross-type isolation (DEC-pay_model-005)', () => {
    // serviceDurationDays's required constraint must ONLY act on non_renewing;
    // it must not flag one_time or recurring rows that leave it null (those
    // branches never set it). Pins the three-branch isolation so a regression
    // that widens the refinement to "any row" surfaces here.

    it('does NOT flag serviceDurationDays on one_time rows (cross-type isolation)', () => {
      const result = createEntitlementMappingSchema.safeParse(
        makeValidForm({
          billingType: 'one_time',
          billingPeriod: null,
          validityDays: 30,
          serviceDurationDays: null,
        })
      )

      expect(result.success).toBe(true)
    })

    it('does NOT flag serviceDurationDays on recurring rows', () => {
      const result = createEntitlementMappingSchema.safeParse(
        makeValidForm({
          billingType: 'recurring',
          billingPeriod: 'monthly',
          serviceDurationDays: null,
        })
      )

      expect(result.success).toBe(true)
    })
  })

  it.each(['', null])('rejects an empty/missing billingType (%s)', (billingType) => {
    const result = createEntitlementMappingSchema.safeParse(makeValidForm({ billingType }))

    expect(result.success).toBe(false)
    if (!result.success) {
      const paths = result.error.issues.map((issue) => String(issue.path[0]))
      expect(paths).toContain('billingType')
    }
  })

  it('rejects an invalid billingPeriod enum value', () => {
    const result = createEntitlementMappingSchema.safeParse(
      makeValidForm({ billingPeriod: 'weekly' as unknown })
    )

    expect(result.success).toBe(false)
    if (!result.success) {
      const paths = result.error.issues.map((issue) => String(issue.path[0]))
      expect(paths).toContain('billingPeriod')
    }
  })

  it.each(['paymentProvider', 'externalProductId', 'entitlementKey'])(
    'rejects an empty required field: %s',
    (field) => {
      const result = createEntitlementMappingSchema.safeParse(makeValidForm({ [field]: '' }))

      expect(result.success).toBe(false)
      if (!result.success) {
        const paths = result.error.issues.map((issue) => String(issue.path[0]))
        expect(paths).toContain(field)
      }
    }
  )

  it('allows externalPriceId to be null/optional (IAP & Creem leave it empty)', () => {
    const result = createEntitlementMappingSchema.safeParse(
      makeValidForm({ externalPriceId: null })
    )

    expect(result.success).toBe(true)
  })

  it('getCreateEntitlementMappingDefaults returns a baseline the schema accepts once required fields are filled', () => {
    // Guards against the defaults themselves drifting into an invalid shape
    // (e.g. billingType defaulting to 'recurring' while billingPeriod is null).
    const defaults = getCreateEntitlementMappingDefaults()

    // an independent column) so the create dialog renders the input empty until
    // the admin fills it. Pinned so the default does not drift into an invalid
    // shape (e.g. defaulting to 0, which the required-refinement would reject).
    expect(defaults.serviceDurationDays).toBeNull()

    // Fill only the human-entered required strings; billingType stays '' so no
    // recurring refinement triggers.
    const result = createEntitlementMappingSchema.safeParse({
      ...defaults,
      paymentProvider: 'google',
      externalProductId: 'com.example.app.gold',
      entitlementKey: 'gold',
      bucketId: 'bucket-1',
      billingType: 'one_time',
    })

    expect(result.success).toBe(true)
  })

  it('getCreateEntitlementMappingDefaults yields a valid non_renewing form once serviceDurationDays is filled', () => {
    // The defaults baseline must NOT drift into a shape that the non_renewing
    // branch can never accept: starting from defaults + filling the
    // human-entered required fields (and the non-renewing duration) must parse.
    const result = createEntitlementMappingSchema.safeParse({
      ...getCreateEntitlementMappingDefaults(),
      paymentProvider: 'google',
      externalProductId: 'com.example.app.gold',
      entitlementKey: 'gold',
      bucketId: 'bucket-1',
      billingType: 'non_renewing',
      serviceDurationDays: 30,
    })

    expect(result.success).toBe(true)
  })

  describe('WeChat manual price (wechat-support PRD §2.2 / §8.1)', () => {
    // WeChat has no hosted catalog, so the mapping price is entered by hand;
    // a WeChat mapping without a positive price can never produce a valid
    // order (the backend create-order call requires a positive amount), and
    // WeChat has no auto-renewal in scope, so recurring must be stopped at
    // the schema boundary — the backend rejects both with a 400.

    function makeValidWechatForm(overrides: Record<string, unknown> = {}) {
      return makeValidForm({
        paymentProvider: 'wechat',
        billingType: 'non_renewing',
        billingPeriod: null,
        serviceDurationDays: 30,
        priceYuan: '19.9',
        currency: 'CNY',
        ...overrides,
      })
    }

    it('accepts a WeChat non_renewing form with a valid manual price', () => {
      const result = createEntitlementMappingSchema.safeParse(makeValidWechatForm())
      expect(result.success).toBe(true)
    })

    it.each([
      ['missing', undefined],
      ['empty', ''],
      ['zero', '0'],
      ['malformed', '19.999'],
      ['negative', '-5'],
    ])('rejects a WeChat form when the price is %s', (_label, priceYuan) => {
      const result = createEntitlementMappingSchema.safeParse(
        makeValidWechatForm({ priceYuan: priceYuan as unknown })
      )
      expect(result.success).toBe(false)
      if (!result.success) {
        const paths = result.error.issues.map((issue) => String(issue.path[0]))
        expect(paths).toContain('priceYuan')
      }
    })

    it.each([
      ['missing', undefined],
      ['empty', ''],
      ['lowercase', 'cny'],
      ['too long', 'CNYX'],
    ])('rejects a WeChat form when the currency is %s', (_label, currency) => {
      const result = createEntitlementMappingSchema.safeParse(
        makeValidWechatForm({ currency: currency as unknown })
      )
      expect(result.success).toBe(false)
      if (!result.success) {
        const paths = result.error.issues.map((issue) => String(issue.path[0]))
        expect(paths).toContain('currency')
      }
    })

    it('rejects recurring for WeChat at the billingType path', () => {
      const result = createEntitlementMappingSchema.safeParse(
        makeValidWechatForm({ billingType: 'recurring', billingPeriod: 'monthly' })
      )
      expect(result.success).toBe(false)
      if (!result.success) {
        const paths = result.error.issues.map((issue) => String(issue.path[0]))
        expect(paths).toContain('billingType')
      }
    })

    it('does not require the manual price for non-WeChat providers (cross-provider isolation)', () => {
      // Catalog/IAP providers price via sync or the store; the refinement must
      // stay scoped to WeChat so existing forms keep parsing unchanged.
      const result = createEntitlementMappingSchema.safeParse(makeValidForm())
      expect(result.success).toBe(true)
    })
  })

  describe('major-unit ↔ minor-unit conversion', () => {
    // The manual price is read by a human in major units but stored/sent in
    // integer minor units (fen). String-split parsing must be exact — float
    // math would turn 19.9 into 1989.99… and silently misprice the order.
    it.each([
      ['19.9', 1990],
      ['19.90', 1990],
      ['19', 1900],
      ['0.01', 1],
      ['123456.78', 12345678],
    ])('converts %s yuan to %s fen exactly', (input, expected) => {
      expect(majorUnitsToMinor(input)).toBe(expected)
    })

    it.each([
      ['', null],
      ['abc', null],
      ['19.999', null],
      ['-3', null],
      [null, null],
      [undefined, null],
    ])('returns null for %s (schema gates validity first)', (input, expected) => {
      expect(majorUnitsToMinor(input as string | null | undefined)).toBe(expected)
    })

    it.each([
      [1990, '19.90'],
      [1900, '19.00'],
      [1, '0.01'],
      [12345678, '123456.78'],
    ])('renders %s fen as %s for the edit input', (input, expected) => {
      expect(minorToMajorUnits(input)).toBe(expected)
    })

    it('round-trips minor units through both converters without drift', () => {
      for (const fen of [1, 99, 100, 1990, 12345678]) {
        expect(majorUnitsToMinor(minorToMajorUnits(fen))).toBe(fen)
      }
    })
  })
})
