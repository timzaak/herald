/**
 * Mixed Consumption Quota Demo Tests (DE-D05)
 *
 * Role: regular-user
 * Route: /{realmId}/user/points (with external API consume)
 *
 * User Story:
 * - US-PU-010 (docs/user-stories/billing/points-user.md) — 滚动窗口额度与充值余额的可用性体验
 *
 * Quota seeding: the window rows asserted here come from a direct internal-API
 * `grantQuotaEntitlement` (revoke-then-grant per test), NOT from a purchase —
 * the purchase→fulfill path never yields quota windows.
 * Mirrors `points-quota-dashboard-demo.e2e.ts` (DE-D02).
 *
 * Two empirically-derived setup constraints (verified against the demo DB
 * after the final3 run):
 *
 * 1. SINGLE window only. The consume engine's window capacity is derived from
 *    the seeded windows; extra (week/month) windows change what the engine can
 *    still draw on, decoupling it from the single 5h row this suite asserts
 *    against. Seeding exactly one 5h/500 window pins engine window capacity
 *    to the visible row.
 * 2. FRESH user per run. Window `used` is a rolling SUM of consume
 *    transactions inside the window look-back (re-granting resets limits, NOT
 *    usage history), so a long-lived demo user accumulates usage across runs
 *    and pre-exhausts the 5h window. A per-run throwaway user starts every
 *    scenario from a clean 500/500 window.
 */

import { expect, type Page } from '@playwright/test'

import { test, cleanupTestData } from '../fixtures/demo-page.fixtures'
import { SELECTORS } from '../selectors'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { createBearerApiContext } from '../helpers/auth'
import { listBucketsViaApi } from '../helpers/bucket-helpers'
import {
  createTestApiKeyWithPermission,
  grantPointsViaExtApi,
  type ApiKeyWithPermission,
} from '../helpers/grant-points-helpers'
import {
  grantQuotaEntitlement,
  revokeQuotaEntitlement,
} from '../helpers/quota-entitlement-helpers'
import {
  consumePointsViaExtApi,
  getWindowRemaining,
} from '../helpers/points-quota-helpers'
import { registerUser } from '../helpers/points-helpers'
import {
  QUOTA_DEMO_REALM,
  QUOTA_DEMO_USER_EMAIL,
  QUOTA_DEMO_PASSWORD,
  QUOTA_DEMO_ADMIN_EMAIL,
  type QuotaWindowFixture,
} from '../fixtures/points-quota.fixtures'

// ============================================================================
// Constants
// ============================================================================

const TEST_REALM = QUOTA_DEMO_REALM
/** Seeded user only used for environment verification; scenarios run as a per-run throwaway user. */
const SEED_USER_EMAIL = QUOTA_DEMO_USER_EMAIL
const ADMIN_EMAIL = QUOTA_DEMO_ADMIN_EMAIL
const ADMIN_PASSWORD = QUOTA_DEMO_PASSWORD
const USER_PASSWORD = QUOTA_DEMO_PASSWORD
const TOPUP_GRANT_AMOUNT = 1_000

const SMALLEST_WINDOW_KEY = '5h'

/**
 * The only quota window this suite seeds: a single 5h/500 window (US-PU-010's
 * example). One window means the engine's window capacity IS the visible 5h
 * row — the "overspend to top-up" and "insufficient → reject wholesale"
 * scenarios then observe exactly the window+pool split they assert on.
 */
const MIXED_CONSUME_QUOTA_WINDOWS: QuotaWindowFixture[] = [
  { windowSeconds: 5 * 60 * 60, limit: 500, key: '5h' },
]

/**
 * Stable anchor for the internal quota-entitlement grant. Distinct from
 * DE-D02's `demo-quota-dashboard` so the two files never revoke each other's
 * baseline; each file re-seeds its own grant in beforeEach anyway.
 */
const QUOTA_SOURCE_ID = 'demo-quota-mixed-consume'

// ============================================================================
// Shared setup context
// ============================================================================

interface SetupContext {
  bucketId: string
  userId: string
  /** Per-run throwaway user the scenarios log in as. */
  userEmail: string
  clientAppId: string
  apiKey: ApiKeyWithPermission
}

let setupCtx: SetupContext | null = null
let setupStartTime = 0

// ============================================================================
// Helpers
// ============================================================================

function assertSetup(): SetupContext {
  expect(setupCtx, 'beforeAll must have resolved setup context').not.toBeNull()
  return setupCtx!
}

function backendBaseUrl(): string {
  return (
    process.env.API_BASE_URL ||
    process.env.BASE_URL?.replace(/:\d+/, ':8080') ||
    'http://localhost:8080'
  )
}

