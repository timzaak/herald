/**
 * Live One-Time Mapping Purchase Flow Test
 *
 * Related User Stories: US-PU-006, US-PU-006 S2, US-PA-001, US-PA-002
 * Coverage: partial live smoke; Stripe/Creem redirect initiation,
 *   payment state recovery after page refresh.
 * Not Covered: payment completion via webhook callback, points fulfillment, expired/failed
 *   payment, cancel flow, idempotency, or audit outcomes.
 * Live Dependency: real Stripe / Creem credentials and webhook endpoints
 * Manual Step: none for flow initiation; payment completion requires external webhook callback
 * Run Command:
 *   cd demo
 *   npx playwright test e2e/live/billing/one-time-mapping-purchase/us-pu-006-one-time-purchase-live.e2e.ts --project=demo-fast
 * Skip/Fail Policy:
 *   Fails loud when payment attempt creation returns an error from the backend.
 *   Tests only verify payment INITIATION and UI states, not fulfillment.
 *
 * Uses Demo Seed data (realm-001 with pre-configured one-time entitlement mappings).
 *
 * NOTE: Payment completion requires webhook simulation by external services or manual
 * internal API calls. Tests verify payment INITIATION and UI states only.
 *
 * Prerequisites:
 *   - Stripe / Creem credentials configured in realm_config for realm-001
 *     (seeded by demo_seed.py)
 *   - Demo seed data loaded (realm-001, user@realm-001.com, one-time entitlement mappings)
 *   - Backend and frontend running
 */

import { test, expect } from '../../../fixtures/demo-auth.fixtures'
import { verifyTestEnvironment, cleanupDemoTestData } from '../../../helpers/environment-setup'
import { loginWithCredentials } from '../../../helpers/auth'
import { SELECTORS } from '../../../selectors'
import {
  initiatePurchaseFlow,
  selectFirstMappingAndProceed,
  selectPaymentMethodAndProceed,
  extractPaymentAttemptId,
  verifyRedirectPromptOrDegraded,
  TEST_DATA,
} from '../../../helpers/unified-purchase.helpers'

const REALM_ID = TEST_DATA.REALMS.REALM_001
const USER_EMAIL = TEST_DATA.USERS.USER_REALM_001

