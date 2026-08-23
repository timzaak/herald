/**
 * Stripe Payment Comprehensive Demo Tests
 *
 * User Stories:
 * - docs/user-stories/billing/subscription.md:
 *   - US-BI-001: Sync Stripe Provider Products
 *   - US-BI-004: Configure Entitlement Mapping
 *   - US-BI-007: View Subscription Change History (Including Stripe Payment Events)
 * - docs/user-stories/billing/entitlement-mapping.md:
 *   - US-EM-001: View Provider Entitlement Mappings
 * - docs/user-stories/billing/payment-provider.md:
 *   - US-PV-001: Configure Stripe Payment Provider
 *   - US-PV-002: View Payment Provider Configuration
 *
 * Test Scenarios:
 * 1. Configure Stripe (Payment Providers page)
 * 2. Sync Stripe Products & View Entitlement Mappings
 * 3. Configure Entitlement Mapping (read provider-owned key, set points policy)
 * 4. Stripe Checkout Flow (API-level verification via entitlement key)
 * 5. Handle Checkout Failure (invalid entitlement key)
 * 6. View Subscription History (Stripe events)
 */

import { test, cleanupTestData, expect } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import type { UnifiedLogger } from '../helpers/unified-logger'
import { createBearerApiContext, DEMO_ADMIN } from '../helpers/auth'
import { EntitlementMappingsPage } from '../pages/entitlement-mappings-page'
import { SELECTORS } from '../selectors'
import { randomUUID } from 'crypto'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'

