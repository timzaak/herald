/**
 * Multi-wallet purchase distribution Demo.
 *
 * Draft user story: US-MWGR-003
 * Source: docs/user-stories/billing/multi-wallet-grant-rules.md
 */

import { expect } from '@playwright/test'

import { test, cleanupTestData } from '../fixtures/demo-page.fixtures'
import { loginWithCredentials } from '../helpers/auth'
import { fulfillPayment } from '../helpers/payment-simulation'
import {
  initiateMultiPriceCheckout,
  selectPriceCard,
} from '../helpers/multi-price-purchase.helpers'
import { extractPaymentAttemptId } from '../helpers/unified-purchase.helpers'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { SELECTORS } from '../selectors'

const REALM_ID = 'realm-001'
const USER_EMAIL = 'user@realm-001.com'
const USER_PASSWORD = 'password'
const MULTI_WALLET_MAPPING_ID = '0198f21a-1111-7000-8000-000000000001'

function parseAmount(text: string | null): number {
  const normalized = (text ?? '').replace(/[^\d.-]/g, '')
  const value = Number(normalized)
  if (!Number.isFinite(value)) {
    throw new Error(`Could not parse points amount from ${JSON.stringify(text)}`)
  }
  return value
}

test.describe('[Regular User] 多账户购买分发 (US-MWGR-003)', () => {
  test.beforeEach(async ({ page }) => {
    await verifyTestEnvironment(page, {
      requiredRealms: [REALM_ID],
      requiredUsers: [USER_EMAIL],
    })
    await loginWithCredentials(page, {
      realmId: REALM_ID,
      email: USER_EMAIL,
      password: USER_PASSWORD,
    })
  })

  test.afterEach(async ({ page, testStartTime }) => {
    await cleanupTestData(page, REALM_ID, {
      keepUsers: [USER_EMAIL],
      timestamp: testStartTime,
    })
  })

  test('US-MWGR-003 场景1/2: 一次购买向两个账户发放且重复履约不重复到账', async ({
    page,
    request,
    demoLogger,
  }) => {
    void demoLogger
    let attemptId = ''

    await test.step('Given: 购买页展示两条目标账户规则', async () => {
      await page.evaluate(() => localStorage.removeItem('cas-purchase-flow'))
      await page.goto(`/user/purchase-points`)
      await expect(page.locator(SELECTORS.purchasePoints.page)).toBeVisible()

      const card = page.locator(
        SELECTORS.purchasePriceCard.priceCard(MULTI_WALLET_MAPPING_ID),
      )
      await expect(card).toBeVisible()
      await expect(card.locator(SELECTORS.purchasePointRule.row(
        '0198f21a-1111-7000-8000-000000000011',
      ))).toBeVisible()
      await expect(card.locator(SELECTORS.purchasePointRule.row(
        '0198f21a-1111-7000-8000-000000000012',
      ))).toBeVisible()
    })

    await test.step('When: 用户发起购买并完成模拟支付', async () => {
      await selectPriceCard(page, MULTI_WALLET_MAPPING_ID)
      const checkoutResponse = await initiateMultiPriceCheckout(page, {
        mappingId: MULTI_WALLET_MAPPING_ID,
        paymentProvider: 'stripe',
      })
      expect(checkoutResponse.ok()).toBe(true)
      await page.goto(`/user/purchase-points`)
      attemptId = await extractPaymentAttemptId(page)

      const fulfillment = await fulfillPayment(request, REALM_ID, attemptId)
      expect(fulfillment.success, fulfillment.error).toBe(true)
      expect(fulfillment.pointGrants).toHaveLength(2)
      expect(new Set(fulfillment.pointGrants?.map((grant) => grant.bucketId)).size).toBe(2)
      expect(fulfillment.pointGrants?.every((grant) => (grant.points ?? 0) > 0)).toBe(true)
    })

    const balancesAfterFirstGrant: Record<string, number> = {}
    let firstResultIds: string[] = []

    await test.step('Then: 两个目标账户分别显示到账余额', async () => {
      const firstFulfillment = await fulfillPayment(request, REALM_ID, attemptId)
      expect(firstFulfillment.success, firstFulfillment.error).toBe(true)
      const grants = firstFulfillment.pointGrants ?? []
      firstResultIds = grants.map((grant) => grant.resultId).sort()

      await page.goto(`/user/points`)
      await expect(page.locator(SELECTORS.pointsUser.page)).toBeVisible()

      for (const grant of grants) {
        const balance = page.locator(
          SELECTORS.pointsUser.balanceTotalByBucket(grant.bucketId),
        )
        await expect(balance).toBeVisible()
        const amount = parseAmount(await balance.textContent())
        expect(amount).toBeGreaterThanOrEqual(grant.points ?? 0)
        balancesAfterFirstGrant[grant.bucketId] = amount
      }
    })

    await test.step('And: 再次履约返回原结果且账户余额不变', async () => {
      const replay = await fulfillPayment(request, REALM_ID, attemptId)
      expect(replay.success, replay.error).toBe(true)
      expect((replay.pointGrants ?? []).map((grant) => grant.resultId).sort()).toEqual(
        firstResultIds,
      )

      await page.reload()
      for (const [bucketId, previousBalance] of Object.entries(balancesAfterFirstGrant)) {
        const balance = page.locator(
          SELECTORS.pointsUser.balanceTotalByBucket(bucketId),
        )
        await expect(balance).toBeVisible()
        expect(parseAmount(await balance.textContent())).toBe(previousBalance)
      }
    })
  })
})
