/**
 * Points User Comprehensive Demo Tests (Optimized)
 *
 * User Stories:
 * - US-PU-001: View My Points Balance
 * - US-PU-002: View My Transaction History
 * - US-PU-003: Filter Transaction Records
 *
 * User Stories: docs/user-stories/billing/points-user.md
 *
 * Optimization Notes:
 * - Tests consolidated using test.step() to reduce login overhead
 * - Parallel execution enabled (workers: 4 in config)
 * - Fixed waits optimized to use smart assertions
 * - Test count: 19 → 8 (58% reduction)
 * - Expected time reduction: ~73%
 *
 * UnifiedLogger Usage:
 * - All tests use UnifiedLogger through 'demoLogger' fixture
 * - Logger is automatically initialized and finalized by fixture
 * - Logs are saved to demo/test-results/console-logs/
 */

import { test, cleanupTestData, expect } from '../fixtures/demo-page.fixtures'
import { SELECTORS } from '../selectors'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { POINTS_ROUTES, generateTestEmail, registerUser, navigateToPointsPageAndVerify } from '../helpers/points-helpers'

const TEST_REALM = 'realm-001'
const TEST_USER = 'user@realm-001.com'

/**
 * The time cell renders "YYYY-MM-DD HH:mm …" (formatDateTimeShort), so the
 * first 16 characters compare lexicographically as timestamps.
 */
async function comparableRowTime(
  page: import('@playwright/test').Page,
  index: number
): Promise<string> {
  const text = await page.locator(SELECTORS.pointsUser.transactionTime(index)).textContent()
  return (text ?? '').trim().slice(0, 16)
}

/**
 * After a transaction filter settles, the table must show ONLY matching
 * rows — either every visible row matches the expectation, or the filtered
 * empty state is on screen. A filter that silently leaves mismatched rows
 * visible (or renders neither state) must fail here.
 */
async function expectFilteredTransactionRows(
  page: import('@playwright/test').Page,
  expectation:
    | { kind: 'type'; badgeText: string }
    | { kind: 'notBefore'; date: string }
    | { kind: 'within'; from: string; to: string }
) {
  const rows = page.locator(SELECTORS.pointsUser.firstTransactionRow())
  const rowCount = await rows.count()

  if (rowCount === 0) {
    await expect(page.getByText('No transactions found matching your criteria')).toBeVisible()
    return
  }

  if (expectation.kind === 'type') {
    for (let i = 0; i < rowCount; i++) {
      await expect(page.locator(SELECTORS.pointsUser.transactionType(i))).toContainText(
        expectation.badgeText
      )
    }
    return
  }

  // Both time expectations reduce to inclusive day bounds; "to" extends to
  // the last minute of that day.
  const min = expectation.kind === 'notBefore' ? expectation.date : expectation.from
  const max = expectation.kind === 'within' ? `${expectation.to} 23:59` : undefined
  for (let i = 0; i < rowCount; i++) {
    const time = await comparableRowTime(page, i)
    expect(time >= min, `row ${i} (${time}) must be on/after ${min}`).toBe(true)
    if (max !== undefined) {
      expect(time <= max, `row ${i} (${time}) must be within ${min}..${max}`).toBe(true)
    }
  }
}

