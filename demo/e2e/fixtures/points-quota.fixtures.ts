/**
 * Points Quota Demo Fixtures
 *
 * Seeded constants and reusable test data for the `points-grant-redesign`
 * demo E2E suite (DE-D02..DE-D06).
 *
 * Sources:
 * - docs/user-stories/billing/points-user.md (US-PU-010, converged testid contract)
 * - quota-window semantics per US-PU-010 acceptance scenarios
 * - `scripts/lib/demo_seed.py` (seeded realm / users / entitlement mappings)
 */

export interface QuotaWindowFixture {
  /** Window length in seconds. */
  windowSeconds: number
  /** Quota upper limit for this window. */
  limit: number
  /** Config-derived stable winKey (e.g. `5h`, `week`, `month`). */
  key: string
}

/** Realm used by the quota redesign demo tests. */
export const QUOTA_DEMO_REALM = 'realm-001'

/** Seeded regular user in the quota demo realm. */
export const QUOTA_DEMO_USER_EMAIL = 'user@realm-001.com'

/** Seeded realm admin in the quota demo realm. */
export const QUOTA_DEMO_ADMIN_EMAIL = 'admin@realm-001.com'

/** Default password for seeded demo accounts. */
export const QUOTA_DEMO_PASSWORD = 'password'

/**
 * Entitlement key for the multi-price subscription product used by quota demos.
 *
 * The placeholder product id (`prod_stripe_multi_pro`) was removed from the
 * seed. Live quota demos resolve the REAL Stripe product id at runtime via
 * `ensureMultiPriceProduct()` (see
 * `demo/e2e/helpers/multi-price-live-product.ts`) and pass it to
 * `createEntitlementMappingWithQuotaWindows`. The shared entitlement key
 * remains stable across seed and live catalogs.
 */
export const QUOTA_DEMO_ENTITLEMENT_KEY = 'pro-plan'

/** Stable bucket hint used by points-quota demos (registration / primary pool). */
export const QUOTA_DEMO_BUCKET_KEY = 'primary-pool'

/**
 * Example multi-window quota configuration used across DE-D02..DE-D05.
 *
 * Mirrors the user-story examples in `docs/user-stories/billing/points-admin.md`
 * (US-PO-009 Scenario 1) and `docs/user-stories/billing/points-user.md`
 * (US-PU-010 Scenario 2).
 */
export const DEMO_QUOTA_WINDOWS: QuotaWindowFixture[] = [
  { windowSeconds: 5 * 60 * 60, limit: 500, key: '5h' },
  { windowSeconds: 7 * 24 * 60 * 60, limit: 5_000, key: 'week' },
  { windowSeconds: 30 * 24 * 60 * 60, limit: 20_000, key: 'month' },
]

/**
 * Free-periodic window configuration used for realm-default / free-user demos.
 *
 * Mirrors US-PO-009 Scenario 2 and US-FU-005 in the published user stories.
 */
export const DEMO_FREE_QUOTA_WINDOWS: QuotaWindowFixture[] = [
  { windowSeconds: 24 * 60 * 60, limit: 50, key: '1d' },
  { windowSeconds: 7 * 24 * 60 * 60, limit: 200, key: '7d' },
]

/**
 * Editor testid prefixes from the converged FE-A07 contract.
 */
export const QUOTA_EDITOR_PREFIX = 'quota-window' as const
export const REALM_DEFAULT_EDITOR_PREFIX = 'realm-default-window' as const
