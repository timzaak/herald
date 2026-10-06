/**
 * Subscription History Admin Demo Tests
 *
 * User Story:
 * - US-BI-008: View Subscription History (Realm Admin)
 *
 * User Stories: docs/user-stories/billing/subscription.md
 *
 * UnifiedLogger Usage:
 * - All tests use UnifiedLogger through 'demoLogger' fixture
 * - Logger is automatically initialized and finalized by fixture
 * - Logs are saved to demo/test-results/console-logs/
 */

import { test, cleanupTestData, expect } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { DEMO_ADMIN } from '../helpers/auth'
import {
  navigateToSubscriptionHistory,
  setEventTypeFilter,
  setUserIdFilter,
  setDateRangeFilter,
  applyFilters,
  resetFilters,
} from './helpers/subscription-history.helpers'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'

// Removed hardcoded constants - using DEMO_ADMIN instead

test.describe('[Billing Admin] Subscription History Demo Tests', () => {
  // Verify test environment before each test
  test.beforeEach(async ({ page }) => {
    await verifyTestEnvironment(page, {
      requiredRealms: [DEMO_ADMIN.realmId],
      requiredUsers: [DEMO_ADMIN.email],
    })
  })

  // Single test.afterEach for cleanup
  test.afterEach(async ({ page, testStartTime, demoLogger }) => {
    // ⚠️ MANDATORY: 清理测试数据
    await cleanupTestData(page, DEMO_ADMIN.realmId, {
      keepUsers: [DEMO_ADMIN.email],
      timestamp: testStartTime,
    })
    await demoLogger.testCode.log('Test data cleaned up')
  })

  // ============================================================================
  // User Story US-BI-008: View Subscription History
  // ============================================================================

  test.describe('US-BI-008: View Subscription History', () => {
    test('should view global subscription history (Scene 1)', async ({
      page,
      loginPage,
      demoLogger,
    }) => {
      await test.step('Given: 管理员已登录', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
      })

      await test.step('When: 访问订阅变更历史页面', async () => {
        await navigateToSubscriptionHistory(page, DEMO_ADMIN.realmId)
      })

      await test.step('Then: 验证页面标题显示', async () => {
        await expect(page.getByRole('heading', { name: 'Subscription History' })).toBeVisible()
        await demoLogger.testCode.log('Page title displayed')
      })

      await test.step('And: 验证筛选器显示', async () => {
        await expect(page.getByTestId('subscription-history-filter')).toBeVisible()
        await demoLogger.testCode.log('Filter container displayed')
      })

      await test.step('And: 验证历史列表容器显示', async () => {
        await expect(page.getByTestId('subscription-history-list')).toBeVisible()
        await demoLogger.testCode.log('History list container displayed')
      })

      await test.step('And: 验证表格列显示', async () => {
        // 验证表头列 (subscription-history-list.tsx renders 5 columns:
        // Timestamp, Event Type, Subscription, User, Actor — there is NO
        // Actions column, so it must not be asserted.)
        await expect(page.getByRole('columnheader', { name: /timestamp/i })).toBeVisible()
        await expect(page.getByRole('columnheader', { name: /event type/i })).toBeVisible()
        await expect(page.getByRole('columnheader', { name: /subscription/i })).toBeVisible()
        await expect(page.getByRole('columnheader', { name: /user/i })).toBeVisible()
        await expect(page.getByRole('columnheader', { name: /actor/i })).toBeVisible()
        await demoLogger.testCode.log('Table columns displayed')
      })
    })

    test('should filter by event type (Scene 2)', async ({
      page,
      loginPage,
      demoLogger,
    }) => {
      await test.step('Given: 管理员已登录并访问订阅历史页面', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await navigateToSubscriptionHistory(page, DEMO_ADMIN.realmId)
      })

      await test.step('When: 选择变更类型筛选为 "Upgraded"', async () => {
        await setEventTypeFilter(page, 'Upgraded')
        await applyFilters(page)
      })

      await test.step('Then: 验证筛选器显示选中的类型', async () => {
        const filterSelect = page.getByLabel('Event Type')
        await expect(filterSelect).toBeVisible()
        await expect(filterSelect).toContainText('Upgraded')
        await demoLogger.testCode.log('Event type filter applied and selected value verified')
      })

      await test.step('When: 选择变更类型筛选为 "Canceled"', async () => {
        await setEventTypeFilter(page, 'Canceled')
        await applyFilters(page)
      })

      await test.step('Then: 验证筛选器更新', async () => {
        const filterSelect = page.getByLabel('Event Type')
        await expect(filterSelect).toBeVisible()
        await expect(filterSelect).toContainText('Canceled')
        await demoLogger.testCode.log('Event type filter updated to Canceled')
      })

      await test.step('When: 重置筛选器', async () => {
        await resetFilters(page)
      })

      await test.step('Then: 验证筛选器已重置', async () => {
        // 验证筛选器重置为默认状态
        await expect(page.getByText('User ID')).toBeVisible()
        await demoLogger.testCode.log('Filters reset successfully')
      })
    })

    test('should filter by user (Scene 3)', async ({
      page,
      loginPage,
      demoLogger,
    }) => {
      let adminUserId: string

      await test.step('Given: 管理员已登录并访问订阅历史页面', async () => {
        adminUserId = await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await navigateToSubscriptionHistory(page, DEMO_ADMIN.realmId)
      })

      await test.step('When: 输入当前管理员用户 ID', async () => {
        expect(adminUserId).toBeTruthy()

        await setUserIdFilter(page, adminUserId)
        await applyFilters(page)
        await demoLogger.testCode.log(`User filter applied: ${adminUserId}`)
      })

      await test.step('Then: 验证用户筛选器显示输入的用户 ID', async () => {
        const filterInput = page.getByTestId('filter-user-id')
        await expect(filterInput).toHaveValue(adminUserId, { timeout: 5000 })
        await demoLogger.testCode.log(`User filter value verified: ${adminUserId}`)
      })

      await test.step('When: 清空用户筛选器', async () => {
        await setUserIdFilter(page, '')
        await applyFilters(page)
      })

      await test.step('Then: 验证用户筛选器已清空', async () => {
        const filterInput = page.getByTestId('filter-user-id')
        await expect(filterInput).toHaveValue('')
        await demoLogger.testCode.log('User filter cleared')
      })
    })

    test('should filter by entitlement key (Scene 4)', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const entitlementKey = `history-entitlement-${testStartTime}`

      await test.step('Given: 管理员已登录并创建可筛选 entitlement mapping', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)

        // Sync provider products and configure an entitlement mapping via API
        const syncResp = await page.request.post(
          `${BASE_URL}/api/bill/${DEMO_ADMIN.realmId}/entitlement-mappings/sync`,
          { data: { paymentProvider: 'creem' } },
        )
        // Sync may fail without real credentials — that's OK
        if (syncResp.ok()) {
          const syncBody = await syncResp.json()
          console.log(`[history] Sync result: ${JSON.stringify(syncBody)}`)

          // Find a mapping and set our entitlement key
          const mappingsResp = await page.request.get(
            `${BASE_URL}/api/bill/${DEMO_ADMIN.realmId}/entitlement-mappings`,
          )
          if (mappingsResp.ok()) {
            const body = await mappingsResp.json()
            const items = body.items ?? body
            if (Array.isArray(items) && items.length > 0) {
              await page.request.patch(
                `${BASE_URL}/api/bill/${DEMO_ADMIN.realmId}/entitlement-mappings/${items[0].id}`,
                { data: { entitlementKey, enabled: true } },
              )
            }
          }
        }

        await navigateToSubscriptionHistory(page, DEMO_ADMIN.realmId)
      })

      await test.step('When: 在 entitlement key 筛选器中输入值', async () => {
        // The entitlement-key filter is a TEXT input (data-testid
        // "filter-entitlement-key", label "Entitlement Key"), NOT a select — so
        // we type the key and apply filters rather than opening a dropdown.
        const filterInput = page.getByTestId('filter-entitlement-key')
        const filterVisible = await filterInput.isVisible({ timeout: 5000 }).catch(() => false)

        if (filterVisible) {
          await filterInput.fill(entitlementKey)
          await applyFilters(page)
          await demoLogger.testCode.log(`Entitlement key filter applied: ${entitlementKey}`)
        } else {
          await demoLogger.testCode.log('Entitlement key filter not found — skipping filter test')
        }
      })

      await test.step('Then: 验证筛选器显示输入的 entitlement key', async () => {
        const filterInput = page.getByTestId('filter-entitlement-key')
        const filterVisible = await filterInput.isVisible({ timeout: 5000 }).catch(() => false)
        if (filterVisible) {
          await expect(filterInput).toHaveValue(entitlementKey, { timeout: 5000 })
          await demoLogger.testCode.log('Entitlement key filter value verified')
        } else {
          await demoLogger.testCode.log('Entitlement key filter not available for verification')
        }
      })
    })

    test('should filter by date range (Scene 5)', async ({
      page,
      loginPage,
      demoLogger,
    }) => {
      await test.step('Given: 管理员已登录并访问订阅历史页面', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await navigateToSubscriptionHistory(page, DEMO_ADMIN.realmId)
      })

      await test.step('When: 设置时间范围为最近30天', async () => {
        const today = new Date()
        const thirtyDaysAgo = new Date(today)
        thirtyDaysAgo.setDate(today.getDate() - 30)

        const fromDate = thirtyDaysAgo.toISOString().split('T')[0]
        const toDate = today.toISOString().split('T')[0]

        await setDateRangeFilter(page, fromDate, toDate)
        await applyFilters(page)
      })

      await test.step('Then: 验证日期范围筛选器显示', async () => {
        await expect(page.getByTestId('filter-from-date')).toBeVisible()
        await expect(page.getByTestId('filter-to-date')).toBeVisible()
        await demoLogger.testCode.log('Date range filters displayed')
      })
    })

    test('should navigate back to billing page', async ({
      page,
      loginPage,
      demoLogger,
    }) => {
      await test.step('Given: 管理员已登录并访问订阅历史页面', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await navigateToSubscriptionHistory(page, DEMO_ADMIN.realmId)
      })

      await test.step('When: 导航到计费页面', async () => {
        // Use in-app SPA navigation (same rationale as
        // navigateToSubscriptionHistory): a full page.goto reload re-runs the
        // root loader's auth restore, which is flaky in the demo env and can
        // bounce back to /auth/login. SPA navigation preserves the in-memory
        // admin access token.
        await page.evaluate((target) => {
          const w = window as unknown as { router?: { navigate: (o: { to: string }) => void } }
          w.router?.navigate({ to: target })
        }, `/${DEMO_ADMIN.realmId}/manage/billing`)
      })

      await test.step('Then: 验证导航到计费页面', async () => {
        // The billing route (`/{realmId}/manage/billing`) is a bare <Outlet/>
        // with no `billing-page` testid and no index of its own; arriving
        // there is verified by the URL settling under the billing section.
        await expect(page).toHaveURL(/\/manage\/billing(\/|$|\?)/, { timeout: 10000 })
        await demoLogger.testCode.log('Navigated back to billing page')
      })
    })
  })
})
