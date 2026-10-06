/**
 * Invoice Fallback Admin Demo Tests
 *
 * User Stories:
 * - US-IF-001: Configure invoice policy (provider_first/manual_only/none)
 *   and per-provider external invoice capability toggle
 * - US-IF-004: Admin views external provider invoices with provider column,
 *   provider filter, and readonly actions
 *
 * User Stories: docs/user-stories/billing/invoice-fallback.md
 *
 * Coverage gaps (accepted, better suited for backend integration tests):
 * - US-IF-001 Scenario 4: Creem MoR override protection
 * - US-IF-001 Scenario 5: Unconfigured provider hidden from policy config
 *
 * UnifiedLogger Usage:
 * - All tests use UnifiedLogger through 'demoLogger' fixture
 * - Logger is automatically initialized and finalized by fixture
 * - Logs are saved to demo/test-results/console-logs/
 */

import { test, cleanupTestData, expect } from '../fixtures/demo-page.fixtures'
import { DEMO_ADMIN } from '../helpers/auth'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import {
  navigateToInvoiceAdminPage,
  createInvoice,
  verifyInvoiceInTable,
} from './helpers/invoice-helpers'
import {
  seedExternalInvoice,
  openPolicyConfigDialog,
  setInvoicePolicy,
  toggleProviderCapability,
  verifyProviderColumnInRow,
  verifyExternalInvoiceActions,
  verifyProviderBannerInDetail,
  verifyNoProviderBannerInDetail,
} from './helpers/invoice-fallback-helpers'

/**
 * Compute a future due date string (ISO format YYYY-MM-DD).
 * Returns a date 30 days from now.
 */
function futureDueDate(): string {
  const d = new Date()
  d.setDate(d.getDate() + 30)
  return d.toISOString().slice(0, 10)
}

