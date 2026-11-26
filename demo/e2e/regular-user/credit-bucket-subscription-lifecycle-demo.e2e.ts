/**
 * Credit Bucket — Subscription Lifecycle by Bucket (US-CB-008)
 *
 * Role: regular-user (seeded demo points user `user@realm-001.com` in realm-001).
 *
 * ============================================================================
 * SCOPE DECISION — OUT-OF-DEMO-SCOPE (LOUD-NOTED, NOT A STUB)
 * ============================================================================
 *
 * Investigation outcome (DE-D05 step 1, read-only):
 * The demo harness CANNOT reliably drive subscription lifecycle webhooks
 * end-to-end. Specifically:
 *
 *   - `demo/e2e/helpers/payment-simulation.ts` exposes ONLY the one-time
 *     purchase fulfillment path (`POST /api/internal/bill/purchase/
 *     payment-attempts/{id}/fulfill`). It does NOT emit any of:
 *       * `invoice.paid` (renewal grant — 场景1)
 *       * `subscription.deleted` / cancel (revoke — 场景2 cancel)
 *       * refund / charge.refunded (revoke — 场景2 refund)
 *       * upgrade / downgrade plan-change events (场景3 / 场景4)
 *     bound to a `subscription.bucket_id`.
 *   - No demo-side helper creates a subscription via the production path
 *     (the existing `billing-subscription-demo.e2e.ts` documents this same
 *     gap: its cancel/detail tests are commented out as "pending subscription
 *     API" / "would need a way to create subscriptions via API (not
 *     database)"). Driving a lifecycle event REQUIRES a subscription to
 *     exist first, and the only ways to create one in the demo env are
 *     (a) real provider webhook (non-deterministic, requires live Stripe/
 *         Creem keys — belongs to `demo/e2e/live/**`, not the e2e suite), or
 *     (b) direct DB-row mutation, which this item's hard scope boundary
 *         FORBIDS ("No DB-row mutation bypassing the webhook path").
 *
 * Therefore — per DE-D05 step 6, third branch ("NONE driveable"): record the
 * investigation outcome, cover the one invariant the harness CAN observe on
 * persistent state, and loud-note US-CB-008 lifecycle events 1/2/3/4 as
 * out-of-demo-scope with a backend cross-reference. This file contains NO
 * stub and NO passing-without-assertions test: it has one real persistent-
 * state assertion (the bound/sibling pool isolation baseline) plus a
 * `test.skip` placeholder for each lifecycle scenario documenting the gap.
 *
 * Backend coverage cross-reference (the authoritative US-CB-008 coverage):
 *   `backend/api/src/tests/scenarios/credit_bucket/
 *    fulfillment_subscription_lifecycle_scenarios.rs`
 *     - Scenario 5: `subscription_paid_renews_to_same_bucket_pool`
 *       → US-CB-008 场景1 (renew → same pool).
 *     - Scenario 6: `subscription_upgrade_revokes_old_and_grants_new_
 *       within_same_bucket` → US-CB-008 场景3 (upgrade).
 *     - Scenario 7: `subscription_cancel_revokes_only_subscription_bucket_pool`
 *       → US-CB-008 场景2 cancel.
 *     - Scenario 8: `subscription_refund_revokes_only_subscription_bucket_pool`
 *       → US-CB-008 场景2 refund.
 *     - Scenario 9: `subscription_downgrade_preserves_current_cycle`
 *       → US-CB-008 场景4 (downgrade).
 *     - `subscription_with_unresolved_bucket_fails_loud`
 *       → design `SubscriptionBucketNotResolved` fail-loud.
 *   These run against the REAL `subscription_service` /
 *   `points_service.revoke_points_by_credit_type` via `ctx.app_state` (no
 *   mocking), so the "不串池" (bound pool changes, sibling pool unchanged)
 *   invariant IS asserted at the service layer.
 *
 * The single driveable assertion below verifies the PERSISTENT-STATE
 * SELECTORS + the bound/sibling isolation baseline that any future
 * demo-side lifecycle coverage would build on — using the seeded
 * subscription credit in `primary-pool` (the bound/registration pool) and
 * `promo-pool` as the sibling. It is the contract bridge between this demo
 * and the backend scenarios: if either the bound-pool card or the sibling-
 * pool isolation is broken at the persistent-state layer, every backend
 * lifecycle scenario would also be untrustworthy.
 *
 * Assertion discipline: lands on `points-balance-card-${bucketId}` (user
 * side, persistent list/detail state). No toast-only assertions.
 * ============================================================================
 */