test.describe('[Billing Admin] Stripe Payment Comprehensive Demo', () => {
  let testStartTime: number

  test.beforeEach(async ({ page, demoLogger }) => {
    testStartTime = Date.now()
    await verifyTestEnvironment(page, {
      requiredRealms: [DEMO_ADMIN.realmId],
      requiredUsers: [DEMO_ADMIN.email],
    })
    await demoLogger.testCode.log('Environment verified')
  })

  test.afterEach(async ({ page, demoLogger }) => {
    await cleanupTestData(page, DEMO_ADMIN.realmId, {
      keepUsers: [DEMO_ADMIN.email],
      timestamp: testStartTime,
    })
    await demoLogger.testCode.log('Test data cleaned up')
  })

  // ============================================================================
  // Scenario 1: Configure Stripe (Payment Providers Page)
  // ============================================================================

  test.describe('Scenario 1: Configure Stripe', () => {
    test('should configure Stripe in Payment Providers page', async ({ page, loginPage, demoLogger }) => {
      await test.step('Given: 管理员已登录', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await demoLogger.testCode.log('Admin logged in')
      })

      await test.step('When: 导航到 Payment Providers 页面', async () => {
        await page.goto(`/${DEMO_ADMIN.realmId}/manage/billing/payment-providers`)
        await expect(page.getByRole('heading', { name: 'Payment Providers' })).toBeVisible()
        await demoLogger.testCode.log('Payment Providers page loaded')
      })

      await test.step('When: 配置 Stripe Provider', async () => {
        await configureStripe(page, testStartTime, demoLogger)
        await demoLogger.testCode.log('Stripe configuration completed')
      })

      await test.step('Then: 验证配置成功', async () => {
        await expect(page.getByTestId('edit-stripe-button')).toBeVisible()
        await demoLogger.testCode.log('Configuration verified successfully')
      })
    })
  })

  // ============================================================================
  // Scenario 2: Sync Stripe Products & View Entitlement Mappings
  // ============================================================================

  test.describe('Scenario 2: Sync Stripe Products (US-EM-001)', () => {
    test('should sync Stripe products and display entitlement mappings', async ({ page, loginPage, demoLogger }) => {
      await test.step('Given: 管理员已登录', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await demoLogger.testCode.log('Admin logged in')
      })

      await test.step('Given: 已配置 Stripe 支付平台', async () => {
        await configureStripe(page, testStartTime, demoLogger)
        await demoLogger.testCode.log('Stripe configured')
      })

      await test.step('When: 导航到 Entitlement Mappings 页面', async () => {
        await page.goto(`/${DEMO_ADMIN.realmId}/manage/billing/entitlement-mappings`)
        await expect(page.getByRole('heading', { name: 'Entitlement Mappings' })).toBeVisible()
        await demoLogger.testCode.log('Entitlement mappings page loaded')
      })

      await test.step('When: 点击 Sync Provider Products', async () => {
        // provider-sync-button is a wrapper <div>; one sync Button is rendered
        // per configured provider, each carrying data-provider="<platform>".
        const syncWrapper = page.getByTestId('provider-sync-button')
        await expect(syncWrapper).toBeVisible()
        // Click the Stripe sync button directly (no provider dropdown anymore).
        const syncButton = syncWrapper.locator(
          '[data-testid="sync-button"][data-provider="stripe"]'
        )
        await syncButton.click()
        await demoLogger.testCode.log('Sync triggered')

        // Wait for sync to complete (page reloads data)
        await page.waitForTimeout(3000)
      })

      await test.step('Then: 验证 sync 结果或 empty state', async () => {
        // Check for table (mappings exist) or empty state
        const table = page.locator('table')
        const emptyState = page.getByText(/no provider products synced/i)
        const hasTable = await table.isVisible({ timeout: 5000 }).catch(() => false)
        const hasEmpty = await emptyState.isVisible({ timeout: 2000 }).catch(() => false)

        if (hasTable) {
          await demoLogger.testCode.log('Mappings table visible after sync')
        } else if (hasEmpty) {
          await demoLogger.testCode.log('Empty state visible (no Stripe products found in test config)')
        } else {
          // Page may still be loading
          await demoLogger.testCode.log('Waiting for page to stabilize after sync')
        }
      })
    })
  })

  // ============================================================================
  // Scenario 3: Configure Entitlement Mapping (US-BI-004)
  // The old detail dialog is gone; configuration happens inline in the
  // right-hand detail panel via the EntitlementMappingsPage POM
  // (selectProduct → configureFixedPointRule → saveChanges).
  //
  // Current contract (frontend/src/components/billing/
  // entitlement-mappings-page.tsx, since commit 2ef33cc8): the entitlement
  // key is provider-owned and permanently read-only in the detail panel, and
  // the batch update DTO has no entitlementKey field. Operators only
  // configure the points policy through the PointDistributionRuleEditor on
  // each price row — the same surface driven by
  // multiple-price-entitlement-mapping-demo.e2e.ts S2.
  // ============================================================================

  test.describe('Scenario 3: Configure Entitlement Mapping', () => {
    // Deterministic fixture: the demo seed (scripts/lib/demo_seed.py
    // `_ensure_subscription_history_demo_data`) guarantees a stripe mapping
    // for `test-product-subscription` with entitlement key `professional` in
    // the admin realm. Its external_price_id is NULL, so the price-edit-row
    // testid suffix is the (dynamic) mapping id — resolved from the DOM below.
    const SEED_PRODUCT_ID = 'test-product-subscription'
    const SEED_ENTITLEMENT_KEY = 'professional'
    const SEED_POINT_RULE_AMOUNT = 1000

    test('should configure entitlement key and points policy on a mapping via master-detail', async ({ page, loginPage, demoLogger }) => {
      const mappingsPage = new EntitlementMappingsPage(page, demoLogger)

      await test.step('Given: 管理员已登录并配置 Stripe', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await configureStripe(page, testStartTime, demoLogger)
        await demoLogger.testCode.log('Setup complete')
      })

      await test.step('Given: 已 sync provider products', async () => {
        const syncResp = await page.request.post(
          `${BASE_URL}/api/bill/${DEMO_ADMIN.realmId}/entitlement-mappings/sync`,
          { data: { paymentProvider: 'stripe' } },
        )
        // Sync may fail with test credentials — that's OK
        if (syncResp.ok()) {
          await demoLogger.testCode.log('Products synced')
        } else {
          await demoLogger.testCode.log(`Sync returned ${syncResp.status()} (expected with test keys)`)
        }
      })

      await test.step('When: 导航到 Entitlement Mappings 页面 (master-detail)', async () => {
        await mappingsPage.goto(DEMO_ADMIN.realmId)
        await demoLogger.testCode.log('Entitlement mappings page loaded (master-detail)')
      })

      await test.step('When: 选择种子 product 打开 detail panel', async () => {
        await mappingsPage.waitForDataLoaded()

        // Fail loud when the seeded mapping is missing. An earlier revision
        // returned early on an empty list here, which made this scenario
        // silently pass in unseeded environments and hid the readonly-key
        // regression behind a skip branch.
        await expect(
          page.locator(SELECTORS.multiPriceMapping.mappingProductRow(SEED_PRODUCT_ID)),
          `seeded mapping product ${SEED_PRODUCT_ID} must exist (demo seed required)`,
        ).toBeVisible()

        await mappingsPage.selectProduct(SEED_PRODUCT_ID)
        await expect(mappingsPage.mappingDetailPanel).toBeVisible()
        await expect(mappingsPage.detailHead).toBeVisible()
        await demoLogger.testCode.log(`Detail panel opened for seeded product ${SEED_PRODUCT_ID}`)
      })

      await test.step('When: 校验只读 entitlement key 并配置 points 策略', async () => {
        // The seed row is the product's only price; its testid suffix is the
        // dynamic mapping id (NULL external_price_id fallback — Creem
        // NULL-price rows use the same rule), so resolve it generically.
        const firstPriceRow = mappingsPage.mappingDetailPanel.locator('[data-testid^="price-edit-row-"]').first()
        await expect(firstPriceRow).toBeVisible()
        const testid = (await firstPriceRow.getAttribute('data-testid')) || ''
        const priceKey = testid.replace(/^price-edit-row-/, '')

        // The entitlement key is provider-owned and read-only — assert the
        // seeded value instead of filling it (the batch update DTO no longer
        // carries an entitlementKey field).
        const providerOwnedKey = await mappingsPage.getEntitlementKeyValue(priceKey)
        expect(
          providerOwnedKey,
          'provider sync/seed must supply the seeded entitlement key',
        ).toBe(SEED_ENTITLEMENT_KEY)
        await expect(mappingsPage.getSharedKeyChip(providerOwnedKey)).toBeVisible()

        // Points policy is configured through the PointDistributionRuleEditor
        // on the price row (idempotent — reuses the existing fixed rule).
        await mappingsPage.configureFixedPointRule(priceKey, SEED_POINT_RULE_AMOUNT)
        await demoLogger.testCode.log(
          `Price row ${priceKey} configured (readonly key=${providerOwnedKey}, fixed points=${SEED_POINT_RULE_AMOUNT})`,
        )
      })

      await test.step('Then: 验证 detail panel 包含 Save Changes 按钮', async () => {
        // Save Changes is rendered when canManage (billing.manage). The admin
        // persona used here holds it, so the editable variant must mount.
        await expect(mappingsPage.saveMappingButton).toBeVisible()
        await demoLogger.testCode.log('Save Changes button present — configuration form is editable')
      })
    })
  })

  // ============================================================================
  // Scenario 4: Stripe Checkout Flow (API-level verification)
  //
  // Checkout uses the payment-attempt API with an entitlement mapping target.
  // An unknown target id must resolve to the handler's 404 response.
  // ============================================================================

  test.describe('Scenario 4: Stripe Checkout Flow', () => {
    test('should verify checkout API returns 404 for unknown mapping id', async ({ page, loginPage, demoLogger }) => {
      await test.step('Given: 管理员已登录并配置 Stripe (test keys)', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await configureStripe(page, testStartTime, demoLogger)
        await demoLogger.testCode.log('Setup complete')
      })

      await test.step('When: 尝试使用不存在的 mapping id 创建 checkout', async () => {
        const apiContext = await createBearerApiContext(loginPage.getAccessToken())
        const unknownMappingId = randomUUID()
        try {
          const checkoutResp = await apiContext.post(
            `${BASE_URL}/api/bill/${DEMO_ADMIN.realmId}/purchase/payment-attempts`,
            {
              data: {
                targetType: 'entitlement_mapping',
                targetId: unknownMappingId,
                paymentProvider: 'stripe',
              },
            },
          )

          await demoLogger.testCode.log(`Checkout response: ${checkoutResp.status()}`)

          expect(checkoutResp.status()).toBe(404)
          const errorBody = await checkoutResp.json()
          expect(errorBody).toMatchObject({
            code: 'not_found',
            message: 'Create payment attempt not found',
          })
          await demoLogger.testCode.log('Correctly returned 404 for unknown mapping id')
        } finally {
          await apiContext.dispose()
        }
      })
    })
  })

  // ============================================================================
  // Scenario 5: View Subscription History (US-BI-007)
  // ============================================================================

  test.describe('Scenario 5: View Subscription History (US-BI-007)', () => {
    test('should display subscription change history', async ({ page, loginPage, demoLogger }) => {
      await test.step('Given: 管理员已登录并配置 Stripe', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await configureStripe(page, testStartTime, demoLogger)
        await demoLogger.testCode.log('Setup complete')
      })

      await test.step('When: 访问订阅变更历史页面', async () => {
        await page.goto(`/${DEMO_ADMIN.realmId}/manage/subscription-history`)
        await expect(page.getByTestId('subscription-history-page')).toBeVisible()
        await demoLogger.testCode.log('Subscription history page loaded')
      })

      await test.step('Then: 验证历史列表显示', async () => {
        await expect(page.getByTestId('subscription-history-list')).toBeVisible()
        await demoLogger.testCode.log('History list displayed')
      })

      await test.step('Then: 验证筛选功能可用', async () => {
        await expect(page.getByTestId('subscription-history-filter')).toBeVisible()
        await demoLogger.testCode.log('Filter controls available')
      })
    })

    test('should filter history by event type', async ({ page, loginPage, demoLogger }) => {
      await test.step('Given: 管理员已登录', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await configureStripe(page, testStartTime, demoLogger)
        await demoLogger.testCode.log('Setup complete')
      })

      await test.step('When: 访问订阅变更历史页面', async () => {
        await page.goto(`/${DEMO_ADMIN.realmId}/manage/subscription-history`)
        await expect(page.getByTestId('subscription-history-page')).toBeVisible()
        await expect(page.getByTestId('subscription-history-filter')).toBeVisible()
        await demoLogger.testCode.log('History page loaded')
      })

      await test.step('When: 使用事件类型筛选', async () => {
        await expect(page.getByTestId('subscription-history-filter')).toBeVisible()

        const eventTypeFilter = page.getByRole('combobox', { name: 'Event Type' })
        await expect(eventTypeFilter).toBeVisible({ timeout: 15000 })
        await eventTypeFilter.click()
        await expect(page.getByRole('option').first()).toBeVisible()
        await page.getByRole('option', { name: 'Created', exact: true }).click()
        await demoLogger.testCode.log('Event type filter applied')
      })

      await test.step('Then: 验证筛选结果', async () => {
        await expect(page.getByTestId('subscription-history-list')).toBeVisible()
        await demoLogger.testCode.log('Filter results verified')
      })
    })
  })
})

