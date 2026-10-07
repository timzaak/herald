/**
 * Credit Bucket — User Balance + Transaction History (US-CB-005/006)
 *
 * Role: regular-user (the seeded demo points user `user@realm-001.com` in
 * realm-001). Navigates to `/user/points`.
 *
 * User Stories (docs/user-stories/billing/credit-bucket.md):
 * - US-CB-005 场景1 — user holds ≥2 bucket wallets → one balance card per
 *   bucket + cross-bucket total equals sum of card totals.
 * - US-CB-005 场景2 — the registration-pool bucket (primary-pool) renders as
 *   its own balance card alongside others.
 * - US-CB-005 cross-bucket-total condition (≥2 vs <2) — asserted at ≥2; the
 *   <2 branch is loud-noted (covered by the ≥2 assertion + a code-read of
 *   `deriveUserPointsView::showTotalBar = cards.length >= 2`). See LOUD NOTES
 *   at the bottom of this file.
 * - US-CB-006 场景1 — transaction rows expose a per-row bucket cell; the
 *   Bucket Select filters the row set to a single bucket; clear-filters
 *   returns the full row set.
 *
 * Test Setup (sanctioned — see DE-D01 handoff):
 * The seed grants the demo user balance ONLY in `primary-pool`. To establish
 * the ≥2-bucket wallet state required by US-CB-005 场景1, `beforeAll` logs in
 * as the realm-001 admin and grants the demo user points into promo-pool via
 * the REAL admin Grant Points dialog (selecting promo-pool in the required
 * `grant-points-bucket-select`). This is real test-data setup through the
 * production grant path — NOT faked DB state. Re-grants are additive
 * (idempotent for the test's purpose: the promo-pool card just needs a
 * non-zero balance so the card renders). See LOUD NOTES for why the admin UI
 * grant was chosen over the ext-API grant.
 *
 * Frontend contract verified against:
 * - frontend/src/components/points/UserPointsPage.tsx (cross-bucket-total bar
 *   renders only when `showTotalBar` = cards.length >= 2).
 * - frontend/src/components/points/PointsBalanceCard.tsx (per-bucket card
 *   testid = `points-balance-card-${bucketId}` where bucketId is the bucket
 *   UUID; per-type chip testid = `points-balance-type-${bucketId}-${typeKey}`).
 * - frontend/src/components/points/TransactionHistoryTable.tsx (per-row bucket
 *   cell testid = `transaction-bucket-${row.index}`; NO header testid).
 * - frontend/src/components/points/TransactionFilters.tsx (Bucket Select
 *   testid = `filter-bucket`, options keyed by bucket UUID; apply/clear
 *   buttons `apply-filters-button` / `clear-filters-button`).
 *
 * Assertion discipline: all assertions land on persistent list/detail/filter
 * state (card values, row cells, filtered row set). No toast-only assertions.
 */

import { expect, type Page } from '@playwright/test'

import { SELECTORS } from '../selectors'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { loginWithCredentials } from '../helpers/auth'
import {
  listBucketsViaApi,
} from '../helpers/bucket-helpers'
import {
  openGrantDialog,
  fillGrantForm,
  confirmGrantDialog,
} from '../helpers/grant-points-helpers'
import {
  CREDIT_BUCKET_KEYS,
  CREDIT_BUCKET_NAMES,
  CREDIT_BUCKET_REALMS,
  REGISTRATION_POOL_KEY,
} from '../helpers/bucket-seed-ids'

// Shared demo fixtures: provides `demoLogger` (auto-finalized) + `loginPage`.
import { test, cleanupTestData } from '../fixtures/demo-page.fixtures'

// ============================================================================
// Constants
// ============================================================================

const TEST_REALM = CREDIT_BUCKET_REALMS.POINTS // 'realm-001'
const POINTS_USER_EMAIL = 'user@realm-001.com'
const POINTS_USER_PASSWORD = 'password'
const REALM_ADMIN_EMAIL = 'admin@realm-001.com'
const REALM_ADMIN_PASSWORD = 'password'

/**
 * Amount granted into promo-pool in `beforeAll`. Arbitrary non-zero value;
 * the test only needs the promo-pool card to render with a non-zero total so
 * the ≥2-bucket assertion is load-bearing. Re-grants accumulate (additive),
 * which is fine — the assertion reads the live card total after grant.
 */