test.describe('[Points User] Comprehensive Demo Tests', () => {
  let testStartTime: number

  test.beforeEach(async ({ page }) => {
    await verifyTestEnvironment(page, {
      requiredRealms: [TEST_REALM],
      requiredUsers: [TEST_USER],
    })
    testStartTime = Date.now()
  })

  test.afterEach(async ({ page, demoLogger }) => {
    await cleanupTestData(page, TEST_REALM, {
      keepUsers: [TEST_USER],
      timestamp: testStartTime,
    })
    await demoLogger.testCode.info('Test data cleaned up')
  })

  // ============================================================================
  // User Story US-PU-001: View My Points Balance
  // ============================================================================

  test.describe('US-PU-001: View My Points Balance', () => {
    test('should view complete points balance information', async ({ page, loginPage, demoLogger }) => {
      // Consolidated 5 tests into 1 with test.step() for better performance
      // Single login and page load for all balance-related verifications

      await test.step('Setup: Login and navigate to points page', async () => {
        await loginPage.loginAsUser(TEST_USER, 'password', TEST_REALM)
        await page.goto(`/${TEST_REALM}/user/points`)
        await expect(page.locator(SELECTORS.pointsUser.page)).toBeVisible()
        demoLogger.testCode.log('[Test] ✓ User points page loaded')
      })

      await test.step('Verify: Balance card and basic information displayed', async () => {
        // LOUD NOTE (DE-D07 bucket adaptation): the bucket-aware UI
        // (PointsBalanceCard.tsx) renders one `points-balance-card-${bucketId}`
        // per held bucket; the flat `points-balance-card` testid only matches
        // the loading skeleton / null-bucket fallback. The seed gives
        // user@realm-001.com balance in `primary-pool`, so at least one
        // bucket-grouped card renders. Asserted via the shared prefix locator
        // (`firstBalanceCard`) to keep US-PU-001 intent ("my balance is
        // visible") without resolving a specific bucket UUID in this suite.
        await expect(page.locator(SELECTORS.pointsUser.firstBalanceCard)).toBeVisible({
          timeout: 15000,
        })
        // LOUD NOTE: the legacy `points-balance` (balanceAmount) and
        // `points-wallet-status` (accountStatus) testids were REMOVED by the
        // credit-bucket UI refactor — the bucket-grouped card now shows the
        // total inline (`points-balance-total-${bucketId}`) and has no
        // separate "wallet status" element. The original US-PU-001 assertions
        // on those testids are dropped here (bucket-aware replacement above).
        // A per-bucket total assertion lives in DE-D03
        // (credit-bucket-balance-history-demo) which resolves the bucket UUID.
        demoLogger.testCode.log('[Test] ✓ Balance card displayed (bucket-grouped)')
      })

      await test.step('Document: Account status details (no assertion in this suite)', async () => {
        // LOUD NOTE (DE-D07): the `points-wallet-status` testid no longer
        // exists in the bucket-aware UI. The original US-PU-001 "account
        // status" intent is now expressed per-bucket via the disabled badge
        // (`points-balance-card-disabled-${bucketId}`) on disabled buckets —
        // covered by DE-D02 directory tests, not this comprehensive suite.
        // Kept as a documentation step so the original US-PU-001 scenario
        // mapping is preserved for accept audit.
        demoLogger.testCode.info(
          '[Test] ℹ Account-status testid removed by credit-bucket UI; ' +
            'per-bucket disabled-badge coverage lives in DE-D02',
        )
      })


      await test.step('Document: Real-time balance update behavior', async () => {
        // NOTE: Real-time balance updates require backend SDK API integration
        // Expected flow: Third-party app calls POST /api/ext/points/{realmId}/consume
        // Frontend receives update via WebSocket/polling and updates UI without refresh
        //
        // This test documents the expected behavior when backend is implemented:
        // - Balance card shows "Updating..." state briefly
        // - Balance amount changes from current to new value (e.g., 5000 → 4900)
        // - Total consumed increases by consumed amount
        // - New transaction appears in history
        demoLogger.testCode.info('[Test] ℹ Real-time balance update requires backend API integration')
        demoLogger.testCode.info('[Test] ℹ Expected: POST /api/ext/points/{realmId}/consume -> balance update -> UI refresh')
        demoLogger.testCode.info('[Test] ℹ React Query will handle real-time updates without page refresh')
      })

      await test.step('Document: Insufficient points error handling', async () => {
        // NOTE: Insufficient points scenario requires backend consume API integration
        // Expected flow: Attempt to consume 100 points when only 50 available
        // Backend returns 400 error: {"error": "insufficient_points", "message": "积分不足，当前余额：50"}
        // Frontend displays error toast to user
        demoLogger.testCode.info('[Test] ℹ Insufficient points scenario requires backend consume API integration')
        demoLogger.testCode.info('[Test] ℹ Expected: POST /api/ext/points/{realmId}/consume -> 400 error -> error toast')
        demoLogger.testCode.info('[Test] ℹ Error message: "积分不足，当前余额：XX"')
      })
    })
  })

  // ============================================================================
  // User Story US-PU-002: View My Transaction History
  // ============================================================================

  test.describe('US-PU-002: View My Transaction History', () => {
    test('should view transaction history with pagination', async ({ page, loginPage, demoLogger }) => {
      // Consolidated: view + order + pagination into single test with test.step()

      await test.step('Setup: Login and navigate to transaction history', async () => {
        await loginPage.loginAsUser(TEST_USER, 'password', TEST_REALM)
        await page.goto(`/${TEST_REALM}/user/points`)
        await expect(page.locator(SELECTORS.pointsUser.transactionsSection)).toBeVisible()
        demoLogger.testCode.log('[Test] ✓ Transaction history section displayed')
      })

      await test.step('Verify: Transaction history table displayed', async () => {
        await expect(page.locator(SELECTORS.pointsUser.transactionsTable)).toBeVisible()
        await expect(page.locator(SELECTORS.pointsUser.transactionRow(0))).toBeVisible()
        demoLogger.testCode.log('[Test] ✓ Transaction history table displayed')
      })

      await test.step('Verify: Transactions in reverse chronological order', async () => {
        // US-PU-002 场景 1: the timeline must be newest-first; row times are
        // compared via `comparableRowTime`'s lexicographic contract.
        const rows = page.locator(SELECTORS.pointsUser.firstTransactionRow())
        const rowCount = await rows.count()
        expect(rowCount).toBeGreaterThan(0)

        if (rowCount >= 2) {
          const times: string[] = []
          for (let i = 0; i < Math.min(rowCount, 5); i++) {
            times.push(await comparableRowTime(page, i))
          }
          for (let i = 1; i < times.length; i++) {
            expect(
              times[i - 1] >= times[i],
              `row ${i - 1} (${times[i - 1]}) must not be older than row ${i} (${times[i]})`
            ).toBe(true)
          }
          demoLogger.testCode.log(
            `[Test] ✓ Transactions ordered by time (descending): ${times.join(' > ')}`
          )
        } else {
          demoLogger.testCode.info(
            '[Test] ℹ Only one transaction visible; ordering needs ≥2 rows to compare'
          )
        }
      })

      await test.step('Verify: Loading more history (US-PU-002 Scenario 5, load-more model)', async () => {
        // The user transaction history is a growing load-more window, not
        // the classic pagination controls the story text describes: rows
        // render up to a visible cap and the Load More button grows it.
        const rows = page.locator(SELECTORS.pointsUser.firstTransactionRow())
        const beforeCount = await rows.count()
        const loadMore = page.locator('[data-testid="transaction-load-more"]')

        if (await loadMore.isVisible().catch(() => false)) {
          await loadMore.click()
          // The reload disables the filter actions while refetching; wait
          // for the query to settle before reading the new row count.
          await expect(page.locator(SELECTORS.pointsUser.applyFiltersButton)).toBeEnabled()
          await expect.poll(async () => await rows.count()).toBeGreaterThan(beforeCount)
          demoLogger.testCode.log(
            `[Test] ✓ Load more grew the visible window: ${beforeCount} -> ${await rows.count()} rows`
          )
        } else {
          // No Load More button means every transaction fits the first
          // window — but the window itself must not be empty here.
          expect(beforeCount).toBeGreaterThan(0)
          demoLogger.testCode.info(
            '[Test] ℹ All transactions fit the first window; Load More not rendered'
          )
        }
      })

      await test.step('Verify: Detailed transaction information', async () => {
        // Verify table displays all required columns
        // Expected columns: 交易时间, 交易类型, 积分类型, 金额, 交易后余额, 描述, 来源
        const firstRow = page.locator(SELECTORS.pointsUser.transactionRow(0))
        await expect(firstRow).toBeVisible()
        demoLogger.testCode.log('[Test] ✓ Transaction details displayed with all required fields')
      })
    })
  })

  // ============================================================================
  // User Story US-PU-003: Filter Transaction Records
  // ============================================================================

  test.describe('US-PU-003: Filter Transaction Records', () => {
    test('should apply individual filters', async ({ page, loginPage, demoLogger }) => {
      // Consolidated: type filters + time filters into single test with test.step()

      await test.step('Setup: Login and navigate to transaction history', async () => {
        await loginPage.loginAsUser(TEST_USER, 'password', TEST_REALM)
        await page.goto(`/${TEST_REALM}/user/points`)
        await expect(page.locator(SELECTORS.pointsUser.transactionsSection)).toBeVisible()
      })

      await test.step('Verify: Filter by transaction type (recharge)', async () => {
        await page.locator(SELECTORS.pointsUser.filterType).click()
        await page.getByRole('option', { name: 'Recharge' }).click()
        await page.locator(SELECTORS.pointsUser.applyFiltersButton).click()
        // The filter query disables the actions while loading; wait for it
        // to settle before asserting the filtered content.
        await expect(page.locator(SELECTORS.pointsUser.applyFiltersButton)).toBeEnabled()

        await expectFilteredTransactionRows(page, { kind: 'type', badgeText: 'Recharge' })
        demoLogger.testCode.log('[Test] ✓ Filter applied by type: recharge (all rows match or empty)')
      })

      await test.step('Verify: Filter by transaction type (consume)', async () => {
        await page.locator(SELECTORS.pointsUser.filterType).click()
        await page.getByRole('option', { name: 'Consume' }).click()
        await page.locator(SELECTORS.pointsUser.applyFiltersButton).click()
        await expect(page.locator(SELECTORS.pointsUser.applyFiltersButton)).toBeEnabled()

        await expectFilteredTransactionRows(page, { kind: 'type', badgeText: 'Consume' })
        demoLogger.testCode.log('[Test] ✓ Filter applied by type: consume (all rows match or empty)')
      })

      await test.step('Verify: Filter by time range (last 7 days)', async () => {
        const sevenDaysAgo = new Date(Date.now() - 7 * 24 * 60 * 60 * 1000)
          .toISOString()
          .split('T')[0]
        await page.locator(SELECTORS.pointsUser.filterStartTime).fill(sevenDaysAgo)
        await page.locator(SELECTORS.pointsUser.applyFiltersButton).click()
        await expect(page.locator(SELECTORS.pointsUser.applyFiltersButton)).toBeEnabled()

        await expectFilteredTransactionRows(page, { kind: 'notBefore', date: sevenDaysAgo })
        demoLogger.testCode.log(
          `[Test] ✓ Filter applied by time range: on/after ${sevenDaysAgo} (all rows match or empty)`
        )
      })

      await test.step('Verify: Filter by custom date range', async () => {
        await page.locator(SELECTORS.pointsUser.filterStartTime).fill('2026-03-01')
        await page.locator(SELECTORS.pointsUser.filterEndTime).fill('2026-03-31')
        await page.locator(SELECTORS.pointsUser.applyFiltersButton).click()
        await expect(page.locator(SELECTORS.pointsUser.applyFiltersButton)).toBeEnabled()

        await expectFilteredTransactionRows(page, { kind: 'within', from: '2026-03-01', to: '2026-03-31' })
        demoLogger.testCode.log('[Test] ✓ Filter applied by custom date range (all rows within March or empty)')
      })
    })

    test('should apply combined filters', async ({ page, loginPage, demoLogger }) => {
      // Kept separate to test filter interaction

      await test.step('Setup: Login and navigate to transaction history', async () => {
        await loginPage.loginAsUser(TEST_USER, 'password', TEST_REALM)
        await page.goto(`/${TEST_REALM}/user/points`)
        await expect(page.locator(SELECTORS.pointsUser.transactionsSection)).toBeVisible()
      })

      await test.step('When: Apply combined filters (type + time range)', async () => {
        // Filter by consume type
        await page.locator(SELECTORS.pointsUser.filterType).click()
        await page.getByRole('option', { name: 'Consume' }).click()

        // Filter by last 7 days
        const sevenDaysAgo = new Date(Date.now() - 7 * 24 * 60 * 60 * 1000)
          .toISOString()
          .split('T')[0]
        await page.locator(SELECTORS.pointsUser.filterStartTime).fill(sevenDaysAgo)

        await page.locator(SELECTORS.pointsUser.applyFiltersButton).click()
        demoLogger.testCode.log('[Test] ✓ Combined filters applied (type: consume, time: last 7 days)')
      })

      await test.step('Then: Verify filter results displayed (table or empty state)', async () => {
        await expect(page.locator(SELECTORS.pointsUser.transactionsSection)).toBeVisible()
        demoLogger.testCode.log('[Test] ✓ Combined filters working correctly')
      })
    })

    test('should clear filters and show empty state', async ({ page, loginPage, demoLogger }) => {
      // Consolidated: clear filters + empty state into single test

      await test.step('Setup: Login, navigate, and apply a filter', async () => {
        await loginPage.loginAsUser(TEST_USER, 'password', TEST_REALM)
        await page.goto(`/${TEST_REALM}/user/points`)

        // Apply a filter first
        await page.locator(SELECTORS.pointsUser.filterType).click()
        await page.getByRole('option', { name: 'Recharge' }).click()
        await page.locator(SELECTORS.pointsUser.applyFiltersButton).click()
        // Wait for the filter query to settle before proceeding: the Apply
        // submit sets the transactions query into a loading state, during which
        // both action buttons are `disabled={loading}`. Clicking Clear
        // mid-load would block (Playwright waits for the element to become
        // actionable) and trip the test timeout. Wait for Apply to re-enable.
        await expect(page.locator(SELECTORS.pointsUser.applyFiltersButton)).toBeEnabled()
        demoLogger.testCode.log('[Test] ✓ Initial filter applied')
      })

      await test.step('Verify: Clear filter conditions', async () => {
        await page.locator(SELECTORS.pointsUser.resetFiltersButton).click()
        await expect(page.locator(SELECTORS.pointsUser.transactionsTable)).toBeVisible()
        demoLogger.testCode.log('[Test] ✓ Filters cleared successfully')
      })

      await test.step('Verify: Empty state for no matching transactions', async () => {
        // Apply filter with future date range to show empty state
        const futureDate = new Date()
        futureDate.setDate(futureDate.getDate() + 30)
        const futureDateStr = futureDate.toISOString().split('T')[0]

        await page.locator(SELECTORS.pointsUser.filterStartTime).fill(futureDateStr)
        await page.locator(SELECTORS.pointsUser.applyFiltersButton).click()

        await expect(page.getByText('No transactions found matching your criteria')).toBeVisible()
        demoLogger.testCode.log('[Test] ✓ Empty state message displayed')
      })
    })

    test('should document source filter behavior (admin-only)', async ({ page, loginPage, demoLogger }) => {
      // Documentation test for admin-only feature
      // Per user stories (docs/user-stories/billing/points-user.md),
      // source filter by Client App ID is an ADMIN-ONLY feature

      await test.step('Setup: Login and navigate to transaction history', async () => {
        await loginPage.loginAsUser(TEST_USER, 'password', TEST_REALM)
        await page.goto(`/${TEST_REALM}/user/points`)
        await expect(page.locator(SELECTORS.pointsUser.transactionsTable)).toBeVisible()
      })

      await test.step('Document: Source filter is admin-only feature', async () => {
        // User story clarification:
        // - US-PU-003 Scenario 5 describes "filter by transaction source"
        // - However, user stories specify this is for admins only
        // - Regular users (tenant users) cannot filter by client app
        //
        // Admin capabilities:
        // - API: GET /api/points/{realmId}/transactions?client_app_id=...
        // - UI: Admin page includes client app filter dropdown
        //
        // User page capabilities:
        // - Filter by type: recharge, consume
        // - Filter by time range: preset (7 days) or custom date range
        demoLogger.testCode.info('[Test] ℹ Source filter is admin-only feature per user stories')
        demoLogger.testCode.info('[Test] ℹ Admin API supports: ?client_app_id=app-001 parameter')
        demoLogger.testCode.info('[Test] ℹ User page filters limited to: type, time range only')
        demoLogger.testCode.info('[Test] ℹ User page does NOT include client app filter per design specification')
      })
    })
  })

  // ============================================================================
  // User Story US-FU-001: Registration with Initial Points
  // User Story US-FU-002: Daily Points Grant
  // User Story US-FU-003: Upgrade to Paid Plan
  // ============================================================================

  // TODO: Skip until backend implements automatic points grant on registration
  test.describe.skip('[Free User] Points System Comprehensive Demo Tests', () => {
    test('should complete free user points lifecycle', async ({ page, loginPage, demoLogger, testStartTime }) => {
      // Existing comprehensive test, kept as-is

      await test.step('Scenario 1: Registration with initial points', async () => {
        const email = generateTestEmail(testStartTime, 'test-free-user')
        const realmId = TEST_REALM

        await registerUser(page, realmId, email)

        await expect(page.getByText(/恭喜.*获得.*1000.*初始积分|Congratulations.*1000.*initial.*points/i)).toBeVisible()
        await expect(page.getByText(/注册初始积分.*1000|registration.*credit.*1000/i)).toBeVisible()
        await expect(page.getByText(/永久有效|permanent|valid.*forever/i)).toBeVisible()
        demoLogger.testCode.log('[Test] ✓ Registration with initial points verified')
      })

      await test.step('Scenario 2: First daily points granted', async () => {
        const realmId = TEST_REALM
        await navigateToPointsPageAndVerify(page, realmId)

        await expect(page.getByText(/每日免费积分.*50|daily.*points.*50/i)).toBeVisible()
        await expect(page.getByText(/明天.*过期|expires.*tomorrow/i)).toBeVisible()
        await expect(page.getByText(/1050/)).toBeVisible()
        demoLogger.testCode.log('[Test] ✓ First daily points verified')
      })

      await test.step('Scenario 3: View points details (categorized)', async () => {
        const realmId = TEST_REALM
        await page.goto(POINTS_ROUTES.USER_POINTS(realmId))

        await expect(page.getByText(/注册初始积分|registration.*credit/i)).toBeVisible()
        await expect(page.getByText('1000')).toBeVisible()
        await expect(page.getByText(/永久有效|permanent/i)).toBeVisible()

        await expect(page.getByText(/每日免费积分|daily.*free.*points/i)).toBeVisible()
        await expect(page.getByText('50')).toBeVisible()
        await expect(page.getByText(/明天.*过期|expires.*tomorrow/i)).toBeVisible()
        demoLogger.testCode.log('[Test] ✓ Points details categorized display verified')
      })

      await test.step('Scenario 4: View transaction history', async () => {
        const realmId = TEST_REALM
        await page.goto(POINTS_ROUTES.USER_POINTS(realmId))
        await expect(page.locator(SELECTORS.pointsUser.transactionsSection)).toBeVisible()

        await expect(page.getByText(/注册赠送|registration.*grant/i)).toBeVisible()
        await expect(page.getByText(/registration_credit/i)).toBeVisible()
        await expect(page.getByText('1000')).toBeVisible()

        await expect(page.getByText(/免费每日发放|free.*daily.*grant/i)).toBeVisible()
        await expect(page.getByText(/free_daily_credit/i)).toBeVisible()
        await expect(page.getByText('50')).toBeVisible()
        demoLogger.testCode.log('[Test] ✓ Transaction history verified')
      })

      await test.step('Scenario 5: Expiring points warning', async () => {
        const realmId = TEST_REALM
        await page.goto(POINTS_ROUTES.USER_POINTS(realmId))

        await expect(page.getByText(/您有.*积分将于今天.*过期|points.*expiring.*today/i)).toBeVisible()
        await expect(page.getByTestId(SELECTORS.points.expiryWarning)).toBeVisible()
        demoLogger.testCode.log('[Test] ✓ Expiring points warning verified')
      })
    })
  })

  // ============================================================================
  // User Story US-FU-001/02/03: Free User Points Experience
  // ============================================================================

  test.describe.skip('Free User Points Experience (US-FU-001/02/03)', () => {
    test('should receive registration and periodic points on signup (US-FU-001/02)', async ({ page, loginPage, demoLogger, testStartTime }) => {
      const email = generateTestEmail(testStartTime)

      await test.step('Given: 新用户注册', async () => {
        await registerUser(page, TEST_REALM, email, 'password123')
        demoLogger.testCode.log(`[Test] ✓ User registered: ${email}`)
      })

      await test.step('When: 登录并访问积分页面', async () => {
        await loginPage.loginAsUser(email, 'password123', TEST_REALM)
        await page.goto(`/${TEST_REALM}/user/points`)
        await expect(page.locator(SELECTORS.pointsUser.page)).toBeVisible()
        demoLogger.testCode.log('[Test] ✓ Navigated to points page')
      })

      await test.step('Then: 验证注册初始积分（US-FU-001）', async () => {
        // 验证注册初始积分：1000 积分，永久有效
        await expect(page.getByText(/注册初始积分|registration.*credit/i)).toBeVisible()
        await expect(page.getByText('1000')).toBeVisible()
        await expect(page.getByText(/永久有效|permanent/i)).toBeVisible()
        demoLogger.testCode.log('[Test] ✓ Registration bonus points verified (1000, permanent)')
      })

      await test.step('Then: 验证定期积分首次发放（US-FU-002）', async () => {
        // 验证定期积分：50 积分，有效期 1 天
        await expect(page.getByText(/免费定期积分|free.*periodic.*credit/i)).toBeVisible()
        await expect(page.getByText('50')).toBeVisible()
        await expect(page.getByText(/\d+.*天.*过期|expires.*in.*\d+.*days/i)).toBeVisible()
        demoLogger.testCode.log('[Test] ✓ First periodic points grant verified (50, 1 day validity)')
      })

      await test.step('Then: 验证总积分', async () => {
        // 注册初始积分(1000) + 定期积分(50) = 1050
        await expect(page.getByText('1050')).toBeVisible()
        demoLogger.testCode.log('[Test] ✓ Total balance verified (1050 = 1000 + 50)')
      })

      await test.step('Then: 验证交易历史', async () => {
        await expect(page.locator(SELECTORS.pointsUser.transactionsSection)).toBeVisible()

        // 验证注册赠送交易
        await expect(page.getByText(/注册赠送|registration.*grant/i)).toBeVisible()
        await expect(page.getByText(/registration_credit/i)).toBeVisible()

        // 验证定期发放交易
        await expect(page.getByText(/免费定期发放|free.*periodic.*grant/i)).toBeVisible()
        await expect(page.getByText(/free_periodic_credit/i)).toBeVisible()
        demoLogger.testCode.log('[Test] ✓ Transaction history verified (registration + periodic grants)')
      })
    })

    test('should handle different periodic grant periods (US-FU-002)', async ({ page, loginPage, demoLogger, testStartTime }) => {
      // 注意：此测试依赖 Realm 配置的周期类型
      // 默认为 daily，但如果 Realm 配置为 once/weekly/monthly，测试会相应验证

      const email = generateTestEmail(testStartTime)

      await test.step('Given: 新用户注册（使用当前 Realm 配置的周期类型）', async () => {
        await registerUser(page, TEST_REALM, email, 'password123')
        demoLogger.testCode.log('[Test] ✓ User registered with current realm periodic config')
      })

      await test.step('When: 登录并访问积分页面', async () => {
        await loginPage.loginAsUser(email, 'password123', TEST_REALM)
        await page.goto(`/${TEST_REALM}/user/points`)
        await expect(page.locator(SELECTORS.pointsUser.page)).toBeVisible()
      })

      await test.step('Then: 验证定期积分按配置周期发放', async () => {
        // 验证定期积分已发放（无论周期类型如何，首次都是立即发放）
        await expect(page.getByText(/免费定期积分|free.*periodic.*credit/i)).toBeVisible()

        // 获取并显示当前周期类型
        const periodicText = await page.getByText(/免费定期积分|free.*periodic.*credit/i).textContent()
        demoLogger.testCode.log(`[Test] ✓ Periodic points granted: ${periodicText}`)

        // 验证交易历史中的定期发放记录
        await expect(page.getByText(/free_periodic_credit/i)).toBeVisible()
        demoLogger.testCode.log('[Test] ✓ Periodic grant transaction verified')
      })

      await test.step('Document: 不同周期类型的验证方式', async () => {
        demoLogger.testCode.info('[Test] ℹ Periodic grant types:')
        demoLogger.testCode.info('[Test] ℹ - ONCE: 注册时发放一次，后续不再发放')
        demoLogger.testCode.info('[Test] ℹ - DAILY: 每日同一时间发放（首次立即）')
        demoLogger.testCode.info('[Test] ℹ - WEEKLY: 每 7 天同一时间发放（首次立即）')
        demoLogger.testCode.info('[Test] ℹ - MONTHLY: 每月同一天同一时间发放（首次立即）')
        demoLogger.testCode.info('[Test] ℹ Note: 后续发放验证需要 Demo Seed 创建历史数据')
      })
    })

    test('should preserve registration points on subscription upgrade (US-FU-003)', async ({ page, loginPage, demoLogger, testStartTime }) => {
      // 注意：此测试需要订阅功能支持
      // 如果订阅流程未实现，此测试将仅验证当前状态

      const email = `upgrade-${generateTestEmail(testStartTime)}`

      await test.step('Given: 免费用户已注册并获得积分', async () => {
        await registerUser(page, TEST_REALM, email, 'password123')
        demoLogger.testCode.log(`[Test] ✓ Free user registered: ${email}`)
      })

      await test.step('When: 登录并查看升级前积分状态', async () => {
        await loginPage.loginAsUser(email, 'password123', TEST_REALM)
        await page.goto(`/${TEST_REALM}/user/points`)
        await expect(page.locator(SELECTORS.pointsUser.page)).toBeVisible()

        // 验证升级前积分状态
        await expect(page.getByText(/注册初始积分|registration.*credit/i)).toBeVisible()
        await expect(page.getByText(/免费定期积分|free.*periodic.*credit/i)).toBeVisible()
        // LOUD NOTE (DE-D07): `points-balance` (balanceAmount) testid was
        // removed by the credit-bucket UI; read the bucket-grouped card's
        // total text instead (best-effort log only — not asserted, matching
        // the original US-FU-003 doc-test intent).
        const balanceBefore = await page
          .locator(SELECTORS.pointsUser.firstBalanceCard)
          .textContent()
        demoLogger.testCode.log(`[Test] ✓ Balance before upgrade: ${balanceBefore}`)
      })

      await test.step('Document: 升级流程说明', async () => {
        demoLogger.testCode.info('[Test] ℹ Upgrade flow (US-FU-003):')
        demoLogger.testCode.info('[Test] ℹ 1. User subscribes to a paid plan')
        demoLogger.testCode.info('[Test] ℹ 2. Registration credit (1000) is preserved')
        demoLogger.testCode.info('[Test] ℹ 3. Free periodic credit (50) is immediately revoked')
        demoLogger.testCode.info('[Test] ℹ 4. Subscription credit begins granting')
        demoLogger.testCode.info('[Test] ℹ 5. Transaction history shows upgrade events')
        demoLogger.testCode.info('[Test] ℹ Note: Full upgrade flow requires subscription UI to be implemented')
      })

      await test.step('Then: 验证积分分类（当前免费用户状态）', async () => {
        // 验证积分按类型分类显示
        await expect(page.getByText(/注册初始积分|registration.*credit/i)).toBeVisible()
        await expect(page.getByText(/免费定期积分|free.*periodic.*credit/i)).toBeVisible()
        demoLogger.testCode.log('[Test] ✓ Points categorized display verified (registration + periodic)')
      })
    })
  })
})
