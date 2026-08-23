/**
 * User Purchase Records Demo Tests (formerly "User Subscription Timeline")
 *
 * User Story:
 * - US-BI-009: View Own Purchase Records (Regular User)
 *   The route /user/subscription-history was rewritten in commit 9fb5d023
 *   (2026-06-16, "rework user purchase-records and points surface") from a
 *   subscription-timeline page to the Purchase Records page
 *   (frontend/src/routes/$realmId/user/subscription-history.tsx ->
 *   PurchaseRecordsRoute + PurchaseHistoryList). The URL is unchanged, but the
 *   page now lists the user's point purchases, not subscription events.
 *
 * Migration note (2026-08-22):
 * - Old assertions targeted subscription-timeline testids
 *   (`subscription-timeline` / `subscription-timeline-empty` /
 *   `toggle-event-details-*` / `event-badge-*`) that no longer render on this
 *   page. All assertions now target the Purchase Records contract
 *   (selectors.ts `SELECTORS.purchaseHistory.*`).
 * - The page consumes GET /api/user/bill/purchase/history -> { items, total }.
 *   Demo seed creates NO purchase history for user@realm-001.com
 *   (scripts/lib/demo_seed.py removed `_ensure_purchase_history_demo_data`
 *   intentionally), so a targeted run renders `purchase-history-empty`.
 *   Because other purchase demos in a full-suite run share this seed user and
 *   may complete purchases for them, the history-state assertions branch on
 *   the actually rendered state (empty OR populated) with hard assertions in
 *   both branches — no soft `.catch(() => false)` pass-throughs remain.
 * - Scene 5 (empty state) is merged into Scene 1+2: under the current
 *   contract the seed user's empty history IS the default page state, so a
 *   separate empty-state test would duplicate the same assertions.
 * - Scenes 7-9 (profile page subscription display) are DELETED: the current
 *   /user/profile (frontend/src/routes/$realmId/user/profile.tsx) renders
 *   only account info (email/nickname/status); the SubscriptionInfoCard /
 *   "Subscription Status" section is no longer mounted by any route. Profile
 *   account-info display is covered by regular-user-comprehensive-demo.e2e.ts.
 *
 * UnifiedLogger Usage:
 * - All tests use UnifiedLogger through 'demoLogger' fixture
 * - Logger is automatically initialized and finalized by fixture
 * - Logs are saved to demo/test-results/console-logs/
 *
 * Test Coverage:
 * - Scene 1+2(+5): Purchase Records page renders for the signed-in user and
 *   the history section reflects the user's own purchase data (empty state or
 *   populated list)
 * - Scene 3+4: purchase history data contract via the page's own API
 *   (GET /api/user/bill/purchase/history with the user's Bearer context)
 * - Scene 6: permission isolation — the purchase history API is unreadable
 *   without the user's Bearer credential (401 anonymous vs 200 authenticated)
 *
 * Total tests: 3
 */

import { test, cleanupTestData, expect } from '../fixtures/demo-page.fixtures'
import { loginWithCredentials, createBearerApiContext } from '../helpers/auth'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { SELECTORS } from '../selectors'
import { request, type APIRequestContext, type Page } from '@playwright/test'

const TEST_REALM = 'realm-001'
const TEST_USER_EMAIL = 'user@realm-001.com' // Created by Demo Seed
const TEST_USER_PASSWORD = 'password'

// Route path is unchanged from the old subscription-history page; the page
// itself is session-scoped (realm resolved from the session, not the URL).
const PURCHASE_RECORDS_PATH = '/user/subscription-history'

const PURCHASE_HISTORY_ENDPOINT =
  '/api/user/bill/purchase/history?page=1&page_size=20'

function frontendBaseUrl(): string {
  return process.env.BASE_URL || 'http://localhost:3000'
}

function backendBaseUrl(): string {
  return (
    process.env.API_BASE_URL ||
    process.env.BASE_URL?.replace(/:\d+/, ':8080') ||
    'http://localhost:8080'
  )
}

/**
 * Navigate to the Purchase Records page and wait for its root testid.
 * Replaces the old navigateToSubscriptionDetailHistory helper, which waited
 * on subscription-timeline testids that this page no longer renders.
 */