test.describe('[Live][Billing One-Time Mapping] US-PU-006: One-Time Mapping Purchase Flow', () => {
  test.beforeEach(async ({ page, demoLogger }) => {
    await verifyTestEnvironment(page, {
      requiredRealms: [REALM_ID],
      requiredUsers: [USER_EMAIL],
    })

    await loginWithCredentials(page, {
      realmId: REALM_ID,
      email: USER_EMAIL,
      password: TEST_DATA.CREDENTIALS.DEFAULT_PASSWORD,
    })

    await page.waitForURL(`**/${REALM_ID}/user**`)
    demoLogger.testCode.log(`[Live] ✓ logged in as ${USER_EMAIL} @ ${REALM_ID}`)
  })

  test.afterEach(async ({ page, testStartTime, demoLogger }) => {
    await cleanupDemoTestData(page, REALM_ID, {
      keepUsers: [USER_EMAIL],
      timestamp: testStartTime,
    })
    demoLogger.testCode.log('[Live] ✓ test data cleanup complete')
  })

  test('should initiate Stripe redirect payment when selecting mapping and Stripe provider (US-PU-006 S2)', async ({
    page,
    demoLogger,
  }) => {
    await test.step('Clear previous purchase state', async () => {
      await page.evaluate(() => localStorage.removeItem('cas-purchase-flow'))
    })

    await test.step('Navigate to purchase page', async () => {
      await page.goto(`/${REALM_ID}/user/purchase-points`)
      await expect(page.locator(SELECTORS.purchasePoints.page)).toBeVisible()
    })

    await test.step('Select first mapping card and proceed to payment', async () => {
      await selectFirstMappingAndProceed(page)
      await expect(page.locator(SELECTORS.purchasePoints.stepPayment)).toBeVisible()
    })

    await test.step('Select Stripe payment method', async () => {
      await selectPaymentMethodAndProceed(page, TEST_DATA.PAYMENT_PROVIDERS.STRIPE)
    })

    await test.step('Verify processing step is visible', async () => {
      await expect(page.locator(SELECTORS.purchasePoints.stepProcessing)).toBeVisible({
        timeout: TEST_DATA.TIMEOUTS.ELEMENT_VISIBLE,
      })
    })

    await test.step('Verify redirect prompt or degraded UI', async () => {
      // Stripe may show redirect prompt (with checkout URL) or degraded UI (without)
      await verifyRedirectPromptOrDegraded(page, 'Stripe')
    })

    await test.step('Verify payment attempt ID persisted in localStorage', async () => {
      const attemptId = await extractPaymentAttemptId(page)
      expect(attemptId).toBeTruthy()
      demoLogger.testCode.log(`[Live] ✓ Stripe redirect attempt persisted: ${attemptId}`)
    })
  })

  test('should initiate Creem redirect payment when selecting mapping and Creem provider (US-PU-006 S2, US-PA-001)', async ({
    page,
    demoLogger,
  }) => {
    await test.step('Clear previous purchase state', async () => {
      await page.evaluate(() => localStorage.removeItem('cas-purchase-flow'))
    })

    await test.step('Navigate to purchase page', async () => {
      await page.goto(`/${REALM_ID}/user/purchase-points`)
      await expect(page.locator(SELECTORS.purchasePoints.page)).toBeVisible()
    })

    await test.step('Select first mapping card and proceed to payment', async () => {
      await selectFirstMappingAndProceed(page)
      await expect(page.locator(SELECTORS.purchasePoints.stepPayment)).toBeVisible()
    })

    await test.step('Select Creem payment method', async () => {
      await selectPaymentMethodAndProceed(page, TEST_DATA.PAYMENT_PROVIDERS.CREEM)
    })

    await test.step('Verify processing step is visible', async () => {
      await expect(page.locator(SELECTORS.purchasePoints.stepProcessing)).toBeVisible({
        timeout: TEST_DATA.TIMEOUTS.ELEMENT_VISIBLE,
      })
    })

    await test.step('Verify redirect prompt or degraded UI', async () => {
      await verifyRedirectPromptOrDegraded(page, 'Creem')
    })

    await test.step('Verify payment attempt ID persisted in localStorage', async () => {
      const attemptId = await extractPaymentAttemptId(page)
      expect(attemptId).toBeTruthy()
      demoLogger.testCode.log(`[Live] ✓ Creem redirect attempt persisted: ${attemptId}`)
    })
  })

  test('should recover payment state after page refresh (US-PU-006)', async ({
    page,
    demoLogger,
  }) => {
    let attemptIdBeforeRefresh: string

    await test.step('Initiate Stripe purchase flow via helper', async () => {
      attemptIdBeforeRefresh = await initiatePurchaseFlow(
        page,
        TEST_DATA.PAYMENT_PROVIDERS.STRIPE,
        REALM_ID
      )
      expect(attemptIdBeforeRefresh).toBeTruthy()
    })

    await test.step('Verify processing step visible before refresh', async () => {
      await expect(page.locator(SELECTORS.purchasePoints.stepProcessing)).toBeVisible()
    })

    await test.step('Reload page', async () => {
      await page.reload()
      await page.waitForLoadState('domcontentloaded')
    })

    await test.step('Verify processing step is still visible after refresh', async () => {
      await expect(page.locator(SELECTORS.purchasePoints.stepProcessing)).toBeVisible({
        timeout: TEST_DATA.TIMEOUTS.ELEMENT_VISIBLE,
      })
    })

    await test.step('Verify attempt ID is preserved after refresh', async () => {
      const attemptIdAfterRefresh = await page.evaluate(() => {
        const state = localStorage.getItem('cas-purchase-flow')
        if (state) {
          const parsed = JSON.parse(state)
          return parsed?.state?.attemptId
        }
        return null
      })

      expect(attemptIdAfterRefresh).toBe(attemptIdBeforeRefresh)
      demoLogger.testCode.log(`[Live] ✓ attempt id preserved across refresh: ${attemptIdAfterRefresh}`)
    })
  })
})
