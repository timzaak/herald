/**
 * Invoice Admin List & Detail Demo Tests
 *
 * User Stories:
 * - US-IV-003: View invoice list with filters (status, source, date range)
 * - US-IV-004: View invoice detail with line items, amounts, and status history
 * - Status-dependent action menu visibility
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
  issueInvoice,
  voidInvoice,
  markPaidInvoice,
  verifyInvoiceInTable,
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

test.describe('[Billing Admin] Invoice Admin List & Detail Demo Tests', () => {
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
  // User Story US-IV-003: View Invoice List
  // ============================================================================

  test.describe('US-IV-003: View Invoice List', () => {
    test('should display invoice list with table columns and pagination', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const billingName = `buyer-list-${testStartTime}`
      const sellerName = `seller-list-${testStartTime}`
      let userId: string

      await test.step('Given: admin is logged in', async () => {
        userId = await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
      })

      await test.step('And: invoices exist in the system', async () => {
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
        // Create two invoices to ensure the list has data
        await createInvoice(page, DEMO_ADMIN.realmId, {
          accountId: userId,
          billingName,
          sellerName,
          lineItems: [{ name: 'Plan A', quantity: '1', unitPrice: 5000 }],
          dueDate: futureDueDate(),
        })
        await createInvoice(page, DEMO_ADMIN.realmId, {
          accountId: userId,
          billingName: `buyer-list-2-${testStartTime}`,
          sellerName,
          lineItems: [{ name: 'Plan B', quantity: '2', unitPrice: 3000 }],
          dueDate: futureDueDate(),
        })
      })

      await test.step('Then: invoice list page displays expected elements', async () => {
        // Verify page heading
        await expect(page.getByTestId('invoice-heading')).toBeVisible()
        await expect(page.getByTestId('invoice-heading')).toHaveText('Invoices')

        // Verify filter bar is present
        await expect(page.getByTestId('invoice-filter-bar')).toBeVisible()

        // Verify table is present
        await expect(page.getByTestId('invoice-table')).toBeVisible()

        // Verify the complete business column set and order.
        const tableHeaders = page.getByTestId('invoice-table').locator('th')
        await expect(tableHeaders).toHaveText([
          '#',
          'Invoice Number',
          'Buyer',
          'Source',
          'Provider',
          'Status',
          'Total',
          'Refunded',
          'Due Date',
          'Created At',
          'Actions',
        ])

        // Verify filter controls
        await expect(page.getByTestId('invoice-status-filter')).toBeVisible()
        await expect(page.getByTestId('invoice-source-filter')).toBeVisible()
        await expect(page.getByTestId('invoice-date-from-filter')).toBeVisible()
        await expect(page.getByTestId('invoice-date-to-filter')).toBeVisible()
        await expect(page.getByTestId('invoice-search-input')).toBeVisible()

        // Verify create button
        await expect(page.getByTestId('create-invoice-button')).toBeVisible()
      })

      await test.step('And: pagination shows total results when invoices exist', async () => {
        // With at least 2 invoices created, pagination summary should be visible
        await expect(page.getByTestId('invoice-pagination')).toBeVisible()
      })

      await demoLogger.testCode.log('Invoice list verified with columns, filters, and pagination')
    })

    test('should filter invoices by status', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const billingNameDraft = `buyer-filter-draft-${testStartTime}`
      const billingNameIssued = `buyer-filter-issued-${testStartTime}`
      const sellerName = `seller-filter-${testStartTime}`
      let draftInvoiceNumber: string
      let issuedInvoiceNumber: string
      let userId: string

      await test.step('Given: admin is logged in and on invoice admin page', async () => {
        userId = await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
      })

      await test.step('And: a draft and an issued invoice exist', async () => {
        draftInvoiceNumber = await createInvoice(page, DEMO_ADMIN.realmId, {
          accountId: userId,
          billingName: billingNameDraft,
          sellerName,
          lineItems: [{ name: 'Draft Item', quantity: '1', unitPrice: 2000 }],
          dueDate: futureDueDate(),
        })
        await verifyInvoiceInTable(page, draftInvoiceNumber, 'draft')

        issuedInvoiceNumber = await createInvoice(page, DEMO_ADMIN.realmId, {
          accountId: userId,
          billingName: billingNameIssued,
          billingEmail: 'issued-test@example.com',
          sellerName,
          lineItems: [{ name: 'Issued Item', quantity: '1', unitPrice: 4000 }],
          dueDate: futureDueDate(),
        })
        await issueInvoice(page, issuedInvoiceNumber)
        await verifyInvoiceInTable(page, issuedInvoiceNumber, 'issued')
      })

      await test.step('When: filter by status "draft"', async () => {
        await page.getByTestId('invoice-status-filter').click()
        await page.getByRole('option', { name: 'Draft' }).click()
        // Technical delay: wait for React Query to refetch with filter
        await page.waitForLoadState('networkidle')
        await page.waitForTimeout(300)
      })

      await test.step('Then: only draft invoices are shown', async () => {
        await verifyInvoiceInTable(page, draftInvoiceNumber, 'draft')
      })

      await test.step('And: issued invoice is not shown', async () => {
        await expect(
          page.locator('tr').filter({ hasText: billingNameIssued }).first()
        ).not.toBeVisible()
      })

      await test.step('When: filter by status "issued"', async () => {
        await page.getByTestId('invoice-status-filter').click()
        await page.getByRole('option', { name: 'Issued' }).click()
        await page.waitForLoadState('networkidle')
        await page.waitForTimeout(300)
      })

      await test.step('Then: only issued invoices are shown', async () => {
        await verifyInvoiceInTable(page, issuedInvoiceNumber, 'issued')
      })

      await test.step('And: draft invoice is not shown', async () => {
        await expect(
          page.locator('tr').filter({ hasText: billingNameDraft }).first()
        ).not.toBeVisible()
      })

      await test.step('When: reset filter to "All Statuses"', async () => {
        await page.getByTestId('invoice-status-filter').click()
        await page.getByRole('option', { name: 'All Statuses' }).click()
        await page.waitForLoadState('networkidle')
        await page.waitForTimeout(300)
      })

      await test.step('Then: both invoices are visible again', async () => {
        await verifyInvoiceInTable(page, draftInvoiceNumber, 'draft')
        await verifyInvoiceInTable(page, issuedInvoiceNumber, 'issued')
      })

      await demoLogger.testCode.log('Status filter verified')
    })

    test('should filter invoices by date range', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const billingName = `buyer-date-${testStartTime}`
      const sellerName = `seller-date-${testStartTime}`
      let invoiceNumber: string
      let userId: string

      await test.step('Given: admin is logged in and on invoice admin page', async () => {
        userId = await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
      })

      await test.step('And: an invoice exists', async () => {
        invoiceNumber = await createInvoice(page, DEMO_ADMIN.realmId, {
          accountId: userId,
          billingName,
          sellerName,
          lineItems: [{ name: 'Date Item', quantity: '1', unitPrice: 1000 }],
          dueDate: futureDueDate(),
        })
        await verifyInvoiceInTable(page, invoiceNumber, 'draft')
      })

      await test.step('When: set date-from filter to today', async () => {
        const today = new Date().toISOString().slice(0, 10)
        await page.getByTestId('invoice-date-from-filter').fill(today)
        await page.waitForLoadState('networkidle')
        // Technical delay: wait for React Query to refetch with filter
        await page.waitForTimeout(300)
      })

      await test.step('Then: invoice created today is visible', async () => {
        await verifyInvoiceInTable(page, invoiceNumber, 'draft')
      })

      await test.step('When: set date-to filter to a past date (2020-01-01)', async () => {
        await page.getByTestId('invoice-date-to-filter').fill('2020-01-01')
        await page.waitForLoadState('networkidle')
        // Technical delay: wait for React Query to refetch with filter
        await page.waitForTimeout(300)
      })

      await test.step('Then: invoice is not visible (created after filter range)', async () => {
        await expect(
          page.locator('tr').filter({ hasText: invoiceNumber }).first()
        ).not.toBeVisible()
      })

      await test.step('When: clear date filters', async () => {
        await page.getByTestId('invoice-date-from-filter').fill('')
        await page.getByTestId('invoice-date-to-filter').fill('')
        await page.waitForLoadState('networkidle')
        // Technical delay: wait for React Query to refetch without filter
        await page.waitForTimeout(300)
      })

      await test.step('Then: invoice is visible again', async () => {
        await verifyInvoiceInTable(page, invoiceNumber, 'draft')
      })

      await demoLogger.testCode.log('Date range filter verified')
    })

    test('should filter invoices by source', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const billingName = `buyer-source-${testStartTime}`
      const sellerName = `seller-source-${testStartTime}`
      let invoiceNumber: string
      let userId: string

      await test.step('Given: admin is logged in and on invoice admin page', async () => {
        userId = await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
      })

      await test.step('And: a manually created invoice exists (source=admin_manual)', async () => {
        invoiceNumber = await createInvoice(page, DEMO_ADMIN.realmId, {
          accountId: userId,
          billingName,
          sellerName,
          lineItems: [{ name: 'Manual Item', quantity: '1', unitPrice: 3000 }],
          dueDate: futureDueDate(),
        })
        await verifyInvoiceInTable(page, invoiceNumber, 'draft')
      })

      await test.step('When: filter by source "Manual" (admin_manual)', async () => {
        await page.getByTestId('invoice-source-filter').click()
        await page.getByRole('option', { name: 'Manual' }).click()
        await page.waitForLoadState('networkidle')
        // Technical delay: wait for React Query to refetch with filter
        await page.waitForTimeout(300)
      })

      await test.step('Then: manually created invoice is visible', async () => {
        await verifyInvoiceInTable(page, invoiceNumber, 'draft')
      })

      await test.step('When: filter by source "Application" (user_application)', async () => {
        await page.getByTestId('invoice-source-filter').click()
        await page.getByRole('option', { name: 'Application' }).click()
        await page.waitForLoadState('networkidle')
        // Technical delay: wait for React Query to refetch with filter
        await page.waitForTimeout(300)
      })

      await test.step('Then: manually created invoice is not visible', async () => {
        await expect(
          page.locator('tr').filter({ hasText: invoiceNumber }).first()
        ).not.toBeVisible()
      })

      await demoLogger.testCode.log('Source filter verified')
    })
  })

  // ============================================================================
  // User Story US-IV-004: View Invoice Detail
  // ============================================================================

  test.describe('US-IV-004: View Invoice Detail', () => {
    test('should display invoice detail dialog with all sections', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const billingName = `buyer-detail-${testStartTime}`
      const billingEmail = `buyer-detail-${testStartTime}@example.com`
      const sellerName = `seller-detail-${testStartTime}`
      const sellerEmail = `seller-detail-${testStartTime}@example.com`
      let invoiceNumber: string
      let userId: string

      await test.step('Given: admin is logged in and on invoice admin page', async () => {
        userId = await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
      })

      await test.step('And: an issued invoice exists', async () => {
        invoiceNumber = await createInvoice(page, DEMO_ADMIN.realmId, {
          accountId: userId,
          billingName,
          billingEmail,
          sellerName,
          sellerEmail,
          lineItems: [{ name: 'Detail Item', quantity: '2', unitPrice: 5000 }],
          dueDate: futureDueDate(),
        })
        await issueInvoice(page, invoiceNumber)
        await verifyInvoiceInTable(page, invoiceNumber, 'issued')
      })

      await test.step('When: click View action for the invoice', async () => {
        const row = page.locator('tr').filter({ hasText: invoiceNumber }).first()
        await expect(row).toBeVisible({ timeout: 5000 })

        // Open the actions dropdown menu
        const menuButton = row.getByRole('button', { name: 'Open menu' })
        await menuButton.click()

        // Click View menu item
        const viewItems = page.getByRole('menuitem', { name: 'View' })
        await viewItems.first().click()
      })

      await test.step('Then: detail dialog is visible', async () => {
        await expect(page.getByTestId('invoice-detail-dialog')).toBeVisible({ timeout: 5000 })
      })

      await test.step('And: dialog shows invoice number in title', async () => {
        await expect(
          page.getByTestId('invoice-detail-dialog').getByText(invoiceNumber)
        ).toBeVisible()
      })

      await test.step('And: seller info section is displayed', async () => {
        await expect(page.getByTestId('invoice-seller-info')).toBeVisible()
        await expect(page.getByTestId('invoice-seller-info')).toContainText(sellerName)
      })

      await test.step('And: buyer info section is displayed', async () => {
        await expect(page.getByTestId('invoice-buyer-info')).toBeVisible()
        await expect(page.getByTestId('invoice-buyer-info')).toContainText(billingName)
      })

      await test.step('And: line items section is displayed with item details', async () => {
        await expect(page.getByTestId('invoice-line-items-section')).toBeVisible()
        // Verify line item table headers
        const lineItemSection = page.getByTestId('invoice-line-items-section')
        await expect(lineItemSection.getByText('Name')).toBeVisible()
        await expect(lineItemSection.getByText('Qty')).toBeVisible()
        await expect(lineItemSection.getByText('Unit Price')).toBeVisible()
        await expect(lineItemSection.getByText('Subtotal')).toBeVisible()
        // Verify the line item data
        await expect(lineItemSection.getByText('Detail Item')).toBeVisible()
      })

      await test.step('And: amount breakdown section is displayed', async () => {
        await expect(page.getByTestId('invoice-amount-breakdown')).toBeVisible()
        await expect(page.getByTestId('invoice-amount-breakdown').getByText('Subtotal')).toBeVisible()
        await expect(page.getByTestId('invoice-amount-breakdown').getByText('Discount')).toBeVisible()
        await expect(page.getByTestId('invoice-amount-breakdown').getByText('Tax')).toBeVisible()
        await expect(page.getByTestId('invoice-amount-breakdown').getByText('Shipping')).toBeVisible()
        await expect(page.getByTestId('invoice-total-amount')).toBeVisible()
      })

      await test.step('And: additional info section is displayed', async () => {
        await expect(page.getByTestId('invoice-additional-info')).toBeVisible()
      })

      await test.step('And: status history section is displayed', async () => {
        await expect(page.getByTestId('invoice-status-history')).toBeVisible()
        // History should show at least "Created" and "Issued" events
        await expect(page.getByTestId('invoice-status-history').getByText('Created')).toBeVisible()
        await expect(page.getByTestId('invoice-status-history').getByText('Issued', { exact: true })).toBeVisible()
      })

      await test.step('And: PDF download button is visible (issued invoice)', async () => {
        await expect(page.getByTestId('invoice-download-pdf-button')).toBeVisible()
      })

      await test.step('Cleanup: close the detail dialog', async () => {
        // Close the dialog by pressing Escape
        await page.keyboard.press('Escape')
        await expect(page.getByTestId('invoice-detail-dialog')).toBeHidden({ timeout: 5000 })
      })

      await demoLogger.testCode.log('Invoice detail dialog verified with all sections')
    })

    test('should display status change history in detail dialog', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const billingName = `buyer-history-${testStartTime}`
      const sellerName = `seller-history-${testStartTime}`
      let invoiceNumber: string
      let userId: string

      await test.step('Given: admin is logged in and on invoice admin page', async () => {
        userId = await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
      })

      await test.step('And: a paid invoice exists (draft -> issued -> paid)', async () => {
        invoiceNumber = await createInvoice(page, DEMO_ADMIN.realmId, {
          accountId: userId,
          billingName,
          billingEmail: 'history-test@example.com',
          sellerName,
          lineItems: [{ name: 'History Item', quantity: '1', unitPrice: 7000 }],
          dueDate: futureDueDate(),
        })
        await issueInvoice(page, invoiceNumber)
        await markPaidInvoice(page, invoiceNumber)
        await verifyInvoiceInTable(page, invoiceNumber, 'paid')
      })

      await test.step('When: open detail dialog for the paid invoice', async () => {
        const row = page.locator('tr').filter({ hasText: invoiceNumber }).first()
        await expect(row).toBeVisible({ timeout: 5000 })

        const menuButton = row.getByRole('button', { name: 'Open menu' })
        await menuButton.click()
        await page.getByRole('menuitem', { name: 'View' }).first().click()
        await expect(page.getByTestId('invoice-detail-dialog')).toBeVisible({ timeout: 5000 })
      })

      await test.step('Then: status history shows created, issued, and paid events', async () => {
        const historySection = page.getByTestId('invoice-status-history')
        await expect(historySection.getByText('Created')).toBeVisible()
        await expect(historySection.getByText('Issued', { exact: true })).toBeVisible()
        await expect(historySection.getByText('Paid', { exact: true })).toBeVisible()
      })

      await test.step('And: each history event shows actor type and timestamp', async () => {
        const historySection = page.getByTestId('invoice-status-history')
        // Timestamps contain 4-digit years (e.g. "May 12, 2026, 9:36 AM")
        const eventTimestamps = historySection.getByText(/\d{4}/)
        await expect(eventTimestamps.first()).toBeVisible()
      })

      await test.step('Cleanup: close the detail dialog', async () => {
        await page.keyboard.press('Escape')
        await expect(page.getByTestId('invoice-detail-dialog')).toBeHidden({ timeout: 5000 })
      })

      await demoLogger.testCode.log('Status change history verified')
    })
  })

  // ============================================================================
  // Status-Dependent Action Menu Visibility
  // ============================================================================

  test.describe('Status-Dependent Actions', () => {
    test('should show correct actions for each invoice status', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const billingNameDraft = `buyer-actions-draft-${testStartTime}`
      const billingNameIssued = `buyer-actions-issued-${testStartTime}`
      const billingNamePaid = `buyer-actions-paid-${testStartTime}`
      const billingNameVoid = `buyer-actions-void-${testStartTime}`
      const sellerName = `seller-actions-${testStartTime}`
      let draftInvoiceId: string
      let issuedInvoiceId: string
      let paidInvoiceId: string
      let voidInvoiceId: string
      let userId: string

      await test.step('Given: admin is logged in and on invoice admin page', async () => {
        userId = await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        await navigateToInvoiceAdminPage(page, DEMO_ADMIN.realmId)
      })

      await test.step('And: invoices in all four statuses exist', async () => {
        // Create a draft invoice
        draftInvoiceId = await createInvoice(page, DEMO_ADMIN.realmId, {
          accountId: userId,
          billingName: billingNameDraft,
          sellerName,
          lineItems: [{ name: 'Draft Item', quantity: '1', unitPrice: 1000 }],
          dueDate: futureDueDate(),
        })
        await verifyInvoiceInTable(page, draftInvoiceId, 'draft')

        // Create and issue an invoice
        issuedInvoiceId = await createInvoice(page, DEMO_ADMIN.realmId, {
          accountId: userId,
          billingName: billingNameIssued,
          billingEmail: 'actions-issued-test@example.com',
          sellerName,
          lineItems: [{ name: 'Issued Item', quantity: '1', unitPrice: 2000 }],
          dueDate: futureDueDate(),
        })
        await issueInvoice(page, issuedInvoiceId)
        await verifyInvoiceInTable(page, issuedInvoiceId, 'issued')

        // Create, issue, and mark as paid
        paidInvoiceId = await createInvoice(page, DEMO_ADMIN.realmId, {
          accountId: userId,
          billingName: billingNamePaid,
          billingEmail: 'actions-paid-test@example.com',
          sellerName,
          lineItems: [{ name: 'Paid Item', quantity: '1', unitPrice: 3000 }],
          dueDate: futureDueDate(),
        })
        await issueInvoice(page, paidInvoiceId)
        await markPaidInvoice(page, paidInvoiceId)
        await verifyInvoiceInTable(page, paidInvoiceId, 'paid')

        // Create and void
        voidInvoiceId = await createInvoice(page, DEMO_ADMIN.realmId, {
          accountId: userId,
          billingName: billingNameVoid,
          sellerName,
          lineItems: [{ name: 'Void Item', quantity: '1', unitPrice: 4000 }],
          dueDate: futureDueDate(),
        })
        await voidInvoice(page, voidInvoiceId, 'Test void')
        await verifyInvoiceInTable(page, voidInvoiceId, 'void')
      })

      await test.step('Then: draft invoice has view, edit, issue, and void actions', async () => {
        await verifyActionsForInvoice(page, billingNameDraft, [
          { label: 'View', expected: true },
          { label: 'Edit', expected: true },
          { label: 'Issue', expected: true },
          { label: 'Void', expected: true },
          { label: 'Mark Paid', expected: false },
          { label: 'Download PDF', expected: false },
        ])
      })

      await test.step('Then: issued invoice has view, void, mark-paid, and download actions', async () => {
        await verifyActionsForInvoice(page, billingNameIssued, [
          { label: 'View', expected: true },
          { label: 'Edit', expected: false },
          { label: 'Issue', expected: false },
          { label: 'Void', expected: true },
          { label: 'Mark Paid', expected: true },
          { label: 'Download PDF', expected: true },
        ])
      })

      await test.step('Then: paid invoice has view and download actions only', async () => {
        await verifyActionsForInvoice(page, billingNamePaid, [
          { label: 'View', expected: true },
          { label: 'Edit', expected: false },
          { label: 'Issue', expected: false },
          { label: 'Void', expected: false },
          { label: 'Mark Paid', expected: false },
          { label: 'Download PDF', expected: true },
        ])
      })

      await test.step('Then: void invoice has view action only', async () => {
        await verifyActionsForInvoice(page, billingNameVoid, [
          { label: 'View', expected: true },
          { label: 'Edit', expected: false },
          { label: 'Issue', expected: false },
          { label: 'Void', expected: false },
          { label: 'Mark Paid', expected: false },
          { label: 'Download PDF', expected: false },
        ])
      })

      await demoLogger.testCode.log('Status-dependent action visibility verified for all statuses')
    })
  })
})

// ============================================================================
// Helper: Verify action menu items for an invoice
// ============================================================================

/**
 * Opens the actions dropdown for a specific invoice row (identified by billingName)
 * and verifies which menu items are visible or hidden.
 *
 * @param page Playwright Page
 * @param billingName Text to identify the invoice row
 * @param expectedActions Array of { label, expected } pairs
 */
async function verifyActionsForInvoice(
  page: import('@playwright/test').Page,
  billingName: string,
  expectedActions: Array<{ label: string; expected: boolean }>,
): Promise<void> {
  const row = page.locator('tr').filter({ hasText: billingName }).first()
  await expect(row).toBeVisible({ timeout: 5000 })

  // Open the actions menu
  const menuButton = row.getByRole('button', { name: 'Open menu' })
  await menuButton.click()

  // Wait for the dropdown to appear
  const menuContent = page.locator('[role="menu"]')
  await expect(menuContent).toBeVisible({ timeout: 3000 })

  for (const { label, expected } of expectedActions) {
    const menuItem = page.getByRole('menuitem', { name: label })
    const isVisible = await menuItem.isVisible().catch(() => false)

    if (expected) {
      expect(isVisible).toBe(true)
    } else {
      expect(isVisible).toBe(false)
    }
  }

  // Close the menu by pressing Escape
  await page.keyboard.press('Escape')
  await expect(menuContent).toBeHidden({ timeout: 3000 })
}
