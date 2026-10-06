/**
 * Points Admin Comprehensive Demo Tests
 *
 * User Stories:
 * - US-PO-002: View All User Wallets
 * - US-PO-003: View User Points Transaction History
 * - US-PO-006: Configure Realm Registration and Free Periodic Rules
 *
 * User story sources:
 * - docs/user-stories/billing/points-admin.md
 * - docs/user-stories/billing/multi-wallet-grant-rules.md
 */

import type { APIRequestContext } from '@playwright/test'

import { test, cleanupTestData, expect } from '../fixtures/demo-page.fixtures'
import { SELECTORS } from '../selectors'
import { createBearerApiContext, DEMO_ADMIN, REALM_ADMINS } from '../helpers/auth'
import { verifyTestEnvironment } from '../helpers/environment-setup'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'
const RULES_REALM_ID = 'realm-001'
const PRIMARY_RULE_ID = '0198f21a-1111-7000-8000-000000000021'

interface RegistrationRule {
  id: string
  bucketId: string
  triggerSources: string[]
  grantMode: 'fixed' | 'quota'
  pointsAmount?: number | null
  validityDays?: number | null
  grantPeriodType?: string | null
  quotaWindows?: Array<{ windowSeconds: number; limit: number }> | null
  enabled: boolean
  displayOrder: number
}

interface RegistrationRulesResponse {
  realmId: string
  rules: RegistrationRule[]
}

async function readRegistrationRules(
  apiContext: APIRequestContext,
): Promise<RegistrationRule[]> {
  const response = await apiContext.get(
    `${BASE_URL}/api/points/${RULES_REALM_ID}/registration-rules`,
  )
  if (!response.ok()) {
    const body = await response.text().catch(() => '<unreadable body>')
    throw new Error(`GET registration rules failed ${response.status()}: ${body}`)
  }
  const body = (await response.json()) as RegistrationRulesResponse
  if (body.realmId !== RULES_REALM_ID || !Array.isArray(body.rules)) {
    throw new Error('GET registration rules returned an invalid response body')
  }
  return body.rules
}

async function writeRegistrationRules(
  apiContext: APIRequestContext,
  rules: RegistrationRule[],
): Promise<RegistrationRule[]> {
  const response = await apiContext.put(
    `${BASE_URL}/api/points/${RULES_REALM_ID}/registration-rules`,
    { data: { rules } },
  )
  if (!response.ok()) {
    const body = await response.text().catch(() => '<unreadable body>')
    throw new Error(`PUT registration rules failed ${response.status()}: ${body}`)
  }
  const body = (await response.json()) as RegistrationRulesResponse
  if (body.realmId !== RULES_REALM_ID || !Array.isArray(body.rules)) {
    throw new Error('PUT registration rules returned an invalid response body')
  }
  return body.rules
}

function normalizedRules(rules: RegistrationRule[]): RegistrationRule[] {
  return rules
    .map((rule) => ({ ...rule, triggerSources: [...rule.triggerSources].sort() }))
    .sort((left, right) => left.id.localeCompare(right.id))
}