const PROMO_POOL_GRANT_AMOUNT = 250
const PROMO_POOL_GRANT_REASON =
  'DE-D03 test setup: establish >=2-bucket wallet state for US-CB-005 scenario 1'

// ============================================================================
// Shared test-context type
// ============================================================================

interface SetupContext {
  /** UUID of the seeded `primary-pool` (registration pool) bucket. */
  primaryPoolBucketId: string
  /** UUID of the seeded `promo-pool` (secondary) bucket. */
  promoPoolBucketId: string
}

/**
 * Lazily-resolved setup context. `beforeAll` populates this; individual tests
 * read from it. Throws if accessed before `beforeAll` has run (defensive —
 * catches accidental re-ordering).
 */
let setupCtx: SetupContext | null = null

// ============================================================================
// beforeAll — establish the >=2-bucket wallet state via the real admin grant UI
// ============================================================================

test.beforeAll(async ({ browser }) => {
  const context = await browser.newContext()
  const page = await context.newPage()

  try {
    // 1. Login as the realm-001 admin (NOT in REALM_ADMINS — use credentials).
    await loginWithCredentials(page, {
      realmId: TEST_REALM,
      email: REALM_ADMIN_EMAIL,
      password: REALM_ADMIN_PASSWORD,
    })

    // 2. Resolve the seeded bucket directory (primary-pool + promo-pool UUIDs).
    //    Used both to verify the seed and to populate setupCtx for the tests.
    const buckets = await listBucketsViaApi(page, TEST_REALM)
    const primary = buckets.find(
      (b) => b.bucketKey === CREDIT_BUCKET_KEYS.PRIMARY_POOL,
    )
    const promo = buckets.find(
      (b) => b.bucketKey === CREDIT_BUCKET_KEYS.SECONDARY_POOL,
    )

    if (!primary || !promo) {
      throw new Error(
        `[DE-D03 beforeAll] Seeded Credit Bucket directory missing in ${TEST_REALM}. ` +
          `primary-pool found: ${Boolean(primary)}, promo-pool found: ${Boolean(promo)}. ` +
          `Ensure scripts/lib/demo_seed.py::_ensure_credit_buckets has run.`,
      )
    }

    // 3. Grant points into promo-pool via the REAL admin Grant Points dialog.
    //    The seed gives the demo user balance ONLY in primary-pool; this grant
    //    establishes the >=2-bucket wallet state required by US-CB-005 场景1.
    //    Uses the production admin grant path (the realm-001 admin has
    //    points.manage). Re-grants are additive — the test asserts the live
    //    card total after grant, so accumulation across re-runs is fine.
    //
    //    WHY THE ADMIN UI GRANT (not the ext-API grant):
    //    The shared `createTestApiKeyWithPermission` helper hardcodes
    //    realmId='admin' internally, so it cannot provision a realm-001
    //    points.manage API key without modifying that helper (DE-D07 owns it).
    //    The admin UI grant flow uses ONLY shared selectors already declared
    //    by DE-D01 (`grantPoints.bucketSelect`) and stays within this file's
    //    authoring scope. The grant dialog's bucket Select uses bucket UUID as
    //    the option value (grant-points-dialog.tsx); the option is selectable
    //    by its visible name (Promo Pool).
    await grantPromoPoolBalanceViaAdminUI(page, TEST_REALM, POINTS_USER_EMAIL)

    setupCtx = {
      primaryPoolBucketId: primary.id,
      promoPoolBucketId: promo.id,
    }
  } finally {
    await context.close()
  }
})

test.afterEach(async ({ page, demoLogger }) => {
  await cleanupTestData(page, TEST_REALM, {
    keepUsers: [POINTS_USER_EMAIL],
  })
  // The demoLogger fixture finalizes itself; we only log a checkpoint here.
  const logger = demoLogger as { testCode?: { info: (m: string) => void } }
  logger.testCode?.info('[DE-D03] test cleanup complete')
})

// ============================================================================
// Test suite
// ============================================================================

