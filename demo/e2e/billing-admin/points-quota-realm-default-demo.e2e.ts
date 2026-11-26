/**
 * Realm registration distribution-rule editor Demo.
 *
 * Draft user story: US-MWGR-002
 * Source: docs/user-stories/billing/multi-wallet-grant-rules.md
 */

import { expect } from '@playwright/test'

import { test, cleanupTestData } from '../fixtures/demo-page.fixtures'
import { loginWithCredentials } from '../helpers/auth'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { SELECTORS } from '../selectors'

const REALM_ID = 'realm-001'
const ADMIN_EMAIL = 'admin@realm-001.com'
const ADMIN_PASSWORD = 'password'
const PRIMARY_RULE_ID = '0198f21a-1111-7000-8000-000000000021'
const PROMO_RULE_ID = '0198f21a-1111-7000-8000-000000000022'

test.describe('[Billing Admin] Realm 多账户注册规则 (US-MWGR-002)', () => {
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

  test('US-MWGR-002 场景1: 编辑并持久化两个账户的注册规则', async ({
    page,
    demoLogger,
  }) => {
    void demoLogger

    await test.step('Given: 打开 Realm 注册规则配置页', async () => {
      await page.goto(`/${REALM_ID}/manage/points/registration-rules`)
      await expect(page.locator(SELECTORS.pointRule.row(PRIMARY_RULE_ID))).toBeVisible()
      await expect(page.locator(SELECTORS.pointRule.row(PROMO_RULE_ID))).toBeVisible()
    })

    await test.step('When: 调整两个注册规则并保存', async () => {
      await page.locator(SELECTORS.pointRule.amountInput(PRIMARY_RULE_ID)).fill('41')
      await page.locator(SELECTORS.pointRule.amountInput(PROMO_RULE_ID)).fill('26')
      await expect(page.locator(SELECTORS.pointRule.trigger('registration'))).toHaveCount(2)
      await page.locator(SELECTORS.pointRule.registrationRulesSave).click()
      await expect(page.locator(SELECTORS.pointRule.registrationRulesSave)).toBeEnabled()
    })

    await test.step('Then: 重新加载后规则仍分别指向两个账户', async () => {
      await page.reload()
      await expect(page.locator(SELECTORS.pointRule.amountInput(PRIMARY_RULE_ID))).toHaveValue('41')
      await expect(page.locator(SELECTORS.pointRule.amountInput(PROMO_RULE_ID))).toHaveValue('26')

      const bucketNames = await page
        .locator(SELECTORS.pointRule.bucketSelect)
        .allTextContents()
      expect(bucketNames).toEqual(expect.arrayContaining(['Primary Pool', 'Promo Pool']))
    })
  })
})