/**
 * Seed a clean window-quota entitlement for the scenario user via the
 * internal API (revoke-then-grant), mirroring DE-D02.
 */
async function seedQuotaEntitlement(page: Page): Promise<void> {
  const { userId, bucketId } = assertSetup()
  const request = page.context().request

  // Clean baseline: revoke any prior active entitlement under this source.
  const revokeResult = await revokeQuotaEntitlement(request, TEST_REALM, {
    userId,
    bucketId,
    sourceId: QUOTA_SOURCE_ID,
  })
  expect(
    revokeResult.success,
    `quota revoke failed: ${revokeResult.error ?? ''}`,
  ).toBeTruthy()

  const grantResult = await grantQuotaEntitlement(request, TEST_REALM, {
    userId,
    bucketId,
    sourceId: QUOTA_SOURCE_ID,
    windows: MIXED_CONSUME_QUOTA_WINDOWS,
  })
  expect(
    grantResult.success,
    `quota grant failed: ${grantResult.error ?? ''}`,
  ).toBeTruthy()
}

async function consumeForTest(amount: number): Promise<{
  status: number
  body: unknown
}> {
  const { apiKey, userId, clientAppId } = assertSetup()
  return consumePointsViaExtApi(apiKey.apiKey, TEST_REALM, {
    userId,
    amount,
    clientAppId,
    description: 'DE-D05 mixed consume',
    idempotencyKey: `de-d05-${setupStartTime}-${Date.now()}`,
  })
}

/**
 * Read the top-up (pool) balance the way the CONSUME ENGINE sees it: the sum
 * of every credit-type balance across ALL of the user's wallets.
 *
 * Why not the primary card's `bucketTotal - 5h remaining`: the engine's pool
 * availability is the sum of active ledgers over covered buckets across ALL
 * credit types, drawn earliest-expiry-first. A card-derived read misses both
 * dimensions — `spendableFromPool` excludes subscription-type ledgers, and a
 * single card excludes other buckets — so an overflow draw can land in a
 * ledger the card read never reflects (observed in the final3 run: the
 * overspend drew from a subscription ledger and the card pool did not move).
 * This read matches the engine's deductions exactly, which is what the
 * before/after delta assertions below require.
 */
async function readTopUpBalance(accessToken: string): Promise<number> {
  const userApi = await createBearerApiContext(accessToken)
  try {
    const response = await userApi.get(`${backendBaseUrl()}/api/user/wallets`)
    expect(response.ok(), 'GET /api/user/wallets for pool read failed').toBe(true)
    const body = await response.json()
    const items = ((body?.items ?? []) as {
      balancesByType?: Record<string, number | null>
    }[])
    return items.reduce((sum, item) => {
      const balances = item.balancesByType ?? {}
      return (
        sum +
        (balances.topup ?? 0) +
        (balances.subscription ?? 0) +
        (balances.registration ?? 0) +
        (balances.freePeriodic ?? 0) +
        (balances.granted ?? 0)
      )
    }, 0)
  } finally {
    await userApi.dispose().catch(() => {})
  }
}

/** Bucket-scoped dashboard card locator (a user may hold several bucket cards). */
function dashboardCard(page: Page) {
  const { bucketId } = assertSetup()
  return page.locator(`[data-testid="points-usage-dashboard-${bucketId}"]`)
}

// ============================================================================
// beforeAll — throwaway user, resolve ids, mint API key, seed top-up balance
// ============================================================================