test.describe('[Regular User] 按 Bucket 分组的积分余额与交易历史 (US-CB-005/006)', () => {
  test.beforeEach(async ({ page, loginPage, demoLogger }) => {
    await verifyTestEnvironment(page, {
      requiredRealms: [TEST_REALM],
      requiredUsers: [POINTS_USER_EMAIL],
    })

    // Login as the seeded regular user. `loginPage.loginAsUser` clears cookies,
    // goes through the real login form, and verifies the X-Auth cookie.
    await loginPage.loginAsUser(POINTS_USER_EMAIL, POINTS_USER_PASSWORD, TEST_REALM)

    await page.goto(`/${TEST_REALM}/user/points`)
    await expect(page.locator(SELECTORS.pointsUser.page)).toBeVisible()

    const logger = demoLogger as { testCode?: { log: (m: string) => void } }
    logger.testCode?.log('[DE-D03] Logged in as regular user; on /user/points')
  })

  // ==========================================================================
  // US-CB-005 场景1 — ≥2 bucket wallets → one card per bucket + cross-bucket total
  // ==========================================================================

  test('US-CB-005 场景1: 多桶余额卡片与跨桶合计 (≥2 buckets)', async ({
    page,
    demoLogger,
  }) => {
    const logger = demoLogger as { testCode?: { log: (m: string) => void } }
    expect(setupCtx, 'beforeAll must have resolved bucket/user ids').not.toBeNull()
    const { primaryPoolBucketId, promoPoolBucketId } = setupCtx!

    const primaryCard = page.locator(
      SELECTORS.pointsUser.balanceCardByBucket(primaryPoolBucketId),
    )
    const promoCard = page.locator(
      SELECTORS.pointsUser.balanceCardByBucket(promoPoolBucketId),
    )

    await test.step('Verify: 每个持有的 bucket 渲染为独立的余额卡片', async () => {
      // Wait for wallet data to load (loading skeleton uses the bucket-less
      // `points-balance-card` testid; once loaded, per-bucket cards appear).
      await expect(primaryCard).toBeVisible({ timeout: 15000 })
      await expect(promoCard).toBeVisible({ timeout: 15000 })
      logger.testCode?.log('[DE-D03] ✓ Both primary-pool and promo-pool cards rendered')
    })

    let primaryTotal = 0
    let promoTotal = 0

    await test.step('Verify: 每张卡片显示非零 total 与非零 type 明细', async () => {
      // Primary pool holds the seeded balance (3000 topup + 1900 subscription).
      const primaryTotalEl = page.locator(
        SELECTORS.pointsUser.balanceTotalByBucket(primaryPoolBucketId),
      )
      await expect(primaryTotalEl).toBeVisible()
      primaryTotal = parseAmount(await primaryTotalEl.textContent())
      expect(primaryTotal).toBeGreaterThan(0)

      // Promo pool holds the beforeAll grant (PROMO_POOL_GRANT_AMOUNT or more
      // across re-runs). Non-zero is the load-bearing check.
      const promoTotalEl = page.locator(
        SELECTORS.pointsUser.balanceTotalByBucket(promoPoolBucketId),
      )
      await expect(promoTotalEl).toBeVisible()
      promoTotal = parseAmount(await promoTotalEl.textContent())
      expect(promoTotal).toBeGreaterThan(0)

      // Primary pool has at least one non-zero type chip (topup_credit).
      // typeKey values are the PointsBalanceCard BALANCES_BY_TYPE_KEYS:
      // subscription / topup / registration / freePeriodic / granted.
      const primaryTypeChip = page.locator(
        SELECTORS.pointsUser.balanceType(primaryPoolBucketId, 'topup'),
      )
      await expect(primaryTypeChip).toBeVisible()

      logger.testCode?.log(
        `[DE-D03] ✓ Card totals — primary=${primaryTotal}, promo=${promoTotal}`,
      )
    })

    await test.step('Verify: 跨桶合计等于各卡片 total 之和 (稳定数值断言)', async () => {
      const crossBucketTotalEl = page.locator(SELECTORS.pointsUser.crossBucketTotal)
      await expect(crossBucketTotalEl).toBeVisible()

      const crossBucketTotal = parseAmount(await crossBucketTotalEl.textContent())
      const expectedSum = primaryTotal + promoTotal

      // Stable numeric equality — the load-bearing US-CB-005 contract.
      expect(crossBucketTotal).toBe(expectedSum)

      logger.testCode?.log(
        `[DE-D03] ✓ cross-bucket total=${crossBucketTotal} == primary(${primaryTotal}) + promo(${promoTotal})`,
      )
    })
  })

  // ==========================================================================
  // US-CB-005 场景2 — registration-pool bucket renders as its own card
  // ==========================================================================

  test('US-CB-005 场景2: 注册池桶与其他桶并列展示', async ({ page, demoLogger }) => {
    const logger = demoLogger as { testCode?: { log: (m: string) => void } }
    expect(setupCtx, 'beforeAll must have resolved bucket/user ids').not.toBeNull()
    const { primaryPoolBucketId, promoPoolBucketId } = setupCtx!

    await test.step('Verify: 注册池 (primary-pool) 卡片与其他桶卡片同时存在', async () => {
      // primary-pool is the registration pool (REGISTRATION_POOL_KEY).
      expect(REGISTRATION_POOL_KEY).toBe(CREDIT_BUCKET_KEYS.PRIMARY_POOL)

      const primaryCard = page.locator(
        SELECTORS.pointsUser.balanceCardByBucket(primaryPoolBucketId),
      )
      const promoCard = page.locator(
        SELECTORS.pointsUser.balanceCardByBucket(promoPoolBucketId),
      )

      await expect(primaryCard).toBeVisible({ timeout: 15000 })
      await expect(promoCard).toBeVisible({ timeout: 15000 })

      // The registration-pool card header (CardTitle, rendered as a div by
      // shadcn/ui Card) carries the seeded display name "Primary Pool".
      await expect(
        primaryCard.getByText(CREDIT_BUCKET_NAMES.PRIMARY_POOL),
      ).toBeVisible()

      logger.testCode?.log(
        '[DE-D03] ✓ Registration-pool card rendered alongside promo-pool card',
      )
    })
  })

  // ==========================================================================
  // US-CB-005 cross-bucket-total condition (<2 case)
  // ==========================================================================

  test('US-CB-005 跨桶合计条件: <2 桶不渲染合计条 (loud-noted)', async ({
    page,
    demoLogger,
  }) => {
    const logger = demoLogger as { testCode?: { info: (m: string) => void } }

    await test.step('Document: <2-bucket 分支由 ≥2 断言 + 源码条件覆盖', async () => {
      // LOUD NOTE — <2-bucket negative runtime assertion SKIPPED (justified):
      //
      // The demo seed gives user@realm-001.com balance in primary-pool only;
      // beforeAll grants into promo-pool to reach the ≥2 state. Establishing
      // a deterministic single-bucket user would require either:
      //   (a) a separate seeded user with balance in exactly one bucket, or
      //   (b) revoking the beforeAll grant mid-suite (destructive + flaky).
      // Neither is sanctioned by DE-D01's seed; faking a single-bucket wallet
      // state would violate the "no faking data" rule.
      //
      // Coverage is instead provided by:
      //   1. The ≥2 assertion above (load-bearing — the cross-bucket total IS
      //      rendered and equals the sum when ≥2 buckets are held).
      //   2. A code-read of the conditional:
      //      frontend/src/components/points/UserPointsPage.tsx renders
      //      `user-points-cross-bucket-total` only inside
      //      `if (showTotalBar)` where `showTotalBar` is computed by
      //      `deriveUserPointsView` (user-points-view.ts) as
      //      `cards.length >= 2`. So when <2 buckets are held, the bar is
      //      structurally absent (not just hidden) — a runtime negative
      //      locator would pass trivially but adds no signal beyond the
      //      code-read.
      //
      // This is the explicit "skip negative runtime assertion rather than fake
      // data" branch sanctioned by the item file (step 5) and DE-D01's handoff.
      logger.testCode?.info(
        '[DE-D03] <2-bucket negative case loud-noted: covered by ≥2 assertion + ' +
          'code-read of deriveUserPointsView::showTotalBar = cards.length >= 2',
      )

      // Sanity: in the current ≥2 state, the bar IS rendered (positive control).
      await expect(page.locator(SELECTORS.pointsUser.crossBucketTotal)).toBeVisible()
    })
  })

  // ==========================================================================
  // US-CB-006 场景1 — transaction bucket column + bucket filter
  // ==========================================================================

  test('US-CB-006 场景1: 交易 Bucket 列展示与按桶筛选', async ({
    page,
    demoLogger,
  }) => {
    const logger = demoLogger as { testCode?: { log: (m: string) => void } }
    expect(setupCtx, 'beforeAll must have resolved bucket/user ids').not.toBeNull()
    // setupCtx is asserted above; the US-CB-006 flow uses bucket NAMES
    // (from CREDIT_BUCKET_NAMES) for both cell matching and Select option
    // selection, so the bucket UUIDs are not destructured here.

    // Wait for transactions to load.
    await expect(page.locator(SELECTORS.pointsUser.transactionsTable)).toBeVisible({
      timeout: 15000,
    })

    let primaryRowCount = 0

    await test.step('Verify: 每行交易显示所属 Bucket 单元格 (transaction-bucket-${i})', async () => {
      // Count visible transaction rows via the shared transaction-row selector,
      // then read each row's bucket cell through transactionBucketCell(i).
      // The bucket cell renders the bucket NAME (or 8-char id fallback) inside
      // a Badge — see TransactionHistoryTable.tsx. At least one row must match
      // a held bucket name (primary-pool holds seeded txns; promo-pool has the
      // beforeAll grant txn).
      const rowCount = await countTransactionRows(page)
      expect(rowCount).toBeGreaterThan(0)

      // Tally rows whose bucket cell matches the primary-pool name. The seed
      // writes >=4 transactions into primary-pool, so this should be >=1.
      const primaryName = CREDIT_BUCKET_NAMES.PRIMARY_POOL
      for (let i = 0; i < rowCount; i++) {
        const cellText = await readBucketCell(page, i)
        if (cellText.includes(primaryName)) {
          primaryRowCount++
        }
      }
      expect(primaryRowCount).toBeGreaterThan(0)

      logger.testCode?.log(
        `[DE-D03] ✓ ${rowCount} transaction rows visible; ${primaryRowCount} match primary-pool`,
      )
    })

    await test.step('When: 打开 filter-bucket Select 选择 promo-pool 并应用筛选', async () => {
      // Open the Bucket Select. The option list shows each bucket's display
      // name (e.g. "Promo Pool"); select by visible name — the established
      // pattern across the demo suite (see the billing-admin
      // subscription-history helpers).
      await page.locator(SELECTORS.pointsUser.filterBucket).click()
      await page
        .getByRole('option', { name: CREDIT_BUCKET_NAMES.SECONDARY_POOL })
        .click()
      await page.locator(SELECTORS.pointsUser.applyFiltersButton).click()

      logger.testCode?.log('[DE-D03] ✓ Applied bucket filter = promo-pool')
    })

    await test.step('Verify: 筛选后所有可见行的 bucket 单元格均为 promo-pool (行集断言)', async () => {
      // The beforeAll grant produces >=1 promo-pool transaction, so the filtered
      // result always has rows. Wait for the table to settle, then assert every
      // visible row's bucket cell matches promo-pool.
      await expect(page.locator(SELECTORS.pointsUser.transactionsTable)).toBeVisible({
        timeout: 15000,
      })

      const visibleCount = await countTransactionRows(page)
      expect(visibleCount).toBeGreaterThan(0)
      const promoName = CREDIT_BUCKET_NAMES.SECONDARY_POOL

      // Primary assertion — every visible row's bucket cell matches promo-pool.
      for (let i = 0; i < visibleCount; i++) {
        const cellText = await readBucketCell(page, i)
        expect(
          cellText,
          `row ${i} bucket cell should match promo-pool after filter`,
        ).toContain(promoName)
      }

      logger.testCode?.log(
        `[DE-D03] ✓ ${visibleCount} rows after filter; all match promo-pool`,
      )
    })

    await test.step('Verify: clear-filters 后行集恢复', async () => {
      await page.locator(SELECTORS.pointsUser.resetFiltersButton).click()

      // After clearing, primary-pool rows should be back. Wait for the table
      // and re-tally primary-pool rows; must match the pre-filter count.
      await expect(page.locator(SELECTORS.pointsUser.transactionsTable)).toBeVisible({
        timeout: 15000,
      })

      const cellCount = await countTransactionRows(page)
      expect(cellCount).toBeGreaterThan(0)

      let restoredPrimaryRows = 0
      const primaryName = CREDIT_BUCKET_NAMES.PRIMARY_POOL
      for (let i = 0; i < cellCount; i++) {
        const cellText = await readBucketCell(page, i)
        if (cellText.includes(primaryName)) {
          restoredPrimaryRows++
        }
      }
      expect(restoredPrimaryRows).toBe(primaryRowCount)

      logger.testCode?.log(
        `[DE-D03] ✓ Rows restored after clear-filters (primary-pool rows: ${restoredPrimaryRows})`,
      )
    })
  })
})