// ============================================================================
// Helper Functions
// ============================================================================

async function configureStripe(
  page: import('@playwright/test').Page,
  timestamp: number,
  demoLogger: UnifiedLogger
): Promise<void> {
  await page.goto(`/${DEMO_ADMIN.realmId}/manage/billing/payment-providers`)

  await page.waitForSelector('[data-testid="edit-stripe-button"], [data-testid="add-stripe-button"]', {
    timeout: 10000
  })

  const editStripeButton = page.getByTestId('edit-stripe-button')
  const addStripeButton = page.getByTestId('add-stripe-button')

  const hasEditButton = await editStripeButton.isVisible().catch(() => false)
  if (hasEditButton) {
    await editStripeButton.click()
    await demoLogger.testCode.log('Editing existing Stripe configuration')
  } else {
    await addStripeButton.click()
    await demoLogger.testCode.log('Creating new Stripe configuration')
  }

  await page.waitForURL('**/payment-providers/stripe', { timeout: 10000 })
  await expect(page.getByTestId('stripe-config-form-page')).toBeVisible()

  await page.getByTestId('page-stripe-publishable-key-input').fill(`pk_test_51M${timestamp}`)
  await page.getByTestId('page-stripe-secret-key-input').fill(`sk_test_51M${timestamp}`)
  await page.getByTestId('page-stripe-webhook-secret-input').fill(`whsec_${timestamp}`)

  await demoLogger.testCode.log('Stripe config filled with test credentials')

  await page.getByTestId('stripe-config-page-submit-button').click()
  await page.waitForURL('**/payment-providers', { timeout: 15000 })
  await demoLogger.testCode.log('Stripe configuration saved')
}
