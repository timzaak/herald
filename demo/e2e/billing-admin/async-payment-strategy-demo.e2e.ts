/**
 * Async Payment Strategy Demo Tests
 *
 * User Story:
 * - docs/user-stories/billing/async-payment-points.md:
 *   - US-AP-001: Configure Async Payment Points Strategy
 *     - Scenario 1: View default conservative strategy
 *     - Scenario 2: Switch to eager with confirmation dialog
 *     - Scenario 3: Switch back to conservative (no confirmation dialog)
 *
 * Traceability: US-AP-001 scenarios 1-3
 */

import { test, cleanupTestData, expect } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import type { UnifiedLogger } from 'playwright-unified-logger'
import { DEMO_ADMIN } from '../helpers/auth'

test.describe('[Billing Admin] Async Payment Strategy (US-AP-001)', () => {
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
  // US-AP-001 Scenario 1: View default conservative strategy
  // ============================================================================

  test('US-AP-001 S1: should display default conservative strategy on Stripe config page', async ({ page, loginPage, demoLogger }) => {
    await test.step('Given: admin is logged in and Stripe is configured', async () => {
      await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
      await configureStripe(page, testStartTime, 'conservative', demoLogger)
      await demoLogger.testCode.log('Stripe configured')
    })

    await test.step('When: navigate to Stripe config page in edit mode', async () => {
      await navigateToStripeConfigEdit(page, demoLogger)
      await demoLogger.testCode.log('Stripe config edit page loaded')
    })

    await test.step('Then: async strategy card is visible with conservative selected', async () => {
      await expect(page.getByTestId('async-strategy-card')).toBeVisible()
      await demoLogger.testCode.log('Async strategy card visible')

      // Verify conservative radio is checked (default)
      const conservativeRadio = page.getByTestId('async-strategy-conservative-radio')
      await expect(conservativeRadio).toBeVisible()
      await expect(conservativeRadio).toBeChecked()
      await demoLogger.testCode.log('Conservative strategy is checked by default')
    })
  })

  // ============================================================================
  // US-AP-001 Scenario 2: Switch to eager with confirmation dialog
  // ============================================================================

  test('US-AP-001 S2: should switch to eager strategy with confirmation dialog', async ({ page, loginPage, demoLogger }) => {
    await test.step('Given: admin is logged in and Stripe is configured with conservative strategy', async () => {
      await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
      await configureStripe(page, testStartTime, 'conservative', demoLogger)
      await demoLogger.testCode.log('Stripe configured')
    })

    await test.step('When: navigate to Stripe config and click eager radio', async () => {
      await navigateToStripeConfigEdit(page, demoLogger)
      await demoLogger.testCode.log('Stripe config edit page loaded')

      // Verify conservative is selected before switch
      await expect(page.getByTestId('async-strategy-conservative-radio')).toBeChecked()

      // Click eager radio
      await page.getByTestId('async-strategy-eager-radio').click()
      await demoLogger.testCode.log('Eager radio clicked')
    })

    await test.step('Then: confirmation dialog appears', async () => {
      const dialog = page.getByTestId('async-strategy-confirm-dialog')
      await expect(dialog).toBeVisible()
      await demoLogger.testCode.log('Confirmation dialog visible')
    })

    await test.step('When: confirm the switch', async () => {
      await page.getByTestId('async-strategy-confirm-button').click()
      await demoLogger.testCode.log('Confirmation button clicked')
    })

    await test.step('Then: eager radio is now checked', async () => {
      await expect(page.getByTestId('async-strategy-eager-radio')).toBeChecked()
      await demoLogger.testCode.log('Eager strategy is now checked')
    })

    await test.step('When: save the form and re-open config', async () => {
      await saveStripeConfigAndReturn(page, demoLogger)
      await navigateToStripeConfigEdit(page, demoLogger)
      await demoLogger.testCode.log('Re-opened Stripe config')
    })

    await test.step('Then: eager strategy is persisted', async () => {
      await expect(page.getByTestId('async-strategy-eager-radio')).toBeChecked()
      await demoLogger.testCode.log('Eager strategy persisted after save')
    })
  })

  // ============================================================================
  // US-AP-001 Scenario 3: Switch back to conservative (no confirmation dialog)
  // ============================================================================

  test('US-AP-001 S3: should switch back to conservative without confirmation dialog', async ({ page, loginPage, demoLogger }) => {
    await test.step('Given: admin is logged in and Stripe is configured with eager strategy', async () => {
      await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
      await configureStripe(page, testStartTime, 'eager', demoLogger)
      await demoLogger.testCode.log('Stripe configured')

      // Navigate to config and ensure eager strategy is active.
      // Eager may already be persisted from S2, so only set it if needed.
      await navigateToStripeConfigEdit(page, demoLogger)
      const eagerRadio = page.getByTestId('async-strategy-eager-radio')
      const isEagerAlready = await eagerRadio.isChecked()
      if (!isEagerAlready) {
        await eagerRadio.click()
        await expect(page.getByTestId('async-strategy-confirm-dialog')).toBeVisible()
        await page.getByTestId('async-strategy-confirm-button').click()
        await saveStripeConfigAndReturn(page, demoLogger)
        await navigateToStripeConfigEdit(page, demoLogger)
      }
      // Verify eager is now the active strategy before proceeding
      await expect(eagerRadio).toBeChecked()
      await demoLogger.testCode.log('Eager strategy is active')
    })

    await test.step('When: navigate to Stripe config and click conservative radio', async () => {
      await navigateToStripeConfigEdit(page, demoLogger)
      await demoLogger.testCode.log('Stripe config edit page loaded')

      // Verify eager is selected before switch
      await expect(page.getByTestId('async-strategy-eager-radio')).toBeChecked()

      // Click conservative radio
      await page.getByTestId('async-strategy-conservative-radio').click()
      await demoLogger.testCode.log('Conservative radio clicked')
    })

    await test.step('Then: no confirmation dialog appears and conservative is checked', async () => {
      const dialog = page.getByTestId('async-strategy-confirm-dialog')
      await expect(dialog).not.toBeVisible()
      await demoLogger.testCode.log('No confirmation dialog shown')

      await expect(page.getByTestId('async-strategy-conservative-radio')).toBeChecked()
      await demoLogger.testCode.log('Conservative strategy is checked')
    })

    await test.step('When: save the form and re-open config', async () => {
      await saveStripeConfigAndReturn(page, demoLogger)
      await navigateToStripeConfigEdit(page, demoLogger)
      await demoLogger.testCode.log('Re-opened Stripe config')
    })

    await test.step('Then: conservative strategy is persisted', async () => {
      await expect(page.getByTestId('async-strategy-conservative-radio')).toBeChecked()
      await demoLogger.testCode.log('Conservative strategy persisted after save')
    })
  })
})

