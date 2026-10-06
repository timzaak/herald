/**
 * Credit Bucket Seed Identifiers
 *
 * Stable constants mirroring the Credit Bucket directory seeded by
 * `scripts/lib/demo_seed.py::_ensure_credit_buckets`. Authoring items
 * (DE-D02..D05, DE-D07) reference these instead of magic strings so that
 * bucket key/name drift is caught at type-check time.
 *
 * Source of truth: `scripts/lib/demo_seed.py` (`_ensure_credit_buckets` /
 * `_ensure_credit_bucket_directory`). If you change a key here, update both.
 *
 * LOUD NOTE:
 * - Exactly one bucket per realm is the registration pool
 *   (`receives_registration_credits = true`). That is the `REGISTRATION_POOL`
 *   key below. Registration / free-periodic grants target this bucket
 *   (docs/user-stories/billing/credit-bucket.md).
 * - The legacy `default` key was the single-bucket predecessor; the directory
 *   now seeds `primary-pool` (registration) + `promo-pool` (secondary). Tests
 *   that need a deterministic non-registration bucket use `SECONDARY_POOL`.
 */

/**
 * Per-realm Credit Bucket keys produced by `_ensure_credit_buckets`.
 *
 * `bucket_key` matches `^[a-z0-9-]{1,64}$` (backend validation).
 */
export const CREDIT_BUCKET_KEYS = {
  /**
   * Primary pool: receives_registration_credits = true (the single
   * registration pool per realm). All seeded points wallet/ledger/txn rows,
   * assigned one-time mappings, and the seeded subscription land in this pool.
   */
  PRIMARY_POOL: 'primary-pool',
  /**
   * Secondary pool: enabled, NOT a registration pool. Covers the points demo
   * client app so DE-D03/DE-D04 can exercise cross-bucket assertions
   * (balance card grouping, SDK cross-pool consume). Holds no seeded balance
   * by default — tests that need balance here must grant explicitly.
   */
  SECONDARY_POOL: 'promo-pool',
} as const

export type CreditBucketKey = (typeof CREDIT_BUCKET_KEYS)[keyof typeof CREDIT_BUCKET_KEYS]

/**
 * Display names for the seeded buckets (matches `_ensure_credit_buckets`
 * `name` column). Stable across re-seeds via `ON CONFLICT ... DO UPDATE`.
 */
export const CREDIT_BUCKET_NAMES = {
  PRIMARY_POOL: 'Primary Pool',
  SECONDARY_POOL: 'Promo Pool',
} as const

/**
 * Convenience: the bucket key that is the registration pool.
 *
 * Tests asserting "registration pool receives registration credits" should
 * reference this constant. Per realm at most one bucket may carry this flag
 * (`uq_credit_buckets_registration_pool`).
 */
export const REGISTRATION_POOL_KEY: CreditBucketKey = CREDIT_BUCKET_KEYS.PRIMARY_POOL

/**
 * Realms seeded with the Credit Bucket directory.
 *
 * Mirrors `scripts/lib/demo_seed.py` (`POINTS_REALM_ID`, `ADMIN_REALM`).
 */
export const CREDIT_BUCKET_REALMS = {
  /** realm-001 — the primary demo realm (points/purchase/subscription demos). */
  POINTS: 'realm-001',
  /** admin realm — the bootstrap realm (subscription history, audit demos). */
  ADMIN: 'admin',
} as const