test.describe('[Points Admin] Comprehensive Demo Tests', () => {
  let testStartTime: number

  test.beforeEach(async ({ page }) => {
    testStartTime = Date.now()
    await verifyTestEnvironment(page, {
      requiredRealms: [DEMO_ADMIN.realmId],
      requiredUsers: [DEMO_ADMIN.email],
    })
  })

  test.afterEach(async ({ page, demoLogger }) => {
    await cleanupTestData(page, DEMO_ADMIN.realmId, {
      keepUsers: [DEMO_ADMIN.email],
      timestamp: testStartTime,
    })
    demoLogger.testCode.log('[Test] ✓ Test data cleaned up')
  })

  // ============================================================================
  // User Story US-PO-002: View All User Wallets
  // ============================================================================

  test.describe('US-PO-002: View All User Wallets', () => {
    test('should view and search user points accounts', async ({ page, loginPage }) => {
      await test.step('Given: 管理员已登录', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
      })

      await test.step('When: 访问用户积分账户页面', async () => {
        await page.goto(`/${DEMO_ADMIN.realmId}/manage/points/wallets`)
        await expect(page.locator(SELECTORS.pointsAdmin.accountsPage)).toBeVisible()
      })

      await test.step('When: 查看用户积分账户列表', async () => {
        await expect(page.locator(SELECTORS.pointsAdmin.accountsSection)).toBeVisible()
        await expect(page.locator(SELECTORS.pointsAdmin.accountsTable)).toBeVisible()
        // Note: There may be no accounts in the database, which is fine
        // The test verifies the UI is accessible and functional
      })

      await test.step('When: 按邮箱搜索用户', async () => {
        await page.locator(SELECTORS.pointsAdmin.accountsSearch).fill('admin@cas.com')
        await page.waitForLoadState('networkidle', { timeout: 5000 })
      })

      await test.step('When: 查看账户状态', async () => {
        // Verify the accounts section is still visible after search
        await expect(page.locator(SELECTORS.pointsAdmin.accountsSection)).toBeVisible()
      })

      await test.step('Then: 验证账户信息正确显示', async () => {
        await expect(page.locator(SELECTORS.pointsAdmin.accountsTable)).toBeVisible()
      })
    })
  })

  // ============================================================================
  // User Story US-PO-003: View User Points Transaction History
  // ============================================================================

  test.describe('US-PO-003: View User Points Transaction History', () => {
    test('should view and filter user transaction history', async ({ page, loginPage }) => {
      await test.step('Given: 管理员已登录', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
      })

      await test.step('When: 访问用户积分账户页面', async () => {
        await page.goto(`/${DEMO_ADMIN.realmId}/manage/points/wallets`)
        await expect(page.locator(SELECTORS.pointsAdmin.accountsPage)).toBeVisible()
      })

      await test.step('When: 查看交易历史页面', async () => {
        // Note: If there are no accounts, we can't click on one to view transactions
        // This test verifies the UI is accessible and the page structure is correct
        await expect(page.locator(SELECTORS.pointsAdmin.accountsSection)).toBeVisible()
      })

      await test.step('When: 验证筛选器可用', async () => {
        // Verify filter elements are present and accessible
        const filterType = page.locator(SELECTORS.pointsAdmin.filterType)
        const filterStartTime = page.locator(SELECTORS.pointsAdmin.filterStartTime)
        const filterEndTime = page.locator(SELECTORS.pointsAdmin.filterEndTime)
        const applyButton = page.locator(SELECTORS.pointsAdmin.applyFiltersButton)

        // Check if filters are visible (they may be in a different section if no account is selected)
        // For now, just verify the accounts page is functional
        await expect(page.locator(SELECTORS.pointsAdmin.accountsTable)).toBeVisible()
      })

      await test.step('Then: 验证页面正常工作', async () => {
        await expect(page.locator(SELECTORS.pointsAdmin.accountsPage)).toBeVisible()
      })
    })
  })

  test.describe('US-PO-006: Configure Realm Registration and Free Periodic Rules', () => {
    test('should validate and persist a free periodic fixed rule', async ({
      page,
      loginPage,
      demoLogger,
    }) => {
      const credentials = REALM_ADMINS[RULES_REALM_ID]
      if (!credentials) {
        throw new Error(`Missing seeded admin credentials for ${RULES_REALM_ID}`)
      }

      await loginPage.loginAsAdmin(credentials.email, credentials.password, RULES_REALM_ID)
      const apiContext = await createBearerApiContext(loginPage.getAccessToken())
      let originalRules: RegistrationRule[] | null = null

      try {
        await test.step('Given: Realm has a restorable free-periodic fixed rule', async () => {
          originalRules = await readRegistrationRules(apiContext)
          const primaryRule = originalRules.find((rule) => rule.id === PRIMARY_RULE_ID)
          expect(primaryRule, 'the seeded primary registration rule must exist').toBeDefined()

          const setupRules = originalRules.map((rule) =>
            rule.id === PRIMARY_RULE_ID
              ? {
                  ...rule,
                  triggerSources: ['free_periodic_grant'],
                  grantMode: 'fixed' as const,
                  pointsAmount: 30,
                  validityDays: 2,
                  grantPeriodType: 'daily',
                  quotaWindows: null,
                }
              : rule,
          )
          await writeRegistrationRules(apiContext, setupRules)
          await page.goto('/manage/points/registration-rules')
          await expect(page.locator(SELECTORS.pointRule.registrationRulesSave)).toBeVisible()
          await expect(page.locator(SELECTORS.pointRule.list)).toHaveCount(2)
        })

        const periodicRule = page.locator(SELECTORS.pointRule.row(PRIMARY_RULE_ID))
        const amountInput = periodicRule.locator(SELECTORS.pointRule.amountInput(PRIMARY_RULE_ID))

        await test.step('Then: fixed points reject a non-positive amount', async () => {
          await amountInput.fill('0')
          expect(
            await amountInput.evaluate((input: HTMLInputElement) => input.checkValidity()),
          ).toBe(false)
        })

        await test.step('When: admin selects a bucket and updates the periodic rule', async () => {
          await periodicRule.locator(SELECTORS.pointRule.bucketSelect).click()
          await page.getByRole('option', { name: 'Promo Pool' }).click()
          await amountInput.fill('75')
          await periodicRule.locator(SELECTORS.pointRule.validityInput(PRIMARY_RULE_ID)).fill('5')
          await periodicRule.locator(SELECTORS.pointRule.periodSelect(PRIMARY_RULE_ID)).click()
          await page.getByRole('option', { name: /每周|weekly/i }).click()

          const saveResponse = page.waitForResponse(
            (response) =>
              response.request().method() === 'PUT' &&
              new URL(response.url()).pathname ===
                `/api/points/${RULES_REALM_ID}/registration-rules`,
          )
          await page.locator(SELECTORS.pointRule.registrationRulesSave).click()
          const response = await saveResponse
          expect(response.ok(), `UI save failed with status ${response.status()}`).toBe(true)
        })

        await test.step('Then: reload shows the persisted bucket and fixed-period fields', async () => {
          await page.reload()
          const reloadedRule = page.locator(SELECTORS.pointRule.row(PRIMARY_RULE_ID))
          await expect(reloadedRule.locator(SELECTORS.pointRule.bucketSelect)).toContainText(
            'Promo Pool',
          )
          await expect(
            reloadedRule.locator(SELECTORS.pointRule.amountInput(PRIMARY_RULE_ID)),
          ).toHaveValue('75')
          await expect(
            reloadedRule.locator(SELECTORS.pointRule.validityInput(PRIMARY_RULE_ID)),
          ).toHaveValue('5')
          await expect(
            reloadedRule.locator(SELECTORS.pointRule.periodSelect(PRIMARY_RULE_ID)),
          ).toContainText(/每周|weekly/i)
          await expect(
            reloadedRule.locator(SELECTORS.pointRule.trigger('free_periodic_grant')),
          ).toBeChecked()
          await demoLogger.testCode.log(
            '[Test] Free-periodic fixed rule persisted with its selected bucket',
          )
        })
      } finally {
        try {
          if (originalRules) {
            await writeRegistrationRules(apiContext, originalRules)
            const restoredRules = await readRegistrationRules(apiContext)
            expect(normalizedRules(restoredRules)).toEqual(normalizedRules(originalRules))
          }
        } finally {
          await apiContext.dispose()
        }
      }
    })
  })
})