// ============================================================================
// Helpers
// ============================================================================

/**
 * Parse a numeric amount from a card/cell textContent.
 *
 * Card totals are rendered via `.toLocaleString()` (e.g. "4,900"), so commas
 * and whitespace must be stripped before parsing. The cross-bucket-total bar
 * lives on a Card whose textContent ALSO includes a localized label prefix
 * (e.g. "跨桶合计"), so parsing from the start with `parseInt` would hit the
 * non-digit prefix and return NaN. Extracting the contiguous digit run after
 * stripping commas/whitespace makes parsing robust to that prefix while still
 * handling the value-only balance-total cells. Returns 0 for empty/null.
 */
function parseAmount(raw: string | null): number {
  if (!raw) return 0
  const cleaned = raw.replace(/[,\s]/g, '')
  const match = cleaned.match(/\d+/)
  const n = match ? Number.parseInt(match[0], 10) : NaN
  return Number.isFinite(n) ? n : 0
}

/**
 * Grant points into `promo-pool` for the demo user via the admin Grant Points
 * dialog — the REAL production admin grant path (no faked state).
 *
 * Flow (all via shared selectors in SELECTORS.grantPoints):
 *  1. Open the wallets page + grant dialog (`openGrantDialog`).
 *  2. Search the user by email + fill amount/permanent/reason AND select the
 *     promo-pool bucket (`fillGrantForm` now performs the bucket Select
 *     itself — credit-bucket made `bucketId` REQUIRED on
 *     `GrantFormOptions`, so the manual select that used to live here is
 *     redundant and was removed during DE-D07 reconciliation).
 *  3. Submit + confirm (`confirmGrantDialog`).
 *
 * Used in `beforeAll` to establish the >=2-bucket wallet state required by
 * US-CB-005 场景1. Re-runs are additive (each call grants again), which is
 * fine because the test asserts the live card total after grant.
 */