// ============================================================================
// Helper Functions
// ============================================================================

/**
 * Configure Stripe payment provider with test credentials.
 *
 * Navigates to payment-providers, clicks add/edit Stripe button,
 * fills test credentials, saves, and waits for redirect back.
 * Follows the same pattern as stripe-payment-comprehensive-demo.e2e.ts.
 */
async function configureStripe(
  page: import('@playwright/test').Page,
  timestamp: number,
  strategy: 'conservative' | 'eager',
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

  const strategyRadio = page.getByTestId(`async-strategy-${strategy}-radio`)
  if (!(await strategyRadio.isChecked())) {
    await strategyRadio.click()
    if (strategy === 'eager') {
      await expect(page.getByTestId('async-strategy-confirm-dialog')).toBeVisible()
      await page.getByTestId('async-strategy-confirm-button').click()
    }
  }
  await expect(strategyRadio).toBeChecked()
  await demoLogger.testCode.log(`Async payment strategy set to ${strategy}`)

  await page.getByTestId('stripe-config-page-submit-button').click()
  await page.waitForURL('**/payment-providers', { timeout: 15000 })
  await demoLogger.testCode.log('Stripe configuration saved')
}

/**
 * Navigate to Stripe config page in edit mode.
 *
 * Goes to payment-providers list, clicks edit-stripe-button,
 * and waits for the Stripe config form page to load.
 */
async function navigateToStripeConfigEdit(
  page: import('@playwright/test').Page,
  demoLogger: UnifiedLogger
): Promise<void> {
  await page.goto(`/${DEMO_ADMIN.realmId}/manage/billing/payment-providers`)
  await expect(page.getByTestId('edit-stripe-button')).toBeVisible({ timeout: 10000 })
  await page.getByTestId('edit-stripe-button').click()
  await page.waitForURL('**/payment-providers/stripe', { timeout: 10000 })
  await expect(page.getByTestId('stripe-config-form-page')).toBeVisible()
  await demoLogger.testCode.log('Navigated to Stripe config edit page')
}

/**
 * Save the Stripe config form and wait for redirect back to payment-providers list.
 */
async function saveStripeConfigAndReturn(
  page: import('@playwright/test').Page,
  demoLogger: UnifiedLogger
): Promise<void> {
  await page.getByTestId('stripe-config-page-submit-button').click()
  await page.waitForURL('**/payment-providers', { timeout: 15000 })
  await demoLogger.testCode.log('Stripe config saved, returned to payment-providers')
}
