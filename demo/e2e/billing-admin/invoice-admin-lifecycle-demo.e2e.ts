/**
 * Admin Invoice Lifecycle Demo Tests
 *
 * User Stories:
 * - US-IV-001: Create invoice draft with line items, discount, and tax
 * - US-IV-002: Edit draft invoice
 * - US-IV-005: Issue draft invoice (draft -> issued)
 * - US-IV-006: Void invoice (draft/issued -> void)
 * - US-IV-007: Mark invoice as paid (issued/overdue -> paid)
 *
 * User Stories: docs/user-stories/billing/invoice.md
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
  editInvoice,
  issueInvoice,
  voidInvoice,
  markPaidInvoice,
  verifyInvoiceInTable,
  selectFeeMode,
} from './helpers/invoice-helpers'

/**
 * Compute a future due date string (ISO format YYYY-MM-DD).
 * Returns a date 30 days from now.
 */
function futureDueDate(): string {
  const d = new Date()
  d.setDate(d.getDate() + 30)
  return d.toISOString().slice(0, 10)
}

test.describe('[Billing Admin] Invoice Admin Lifecycle Demo Tests', () => {
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
  // User Story US-IV-001: Create Invoice Draft
  // ============================================================================

  test.describe('US-IV-001: Create Invoice Draft', () => {
    test('should create an invoice with line items and verify in table', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const billingName = `buyer-create-${testStartTime}`
      const sellerName = `seller-create-${testStartTime}`

      let invoiceNumber: string
      let userId: string

      await test.step('Given: admin is logged in', async () => {
        userId = await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
      })

      await test.step('When: navigate to invoice admin page', async () => {
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
      })

      await test.step('And: create invoice with one line item', async () => {
        invoiceNumber = await createInvoice(page, DEMO_ADMIN.realmId, {
          accountId: userId,
          billingName,
          billingEmail: `buyer-${testStartTime}@example.com`,
          sellerName,
          sellerEmail: `seller-${testStartTime}@example.com`,
          lineItems: [
            { name: 'Pro Plan - Monthly', quantity: '1', unitPrice: 9900 },
          ],
          dueDate: futureDueDate(),
        })
        console.log(`[Test] Invoice created: ${invoiceNumber}`)
      })

      await test.step('Then: invoice appears in table with draft status', async () => {
        await verifyInvoiceInTable(page, invoiceNumber, 'draft')
        // Verify invoice number format: INV-{YEAR}-{SEQ}
        expect(invoiceNumber).toMatch(/^INV-\d{4}-\d+$/)
        await demoLogger.testCode.log('Invoice draft created and verified')
      })
    })

    test('should calculate discount and tax correctly', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const billingName = `buyer-calc-${testStartTime}`
      const sellerName = `seller-calc-${testStartTime}`
      let userId: string

      await test.step('Given: admin is logged in', async () => {
        userId = await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
      })

      await test.step('When: navigate to invoice admin page', async () => {
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
      })

      await test.step('And: navigate to create page', async () => {
        await page.getByTestId('create-invoice-button').click()
        await expect(page.getByTestId('invoice-form-page')).toBeVisible({ timeout: 5000 })
      })

      await test.step('And: fill invoice form with subtotal 9900 cents', async () => {
        await page.getByTestId('invoice-account-id').fill(userId)
        await page.getByTestId('invoice-billing-name').fill(billingName)
        await page.getByTestId('invoice-billing-tax-id').fill('N/A')
        await page.getByTestId('invoice-billing-address').fill('123 Billing St')
        await page.getByTestId('invoice-seller-name').fill(sellerName)
        await page.getByTestId('invoice-seller-tax-id').fill('N/A')
        await page.getByTestId('invoice-seller-address').fill('456 Seller Ave')

        // Fill line item: unitPrice is in display units (major currency).
        // "99" => 9900 cents. qty=1 => subtotal = 9900 cents.
        await page.getByTestId('invoice-line-item-name-0').fill('Test Item')
        await page.getByTestId('invoice-line-item-quantity-0').fill('1')
        await page.getByTestId('invoice-line-item-unit-price-0').fill('99')

        await page.getByTestId('invoice-due-date').fill(futureDueDate())
      })

      await test.step('And: set discount to fixed 500 cents (display value "5")', async () => {
        // Fee input is in display units (major currency).
        // Fixed mode: Math.round(parsed * 100) converts to cents.
        // "5" => 500 cents discount.
        await selectFeeMode(page, 'invoice-discount', 'fixed')
        await expect(page.getByTestId('invoice-discount-value')).toBeEnabled({ timeout: 5000 })
        await page.getByTestId('invoice-discount-value').fill('5')
      })

      await test.step('And: set tax to percent 6%', async () => {
        await selectFeeMode(page, 'invoice-tax', 'percent')
        await expect(page.getByTestId('invoice-tax-value')).toBeEnabled({ timeout: 5000 })
        await page.getByTestId('invoice-tax-value').fill('6')
      })

      await test.step('Then: verify totals preview calculation', async () => {
        await expect(page.getByTestId('invoice-totals-preview')).toBeVisible()

        // Expected (all in cents):
        // subtotal = 9900
        // discount = Math.round(5 * 100) = 500
        // afterDiscount = 9900 - 500 = 9400
        // tax = Math.round(9400 * 6 / 100) = 564
        // total = 9400 + 564 = 9964
        //
        // TotalsPreview formats via formatInvoiceAmount (cents / 100, Intl.NumberFormat currency).
        await expect(page.getByTestId('invoice-totals-subtotal')).toHaveText(/CN?¥99\.00/)
        await expect(page.getByTestId('invoice-totals-discount')).toHaveText(/-(CN)?¥5\.00/)
        await expect(page.getByTestId('invoice-totals-tax')).toHaveText(/\+(CN)?¥5\.64/)
        await expect(page.getByTestId('invoice-totals-total')).toHaveText(/CN?¥99\.64/)
        await demoLogger.testCode.log('Totals verified: subtotal=9900, discount=500, tax=564, total=9964')
      })

      await test.step('And: submit the invoice', async () => {
        await page.getByTestId('invoice-form-submit-button').click()
        // Wait for navigation back to invoices list
        await page.waitForURL(`**/${DEMO_ADMIN.realmId}/manage/billing/invoices`, { timeout: 10000 })
      })

      await test.step('Then: invoice appears in table with draft status', async () => {
        const row = page.locator('tr').filter({ hasText: billingName }).first()
        await expect(row).toBeVisible({ timeout: 10000 })
      })
    })
  })

  // ============================================================================
  // User Story US-IV-002: Edit Draft Invoice
  // ============================================================================

  test.describe('US-IV-002: Edit Draft Invoice', () => {
    test('should edit a draft invoice and verify updated values', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const billingName = `buyer-edit-${testStartTime}`
      const updatedBillingName = `buyer-updated-${testStartTime}`
      const sellerName = `seller-edit-${testStartTime}`
      let invoiceNumber: string
      let userId: string

      await test.step('Given: admin is logged in', async () => {
        userId = await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
      })

      await test.step('And: a draft invoice exists', async () => {
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
        invoiceNumber = await createInvoice(page, DEMO_ADMIN.realmId, {
          accountId: userId,
          billingName,
          sellerName,
          lineItems: [
            { name: 'Service A', quantity: '1', unitPrice: 5000 },
          ],
          dueDate: futureDueDate(),
        })
        await verifyInvoiceInTable(page, invoiceNumber, 'draft')
        console.log(`[Test] Draft invoice created: ${invoiceNumber}`)
      })

      await test.step('When: edit the draft invoice', async () => {
        await editInvoice(page, DEMO_ADMIN.realmId, invoiceNumber, {
          billingName: updatedBillingName,
          lineItems: [
            { name: 'Service A', quantity: '2', unitPrice: 5000 },
            { name: 'Service B', quantity: '1', unitPrice: 3000 },
          ],
        })
        console.log('[Test] Invoice edited')
      })

      await test.step('Then: updated values appear in table', async () => {
        const row = page.locator('tr').filter({ hasText: updatedBillingName }).first()
        await expect(row).toBeVisible({ timeout: 10000 })
        await demoLogger.testCode.log('Draft invoice edited and verified')
      })
    })
  })

  // ============================================================================
  // User Story US-IV-005: Issue Draft Invoice
  // ============================================================================

  test.describe('US-IV-005: Issue Draft Invoice', () => {
    test('should issue a draft invoice and verify status changes to issued', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const billingName = `buyer-issue-${testStartTime}`
      const sellerName = `seller-issue-${testStartTime}`
      let invoiceNumber: string
      let userId: string

      await test.step('Given: admin is logged in', async () => {
        userId = await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
      })

      await test.step('And: a draft invoice exists', async () => {
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
        invoiceNumber = await createInvoice(page, DEMO_ADMIN.realmId, {
          accountId: userId,
          billingName,
          billingEmail: `buyer-issue-${testStartTime}@example.com`,
          sellerName,
          lineItems: [
            { name: 'Pro Plan', quantity: '1', unitPrice: 9900 },
          ],
          dueDate: futureDueDate(),
        })
        await verifyInvoiceInTable(page, invoiceNumber, 'draft')
      })

      await test.step('When: issue the draft invoice', async () => {
        await issueInvoice(page, invoiceNumber)
      })

      await test.step('Then: status changes to issued', async () => {
        await verifyInvoiceInTable(page, invoiceNumber, 'issued')
        await demoLogger.testCode.log('Invoice issued successfully')
      })
    })
  })

  // ============================================================================
  // User Story US-IV-006: Void Invoice
  // ============================================================================

  test.describe('US-IV-006: Void Invoice', () => {
    test('should void a draft invoice', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const billingName = `buyer-void-draft-${testStartTime}`
      const sellerName = `seller-void-draft-${testStartTime}`
      let invoiceNumber: string
      let userId: string

      await test.step('Given: admin is logged in', async () => {
        userId = await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
      })

      await test.step('And: a draft invoice exists', async () => {
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
        invoiceNumber = await createInvoice(page, DEMO_ADMIN.realmId, {
          accountId: userId,
          billingName,
          sellerName,
          lineItems: [
            { name: 'Draft Item', quantity: '1', unitPrice: 3000 },
          ],
          dueDate: futureDueDate(),
        })
        await verifyInvoiceInTable(page, invoiceNumber, 'draft')
      })

      await test.step('When: void the draft invoice', async () => {
        await voidInvoice(page, invoiceNumber)
      })

      await test.step('Then: status changes to void', async () => {
        await verifyInvoiceInTable(page, invoiceNumber, 'void')
        await demoLogger.testCode.log('Draft invoice voided')
      })
    })

    test('should void an issued invoice with reason', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const billingName = `buyer-void-issued-${testStartTime}`
      const sellerName = `seller-void-issued-${testStartTime}`
      let invoiceNumber: string
      let userId: string

      await test.step('Given: admin is logged in', async () => {
        userId = await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
      })

      await test.step('And: an issued invoice exists', async () => {
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
        invoiceNumber = await createInvoice(page, DEMO_ADMIN.realmId, {
          accountId: userId,
          billingName,
          billingEmail: `buyer-void-issued-${testStartTime}@example.com`,
          sellerName,
          lineItems: [
            { name: 'Issued Item', quantity: '1', unitPrice: 5000 },
          ],
          dueDate: futureDueDate(),
        })
        await issueInvoice(page, invoiceNumber)
        await verifyInvoiceInTable(page, invoiceNumber, 'issued')
      })

      await test.step('When: void the issued invoice with a reason', async () => {
        await voidInvoice(page, invoiceNumber, 'Customer requested cancellation')
      })

      await test.step('Then: status changes to void', async () => {
        await verifyInvoiceInTable(page, invoiceNumber, 'void')
        await demoLogger.testCode.log('Issued invoice voided with reason')
      })
    })
  })

  // ============================================================================
  // User Story US-IV-007: Mark Invoice as Paid
  // ============================================================================

  test.describe('US-IV-007: Mark Invoice as Paid', () => {
    test('should mark an issued invoice as paid', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const billingName = `buyer-paid-${testStartTime}`
      const sellerName = `seller-paid-${testStartTime}`
      let invoiceNumber: string
      let userId: string

      await test.step('Given: admin is logged in', async () => {
        userId = await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
      })

      await test.step('And: an issued invoice exists', async () => {
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
        invoiceNumber = await createInvoice(page, DEMO_ADMIN.realmId, {
          accountId: userId,
          billingName,
          billingEmail: `buyer-paid-${testStartTime}@example.com`,
          sellerName,
          lineItems: [
            { name: 'Paid Item', quantity: '1', unitPrice: 8000 },
          ],
          dueDate: futureDueDate(),
        })
        await issueInvoice(page, invoiceNumber)
        await verifyInvoiceInTable(page, invoiceNumber, 'issued')
      })

      await test.step('When: mark the invoice as paid', async () => {
        await markPaidInvoice(page, invoiceNumber)
      })

      await test.step('Then: status changes to paid', async () => {
        await verifyInvoiceInTable(page, invoiceNumber, 'paid')
        await demoLogger.testCode.log('Invoice marked as paid')
      })
    })
  })

  // ============================================================================
  // Comprehensive: Full Invoice Lifecycle in Single Session
  // ============================================================================

  test.describe('Comprehensive: Full Invoice Lifecycle', () => {
    test('should complete create -> edit -> issue -> paid lifecycle', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const billingName = `buyer-lifecycle-${testStartTime}`
      const updatedBillingName = `buyer-lifecycle-updated-${testStartTime}`
      const sellerName = `seller-lifecycle-${testStartTime}`
      let invoiceNumber: string
      let userId: string

      await test.step('Given: admin is logged in and on invoice page', async () => {
        userId = await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
      })

      await test.step('Step 1: Create invoice draft', async () => {
        invoiceNumber = await createInvoice(page, DEMO_ADMIN.realmId, {
          accountId: userId,
          billingName,
          billingEmail: `buyer-lifecycle-${testStartTime}@example.com`,
          sellerName,
          lineItems: [
            { name: 'Basic Plan', quantity: '1', unitPrice: 2000 },
          ],
          dueDate: futureDueDate(),
        })
        await verifyInvoiceInTable(page, invoiceNumber, 'draft')
        console.log(`[Test] Step 1 complete: draft created ${invoiceNumber}`)
      })

      await test.step('Step 2: Edit the draft invoice', async () => {
        await editInvoice(page, DEMO_ADMIN.realmId, invoiceNumber, {
          billingName: updatedBillingName,
          lineItems: [
            { name: 'Pro Plan', quantity: '1', unitPrice: 5000 },
          ],
        })
        await verifyInvoiceInTable(page, invoiceNumber, 'draft')
        console.log('[Test] Step 2 complete: draft edited')
      })

      await test.step('Step 3: Issue the invoice', async () => {
        await issueInvoice(page, invoiceNumber)
        await verifyInvoiceInTable(page, invoiceNumber, 'issued')
        console.log('[Test] Step 3 complete: invoice issued')
      })

      await test.step('Step 4: Mark as paid', async () => {
        await markPaidInvoice(page, invoiceNumber)
        await verifyInvoiceInTable(page, invoiceNumber, 'paid')
        console.log('[Test] Step 4 complete: invoice paid')
      })

      await demoLogger.testCode.log('Full lifecycle completed: draft -> edit -> issue -> paid')
    })

    test('should complete create -> issue -> void lifecycle', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const billingName = `buyer-void-lifecycle-${testStartTime}`
      const sellerName = `seller-void-lifecycle-${testStartTime}`
      let invoiceNumber: string
      let userId: string

      await test.step('Given: admin is logged in and on invoice page', async () => {
        userId = await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
      })

      await test.step('Step 1: Create and issue an invoice', async () => {
        invoiceNumber = await createInvoice(page, DEMO_ADMIN.realmId, {
          accountId: userId,
          billingName,
          billingEmail: `buyer-void-lifecycle-${testStartTime}@example.com`,
          sellerName,
          lineItems: [
            { name: 'Voidable Item', quantity: '2', unitPrice: 4000 },
          ],
          dueDate: futureDueDate(),
        })
        await issueInvoice(page, invoiceNumber)
        await verifyInvoiceInTable(page, invoiceNumber, 'issued')
      })

      await test.step('Step 2: Void the issued invoice', async () => {
        await voidInvoice(page, invoiceNumber, 'No longer needed')
        await verifyInvoiceInTable(page, invoiceNumber, 'void')
      })

      await demoLogger.testCode.log('Void lifecycle completed: draft -> issue -> void')
    })
  })
})
