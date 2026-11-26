/**
 * Dashboard Stats Demo Tests
 *
 * User Stories: US-RA-010, US-RA-011, US-RA-012
 *
 * Test Coverage:
 * - US-RA-010: Dashboard 用户活跃概览 (stats cards: total, new, active users)
 * - US-RA-011: Dashboard 认证趋势图 (auth trend chart visibility)
 * - US-RA-012: Dashboard 快捷导航跳转 (recent-activity card links navigation)
 *
 * Test Data Strategy:
 * - Uses Demo Seed admin realm with admin@cas.com
 * - Dashboard displays stats from seeded data; no additional data creation needed
 */

import { test, cleanupTestData, expect } from '../fixtures/demo-page.fixtures'

const ADMIN_REALM = 'admin'
const ADMIN_EMAIL = 'admin@cas.com'

test.describe('[Dashboard] Stats & Navigation Demo Tests', () => {
  let testStartTime: number

  test.beforeEach(async ({ page, testStartTime: ts }) => {
    testStartTime = ts
  })

  test.afterEach(async ({ page }) => {
    await cleanupTestData(page, ADMIN_REALM, {
      keepUsers: [ADMIN_EMAIL],
      timestamp: testStartTime,
    })
  })

  test('US-RA-010 Dashboard 用户活跃概览', async ({ dashboardPage }) => {
    await test.step('Given: Dashboard is loaded', async () => {
      await dashboardPage.waitForLoad()
    })

    await test.step('Then: Stats row is visible with all three cards', async () => {
      expect(await dashboardPage.isStatsRowVisible()).toBe(true)

      const totalUsers = await dashboardPage.getStatsValue('dashboard-total-users-card')
      expect(totalUsers).toBeGreaterThanOrEqual(0)

      const newUsers = await dashboardPage.getStatsValue('dashboard-new-users-card')
      expect(newUsers).toBeGreaterThanOrEqual(0)

      const activeUsers = await dashboardPage.getStatsValue('dashboard-active-users-card')
      expect(activeUsers).toBeGreaterThanOrEqual(0)
    })
  })

  test('US-RA-011 Dashboard 认证趋势图', async ({ dashboardPage }) => {
    await test.step('Given: Dashboard is loaded', async () => {
      await dashboardPage.waitForLoad()
    })

    await test.step('Then: Auth trend chart area is present', async () => {
      await expect(dashboardPage.authTrendChart).toBeAttached()
    })
  })

  // The 6-card QuickNav this story was written against was replaced by the
  // recent-activity tables (frontend commit 69b33a82): each card carries a
  // "view all" link into its management page.
  test('US-RA-012 Dashboard 快捷导航跳转', async ({ dashboardPage, page }) => {
    await test.step('Given: Dashboard is loaded', async () => {
      await dashboardPage.waitForLoad()
    })

    await test.step('Then: Recent activity cards are shown', async () => {
      await expect(dashboardPage.recentUsersCard).toBeVisible()
      await expect(dashboardPage.recentAuditCard).toBeVisible()
      await expect(dashboardPage.recentPointsCard).toBeVisible()
    })

    await test.step('When: Click "view all" on the recent users card', async () => {
      await dashboardPage.clickRecentCardViewAll(dashboardPage.recentUsersCard)
    })

    await test.step('Then: URL navigates to users page', async () => {
      await expect(page).toHaveURL(/\/admin\/manage\/users/)
    })

    await test.step('When: Navigate back to dashboard and click Total Users card', async () => {
      await dashboardPage.goto()
      await dashboardPage.waitForLoad()

      await dashboardPage.clickTotalUsersCard()
    })

    await test.step('Then: URL navigates to users page again', async () => {
      await expect(page).toHaveURL(/\/admin\/manage\/users/)
    })
  })
})