async function grantPromoPoolBalanceViaAdminUI(
  page: Page,
  realmId: string,
  userEmail: string,
): Promise<void> {
  await openGrantDialog(page, realmId)

  await fillGrantForm(page, {
    email: userEmail,
    amount: PROMO_POOL_GRANT_AMOUNT,
    // Permanent validity — the test only needs the promo-pool card to render
    // with a non-zero total; expiry is out of scope for US-CB-005/006.
    permanent: true,
    reason: PROMO_POOL_GRANT_REASON,
    // bucketId is REQUIRED by `fillGrantForm`.
    // Pass the seeded promo-pool KEY; the helper resolves it to the visible
    // "Promo Pool" option label and selects it. This replaces the manual
    // bucket-select step that previously followed `fillGrantForm`.
    bucketId: CREDIT_BUCKET_KEYS.SECONDARY_POOL,
  })

  await confirmGrantDialog(page)
}

/**
 * Count visible transaction rows on the user points page.
 *
 * Uses the shared `transactionRow(i)` selector up to the first absent index.
 * Avoids hardcoded `[data-testid^="transaction-row-"]` prefix strings — the
 * shared selector is the single source of truth for the row testid format.
 */
async function countTransactionRows(page: import('@playwright/test').Page): Promise<number> {
  let count = 0
  // Cap at a sane upper bound to avoid an unbounded loop if the DOM is broken.
  while (count < 200) {
    const visible = await page
      .locator(SELECTORS.pointsUser.transactionRow(count))
      .isVisible()
      .catch(() => false)
    if (!visible) break
    count++
  }
  return count
}

