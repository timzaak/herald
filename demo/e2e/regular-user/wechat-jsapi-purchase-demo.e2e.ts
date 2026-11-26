/**
 * WeChat Pay JSAPI (in-WeChat) Purchase Demo (US-WP-003)
 *
 * User Stories (published baselines this WeChat scenario extends):
 * - docs/user-stories/billing/payment-attempt.md:
 *   - US-PA-001..004: payment attempt lifecycle
 * WeChat-specific acceptance (US-WP-003 微信内 JSAPI 唤起支付) is draft-stage
 * and tracked in the feature workflow until PRD publish. The caller-supplied
 * openid is injected via the `wechatOpenid` URL search param (explicit
 * web-frontend contract; the payment API only receives, never fetches, the
 * openid).
 *
 * Coverage:
 *   - S1: inside WeChat's browser (MicroMessenger UA) WITH `wechatOpenid`, the
 *     unified order goes out as JSAPI carrying the payer openid (asserted on
 *     the v3 mock), the invoke UI renders, and a simulated WeChat callback
 *     flips the attempt to succeeded.
 *   - S2: inside WeChat's browser WITHOUT `wechatOpenid`, ordering is REFUSED
 *     — no attempt is created, no order reaches the provider, and the page
 *     stays on the packages step.
 *
 * Demo mechanism: same as the Native demo — local v3 mock via the
 * `base_url` override + forged signed callbacks. The whole file runs under a
 * MicroMessenger user agent (`test.use`) so the frontend scene decision picks
 * JSAPI. Headless Chrome has no WeixinJSBridge, so invoking shows the
 * bridge-unavailable feedback (shared `wechat-jsapi-result-fail` region) —
 * the payment outcome is driven by the callback, not the bridge.
 *
 * NOT covered (declared gap):
 *   - US-WP-003 S3 (bridge cancel/fail feedback variants): the bridge result
 *     is user feedback only and cannot be driven without a real WeChat client;
 *     the bridge-unavailable branch is exercised instead. Callback-driven
 *     failure feedback is covered by the Native demo's S3.
 *
 * Assertion discipline: persistent state only (step regions, invoke UI
 * regions, mock-recorded order bodies, localStorage absence). No toasts.
 *
 * Runner:
 *   uv run scripts/demo-test-runner.py \
 *     "demo/e2e/regular-user/wechat-jsapi-purchase-demo.e2e.ts" \
 *     --run-id <RUN_ID> --no-ngrok
 */

import { test, cleanupTestData, expect } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { loginWithCredentials } from '../helpers/auth'
import { SELECTORS } from '../selectors'
import { TEST_DATA, extractPaymentAttemptId } from '../helpers/unified-purchase.helpers'
import { selectPriceCard } from '../helpers/multi-price-purchase.helpers'
import {
  setupWechatPayRealm,
  WECHAT_DEMO,
  type WechatPayDemoEnv,
} from '../helpers/wechat-demo-setup'
import { deliverWechatCallback, forgeWechatCallback } from '../helpers/wechat-webhook-simulation'

const REALM_ID = TEST_DATA.REALMS.REALM_001
const USER_EMAIL = TEST_DATA.USERS.USER_REALM_001
const DEMO_OPENID = 'demo-openid-o001'

// WeChat's in-app browser UA — drives `isWeChatBrowser` to the JSAPI scene.
test.use({
  userAgent:
    'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) ' +
    'Chrome/126.0.0.0 Mobile Safari/537.36 MicroMessenger/8.0.49.2600',
})

let env: WechatPayDemoEnv | null = null