test.describe('[Billing Admin] Invoice Fallback Admin Demo Tests', () => {
  // Verify test environment before each test
  test.beforeEach(async ({ page }) => {
    await verifyTestEnvironment(page, {
      requiredRealms: [DEMO_ADMIN.realmId],
      requiredUsers: [DEMO_ADMIN.email],
    })
  })

  // Single test.afterEach for cleanup
  test.afterEach(async ({ page, testStartTime }) => {
    await cleanupTestData(page, DEMO_ADMIN.realmId, {
      keepUsers: [DEMO_ADMIN.email],
      timestamp: testStartTime,
    })
  })

  // ============================================================================
  // User Story US-IF-001: Invoice Policy Configuration
  // ============================================================================

  test.describe('US-IF-001: Invoice Policy Configuration', () => {
    test('should open policy config dialog and display policy options', async ({
      page,
      loginPage,
      demoLogger,
    }) => {
      await test.step('Given: admin is logged in', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
      })

      await test.step('And: admin navigates to invoice admin page', async () => {
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
      })

      await test.step('When: click policy config button', async () => {
        await page.getByTestId('policy-config-button').click()
      })

      await test.step('Then: policy config dialog is visible', async () => {
        await expect(page.getByTestId('invoice-policy-form-dialog')).toBeVisible({ timeout: 5000 })
      })

      await test.step('And: policy select input is present', async () => {
        await expect(page.getByTestId('invoice-policy-select')).toBeVisible()
      })

      await test.step('And: save and cancel buttons are present', async () => {
        await expect(page.getByTestId('invoice-policy-save-button')).toBeVisible()
        await expect(page.getByTestId('invoice-policy-cancel-button')).toBeVisible()
      })

      await test.step('Cleanup: close the dialog', async () => {
        await page.getByTestId('invoice-policy-cancel-button').click()
        await expect(page.getByTestId('invoice-policy-form-dialog')).toBeHidden({ timeout: 5000 })
      })

      await demoLogger.testCode.log('Policy config dialog verified')
    })

    test('should set policy to provider_first and enable Stripe external invoice', async ({
      page,
      loginPage,
      demoLogger,
    }) => {
      await test.step('Given: admin is logged in and on invoice admin page', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
      })

      await test.step('When: set policy to provider_first', async () => {
        await setInvoicePolicy(page, 'provider_first')
      })

      await test.step('Then: policy config dialog closes after save', async () => {
        await expect(page.getByTestId('invoice-policy-form-dialog')).toBeHidden({ timeout: 5000 })
      })

      await test.step('When: open policy dialog and enable Stripe external invoice', async () => {
        await toggleProviderCapability(page, 'stripe', true)
      })

      await test.step('Then: dialog closes after save', async () => {
        await expect(page.getByTestId('invoice-policy-form-dialog')).toBeHidden({ timeout: 5000 })
      })

      await test.step('And: settings persist after re-opening', async () => {
        await openPolicyConfigDialog(page)

        // Verify Stripe switch is checked
        const stripeSwitch = page.getByTestId('invoice-policy-stripe-switch')
        await expect(stripeSwitch).toBeVisible()
        await expect(stripeSwitch).toBeChecked()

        // Verify the policy select still shows provider_first
        // The select trigger displays the selected value text
        const policySelect = page.getByTestId('invoice-policy-select')
        await expect(policySelect).toContainText('Provider First')

        // Close dialog
        await page.getByTestId('invoice-policy-cancel-button').click()
        await expect(page.getByTestId('invoice-policy-form-dialog')).toBeHidden({ timeout: 5000 })
      })

      await demoLogger.testCode.log('Provider_first policy with Stripe capability verified')
    })

    test('should set policy to manual_only', async ({
      page,
      loginPage,
      demoLogger,
    }) => {
      await test.step('Given: admin is logged in and on invoice admin page', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
      })

      await test.step('When: set policy to manual_only', async () => {
        await setInvoicePolicy(page, 'manual_only')
      })

      await test.step('Then: dialog closes after save', async () => {
        await expect(page.getByTestId('invoice-policy-form-dialog')).toBeHidden({ timeout: 5000 })
      })

      await test.step('And: policy persists after re-opening', async () => {
        await openPolicyConfigDialog(page)

        const policySelect = page.getByTestId('invoice-policy-select')
        await expect(policySelect).toContainText('Manual Only')

        // Close dialog
        await page.getByTestId('invoice-policy-cancel-button').click()
        await expect(page.getByTestId('invoice-policy-form-dialog')).toBeHidden({ timeout: 5000 })
      })

      await demoLogger.testCode.log('Manual_only policy verified')
    })

    test('should set policy to none', async ({
      page,
      loginPage,
      demoLogger,
    }) => {
      await test.step('Given: admin is logged in and on invoice admin page', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
      })

      await test.step('When: set policy to none', async () => {
        await setInvoicePolicy(page, 'none')
      })

      await test.step('Then: dialog closes after save', async () => {
        await expect(page.getByTestId('invoice-policy-form-dialog')).toBeHidden({ timeout: 5000 })
      })

      await test.step('And: policy persists after re-opening', async () => {
        await openPolicyConfigDialog(page)

        const policySelect = page.getByTestId('invoice-policy-select')
        // The i18n key for 'none' renders as "Disabled"
        await expect(policySelect).toContainText('Disabled')

        // Close dialog
        await page.getByTestId('invoice-policy-cancel-button').click()
        await expect(page.getByTestId('invoice-policy-form-dialog')).toBeHidden({ timeout: 5000 })
      })

      await demoLogger.testCode.log('None (disabled) policy verified')
    })
  })

  // ============================================================================
  // User Story US-IF-004: Admin External Invoice Viewing
  // ============================================================================

  test.describe('US-IF-004: Admin External Invoice Viewing', () => {
    test('should display provider column in invoice table', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const billingName = `buyer-provider-${testStartTime}`
      const sellerName = `seller-provider-${testStartTime}`
      let userId: string
      let externalInvoice: ReturnType<typeof seedExternalInvoice>

      await test.step('Given: admin is logged in and on invoice admin page', async () => {
        userId = await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
      })

      await test.step('And: a manual invoice and a Stripe external invoice exist', async () => {
        // Create a manual invoice via UI
        await createInvoice(page, DEMO_ADMIN.realmId, {
          accountId: userId,
          billingName,
          sellerName,
          lineItems: [{ name: 'Manual Plan', quantity: '1', unitPrice: 5000 }],
          dueDate: futureDueDate(),
        })
        await verifyInvoiceInTable(page, billingName, 'draft')

        // Seed an external Stripe invoice via DB
        externalInvoice = seedExternalInvoice(DEMO_ADMIN.realmId, {
          provider: 'stripe',
          total: 2000,
        })

        // Reload the page to pick up the seeded invoice
        await page.reload()
        await expect(page.getByTestId('invoice-admin-page')).toBeVisible({ timeout: 10000 })
      })

      await test.step('Then: manual invoice row shows Manual provider badge', async () => {
        const manualRow = page.locator('tr').filter({ hasText: billingName }).first()
        await expect(manualRow).toBeVisible({ timeout: 10000 })
        await verifyProviderColumnInRow(manualRow, 'manual')
      })

      await test.step('And: Stripe invoice row shows Stripe provider badge', async () => {
        const stripeRow = page.locator('tr').filter({ hasText: externalInvoice!.invoiceNumber }).first()
        await expect(stripeRow).toBeVisible({ timeout: 10000 })
        await verifyProviderColumnInRow(stripeRow, 'stripe')
      })

      await demoLogger.testCode.log('Provider column badges verified')
    })

    test('should filter invoices by provider', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const billingName = `buyer-filter-${testStartTime}`
      const sellerName = `seller-filter-${testStartTime}`
      let userId: string
      let externalInvoice: ReturnType<typeof seedExternalInvoice>

      await test.step('Given: admin is logged in with both manual and Stripe invoices', async () => {
        userId = await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)

        await createInvoice(page, DEMO_ADMIN.realmId, {
          accountId: userId,
          billingName,
          sellerName,
          lineItems: [{ name: 'Filter Item', quantity: '1', unitPrice: 3000 }],
          dueDate: futureDueDate(),
        })

        externalInvoice = seedExternalInvoice(DEMO_ADMIN.realmId, {
          provider: 'stripe',
          total: 1500,
        })

        await page.reload()
        await expect(page.getByTestId('invoice-admin-page')).toBeVisible({ timeout: 10000 })
      })

      await test.step('When: filter by provider Stripe', async () => {
        await page.getByTestId('invoice-provider-filter').click()
        await page.getByRole('option', { name: 'Stripe' }).click()
        await page.waitForLoadState('networkidle')
        await page.waitForTimeout(300)
      })

      await test.step('Then: only Stripe invoices are visible', async () => {
        const stripeRow = page.locator('tr').filter({ hasText: externalInvoice!.invoiceNumber }).first()
        await expect(stripeRow).toBeVisible({ timeout: 10000 })

        // Manual invoice should be hidden
        const manualRow = page.locator('tr').filter({ hasText: billingName }).first()
        await expect(manualRow).not.toBeVisible()
      })

      await test.step('When: filter by provider Manual', async () => {
        await page.getByTestId('invoice-provider-filter').click()
        await page.getByRole('option', { name: 'Manual' }).click()
        await page.waitForLoadState('networkidle')
        await page.waitForTimeout(300)
      })

      await test.step('Then: only manual invoices are visible', async () => {
        const manualRow = page.locator('tr').filter({ hasText: billingName }).first()
        await expect(manualRow).toBeVisible({ timeout: 10000 })

        // Stripe invoice should be hidden
        const stripeRow = page.locator('tr').filter({ hasText: externalInvoice!.invoiceNumber }).first()
        await expect(stripeRow).not.toBeVisible()
      })

      await test.step('When: reset filter to All Providers', async () => {
        await page.getByTestId('invoice-provider-filter').click()
        await page.getByRole('option', { name: 'All Providers' }).click()
        await page.waitForLoadState('networkidle')
        await page.waitForTimeout(300)
      })

      await test.step('Then: both invoices are visible again', async () => {
        const manualRow = page.locator('tr').filter({ hasText: billingName }).first()
        await expect(manualRow).toBeVisible({ timeout: 10000 })

        const stripeRow = page.locator('tr').filter({ hasText: externalInvoice!.invoiceNumber }).first()
        await expect(stripeRow).toBeVisible({ timeout: 10000 })
      })

      await demoLogger.testCode.log('Provider filter verified')
    })

    test('should show readonly actions for external invoices', async ({
      page,
      loginPage,
      demoLogger,
    }) => {
      let externalInvoice: ReturnType<typeof seedExternalInvoice>

      await test.step('Given: admin is logged in and on invoice admin page', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
      })

      await test.step('And: an external Stripe invoice exists', async () => {
        externalInvoice = seedExternalInvoice(DEMO_ADMIN.realmId, {
          provider: 'stripe',
          total: 2500,
        })

        await page.reload()
        await expect(page.getByTestId('invoice-admin-page')).toBeVisible({ timeout: 10000 })
      })

      await test.step('Then: external invoice shows only View action (readonly)', async () => {
        // Verify the external invoice is visible in the table first
        const stripeRow = page.locator('tr').filter({ hasText: externalInvoice!.invoiceNumber }).first()
        await expect(stripeRow).toBeVisible({ timeout: 10000 })

        await verifyExternalInvoiceActions(page, externalInvoice!.id)
      })

      await demoLogger.testCode.log('External invoice readonly actions verified')
    })

    test('should display provider banner and external link in detail dialog', async ({
      page,
      loginPage,
      demoLogger,
    }) => {
      let externalInvoice: ReturnType<typeof seedExternalInvoice>

      await test.step('Given: admin is logged in and on invoice admin page', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
      })

      await test.step('And: an external Stripe invoice with hosted URL exists', async () => {
        externalInvoice = seedExternalInvoice(DEMO_ADMIN.realmId, {
          provider: 'stripe',
          total: 3000,
          externalHostedUrl: 'https://pay.stripe.com/invoice/test-123',
        })

        await page.reload()
        await expect(page.getByTestId('invoice-admin-page')).toBeVisible({ timeout: 10000 })
      })

      await test.step('When: open detail dialog for the external invoice', async () => {
        const stripeRow = page.locator('tr').filter({ hasText: externalInvoice!.invoiceNumber }).first()
        await expect(stripeRow).toBeVisible({ timeout: 10000 })

        // Open action menu and click View
        await page.getByTestId(`invoice-actions-menu-${externalInvoice!.id}`).click()
        await page.getByTestId(`invoice-view-${externalInvoice!.id}`).click()
      })

      await test.step('Then: detail dialog is visible', async () => {
        await expect(page.getByTestId('invoice-detail-dialog')).toBeVisible({ timeout: 10000 })
      })

      await test.step('And: provider banner shows Stripe text', async () => {
        await verifyProviderBannerInDetail(page, 'stripe')
      })

      await test.step('And: view in provider button is visible', async () => {
        await expect(page.getByTestId('invoice-view-in-provider-button')).toBeVisible()
      })

      await test.step('Cleanup: close the detail dialog', async () => {
        await page.keyboard.press('Escape')
        await expect(page.getByTestId('invoice-detail-dialog')).toBeHidden({ timeout: 5000 })
      })

      await demoLogger.testCode.log('Provider banner and external link in detail dialog verified')
    })

    test('should display provider column header in table', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const billingName = `buyer-header-${testStartTime}`
      const sellerName = `seller-header-${testStartTime}`
      let userId: string

      await test.step('Given: admin is logged in and on invoice admin page', async () => {
        userId = await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
      })

      await test.step('And: at least one invoice exists', async () => {
        await createInvoice(page, DEMO_ADMIN.realmId, {
          accountId: userId,
          billingName,
          sellerName,
          lineItems: [{ name: 'Header Item', quantity: '1', unitPrice: 1000 }],
          dueDate: futureDueDate(),
        })
      })

      await test.step('Then: table has a Provider column header', async () => {
        const tableHeaders = page.getByTestId('invoice-table').locator('th')
        // Provider column is at index 4 (after # 0, Invoice Number 1, Buyer 2, Source 3)
        await expect(tableHeaders.nth(4)).toHaveText('Provider')
      })

      await demoLogger.testCode.log('Provider column header verified')
    })
  })
})