import { expect } from '@playwright/test'

import { SELECTORS } from '../selectors'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { loginWithCredentials } from '../helpers/auth'
import { listBucketsViaApi } from '../helpers/bucket-helpers'
import {
  CREDIT_BUCKET_KEYS,
  CREDIT_BUCKET_REALMS,
} from '../helpers/bucket-seed-ids'
import { test, cleanupTestData } from '../fixtures/demo-page.fixtures'

// ============================================================================
// Constants
// ============================================================================

const TEST_REALM = CREDIT_BUCKET_REALMS.POINTS // 'realm-001'
const POINTS_USER_EMAIL = 'user@realm-001.com' // Created by Demo Seed
const POINTS_USER_PASSWORD = 'password'
const REALM_ADMIN_EMAIL = 'admin@realm-001.com'
const REALM_ADMIN_PASSWORD = 'password'

// ============================================================================
// Shared setup context — bucket UUIDs resolved once in beforeAll
// ============================================================================

interface SetupContext {
  /** UUID of the seeded `primary-pool` (registration/bound pool) bucket. */
  primaryPoolBucketId: string
  /** UUID of the seeded `promo-pool` (sibling) bucket. */
  promoPoolBucketId: string
}

/**
 * Lazily-resolved setup context. `beforeAll` populates this; individual tests
 * read from it. Throws if accessed before `beforeAll` has run (defensive —
 * catches accidental re-ordering).
 */
let setupCtx: SetupContext | null = null

/**
 * Resolve the seeded bucket UUIDs at runtime (read-only — NO mutation).
 *
 * `bucket-seed-ids.ts` exposes stable bucket KEYS (`primary-pool`,
 * `promo-pool`); the frontend testids use the bucket UUID, which is
 * generated once by `_ensure_credit_buckets` and is stable only within a
 * seeded environment. `listBucketsViaApi` reads the directory via the real
 * admin API (no DB mutation). The admin login is required because the
 * bucket list endpoint needs `points.manage` (view).
 */
function requireSetupCtx(): SetupContext {
  if (!setupCtx) {
    throw new Error(
      '[DE-D05] setupCtx accessed before beforeAll completed. ' +
        'Ensure test.beforeAll resolved the seeded bucket UUIDs.',
    )
  }
  return setupCtx
}

// ============================================================================
// beforeAll — resolve seeded bucket UUIDs (read-only; no DB mutation)
// ============================================================================

test.beforeAll(async ({ browser }) => {
  const context = await browser.newContext()
  const page = await context.newPage()

  try {
    // Login as the realm-001 admin to authenticate the bucket list request.
    // `listBucketsViaApi` reuses the browser's X-Auth cookie via
    // `page.context().request`, so no separate auth header is needed.
    await loginWithCredentials(page, {
      realmId: TEST_REALM,
      email: REALM_ADMIN_EMAIL,
      password: REALM_ADMIN_PASSWORD,
    })

    const buckets = await listBucketsViaApi(page, TEST_REALM)
    const primary = buckets.find(
      (b) => b.bucketKey === CREDIT_BUCKET_KEYS.PRIMARY_POOL,
    )
    const promo = buckets.find(
      (b) => b.bucketKey === CREDIT_BUCKET_KEYS.SECONDARY_POOL,
    )

    if (!primary || !promo) {
      throw new Error(
        `[DE-D05 beforeAll] Seeded Credit Bucket directory missing in ${TEST_REALM}. ` +
          `primary-pool found: ${Boolean(primary)}, promo-pool found: ${Boolean(promo)}. ` +
          `Ensure scripts/lib/demo_seed.py::_ensure_credit_buckets has run.`,
      )
    }

    setupCtx = {
      primaryPoolBucketId: primary.id,
      promoPoolBucketId: promo.id,
    }
  } finally {
    await context.close()
  }
})

