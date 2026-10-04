/**
 * Mapping distribution-rule editor Demo.
 *
 * Draft user story: US-MWGR-001
 * Source: docs/user-stories/billing/multi-wallet-grant-rules.md
 */

import { expect } from '@playwright/test'

import { test, cleanupTestData } from '../fixtures/demo-page.fixtures'
import { loginWithCredentials } from '../helpers/auth'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { EntitlementMappingsPage } from '../pages/entitlement-mappings-page'
import { SELECTORS } from '../selectors'

const REALM_ID = 'realm-001'
const ADMIN_EMAIL = 'admin@realm-001.com'
const ADMIN_PASSWORD = 'password'
const PRODUCT_ID = 'demo-multi-wallet-topup'
const PRIMARY_RULE_ID = '0198f21a-1111-7000-8000-000000000011'
const PROMO_RULE_ID = '0198f21a-1111-7000-8000-000000000012'

test.describe('[Billing Admin] Mapping 多账户分发规则 (US-MWGR-001)', () => {
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

  test('US-MWGR-001 场景1: 编辑并持久化两个目标账户的 Topup 规则', async ({
    page,
    demoLogger,
  }) => {
    void demoLogger
    const mappingsPage = new EntitlementMappingsPage(page)

    await test.step('Given: 打开 Seed 提供的多钱包一次性购买配置', async () => {
      await mappingsPage.goto(REALM_ID)
      await mappingsPage.selectProduct(PRODUCT_ID)
      await expect(page.locator(SELECTORS.multiPriceMapping.mappingDetailPanel)).toBeVisible()
      await expect(page.locator(SELECTORS.pointRule.row(PRIMARY_RULE_ID))).toBeVisible()
      await expect(page.locator(SELECTORS.pointRule.row(PROMO_RULE_ID))).toBeVisible()
    })

    await test.step('When: 分别调整两个账户规则的固定积分', async () => {
      await page.locator(SELECTORS.pointRule.amountInput(PRIMARY_RULE_ID)).fill('121')
      await page.locator(SELECTORS.pointRule.amountInput(PROMO_RULE_ID)).fill('81')
      await expect(page.locator(SELECTORS.pointRule.trigger('topup'))).toHaveCount(2)
      await mappingsPage.saveChanges()
      await expect(page.locator(SELECTORS.multiPriceMapping.mappingDetailPanel)).toBeVisible()
    })

    await test.step('Then: 重新加载后两条规则与各自积分值保持', async () => {
      await page.reload()
      await mappingsPage.selectProduct(PRODUCT_ID)
      await expect(page.locator(SELECTORS.pointRule.amountInput(PRIMARY_RULE_ID))).toHaveValue('121')
      await expect(page.locator(SELECTORS.pointRule.amountInput(PROMO_RULE_ID))).toHaveValue('81')

      const bucketNames = await page
        .locator(SELECTORS.pointRule.bucketSelect)
        .allTextContents()
      expect(bucketNames).toEqual(expect.arrayContaining(['Primary Pool', 'Promo Pool']))
    })
  })
})