async function navigateToPurchaseRecords(page: Page): Promise<void> {
  await page.goto(`${frontendBaseUrl()}${PURCHASE_RECORDS_PATH}`, {
    waitUntil: 'domcontentloaded',
  })
  await expect(page.locator(SELECTORS.purchaseHistory.page)).toBeVisible({
    timeout: 15000,
  })
}

test.describe('[Regular User] Purchase Records Demo Tests', () => {
  // Verify test environment before each test
  test.beforeEach(async ({ page }) => {
    await verifyTestEnvironment(page, {
      requiredRealms: [TEST_REALM],
      requiredUsers: [TEST_USER_EMAIL],
    })
  })

  // Single test.afterEach for cleanup
  test.afterEach(async ({ page, testStartTime, demoLogger }) => {
    // ⚠️ MANDATORY: 清理测试数据
    await cleanupTestData(page, TEST_REALM, {
      keepUsers: [TEST_USER_EMAIL],
      timestamp: testStartTime,
    })
    await demoLogger.testCode.info('Test data cleanup completed')
  })

  // ============================================================================
  // User Story US-BI-009: View Own Purchase Records
  // ============================================================================

  test.describe('US-BI-009: View Own Purchase Records', () => {
    // ============================================================================
    // Scenes 1+2+5: Purchase Records page and history state
    // ============================================================================

    test('should view purchase records page with own history state (Scene 1+2+5)', async ({
      page,
      demoLogger,
    }) => {
      await test.step('Given: Demo Seed 用户（无预置购买历史）', async () => {
        // Demo seed intentionally does NOT create purchase history for
        // user@realm-001.com; in a targeted run the page shows the empty
        // state. In a full-suite run other purchase demos may complete
        // purchases for this shared seed user, in which case the populated
        // list must render instead — both outcomes are asserted below.
        await demoLogger.testCode.info(
          'Using demo seed user; purchase history state depends on prior demo runs',
        )
      })

      await test.step('When: 用户登录并访问购买记录页面', async () => {
        await loginWithCredentials(page, {
          email: TEST_USER_EMAIL,
          password: TEST_USER_PASSWORD,
          realmId: TEST_REALM,
          waitNavigation: false,
        })
        await demoLogger.testCode.info('User logged in')

        await navigateToPurchaseRecords(page)
      })

      await test.step('Then: 验证页面基础元素', async () => {
        // Page root (title area is a localized string, so assert the stable
        // page testid rather than a hardcoded English heading).
        await expect(page.locator(SELECTORS.purchaseHistory.page)).toBeVisible()

        // The page's primary CTA links to /user/purchase-points; it renders
        // whenever the realm's points feature is visible (seed realm-001 has
        // pointsVisible=true, verified via GET /api/user/feature-availability).
        await expect(
          page.getByTestId('purchase-records-purchase-points-button'),
        ).toBeVisible()
        await demoLogger.testCode.info('Purchase Records page displayed with purchase-points CTA')
      })

      await test.step('And: 验证购买历史区状态（空态或有数据）', async () => {
        const emptyState = page.locator(SELECTORS.purchaseHistory.empty)
        const historyList = page.locator(SELECTORS.purchaseHistory.list)

        // PurchaseHistoryList always resolves to exactly one terminal state
        // (empty | list); waiting for one of them is itself the assertion
        // that the history section settled instead of staying on the
        // loading skeleton.
        await expect(emptyState.or(historyList).first()).toBeVisible({
          timeout: 15000,
        })

        if (await emptyState.isVisible()) {
          // Targeted-run seed reality: no purchases yet.
          await expect(historyList).toHaveCount(0)
          await demoLogger.testCode.info(
            'Empty purchase history displayed (seed user has no purchases)',
          )
        } else {
          // Populated state (full-suite ordering): each completed purchase
          // renders as a row with a details button.
          await expect(historyList).toBeVisible()
          await expect(
            page.locator('[data-testid^="purchase-history-item-"]').first(),
          ).toBeVisible()
          await expect(
            page.locator('[data-testid^="purchase-history-details-button-"]').first(),
          ).toBeVisible()
          await demoLogger.testCode.info('Purchase history list rendered with purchase rows')
        }
      })
    })

    // ============================================================================
    // Scenes 3+4: Purchase history data contract (API layer)
    // ============================================================================

    test('should return purchase history data contract the page consumes (Scene 3+4)', async ({
      loginPage,
      demoLogger,
    }) => {
      await test.step('When: 以普通用户身份登录并获取 Bearer 上下文', async () => {
        await loginPage.loginAsUser(TEST_USER_EMAIL, TEST_USER_PASSWORD, TEST_REALM)
        await demoLogger.testCode.info('User logged in via API-capable session')
      })

      await test.step('Then: GET purchase history 返回 200 且结构为 {items, total}', async () => {
        const apiContext = await createBearerApiContext(loginPage.getAccessToken())
        try {
          const response = await apiContext.get(
            `${backendBaseUrl()}${PURCHASE_HISTORY_ENDPOINT}`,
          )
          expect(response.status()).toBe(200)

          const body = await response.json()
          // The page feeds `body.items` into PurchaseHistoryList and
          // `body.total` into ListPagination — both fields are load-bearing.
          expect(Array.isArray(body.items)).toBe(true)
          expect(typeof body.total).toBe('number')

          if (body.total === 0) {
            // Targeted-run seed reality: empty items must match total=0, and
            // is what drives the `purchase-history-empty` state in Scene 1+2.
            expect(body.items).toHaveLength(0)
            await demoLogger.testCode.info(
              'Purchase history API returned empty items/total (seed user has no purchases)',
            )
          } else {
            // Populated history: every row the page renders must carry the
            // fields PurchaseHistoryList/PurchaseDetailsDialog display.
            expect(body.items.length).toBeGreaterThan(0)
            const firstItem = body.items[0]
            expect(typeof firstItem.attemptId).toBe('string')
            expect(typeof firstItem.createdAt).toBe('string')
            expect(typeof firstItem.status).toBe('string')
            expect(typeof firstItem.paymentProvider).toBe('string')
            await demoLogger.testCode.info(
              `Purchase history API returned ${body.items.length} row(s) on page 1 (total=${body.total})`,
            )
          }
        } finally {
          await apiContext.dispose()
        }
      })
    })

    // ============================================================================
    // Scene 6: Permission isolation
    // ============================================================================

    test('should enforce permission isolation on purchase history access (Scene 6)', async ({
      loginPage,
      demoLogger,
    }) => {
      await test.step('When: 未认证请求购买历史 API', async () => {
        const anonymousApi = await request.newContext()
        try {
          const response = await anonymousApi.get(
            `${backendBaseUrl()}${PURCHASE_HISTORY_ENDPOINT}`,
          )
          // /api/user/* is wrapped by inject_token_identity
          // (backend/api-base/.../identity_middleware.rs): a request without a
          // Bearer access token is rejected with 401 "missing bearer token".
          expect(response.status()).toBe(401)
          await demoLogger.testCode.info('Anonymous purchase history request rejected with 401')
        } finally {
          await anonymousApi.dispose()
        }
      })

      await test.step('And: 以普通用户 Bearer 请求同一 API', async () => {
        await loginPage.loginAsUser(TEST_USER_EMAIL, TEST_USER_PASSWORD, TEST_REALM)

        const userApi = await createBearerApiContext(loginPage.getAccessToken())
        try {
          const response = await userApi.get(
            `${backendBaseUrl()}${PURCHASE_HISTORY_ENDPOINT}`,
          )
          expect(response.status()).toBe(200)

          // The identity is derived from the Bearer token, so the served
          // history is scoped to the authenticated user — a regular user can
          // only ever read their own purchase records through this endpoint
          // (the same one the Purchase Records page consumes).
          const body = await response.json()
          expect(Array.isArray(body.items)).toBe(true)
          expect(typeof body.total).toBe('number')
          await demoLogger.testCode.info(
            'Authenticated purchase history request returned the own-history contract',
          )
        } finally {
          await userApi.dispose()
        }
      })
    })
  })
})
