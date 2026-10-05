/**
 * Billing Statistics page Demo.
 *
 * Coverage (UI main path):
 * - Admin opens the statistics page from the sidebar (Transactions group)
 * - Payment and points consumption live in separate tabs; the inactive
 *   tab's panel stays unmounted
 * - The selected tab (payment|points) and window (7|30 days) are carried in
 *   the querystring, and a browser refresh restores the exact prior view
 *
 * NOT Covered here (anchored by backend scenario tests instead):
 * - Multi-currency aggregation, refund-revocation non-offset, and the
 *   billing.view/points.view permission split (403 surfaces)
 */

import { expect } from '@playwright/test'

import { test, cleanupTestData } from '../fixtures/demo-page.fixtures'
import { loginWithCredentials } from '../helpers/auth'
import { verifyTestEnvironment } from '../helpers/environment-setup'

const REALM_ID = 'realm-001'
const ADMIN_EMAIL = 'admin@realm-001.com'
const ADMIN_PASSWORD = 'password'

test.describe('[Billing Admin] 统计页主路径 (US-BS-001 / US-BS-002)', () => {
  test.beforeEach(async ({ page }) => {
    await verifyTestEnvironment(page, {
      requiredRealms: [REALM_ID],
      requiredUsers: [ADMIN_EMAIL],
    })
    await loginWithCredentials(page, {
      realmId: REALM_ID,
      email: ADMIN_EMAIL,
      password: ADMIN_PASSWORD,
    })
  })

  test.afterEach(async ({ page, testStartTime }) => {
    await cleanupTestData(page, REALM_ID, { timestamp: testStartTime })
  })

  test(
    'US-BS-001/002 场景1+2: 统计页分 tab 展示支付/积分消耗，tab 与天数进入 querystring 且刷新后保留',
    async ({ page, demoLogger }) => {
      void demoLogger

      await test.step('Given: 从侧边栏 Transactions 组进入统计页', async () => {
        // The Transactions group starts collapsed (only Authorization is open
        // by default), so expand it before clicking the Statistics entry.
        await page.getByTestId('sidebar-menu-transactions').click()
        await page.getByTestId('sidebar-menu-statistics').click()
        await expect(page.getByTestId('statistics-heading')).toBeVisible()
      })

      await test.step('When: 页面加载完成，默认为支付统计 tab + 最近 7 天窗口', async () => {
        await expect(page.getByTestId('statistics-tab-payment')).toHaveAttribute(
          'data-state',
          'active'
        )
        await expect(page.getByTestId('statistics-tab-points')).toHaveAttribute(
          'data-state',
          'inactive'
        )
        await expect(page.getByTestId('statistics-window-7-trigger')).toHaveAttribute(
          'data-state',
          'active'
        )
      })

      await test.step('Then: 仅支付统计面板挂载，积分面板不渲染', async () => {
        await expect(page.getByTestId('payment-stats-panel')).toBeVisible()
        await expect(page.getByTestId('payment-success-count-card')).toBeVisible()
        await expect(page.getByTestId('payment-failed-count-card')).toBeVisible()
        await expect(page.getByTestId('payment-success-rate-card')).toBeVisible()
        // The trend area renders the chart or the "no data" placeholder — both
        // live inside the chart card, so visibility of the card is the stable
        // assertion (aggregate calibers are pinned by backend scenario tests).
        await expect(page.getByTestId('payment-trend-chart')).toBeVisible()
        await expect(page.getByTestId('points-stats-panel')).toBeHidden()
      })

      await test.step('When: 切换到积分消耗统计 tab', async () => {
        await page.getByTestId('statistics-tab-points').click()
        await expect(page.getByTestId('statistics-tab-points')).toHaveAttribute(
          'data-state',
          'active'
        )
        await expect(page.getByTestId('points-stats-panel')).toBeVisible()
        await expect(page.getByTestId('points-total-consumed-card')).toBeVisible()
        await expect(page.getByTestId('points-consuming-users-card')).toBeVisible()
        await expect(page.getByTestId('points-trend-chart')).toBeVisible()
        await expect(page.getByTestId('payment-stats-panel')).toBeHidden()
        await expect(page).toHaveURL(/tab=points/)
      })

      await test.step('When: 切换到最近 30 天窗口', async () => {
        await page.getByTestId('statistics-window-30-trigger').click()
        await expect(page.getByTestId('statistics-window-30-trigger')).toHaveAttribute(
          'data-state',
          'active'
        )
        await expect(page.getByTestId('statistics-window-7-trigger')).toHaveAttribute(
          'data-state',
          'inactive'
        )
        // Patches merge: switching the window keeps the tab param.
        await expect(page).toHaveURL(/tab=points/)
        await expect(page).toHaveURL(/days=30/)
        await expect(page.getByTestId('points-total-consumed-card')).toBeVisible()
      })

      await test.step('Then: 刷新页面后 tab 与窗口从 querystring 恢复', async () => {
        await page.reload()
        await expect(page.getByTestId('statistics-tab-points')).toHaveAttribute(
          'data-state',
          'active'
        )
        await expect(page.getByTestId('statistics-tab-points')).toBeVisible()
        await expect(page.getByTestId('statistics-window-30-trigger')).toHaveAttribute(
          'data-state',
          'active'
        )
        await expect(page.getByTestId('points-stats-panel')).toBeVisible()
        await expect(page.getByTestId('points-total-consumed-card')).toBeVisible()
        await expect(page.getByTestId('payment-stats-panel')).toBeHidden()
        await expect(page).toHaveURL(/tab=points/)
        await expect(page).toHaveURL(/days=30/)
      })
    }
  )
})