test.beforeAll(async ({ browser }) => {
  setupStartTime = Date.now()

  const context = await browser.newContext()
  const page = await context.newPage()

  try {
    // Fresh user per run: window `used` is a rolling sum of consume
    // transactions (re-granting resets limits, not history), so reusing the
    // long-lived demo user would let prior runs' consumes pre-exhaust the 5h
    // window. Registration rules grant a small starting balance; every
    // scenario reads balances live, so the exact seed amount never matters.
    const userEmail = `de-d05-${setupStartTime}@realm-001.com`
    await registerUser(page, TEST_REALM, userEmail, USER_PASSWORD)

    const { LoginPage } = await import('../pages/login-page')
    const loginPage = new LoginPage(page)
    await loginPage.loginAsAdmin(ADMIN_EMAIL, ADMIN_PASSWORD, TEST_REALM)

    const buckets = await listBucketsViaApi(page, TEST_REALM)
    const primary = buckets.find((b) => b.bucketKey === 'primary-pool')
    if (!primary) {
      throw new Error(`[DE-D05 beforeAll] primary-pool bucket not found in ${TEST_REALM}`)
    }

    // `context.request` carries only cookies, not the in-memory Bearer access
    // token (auth-rewrite); admin user/client GETs 401 without the Bearer
    // header, so route them through a Bearer context.
    const adminApi = await createBearerApiContext(loginPage.getAccessToken())
    try {
      const usersResponse = await adminApi.get(
        `${backendBaseUrl()}/api/users/${TEST_REALM}?search=${encodeURIComponent(userEmail)}`,
      )
      let userId = ''
      if (usersResponse.ok()) {
        const usersBody = await usersResponse.json()
        const items = (usersBody?.items ?? []) as { id: string; email: string }[]
        const demoUser = items.find((u) => u.email === userEmail)
        userId = demoUser?.id ?? ''
      }
      if (!userId) {
        throw new Error(`[DE-D05 beforeAll] Could not resolve scenario user UUID for ${userEmail}`)
      }

      const clientAppResponse = await adminApi.get(
        `${backendBaseUrl()}/api/client/${TEST_REALM}`,
      )
      let clientAppId = ''
      if (clientAppResponse.ok()) {
        const clientAppBody = await clientAppResponse.json()
        const items = (clientAppBody?.items ?? []) as { id: string; clientId: string }[]
        const demoApp = items.find((a) => a.clientId === 'points-demo-app')
        clientAppId = demoApp?.id ?? ''
      }
      if (!clientAppId) {
        throw new Error('[DE-D05 beforeAll] Could not resolve client app UUID for points-demo-app')
      }

      // The API-key creation endpoints are also Bearer-only, so route them
      // through the same admin Bearer context instead of the cookie-only default.
      const apiKey = await createTestApiKeyWithPermission(
        page,
        'points.manage',
        setupStartTime,
        TEST_REALM,
        clientAppId,
        adminApi,
      )

      const grant = await grantPointsViaExtApi(apiKey.apiKey, TEST_REALM, {
        userId,
        amount: TOPUP_GRANT_AMOUNT,
        bucketId: primary.id,
        reason: 'DE-D05 setup: deterministic top-up balance for mixed consume',
        validityDays: 365,
      })
      if (grant.status !== 200) {
        throw new Error(
          `[DE-D05 beforeAll] Top-up grant failed: status=${grant.status} body=${JSON.stringify(grant.responseBody)}`,
        )
      }

      setupCtx = {
        bucketId: primary.id,
        userId,
        userEmail,
        clientAppId,
        apiKey,
      }
    } finally {
      await adminApi.dispose().catch(() => {})
    }
  } finally {
    await context.close()
  }
})

test.afterEach(async ({ page }) => {
  // Revoke the seeded quota entitlement so no window-quota leaks across demo
  // files (the scenario user is throwaway, but the revoke keeps the grant
  // source tidy for repeated runs).
  const ctx = setupCtx
  if (ctx) {
    await revokeQuotaEntitlement(page.context().request, TEST_REALM, {
      userId: ctx.userId,
      bucketId: ctx.bucketId,
      sourceId: QUOTA_SOURCE_ID,
    }).catch(() => {
      /* best-effort; cleanupTestData still runs below */
    })
  }
  await cleanupTestData(page, TEST_REALM, {
    keepUsers: [SEED_USER_EMAIL],
  })
})

// ============================================================================
// Test suite
// ============================================================================