test.describe('[Regular User] wechat-support — in-WeChat JSAPI purchase', () => {
  let testStartTime: number

  test.beforeEach(async ({ page, loginPage, demoLogger }) => {
    testStartTime = Date.now()
    await verifyTestEnvironment(page, {
      requiredRealms: [REALM_ID],
      requiredUsers: [USER_EMAIL],
    })

    if (!env) {
      env = await setupWechatPayRealm(page, loginPage, REALM_ID)
      await demoLogger.testCode.log(`WeChat demo env seeded (mock ${env.mock.url})`)
    }

    await loginWithCredentials(page, {
      realmId: REALM_ID,
      email: USER_EMAIL,
      password: TEST_DATA.CREDENTIALS.DEFAULT_PASSWORD,
    })
    await page.waitForURL('**/user/profile**')
    await demoLogger.testCode.log('Regular user logged in (WeChat UA)')
  })

  test.afterEach(async ({ page, demoLogger }) => {
    await cleanupTestData(page, REALM_ID, {
      keepUsers: [USER_EMAIL],
      timestamp: testStartTime,
    })
    await demoLogger.testCode.log('Test data cleaned up')
  })

  test.afterAll(async () => {
    await env?.mock.close()
    env = null
  })

  // ==========================================================================
  // S1: openid provided → JSAPI order + invoke UI + callback success
  // ==========================================================================

  test('[US-WP-003 S1] jsapi order carries URL-injected openid; callback succeeds', async ({
    page,
    demoLogger,
  }) => {
    await test.step('Given the purchase page opened with wechatOpenid', async () => {
      await page.evaluate(() => localStorage.removeItem('cas-purchase-flow'))
      await page.goto(`/${REALM_ID}/user/purchase-points?wechatOpenid=${DEMO_OPENID}`)
      await expect(page.locator(SELECTORS.purchasePoints.page)).toBeVisible()
    })

    await test.step('When the user orders the WeChat pack', async () => {
      await selectPriceCard(page, WECHAT_DEMO.PRICE_ID)
      await expect(page.locator(SELECTORS.purchasePriceCard.nextButton)).toBeEnabled()
      await page.locator(SELECTORS.purchasePriceCard.nextButton).click()
      await expect(page.locator(SELECTORS.purchasePoints.stepProcessing)).toBeVisible({
        timeout: TEST_DATA.TIMEOUTS.ELEMENT_VISIBLE,
      })
    })

    const attemptId = await extractPaymentAttemptId(page)
    const order = env!.mock.jsapiOrders[env!.mock.jsapiOrders.length - 1]

    await test.step('Then the unified order is JSAPI and carries the payer openid', async () => {
      expect(order).toBeDefined()
      expect(order!.payerOpenid).toBe(DEMO_OPENID)
      expect(order!.amountTotal).toBe(WECHAT_DEMO.AMOUNT_FEN)
      await demoLogger.testCode.log(
        `Mock recorded jsapi order ${order!.outTradeNo} for openid ${DEMO_OPENID}`,
      )
    })

    await test.step('And the JSAPI invoke UI is rendered', async () => {
      await expect(page.locator(SELECTORS.wechatJsapiPayment.pending)).toBeVisible()
      await expect(page.locator(SELECTORS.wechatJsapiPayment.invokeButton)).toBeVisible()
      await demoLogger.testCode.log('JSAPI invoke UI rendered')
    })

    await test.step('When the user taps invoke (no WeixinJSBridge in demo)', async () => {
      await page.locator(SELECTORS.wechatJsapiPayment.invokeButton).click()
      // bridge_unavailable shares the destructive fail feedback region; the
      // attempt stays Pending (bridge result is feedback only).
      await expect(page.locator(SELECTORS.wechatJsapiPayment.resultFail)).toBeVisible()
      await demoLogger.testCode.log('Bridge-unavailable feedback shown (expected in demo)')
    })

    await test.step('When the WeChat callback arrives, payment succeeds', async () => {
      const material = env!.material
      const response = await deliverWechatCallback(
        page.request,
        REALM_ID,
        forgeWechatCallback({
          eventId: `evt-jsapi-${attemptId}`,
          outTradeNo: order!.outTradeNo,
          transactionId: `demo-txn-jsapi-${attemptId}`,
          tradeState: 'SUCCESS',
          amountTotal: WECHAT_DEMO.AMOUNT_FEN,
          apiV3Key: material.apiV3Key,
          platformPrivateKeyPem: material.platform.privateKeyPem,
        }),
      )
      expect(response.status()).toBe(200)
      // Success transitions the page to the complete step (the user-visible
      // success state — payment-status-succeeded only renders mid-processing).
      await expect(page.locator(SELECTORS.purchasePoints.stepComplete)).toBeVisible({
        timeout: 15000,
      })
      await demoLogger.testCode.log('JSAPI attempt succeeded via callback')
    })
  })

  // ==========================================================================
  // S2: missing openid → refuse to order
  // ==========================================================================

  test('[US-WP-003 S2] missing openid refuses to order (stays on packages)', async ({
    page,
    demoLogger,
  }) => {
    const jsapiOrderCountBefore = env!.mock.jsapiOrders.length
    const nativeOrderCountBefore = env!.mock.nativeOrders.length

    await test.step('Given the purchase page opened WITHOUT wechatOpenid', async () => {
      await page.evaluate(() => localStorage.removeItem('cas-purchase-flow'))
      await page.goto(`/${REALM_ID}/user/purchase-points`)
      await expect(page.locator(SELECTORS.purchasePoints.page)).toBeVisible()
    })

    await test.step('When the user selects the pack and proceeds', async () => {
      await selectPriceCard(page, WECHAT_DEMO.PRICE_ID)
      await expect(page.locator(SELECTORS.purchasePriceCard.nextButton)).toBeEnabled()
      await page.locator(SELECTORS.purchasePriceCard.nextButton).click()
      // Give the (refused) order mutation a moment before asserting absence.
      await page.waitForTimeout(2000)
    })

    await test.step('Then ordering is refused: no attempt, no provider order, stays on packages', async () => {
      // Persistent page state: still on the packages step, never advanced.
      await expect(page.locator(SELECTORS.purchasePoints.stepPackages)).toBeVisible()
      await expect(page.locator(SELECTORS.purchasePoints.stepProcessing)).toHaveCount(0)
      // No purchase attempt was persisted (localStorage state absent).
      const attemptId = await page.evaluate(() => {
        const state = localStorage.getItem('cas-purchase-flow')
        return state ? (JSON.parse(state)?.state?.attemptId ?? null) : null
      })
      expect(attemptId).toBeNull()
      // No unified order reached the provider mock (neither scene).
      expect(env!.mock.jsapiOrders.length).toBe(jsapiOrderCountBefore)
      expect(env!.mock.nativeOrders.length).toBe(nativeOrderCountBefore)
      await demoLogger.testCode.log('Order refused without openid; stayed on packages')
    })
  })
})
