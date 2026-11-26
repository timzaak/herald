/**
 * Credit Note User Visibility Demo Tests
 *
 * User Story:
 * - US-IF-009: Regular user sees refund summary in invoice list/detail without
 *   exposure to internal Credit Note numbers/operators.
 *
 * Covers:
 * - Scenario A: manual invoice refund is visible to the user (list pill + detail
 *   breakdown), but Credit Note lists are absent.
 * - Scenario B: Creem external invoices do not show any refund dimension.
 */

import { test, cleanupTestData, expect } from '../fixtures/demo-page.fixtures'
import { DEMO_ADMIN, DEMO_USERS, loginAsAdmin, loginWithCredentials } from '../helpers/auth'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import {
  createAndPayManualInvoice,
  getAccountIdByEmail,
  getInvoiceIdByNumber,
  openInvoiceDetailDialogByNumber,
  recordRefundViaDialog,
  seedPaidExternalInvoice,
} from '../billing-admin/helpers/credit-note-helpers'
import { navigateToInvoiceAdminPage } from '../billing-admin/helpers/invoice-helpers'

const REALM_ID = DEMO_ADMIN.realmId

async function clearSession(page: Parameters<typeof loginWithCredentials>[0]): Promise<void> {
  await page.context().clearCookies()
  await page.evaluate(() => {
    localStorage.clear()
    sessionStorage.clear()
  })
}

test.describe('[Regular User] Credit Note User Visibility Demo Tests', () => {
  test.beforeEach(async ({ page }) => {
    await verifyTestEnvironment(page, {
      requiredRealms: [REALM_ID],
      requiredUsers: [DEMO_ADMIN.email, DEMO_USERS.user1.email],
    })
  })

  test.afterEach(async ({ page, testStartTime }) => {
    await cleanupTestData(page, REALM_ID, {
      keepUsers: [DEMO_ADMIN.email, DEMO_USERS.user1.email],
      timestamp: testStartTime,
    })
  })

  test('US-IF-009: manual invoice refund is visible to user without Credit Note details', async ({
    page,
    demoLogger,
    testStartTime,
  }) => {
    const billingName = `UserRefund-${testStartTime}`
    let invoiceNumber: string
    let invoiceId: string

    await test.step('Given: admin is logged in and retrieves user1 account id', async () => {
      const accountId = getAccountIdByEmail(REALM_ID, DEMO_USERS.user1.email)
      await loginAsAdmin(page, { realmId: REALM_ID, waitNavigation: true })
      await navigateToInvoiceAdminPage(page, REALM_ID)

      await test.step('When: create and pay a $100.00 manual invoice for user1', async () => {
        invoiceNumber = await createAndPayManualInvoice(page, REALM_ID, {
          accountId,
          billingName,
          billingEmail: 'user-refund-demo@example.com',
          sellerName: 'Herald Demo',
          lineItems: [{ name: 'Service', quantity: '1', unitPrice: 10000 }],
          dueDate: '2026-12-31',
        })
        invoiceId = getInvoiceIdByNumber(REALM_ID, invoiceNumber)
        console.log(`[Test] Paid manual invoice created: ${invoiceNumber} (${invoiceId})`)
      })
    })

    await test.step('When: admin opens the invoice detail and records a $40.00 refund', async () => {
      await openInvoiceDetailDialogByNumber(page, invoiceNumber)
      await recordRefundViaDialog(page, { amount: 40, reason: 'User refund' })
    })

    await test.step('When: switch to user1 session and navigate to My Invoices', async () => {
      await clearSession(page)
      await loginWithCredentials(page, {
        realmId: REALM_ID,
        email: DEMO_USERS.user1.email,
        password: DEMO_USERS.user1.password,
      })
      await page.goto(`/${REALM_ID}/user/invoices`)
      await expect(page.getByTestId('invoice-user-page')).toBeVisible({ timeout: 10000 })
      await expect(page.getByTestId('invoice-user-table')).toBeVisible({ timeout: 10000 })
    })

    await test.step('Then: refund summary pill is visible for the invoice row', async () => {
      await expect(page.getByTestId(`invoice-refund-summary-${invoiceId}`)).toBeVisible({
        timeout: 10000,
      })
    })

    await test.step('When: user opens the invoice detail dialog', async () => {
      await page.getByTestId(`invoice-view-${invoiceId}`).click()
      await expect(page.getByTestId('invoice-detail-dialog')).toBeVisible({ timeout: 10000 })
    })

    await test.step('Then: refund breakdown shows correct amounts', async () => {
      await expect(page.getByTestId('invoice-refund-summary')).toBeVisible({ timeout: 10000 })
      await expect(page.getByTestId('invoice-refunded-amount')).toHaveText('-CN¥40.00')
      await expect(page.getByTestId('invoice-remaining-amount')).toHaveText('CN¥60.00')
    })

    await test.step('Then: Credit Note lists and operator details are not exposed', async () => {
      await expect(page.getByTestId('credit-note-list')).toHaveCount(0)
      await expect(page.getByTestId('credit-note-list-manual')).toHaveCount(0)
      await expect(page.getByTestId('credit-note-list-stripe')).toHaveCount(0)
      await expect(page.locator('[data-testid^="credit-note-voided-"]')).toHaveCount(0)
      await expect(page.getByText('Operator')).toHaveCount(0)
    })

    await demoLogger.testCode.log(
      'User refund visibility verified: summary shown, Credit Note details hidden'
    )
  })

  test('US-IF-009: Creem invoice does not render refund dimension for the user', async ({
    page,
    demoLogger,
  }) => {
    let invoiceId: string

    await test.step('Given: admin seeds a paid Creem invoice for user1', async () => {
      const accountId = getAccountIdByEmail(REALM_ID, DEMO_USERS.user1.email)
      await loginAsAdmin(page, { realmId: REALM_ID, waitNavigation: true })

      const invoice = seedPaidExternalInvoice(REALM_ID, {
        provider: 'creem',
        total: 10000,
        accountId,
      })
      invoiceId = invoice.id
      console.log(`[Test] Paid Creem invoice seeded: ${invoice.invoiceNumber} (${invoiceId})`)
    })

    await test.step('When: switch to user1 session and navigate to My Invoices', async () => {
      await clearSession(page)
      await loginWithCredentials(page, {
        realmId: REALM_ID,
        email: DEMO_USERS.user1.email,
        password: DEMO_USERS.user1.password,
      })
      await page.goto(`/${REALM_ID}/user/invoices`)
      await expect(page.getByTestId('invoice-user-page')).toBeVisible({ timeout: 10000 })
      await expect(page.getByTestId('invoice-user-table')).toBeVisible({ timeout: 10000 })
    })

    await test.step('Then: no refund summary pill is shown for the Creem invoice', async () => {
      await expect(page.getByTestId(`invoice-refund-summary-${invoiceId}`)).toHaveCount(0)
    })

    await test.step('When: user opens the Creem invoice detail dialog', async () => {
      await page.getByTestId(`invoice-view-${invoiceId}`).click()
      await expect(page.getByTestId('invoice-detail-dialog')).toBeVisible({ timeout: 10000 })
    })

    await test.step('Then: refund summary and Credit Note list are absent', async () => {
      await expect(page.getByTestId('invoice-refund-summary')).toHaveCount(0)
      await expect(page.getByTestId('credit-note-list')).toHaveCount(0)
    })

    await demoLogger.testCode.log('Creem refund dimension exclusion verified for regular user')
  })
})