test.describe('[Regular User] 混合消费协调 (US-PU-010)', () => {
  test.beforeEach(async ({ page, loginPage }) => {
    const { userEmail } = assertSetup()
    await verifyTestEnvironment(page, {
      requiredRealms: [TEST_REALM],
      requiredUsers: [SEED_USER_EMAIL],
    })

    await loginPage.loginAsUser(userEmail, USER_PASSWORD, TEST_REALM)
    // Re-seed a fresh window-quota entitlement for each test so consume
    // scenarios start from a known baseline (revoke-then-grant).
    await seedQuotaEntitlement(page)

    await page.goto(`/${TEST_REALM}/user/points`)
    // The scenario user may hold multiple buckets (registration rules grant
    // across buckets), so wait for the TARGET bucket's card — the in-block
    // assertions all key off this bucket.
    await expect(dashboardCard(page)).toBeVisible({ timeout: 15000 })
  })

  test('US-PU-010 场景3: 窗口额度优先，充值余额不变', async ({
    page,
    loginPage,
  }) => {
    const { bucketId } = assertSetup()

    const windowBefore = await getWindowRemaining(page, bucketId, SMALLEST_WINDOW_KEY)
    const topUpBefore = await readTopUpBalance(loginPage.getAccessToken())

    const consumeAmount = Math.min(50, windowBefore - 1)
    const result = await consumeForTest(consumeAmount)
    expect(result.status).toBe(200)

    await page.reload()
    await expect(dashboardCard(page)).toBeVisible({ timeout: 15000 })

    const windowAfter = await getWindowRemaining(page, bucketId, SMALLEST_WINDOW_KEY)
    const topUpAfter = await readTopUpBalance(loginPage.getAccessToken())

    expect(windowAfter).toBe(windowBefore - consumeAmount)
    expect(topUpAfter).toBe(topUpBefore)
  })

  test('US-PU-010 场景3: 超额部分自动转充值余额', async ({
    page,
    loginPage,
  }) => {
    const { bucketId } = assertSetup()

    const windowBefore = await getWindowRemaining(page, bucketId, SMALLEST_WINDOW_KEY)
    const topUpBefore = await readTopUpBalance(loginPage.getAccessToken())
    const consumeAmount = windowBefore + 200

    const result = await consumeForTest(consumeAmount)
    expect(result.status).toBe(200)

    await page.reload()
    await expect(dashboardCard(page)).toBeVisible({ timeout: 15000 })

    const windowAfter = await getWindowRemaining(page, bucketId, SMALLEST_WINDOW_KEY)
    const topUpAfter = await readTopUpBalance(loginPage.getAccessToken())

    expect(windowAfter).toBe(0)
    expect(topUpAfter).toBe(topUpBefore - (consumeAmount - windowBefore))

    await expect(
      page.locator(SELECTORS.pointsUsageDashboard.overspendTopupAlert),
    ).toBeVisible()
  })

  test('US-PU-010 场景4: 窗口+充值合计不足时整体拒绝', async ({
    page,
    loginPage,
  }) => {
    const { bucketId } = assertSetup()

    const windowBefore = await getWindowRemaining(page, bucketId, SMALLEST_WINDOW_KEY)
    const topUpBefore = await readTopUpBalance(loginPage.getAccessToken())
    const consumeAmount = windowBefore + topUpBefore + 1_000

    const result = await consumeForTest(consumeAmount)
    // The rejection notice the user sees for an API-driven consume is the ext
    // API's error contract — 409 `insufficient_points` with have/need — NOT a
    // dashboard alert: the consume goes through the external API (the page
    // never learns about it), and the dashboard's `points-insufficient-alert`
    // only renders for a DRAINED wallet (`bucketTotal <= 0` with no exhausted
    // window), which a wholesale-rejected consume (state unchanged, window +
    // pool both still positive) can never produce.
    expect(result.status).toBe(409)
    const body = (result.body ?? {}) as { code?: string; have?: number; need?: number }
    expect(body.code).toBe('insufficient_points')
    expect(body.need).toBeGreaterThan(body.have ?? 0)

    await page.reload()
    await expect(dashboardCard(page)).toBeVisible({ timeout: 15000 })

    const windowAfter = await getWindowRemaining(page, bucketId, SMALLEST_WINDOW_KEY)
    const topUpAfter = await readTopUpBalance(loginPage.getAccessToken())

    // Wholesale rejection: nothing was partially deducted.
    expect(windowAfter).toBe(windowBefore)
    expect(topUpAfter).toBe(topUpBefore)
  })

  test('US-PU-010 场景4: 并发消费不会超额', async ({ page, loginPage }) => {
    const { bucketId } = assertSetup()

    const windowBefore = await getWindowRemaining(page, bucketId, SMALLEST_WINDOW_KEY)
    const topUpBefore = await readTopUpBalance(loginPage.getAccessToken())
    const totalAvailable = windowBefore + topUpBefore

    // Two concurrent requests whose sum exceeds total available.
    const amount1 = Math.ceil(totalAvailable * 0.6)
    const amount2 = Math.ceil(totalAvailable * 0.6)

    const [result1, result2] = await Promise.all([
      consumeForTest(amount1),
      consumeForTest(amount2),
    ])

    await page.reload()
    await expect(dashboardCard(page)).toBeVisible({ timeout: 15000 })

    const windowAfter = await getWindowRemaining(page, bucketId, SMALLEST_WINDOW_KEY)
    const topUpAfter = await readTopUpBalance(loginPage.getAccessToken())
    const totalConsumed = windowBefore - windowAfter + (topUpBefore - topUpAfter)

    // At least one request should have been rejected or the total consumed
    // must not exceed the original available balance.
    expect(totalConsumed).toBeLessThanOrEqual(totalAvailable)
    if (result1.status === 200 && result2.status === 200) {
      expect(totalConsumed).toBeLessThan(amount1 + amount2)
    }
  })
})