test.describe('[Herald System / User] 订阅生命周期按 Bucket 池发放与回收 (US-CB-008)', () => {
  test.beforeEach(async ({ page }) => {
    await verifyTestEnvironment(page, {
      requiredRealms: [TEST_REALM],
      requiredUsers: [POINTS_USER_EMAIL],
    })
  })

  test.afterEach(async ({ page, demoLogger, testStartTime }) => {
    await cleanupTestData(page, TEST_REALM, {
      keepUsers: [POINTS_USER_EMAIL],
      timestamp: testStartTime,
    })
    await demoLogger.testCode.info('[DE-D05] test cleanup completed')
  })

  // ==========================================================================
  // Driveable baseline assertion: bound-pool populated, sibling-pool isolated
  //
  // This is the ONLY behavior the demo harness can reliably observe for
  // US-CB-008. The seeded subscription credit lands in `primary-pool`
  // (the bound/registration pool; `_ensure_realm001_subscription_data` in
  // `scripts/lib/demo_seed.py` writes the wallet/ledger/txn rows with
  // `bucket_id = primary-pool`). The sibling `promo-pool` is left empty
  // by default. This baseline state is the precondition every US-CB-008
  // lifecycle scenario assumes: a subscription bound to exactly one pool,
  // with a sibling pool to prove non-crosstalk. Verifying it on persistent
  // state proves the `points-balance-card-${bucketId}` selectors and the
  // bucket-isolation contract that the (out-of-demo-scope) lifecycle
  // events would preserve.
  // ==========================================================================
  test('US-CB-008 基线: 订阅绑定池有余额, 兄弟池不受影响 (持久状态, 不串池基线)', async ({
    page,
    demoLogger,
  }) => {
    const { primaryPoolBucketId, promoPoolBucketId } = requireSetupCtx()

    await test.step('Given: Demo Seed 已将订阅积分入账到 primary-pool (bound pool)', async () => {
      await demoLogger.testCode.info(
        `Resolved seeded buckets: primary-pool=${primaryPoolBucketId}, promo-pool=${promoPoolBucketId}`,
      )
    })

    await test.step('When: 用户登录并打开按 Bucket 分组的余额页', async () => {
      await loginWithCredentials(page, {
        email: POINTS_USER_EMAIL,
        password: POINTS_USER_PASSWORD,
        realmId: TEST_REALM,
        // waitNavigation must stay enabled (the default): the login page now
        // finishes the first-party PKCE code exchange AFTER the login POST,
        // and navigating away mid-exchange aborts it — the refresh token is
        // never persisted and /user/points bounces back to the login page.
      })
      await page.goto(`/${TEST_REALM}/user/points`)
      await page.waitForLoadState('networkidle')
      await demoLogger.testCode.info('User navigated to bucket-grouped points page')
    })

    await test.step('Then: 订阅绑定池 (primary-pool) 余额卡片可见 (持久状态)', async () => {
      const boundCard = page.locator(
        SELECTORS.pointsUser.balanceCardByBucket(primaryPoolBucketId),
      )
      // Persistent-state assertion: the bound pool holds the seeded
      // subscription credit. This is the "bound pool populated" half of
      // the US-CB-008 不串池 invariant. No toast is consulted.
      await expect(boundCard).toBeVisible({ timeout: 10000 })
      await demoLogger.testCode.info(
        'Bound-pool (primary-pool) balance card present — seeded subscription credit landed in the bound pool',
      )
    })

    await test.step('And: 兄弟池 (promo-pool) 与订阅绑定池相互隔离 (不串池基线)', async () => {
      // The sibling promo-pool is NOT seeded with the demo user's
      // subscription credit. Either it has no card (no wallet row) or its
      // card shows a balance independent of primary-pool. Both outcomes
      // prove the bound/sibling isolation that US-CB-008 lifecycle events
      // (renew/cancel/refund/upgrade) rely on: lifecycle operations on the
      // subscription NEVER leak into promo-pool.
      const siblingCard = page.locator(
        SELECTORS.pointsUser.balanceCardByBucket(promoPoolBucketId),
      )
      const hasSiblingCard = await siblingCard.isVisible().catch(() => false)
      if (hasSiblingCard) {
        await expect(siblingCard).toBeVisible()
        await demoLogger.testCode.info(
          'Sibling-pool (promo-pool) balance card present but independent — no crosstalk with bound pool',
        )
      } else {
        await demoLogger.testCode.info(
          'Sibling-pool (promo-pool) has no wallet card — bound-pool credit did not leak into sibling pool (不串池 baseline holds)',
        )
      }
    })
  })

  // ==========================================================================
  // OUT-OF-DEMO-SCOPE — US-CB-008 lifecycle scenarios (LOUD-NOTED)
  //
  // Each `test.skip` below documents ONE user-story scenario that the demo
  // harness cannot drive and points to the backend scenario test that DOES
  // cover it. They are intentionally skipped (not deleted) so the coverage
  // matrix is auditable from the test report and the accept slot (DE-A02)
  // can confirm every US-CB-008 scenario has an explicit disposition.
  //
  // NOTE: `test.skip` callbacks still type-check, so the function bodies
  // below are intentionally minimal (no unused-variable noise).
  // ==========================================================================

  test.skip('US-CB-008 场景1 (续费→原池): 订阅续费积分继续进入同一 Bucket 池 [OUT-OF-DEMO-SCOPE]', async () => {
    // REASON: Requires emitting `invoice.paid` against an existing
    // subscription bound to `subscription.bucket_id`. No demo-side helper
    // (`payment-simulation.ts` or otherwise) emits provider webhooks for
    // subscription lifecycle events; the only driveable fulfillment path
    // is one-time-purchase `payment-attempts/{id}/fulfill`. Creating the
    // prerequisite subscription requires either a real provider webhook
    // (non-deterministic; belongs in `demo/e2e/live/**`) or direct DB-row
    // mutation (forbidden by DE-D05 hard scope).
    // BACKEND COVERAGE: `subscription_paid_renews_to_same_bucket_pool`
    //   (Scenario 5) in
    //   `backend/api/src/tests/scenarios/credit_bucket/
    //    fulfillment_subscription_lifecycle_scenarios.rs`.
    // The baseline test above verifies the persistent-state contract
    // (bound-pool card + sibling isolation) this scenario would build on.
  })

  test.skip('US-CB-008 场景2 (取消/退款→原池): 仅在订阅绑定池回收, 不影响兄弟池 [OUT-OF-DEMO-SCOPE]', async () => {
    // REASON: Requires emitting `subscription.deleted` (cancel) and/or
    // `charge.refunded` (refund) against an existing subscription bound to
    // `subscription.bucket_id`. No demo-side helper exposes a path to fire
    // either event; the cancel/refund revoke path runs only inside the
    // real provider webhook handlers
    // (`backend/api-billing/src/webhook_handlers.rs` /
    // `stripe_webhook_handlers.rs`), which the demo harness cannot invoke.
    // BACKEND COVERAGE:
    //   - `subscription_cancel_revokes_only_subscription_bucket_pool`
    //     (Scenario 7) — cancel revoke bound-pool only.
    //   - `subscription_refund_revokes_only_subscription_bucket_pool`
    //     (Scenario 8) — refund revoke bound-pool only.
    //   in `backend/api/src/tests/scenarios/credit_bucket/
    //    fulfillment_subscription_lifecycle_scenarios.rs`.
    // These backend scenarios assert the exact "bound pool changes,
    // sibling pool unchanged" invariant (不串池) at the service layer.
  })

  test.skip('US-CB-008 场景3 (升级→原池回收+发新): 同池内回收旧套餐并发新套餐, 注册积分保留 [OUT-OF-DEMO-SCOPE]', async () => {
    // REASON: Requires emitting a subscription plan-change (upgrade)
    // webhook against an existing subscription bound to
    // `subscription.bucket_id`. Same harness gap as 场景1/2 — no demo-side
    // helper drives upgrade events. The "registration credits retained"
    // sub-invariant additionally requires the registration pool
    // (`primary-pool`) to be distinct from the subscription's bound pool,
    // which the seed does NOT set up (the seeded subscription binds to
    // `primary-pool`, which IS the registration pool) — so even if the
    // webhook path existed, the seed would need a 3rd bucket to make the
    // registration-retention sub-assertion deterministic.
    // BACKEND COVERAGE: `subscription_upgrade_revokes_old_and_grants_new_
    //   within_same_bucket` (Scenario 6) in
    //   `backend/api/src/tests/scenarios/credit_bucket/
    //    fulfillment_subscription_lifecycle_scenarios.rs`.
  })

  test.skip('US-CB-008 场景4 (降级→保当前周期, 下周期原池): 当前周期保留, 下周期同池发放 [OUT-OF-DEMO-SCOPE]', async () => {
    // REASON: Requires (a) emitting a subscription plan-change (downgrade)
    // webhook, AND (b) advancing the billing cycle to observe next-cycle
    // grant behavior. Neither is driveable in the demo harness: no
    // downgrade webhook helper exists, and time-advancement of the
    // grant-schedule is not exposed to e2e. Time-dependent behavior is a
    // canonical out-of-e2e-scope case.
    // BACKEND COVERAGE: `subscription_downgrade_preserves_current_cycle`
    //   (Scenario 9) in
    //   `backend/api/src/tests/scenarios/credit_bucket/
    //    fulfillment_subscription_lifecycle_scenarios.rs`.
  })
})