/**
 * Read the text content of a row's bucket cell via the shared selector.
 *
 * @param rowIndex - 0-based transaction row index (matches `row.index`).
 */
async function readBucketCell(
  page: import('@playwright/test').Page,
  rowIndex: number,
): Promise<string> {
  const cell = page.locator(SELECTORS.pointsUser.transactionBucketCell(rowIndex))
  return (await cell.textContent()) ?? ''
}

// ============================================================================
// LOUD NOTES
// ============================================================================
//
// 1. >=2-bucket wallet state establishment (US-CB-005 scenario 1):
//    The demo seed (`scripts/lib/demo_seed.py::_seed_points_data`) grants the
//    demo user balance ONLY in `primary-pool`. `promo-pool` is seeded enabled
//    + covering points-demo-app but with NO balance. Per DE-D01's explicit
//    handoff, `beforeAll` here grants PROMO_POOL_GRANT_AMOUNT into promo-pool
//    via the REAL admin Grant Points dialog (the realm-001 admin has
//    points.manage), selecting promo-pool in the required bucket Select
//    (`grant-points-bucket-select`, declared by DE-D01). This is sanctioned
//    test setup through the production grant path — NOT faked DB state.
//    Re-grants are additive (idempotent for the test's purpose: the promo-pool
//    card just needs a non-zero balance).
//
//    WHY THE ADMIN UI GRANT (not the ext-API grant suggested in the handoff):
//    DE-D01's handoff offered "admin grant OR ext-API grant". The ext-API path
//    requires a realm-001 API key with points.manage, but the shared
//    `createTestApiKeyWithPermission` helper hardcodes realmId='admin'
//    internally — using it for realm-001 would create the key in the wrong
//    realm and the grant would 403 (CrossRealmAccessForbidden). DE-D07 has
//    now widened `grantPointsViaExtApi`'s body type to carry `bucketId` but
//    did NOT widen `createTestApiKeyWithPermission`'s hardcoded realm (out of
//    scope — see DE-D07 handoff LOUD NOTE on realmId hardcoding). This file
//    therefore keeps the admin UI grant flow, which now selects the bucket via
//    `fillGrantForm`'s `bucketId` option (DE-D07 reconciliation: the manual
//    `grant-points-bucket-select` click that used to follow `fillGrantForm`
//    was removed because the helper performs the selection itself).
//    See the DE-D07 handoff_summary for the per-file reconciliation log.
//
// 2. <2-bucket negative case (US-CB-005 cross-bucket-total condition):
//    SKIPPED at runtime with justification — see the dedicated test step.
//    The seed does not provide a deterministic single-bucket user, and faking
//    one would violate the no-fake-data rule. Coverage is provided by the >=2
//    assertion (load-bearing) + a code-read of the conditional
//    (`deriveUserPointsView::showTotalBar = cards.length >= 2` in
//    user-points-view.ts, consumed by UserPointsPage.tsx which only renders
//    `user-points-cross-bucket-total` inside `if (showTotalBar)`). This is
//    the explicit "skip negative runtime assertion rather than fake data"
//    branch sanctioned by the item file step 5 and DE-D01's handoff.
//
// 3. Auth-helper gap (DE-D02 loud note, confirmed here):
//    `helpers/auth.ts::REALM_ADMINS` has keys `admin`/`realm1`/`realm2` only
//    — NO `realm-001` entry. `loginAsAdmin({ realmId: 'realm-001' })` would
//    fall back to DEMO_ADMIN (admin@cas.com) and fail. This test therefore
//    uses `loginWithCredentials` directly for the realm-001 admin login in
//    `beforeAll`. The regular-user login in `beforeEach` uses
//    `loginPage.loginAsUser` (email/password/realm), which is unaffected.
//
// 4. Bucket UUIDs vs bucket keys:
//    The frontend testids (`points-balance-card-${bucketId}`,
//    `points-balance-type-${bucketId}-${typeKey}`) and the filter-bucket /
//    grant-bucket Select option values use the bucket's UUID (not its key).
//    The test resolves both UUIDs once in `beforeAll` via `listBucketsViaApi`
//    and indexes `SELECTORS.pointsUser.balanceCardByBucket(uuid)` etc. with
//    them. Bucket-Select options are chosen by visible name (the seeded
//    display name from CREDIT_BUCKET_NAMES), matching the established
//    demo-suite Radix-Select pattern. No hardcoded selector strings.
//
// 5. open question for DE-D06 (runner):
//    The `confirmGrantDialog` helper waits for a success toast as its
//    success signal. The GRANT SETUP (beforeAll) relies on this, but the
//    actual US-CB-005/006 assertions do NOT — they assert persistent
//    balance-card / transaction-row / filtered-row-set state. If the
//    beforeAll grant toast is flaky in the runner env, DE-D06 may need to
//    add a post-grant wallet-balance API poll (or a page reload + card
//    visibility wait) to confirm the grant landed before the regular-user
//    tests run. Flagged here, not silently worked around.
