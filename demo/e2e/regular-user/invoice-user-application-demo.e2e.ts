/**
 * Invoice User Application Demo Tests
 *
 * User Stories:
 * - US-IV-008: Regular user views own invoice list
 * - US-IV-010: Admin configures seller info
 * - US-IV-011: Regular user applies for invoice
 * - US-IV-012: Admin reviews and issues user-applied invoice
 *
 * User Stories: docs/user-stories/billing/invoice.md
 *
 * Session switching:
 * - Admin operations: loginAsAdmin -> navigate to /{realmId}/manage/billing/invoices
 * - User operations: loginAsUser -> navigate to /{realmId}/user/invoices
 *
 * UnifiedLogger Usage:
 * - All tests use UnifiedLogger through 'demoLogger' fixture
 * - Logger is automatically initialized and finalized by fixture
 */

import { test, cleanupTestData, expect } from '../fixtures/demo-page.fixtures'
import type { Page } from '@playwright/test'
import { DEMO_ADMIN, DEMO_USERS } from '../helpers/auth'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import {
  navigateToInvoiceAdminPage,
  editInvoice,
  issueInvoice,
  voidInvoice,
} from '../billing-admin/helpers/invoice-helpers'
import { randomUUID } from 'crypto'
import { execSync } from 'child_process'

const REALM_ID = DEMO_ADMIN.realmId
const POSTGRES_CONTAINER = 'cas-demo-postgres'

function futureDueDate(): string {
  const d = new Date()
  d.setDate(d.getDate() + 30)
  return d.toISOString().slice(0, 10)
}

/**
 * Open the apply-invoice form via the per-row Invoice button on the Purchase
 * Records page (`/user/subscription-history`).
 *
 * The standalone "Apply Invoice" page-level button on /user/invoices was
 * removed; the apply form is now reached ONLY via per-row Invoice buttons,
 * which pre-fill the reference (no manual payment-attempt-id input).
 *
 * Verifies the row button is enabled (route === manual_fallback) before
 * clicking, and asserts the prefilled-reference banner rendered post-open.
 */
async function openApplyInvoiceForm(
  page: Page,
  realmId: string,
  paymentAttemptId: string
): Promise<void> {
  await page.goto(`/user/subscription-history`)

  const rowButton = page.getByTestId(`purchase-history-invoice-button-${paymentAttemptId}`)
  await expect(rowButton).toBeVisible({ timeout: 10000 })
  // The eligibility query must resolve to manual_fallback (enabled) before click.
  await expect(rowButton).toBeEnabled({ timeout: 10000 })
  await rowButton.click()

  await expect(page.getByTestId('apply-form-page')).toBeVisible({ timeout: 5000 })
  // The reference is prefilled from the row context; no manual ID input exists.
  await expect(page.getByTestId('apply-prefilled-reference')).toBeVisible()
}

async function submitApplyInvoiceForm(page: Page): Promise<void> {
  await page.getByTestId('apply-invoice-submit-button').click()
  await expect(page.getByTestId('invoice-user-page')).toBeVisible({
    timeout: 10000,
  })
  await page.waitForLoadState('networkidle')
  await page.waitForTimeout(300)
}

function execPgSql(query: string): string {
  // --set ON_ERROR_STOP=on makes psql exit non-zero on any SQL error so a
  // failed INSERT/SELECT surfaces via execSync instead of silently exiting 0.
  return execSync(
    `docker exec -i ${POSTGRES_CONTAINER} psql -U postgres -d herald_demo -t -A --set ON_ERROR_STOP=on`,
    {
      input: query,
      encoding: 'utf-8',
      timeout: 10000,
    }
  ).trim()
}

function seedPaymentAttempt(realmId: string, userEmail: string): string {
  const userId = execPgSql(
    `SELECT id FROM account WHERE email = '${userEmail}' AND realm_id = '${realmId}'`
  )
  if (!userId) throw new Error(`User not found: ${userEmail} in realm ${realmId}`)

  const paId = randomUUID()
  execPgSql(
    `INSERT INTO payment_attempts (id, realm_id, user_id, payment_provider, target_type, target_id, amount, currency, status, expires_at) ` +
      `VALUES ('${paId}', '${realmId}', '${userId}', 'stripe', 'subscription_plan', '${randomUUID()}', 1000, 'USD', 'Pending', NOW() + INTERVAL '1 hour')`
  )
  return paId
}

function seedPurchaseAttempt(
  realmId: string,
  userEmail: string,
  marker: string
): { paymentAttemptId: string } {
  const userId = execPgSql(
    `SELECT id FROM account WHERE email = '${userEmail}' AND realm_id = '${realmId}'`
  )
  if (!userId) throw new Error(`User not found: ${userEmail} in realm ${realmId}`)

  // Provider choice rationale: the seeded payment_attempt must render a row on
  // /user/subscription-history AND that row's per-row Invoice button must be
  // ENABLED (route === 'manual_fallback', canApply === true) so the test can
  // click it to open the apply form. The frontend
  // (purchase-history-list.tsx) only renders the Invoice button when
  // `purchase.paymentProvider !== 'stripe'`, and the backend eligibility rule
  // `determine_invoice_apply_route` (invoice_eligibility.rs) returns:
  //   - 'external_provider' (disabled) for stripe under the default
  //     provider_first policy (Rule 4),
  //   - 'disabled' for creem — Creem is Merchant of Record (Rule 2),
  //   - 'manual_fallback' (enabled) for any other non-MoR provider (Rule 6).
  // Both payment_attempts and provider_entitlement_mappings have a CHECK
  // constraint restricting payment_provider to {stripe, creem, apple, google},
  // so the only values that satisfy (render + enabled + constraint) are
  // 'apple' and 'google'. We use 'apple' (arbitrary; both behave identically).
  // Using 'stripe' (the previous value) hid the button entirely, which is why
  // every apply-flow test failed at openApplyInvoiceForm.
  const SEED_PAYMENT_PROVIDER = 'apple'

  // Reuse an existing enabled one_time mapping if present; otherwise create one
  // with the CURRENT schema only. NOTE: provider_entitlement_mappings and
  // payment_attempts no longer carry bucket_id / points_per_period /
  // validity_days / grant_on_subscribe (those columns were dropped); the
  // legacy INSERT referenced them and failed at seed time.
  let mappingId = execPgSql(
    `SELECT id FROM provider_entitlement_mappings WHERE realm_id = '${realmId}' AND billing_type = 'one_time' AND enabled = true ORDER BY created_at LIMIT 1`
  )
  if (!mappingId) {
    mappingId = randomUUID()
    const externalProductId = `demo-credit-pack-${marker}`
    execPgSql(
      `INSERT INTO provider_entitlement_mappings ` +
        `(id, realm_id, payment_provider, external_product_id, entitlement_key, billing_type, enabled, synced_at, created_at, updated_at) ` +
        `VALUES ('${mappingId}', '${realmId}', '${SEED_PAYMENT_PROVIDER}', '${externalProductId}', 'demo-credit-pack-${marker}', 'one_time', true, NOW(), NOW(), NOW())`
    )
  }

  // Insert a Succeeded entitlement_mapping payment attempt using the current
  // column set (no bucket_id; mirrors scripts/lib/demo_seed.py).
  // list_purchase_history filters status='Succeeded' AND target_type='entitlement_mapping'.
  const paymentAttemptId = randomUUID()
  const providerReference = `cs_demo_${paymentAttemptId.slice(-3)}`
  execPgSql(
    `INSERT INTO payment_attempts ` +
      `(id, realm_id, user_id, payment_provider, target_type, target_id, amount, currency, status, provider_reference, provider_status, metadata, expires_at, completed_at, created_at, updated_at) ` +
      `VALUES ('${paymentAttemptId}', '${realmId}', '${userId}'::uuid, '${SEED_PAYMENT_PROVIDER}', 'entitlement_mapping', '${mappingId}'::uuid, 1000, 'usd', 'Succeeded', '${providerReference}', 'paid', NULL, NOW() + INTERVAL '1 hour', NOW(), NOW(), NOW())`
  )

  return { paymentAttemptId }
}

function ensureDemoUser(realmId: string, email: string, password: string): void {
  const existing = execPgSql(
    `SELECT id FROM account WHERE email = '${email}' AND realm_id = '${realmId}'`
  )
  if (existing) return

  const userId = randomUUID()
  const hash = execPgSql(
    `SELECT password FROM account WHERE email = 'user1@demo.com' AND realm_id = '${realmId}'`
  )
  execPgSql(
    `INSERT INTO account (id, realm_id, email, username, password, status, created_at, updated_at) ` +
      `VALUES ('${userId}', '${realmId}', '${email}', '${email.split('@')[0]}', '${hash}', 1, NOW(), NOW())`
  )
}

test.describe('[Regular User] Invoice User Application Demo Tests', () => {
  test.beforeEach(async ({ page }) => {
    await verifyTestEnvironment(page, {
      requiredRealms: [REALM_ID],
      requiredUsers: [DEMO_ADMIN.email],
    })
  })

  test.afterEach(async ({ page, testStartTime }) => {
    await cleanupTestData(page, REALM_ID, {
      keepUsers: [DEMO_ADMIN.email],
      timestamp: testStartTime,
    })
  })

  // ============================================================================
  // User Story US-IV-010: Configure Seller Info
  // ============================================================================

  test.describe('US-IV-010: Configure Seller Info', () => {
    test('complete seller config flow: configure, verify, update', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const sellerName = `Seller-${testStartTime}`
      const sellerAddress = '123 Test Street'
      const sellerEmail = `seller-${testStartTime}@example.com`
      const sellerPhone = '+1-555-0100'
      const paymentTerms = 'Net 30'

      const updatedName = `Seller-Updated-${testStartTime}`
      const updatedPhone = '+1-555-0200'

      await test.step('Given: admin is logged in and on invoice admin page', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', REALM_ID)
        await navigateToInvoiceAdminPage(page, REALM_ID)
      })

      await test.step('Scene 1 - When: click Seller Config button', async () => {
        await page.getByTestId('seller-config-button').click()
        await expect(page.getByTestId('seller-config-form-dialog')).toBeVisible({ timeout: 5000 })
      })

      await test.step('And: fill seller config form and save', async () => {
        await page.getByTestId('seller-config-name-input').fill(sellerName)
        await page.getByTestId('seller-config-address-input').fill(sellerAddress)
        await page.getByTestId('seller-config-email-input').fill(sellerEmail)
        await page.getByTestId('seller-config-phone-input').fill(sellerPhone)
        await page.getByTestId('seller-config-payment-terms-input').click()
        await page.getByRole('option', { name: paymentTerms }).click()
        await page.getByTestId('seller-config-tax-id-input').fill('TAX123456')

        await page.getByTestId('seller-config-save-button').click()
      })

      await test.step('Then: config dialog closes (save successful)', async () => {
        await expect(page.getByTestId('seller-config-form-dialog')).toBeHidden({
          timeout: 10000,
        })
      })

      await test.step('Scene 2 - When: reopen seller config to verify and update', async () => {
        await page.getByTestId('seller-config-button').click()
        await expect(page.getByTestId('seller-config-form-dialog')).toBeVisible({ timeout: 5000 })
      })

      await test.step('Then: previous values are persisted', async () => {
        await expect(page.getByTestId('seller-config-name-input')).toHaveValue(sellerName)
        await expect(page.getByTestId('seller-config-address-input')).toHaveValue(sellerAddress)
        await expect(page.getByTestId('seller-config-email-input')).toHaveValue(sellerEmail)
        await expect(page.getByTestId('seller-config-phone-input')).toHaveValue(sellerPhone)
      })

      await test.step('And: update the name and phone, then save', async () => {
        await page.getByTestId('seller-config-name-input').clear()
        await page.getByTestId('seller-config-name-input').fill(updatedName)
        await page.getByTestId('seller-config-phone-input').clear()
        await page.getByTestId('seller-config-phone-input').fill(updatedPhone)

        await page.getByTestId('seller-config-save-button').click()
        await expect(page.getByTestId('seller-config-form-dialog')).toBeHidden({
          timeout: 10000,
        })
      })

      await test.step('And: reopen to verify updated values persist', async () => {
        await page.getByTestId('seller-config-button').click()
        await expect(page.getByTestId('seller-config-form-dialog')).toBeVisible({ timeout: 5000 })
        await expect(page.getByTestId('seller-config-name-input')).toHaveValue(updatedName)
        await expect(page.getByTestId('seller-config-phone-input')).toHaveValue(updatedPhone)
      })

      await demoLogger.testCode.log('Seller config flow verified: create, verify, update')
    })
  })

  // ============================================================================
  // User Story US-IV-011: User Applies for Invoice
  // ============================================================================

  test.describe('US-IV-011: User Applies for Invoice', () => {
    test('user applies for invoice from purchase history context', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const sellerName = `Seller-Purchase-${testStartTime}`
      const user = DEMO_USERS.user1
      const billingName = `PurchaseInvoice-${testStartTime}`
      const { paymentAttemptId } = seedPurchaseAttempt(REALM_ID, user.email, String(testStartTime))

      await test.step('Given: admin configures seller info', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', REALM_ID)
        await navigateToInvoiceAdminPage(page, REALM_ID)

        await page.getByTestId('seller-config-button').click()
        await expect(page.getByTestId('seller-config-form-dialog')).toBeVisible({ timeout: 5000 })
        await page.getByTestId('seller-config-name-input').fill(sellerName)
        await page.getByTestId('seller-config-address-input').fill('123 Seller Street')
        await page.getByTestId('seller-config-tax-id-input').fill('TAX123456')
        await page.getByTestId('seller-config-save-button').click()
        await expect(page.getByTestId('seller-config-form-dialog')).toBeHidden({
          timeout: 10000,
        })
      })

      await test.step('When: user opens purchase history', async () => {
        await page.context().clearCookies()
        await loginPage.loginAsUser(user.email, user.password, REALM_ID)
        await page.goto(`/user/subscription-history`)
        await expect(page.getByTestId(`purchase-history-item-${paymentAttemptId}`)).toBeVisible({
          timeout: 10000,
        })
      })

      await test.step('And: user clicks invoice action for the purchase', async () => {
        await page.getByTestId(`purchase-history-invoice-button-${paymentAttemptId}`).click()
        await expect(page.getByTestId('apply-form-page')).toBeVisible({
          timeout: 10000,
        })
        await expect(page.getByTestId('apply-prefilled-reference')).toBeVisible()
        await expect(page.getByTestId('apply-payment-attempt-id-input')).not.toBeVisible()
      })

      await test.step('And: user fills billing info and submits', async () => {
        await page.getByTestId('apply-billing-name-input').fill(billingName)
        await page.getByTestId('apply-billing-address-input').fill('456 User Street')
        await page.getByTestId('apply-billing-tax-id-input').fill('TAX123456')
        await page.getByTestId('apply-due-date-input').fill(futureDueDate())
        await page.getByTestId('apply-notes-input').fill(`Context invoice ${testStartTime}`)
        await submitApplyInvoiceForm(page)
      })

      await test.step('Then: invoice appears in user invoice list', async () => {
        await expect(page.getByTestId('invoice-user-table')).toBeVisible()
        const tableRows = page.getByTestId('invoice-user-table').locator('tbody tr')
        await expect(tableRows.first()).toBeVisible({ timeout: 10000 })
      })

      await demoLogger.testCode.log(
        `User applied from purchase history using payment attempt ${paymentAttemptId}`
      )
    })

    test('user applies for invoice and views application status', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const sellerName = `Seller-Apply-${testStartTime}`
      const user = DEMO_USERS.user1
      const billingName = `Billing-${testStartTime}`
      const billingEmail = `billing-${testStartTime}@example.com`
      const billingAddress = '456 User Street'
      let paymentAttemptIdForApply = ''

      await test.step('Given: admin configures seller info', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', REALM_ID)
        await navigateToInvoiceAdminPage(page, REALM_ID)

        // Configure seller info
        await page.getByTestId('seller-config-button').click()
        await expect(page.getByTestId('seller-config-form-dialog')).toBeVisible({ timeout: 5000 })
        await page.getByTestId('seller-config-name-input').fill(sellerName)
        await page.getByTestId('seller-config-address-input').fill('123 Seller Street')
        await page
          .getByTestId('seller-config-email-input')
          .fill(`seller-apply-${testStartTime}@example.com`)
        await page.getByTestId('seller-config-tax-id-input').fill('TAX123456')
        await page.getByTestId('seller-config-save-button').click()
        await expect(page.getByTestId('seller-config-form-dialog')).toBeHidden({
          timeout: 10000,
        })
      })

      await test.step('When: switch to regular user session', async () => {
        // Clear session and login as user
        await page.context().clearCookies()
        await loginPage.loginAsUser(user.email, user.password, REALM_ID)
      })

      await test.step('And: seed a Stripe purchase-attempt row for the apply context', async () => {
        // The apply form is now reached only via per-row Invoice buttons in
        // purchase history (one_time entitlement_mapping, Succeeded). Seed one.
        paymentAttemptIdForApply = seedPurchaseAttempt(
          REALM_ID,
          user.email,
          `apply-${testStartTime}`
        ).paymentAttemptId
      })

      await test.step('Then: user invoice page still lists invoices (view-only entry)', async () => {
        // The My Invoices page itself still exists and lists invoices; only the
        // standalone page-level "Apply" button was removed.
        await page.goto(`/user/invoices`)
        await expect(page.getByTestId('invoice-user-page')).toBeVisible({
          timeout: 10000,
        })
        await expect(page.getByTestId('invoice-user-heading')).toBeVisible()
        await expect(page.getByTestId('invoice-user-heading')).toHaveText('My Invoices')
        await expect(page.getByTestId('invoice-user-table')).toBeVisible()
        // Standalone Apply button was removed — assert it is gone.
        await expect(page.getByTestId('apply-invoice-button')).toHaveCount(0)
      })

      await test.step('Scene 1 - When: open the apply form via the purchase-history row button', async () => {
        await openApplyInvoiceForm(page, REALM_ID, paymentAttemptIdForApply)
      })

      await test.step('And: fill apply form with billing info (reference is prefilled)', async () => {
        await page.getByTestId('apply-billing-name-input').fill(billingName)
        await page.getByTestId('apply-billing-email-input').fill(billingEmail)
        await page.getByTestId('apply-billing-address-input').fill(billingAddress)
        await page.getByTestId('apply-billing-tax-id-input').fill('TAX123456')
        await page.getByTestId('apply-billing-phone-input').fill('+1-555-0300')
        await page.getByTestId('apply-due-date-input').fill(futureDueDate())
        await page
          .getByTestId('apply-notes-input')
          .fill(`Test invoice application at ${testStartTime}`)
      })

      await test.step('And: submit the application', async () => {
        await submitApplyInvoiceForm(page)
      })

      await test.step('Scene 2 - Then: user invoice list shows the applied invoice', async () => {
        await expect(page.getByTestId('invoice-user-table')).toBeVisible()
        // The table should contain the billing name or show at least one invoice
        const tableRows = page.getByTestId('invoice-user-table').locator('tbody tr')
        await expect(tableRows.first()).toBeVisible({ timeout: 10000 })
      })

      await demoLogger.testCode.log('User applied for invoice and verified in list')
    })
  })

  // ============================================================================
  // User Story US-IV-012: Admin Reviews User Application
  // ============================================================================

  test.describe('US-IV-012: Admin Reviews User Application', () => {
    test('admin reviews, issues, and voids user applications', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const sellerName = `Seller-Review-${testStartTime}`
      const user = DEMO_USERS.user1
      const billingName = `Review-${testStartTime}`

      // -- Setup: configure seller + user applies --
      await test.step('Given: admin configures seller info', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', REALM_ID)
        await navigateToInvoiceAdminPage(page, REALM_ID)

        await page.getByTestId('seller-config-button').click()
        await expect(page.getByTestId('seller-config-form-dialog')).toBeVisible({ timeout: 5000 })
        await page.getByTestId('seller-config-name-input').fill(sellerName)
        await page.getByTestId('seller-config-address-input').fill('123 Seller Street')
        await page.getByTestId('seller-config-tax-id-input').fill('TAX123456')
        await page.getByTestId('seller-config-save-button').click()
        await expect(page.getByTestId('seller-config-form-dialog')).toBeHidden({
          timeout: 10000,
        })
      })

      await test.step('And: user applies for an invoice', async () => {
        await page.context().clearCookies()
        await loginPage.loginAsUser(user.email, user.password, REALM_ID)

        // Seed a Succeeded entitlement_mapping attempt so it appears in purchase
        // history (the new entry point for the apply form).
        const paymentAttemptId = seedPurchaseAttempt(
          REALM_ID,
          user.email,
          `review-${testStartTime}`
        ).paymentAttemptId

        await openApplyInvoiceForm(page, REALM_ID, paymentAttemptId)
        await page.getByTestId('apply-billing-name-input').fill(billingName)
        await page
          .getByTestId('apply-billing-email-input')
          .fill(`review-${testStartTime}@example.com`)
        await page.getByTestId('apply-billing-address-input').fill('456 User Street')
        await page.getByTestId('apply-billing-tax-id-input').fill('TAX123456')
        await page.getByTestId('apply-due-date-input').fill(futureDueDate())
        await submitApplyInvoiceForm(page)
      })

      // -- Scene 1: Admin filters by source "Application" --
      await test.step('When: switch back to admin and filter by source', async () => {
        await page.context().clearCookies()
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', REALM_ID)
        await navigateToInvoiceAdminPage(page, REALM_ID)

        // Filter by source "Application" (user_application)
        await page.getByTestId('invoice-source-filter').click()
        await page.getByRole('option', { name: 'Application' }).click()
        await page.waitForLoadState('networkidle')
        // Technical delay: allow React Query to refetch with filter
        await page.waitForTimeout(300)
      })

      await test.step('Then: user-applied invoice appears with "Application" source', async () => {
        // The table should show the user-applied invoice
        const row = page.locator('tr').filter({ hasText: billingName }).first()
        await expect(row).toBeVisible({ timeout: 10000 })

        // Verify source badge shows "Application"
        await expect(row.getByText('Application')).toBeVisible()
      })

      // -- Scene 2: Admin issues the user application --
      let issuedInvoiceNumber: string
      await test.step('Scene 2 - When: admin adds line items and issues the user application', async () => {
        // Extract invoice number from the row before editing
        const row = page.locator('tr').filter({ hasText: billingName }).first()
        await expect(row).toBeVisible({ timeout: 5000 })
        issuedInvoiceNumber = (await row.locator('td').nth(1).textContent()) ?? ''
        const trimmedNumber = issuedInvoiceNumber.trim() || billingName

        // Edit to add a line item and due date (backend requires both to issue)
        const dueDate = new Date(Date.now() + 30 * 24 * 60 * 60 * 1000).toISOString().split('T')[0]
        await editInvoice(page, REALM_ID, trimmedNumber, {
          lineItems: [{ name: 'Service Fee', quantity: '1', unitPrice: 10000 }],
          dueDate,
        })

        // Issue the invoice
        await issueInvoice(page, trimmedNumber)
      })

      await test.step('Then: invoice status changes to issued', async () => {
        // Use the billing name to find the row since invoice number may vary
        const row = page.locator('tr').filter({ hasText: billingName }).first()
        await expect(row).toBeVisible({ timeout: 5000 })
        await expect(row.getByText('Issued', { exact: true })).toBeVisible()
      })

      await demoLogger.testCode.log(
        'Admin reviewed, issued user application, verified source filter'
      )
    })

    test('admin edits then issues user application', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const sellerName = `Seller-EditIssue-${testStartTime}`
      const user = DEMO_USERS.user1
      const billingName = `EditIssue-${testStartTime}`
      const updatedBillingName = `EditIssue-Updated-${testStartTime}`

      // -- Setup --
      await test.step('Given: admin configures seller info', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', REALM_ID)
        await navigateToInvoiceAdminPage(page, REALM_ID)

        await page.getByTestId('seller-config-button').click()
        await expect(page.getByTestId('seller-config-form-dialog')).toBeVisible({ timeout: 5000 })
        await page.getByTestId('seller-config-name-input').fill(sellerName)
        await page.getByTestId('seller-config-address-input').fill('123 Seller Street')
        await page.getByTestId('seller-config-tax-id-input').fill('TAX123456')
        await page.getByTestId('seller-config-save-button').click()
        await expect(page.getByTestId('seller-config-form-dialog')).toBeHidden({
          timeout: 10000,
        })
      })

      await test.step('And: user applies for an invoice', async () => {
        await page.context().clearCookies()
        await loginPage.loginAsUser(user.email, user.password, REALM_ID)

        const paymentAttemptId = seedPurchaseAttempt(
          REALM_ID,
          user.email,
          `edit-issue-${testStartTime}`
        ).paymentAttemptId

        await openApplyInvoiceForm(page, REALM_ID, paymentAttemptId)
        await page.getByTestId('apply-billing-name-input').fill(billingName)
        await page
          .getByTestId('apply-billing-email-input')
          .fill(`edit-issue-${testStartTime}@example.com`)
        await page.getByTestId('apply-billing-address-input').fill('456 User Street')
        await page.getByTestId('apply-billing-tax-id-input').fill('TAX123456')
        await page.getByTestId('apply-due-date-input').fill(futureDueDate())
        await submitApplyInvoiceForm(page)
      })

      // Scene 4: Admin edits then issues
      await test.step('Scene 4 - When: switch to admin and find the user application', async () => {
        await page.context().clearCookies()
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', REALM_ID)
        await navigateToInvoiceAdminPage(page, REALM_ID)

        // Verify the draft is visible
        const row = page.locator('tr').filter({ hasText: billingName }).first()
        await expect(row).toBeVisible({ timeout: 10000 })
      })

      await test.step('And: extract invoice number and edit the invoice', async () => {
        const row = page.locator('tr').filter({ hasText: billingName }).first()
        const invoiceNumber = (await row.locator('td').nth(1).textContent()) ?? ''
        const trimmedNumber = invoiceNumber.trim()

        // Edit the invoice - update billing name, add line item and due date
        const dueDate = new Date(Date.now() + 30 * 24 * 60 * 60 * 1000).toISOString().split('T')[0]
        await editInvoice(page, REALM_ID, trimmedNumber || billingName, {
          billingName: updatedBillingName,
          lineItems: [{ name: 'Service Fee', quantity: '1', unitPrice: 10000 }],
          dueDate,
        })
      })

      await test.step('Then: the updated billing name is visible in the table', async () => {
        const row = page.locator('tr').filter({ hasText: updatedBillingName }).first()
        await expect(row).toBeVisible({ timeout: 10000 })
      })

      await test.step('And: admin issues the edited invoice', async () => {
        // Extract invoice number from the updated row
        const row = page.locator('tr').filter({ hasText: updatedBillingName }).first()
        const invoiceNumber = (await row.locator('td').nth(1).textContent()) ?? ''
        const trimmedNumber = invoiceNumber.trim()

        await issueInvoice(page, trimmedNumber || updatedBillingName)
      })

      await test.step('Then: invoice status changes to issued', async () => {
        const row = page.locator('tr').filter({ hasText: updatedBillingName }).first()
        await expect(row).toBeVisible({ timeout: 5000 })
        await expect(row.getByText('Issued', { exact: true })).toBeVisible()
      })

      await demoLogger.testCode.log('Admin edited then issued user application verified')
    })

    test('admin voids user application', async ({ page, loginPage, demoLogger, testStartTime }) => {
      const sellerName = `Seller-Void-${testStartTime}`
      const user = DEMO_USERS.user1
      const billingName = `Void-${testStartTime}`

      // -- Setup --
      await test.step('Given: admin configures seller info', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', REALM_ID)
        await navigateToInvoiceAdminPage(page, REALM_ID)

        await page.getByTestId('seller-config-button').click()
        await expect(page.getByTestId('seller-config-form-dialog')).toBeVisible({ timeout: 5000 })
        await page.getByTestId('seller-config-name-input').fill(sellerName)
        await page.getByTestId('seller-config-address-input').fill('123 Seller Street')
        await page.getByTestId('seller-config-tax-id-input').fill('TAX123456')
        await page.getByTestId('seller-config-save-button').click()
        await expect(page.getByTestId('seller-config-form-dialog')).toBeHidden({
          timeout: 10000,
        })
      })

      await test.step('And: user applies for an invoice', async () => {
        await page.context().clearCookies()
        await loginPage.loginAsUser(user.email, user.password, REALM_ID)

        const paymentAttemptId = seedPurchaseAttempt(
          REALM_ID,
          user.email,
          `void-${testStartTime}`
        ).paymentAttemptId

        await openApplyInvoiceForm(page, REALM_ID, paymentAttemptId)
        await page.getByTestId('apply-billing-name-input').fill(billingName)
        await page.getByTestId('apply-billing-address-input').fill('456 User Street')
        await page.getByTestId('apply-billing-tax-id-input').fill('TAX123456')
        await page.getByTestId('apply-due-date-input').fill(futureDueDate())
        await submitApplyInvoiceForm(page)
      })

      // -- Scene 3: Admin voids the user application --
      await test.step('Scene 3 - When: switch to admin and void the application', async () => {
        await page.context().clearCookies()
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', REALM_ID)
        await navigateToInvoiceAdminPage(page, REALM_ID)

        // Find the user application row
        const row = page.locator('tr').filter({ hasText: billingName }).first()
        await expect(row).toBeVisible({ timeout: 10000 })

        // Extract invoice number
        const invoiceNumber = (await row.locator('td').nth(1).textContent()) ?? ''
        const trimmedNumber = invoiceNumber.trim() || billingName

        // Void the invoice
        await voidInvoice(page, trimmedNumber, 'Voided by admin during test review')
      })

      await test.step('Then: invoice status changes to void', async () => {
        const row = page.locator('tr').filter({ hasText: billingName }).first()
        await expect(row).toBeVisible({ timeout: 5000 })
        await expect(row.getByText('Void', { exact: true })).toBeVisible()
      })

      await demoLogger.testCode.log('Admin voided user application verified')
    })
  })

  // ============================================================================
  // User Story US-IV-008: User Views Own Invoice List
  // ============================================================================

  test.describe('US-IV-008: User Views Own Invoice List', () => {
    test('user views invoice list with correct columns and data', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const sellerName = `Seller-View-${testStartTime}`
      const user = DEMO_USERS.user1
      const billingName = `ViewList-${testStartTime}`

      // -- Setup: create an invoice as admin then view as user --
      await test.step('Given: admin configures seller and creates an invoice for the user', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', REALM_ID)
        await navigateToInvoiceAdminPage(page, REALM_ID)

        // Configure seller
        await page.getByTestId('seller-config-button').click()
        await expect(page.getByTestId('seller-config-form-dialog')).toBeVisible({ timeout: 5000 })
        await page.getByTestId('seller-config-name-input').fill(sellerName)
        await page.getByTestId('seller-config-address-input').fill('123 Seller Street')
        await page.getByTestId('seller-config-tax-id-input').fill('TAX123456')
        await page.getByTestId('seller-config-save-button').click()
        await expect(page.getByTestId('seller-config-form-dialog')).toBeHidden({
          timeout: 10000,
        })
      })

      // Have user apply so invoice appears in user's list
      await test.step('And: user applies for an invoice', async () => {
        await page.context().clearCookies()
        await loginPage.loginAsUser(user.email, user.password, REALM_ID)

        const paymentAttemptId = seedPurchaseAttempt(
          REALM_ID,
          user.email,
          `view-${testStartTime}`
        ).paymentAttemptId

        await openApplyInvoiceForm(page, REALM_ID, paymentAttemptId)
        await page.getByTestId('apply-billing-name-input').fill(billingName)
        await page.getByTestId('apply-billing-address-input').fill('456 User Street')
        await page.getByTestId('apply-billing-tax-id-input').fill('TAX123456')
        await page.getByTestId('apply-due-date-input').fill(futureDueDate())
        await submitApplyInvoiceForm(page)
      })

      // Scene 1 & 2: User views own invoice list
      await test.step('Scene 1 & 2 - Then: user invoice list displays expected columns', async () => {
        // Navigate to the user invoice list (after submit we are already there,
        // but assert the page-level structure explicitly).
        await expect(page.getByTestId('invoice-user-page')).toBeVisible({ timeout: 10000 })
        // Verify page structure
        await expect(page.getByTestId('invoice-user-heading')).toBeVisible()
        await expect(page.getByTestId('invoice-user-heading')).toHaveText('My Invoices')
        // The standalone "Apply Invoice" page-level button was removed; assert
        // it no longer renders on /user/invoices.
        await expect(page.getByTestId('apply-invoice-button')).toHaveCount(0)

        // Verify table headers: #, Invoice Number, Provider, Total, Status, Due Date, Actions
        const tableHeaders = page.getByTestId('invoice-user-table').locator('th')
        await expect(tableHeaders.nth(0)).toHaveText('#')
        await expect(tableHeaders.nth(1)).toHaveText('Invoice Number')
        await expect(tableHeaders.nth(2)).toHaveText('Provider')
        await expect(tableHeaders.nth(3)).toHaveText('Total')
        await expect(tableHeaders.nth(4)).toHaveText('Status')
        await expect(tableHeaders.nth(5)).toHaveText('Due Date')
        await expect(tableHeaders.nth(6)).toHaveText('Actions')

        // Verify at least one invoice row exists
        const tableRows = page.getByTestId('invoice-user-table').locator('tbody tr')
        await expect(tableRows.first()).toBeVisible({ timeout: 10000 })

        // Verify the row contains expected data patterns
        const firstRow = tableRows.first()
        // Status badge should be visible (Draft for new application).
        // Column order: #, Invoice Number, Provider, Total, Status, Due Date, Actions -> Status is td index 4.
        await expect(firstRow.locator('td').nth(4)).toBeVisible()
      })

      await test.step('And: user cannot see admin-only elements', async () => {
        // User page should NOT have admin filter bar, create button, or source column
        await expect(page.getByTestId('invoice-filter-bar')).not.toBeVisible()
        await expect(page.getByTestId('create-invoice-button')).not.toBeVisible()
      })

      await demoLogger.testCode.log('User invoice list verified with correct columns and data')
    })

    test('user cannot view other users invoices (isolation)', async ({
      page,
      loginPage,
      demoLogger,
      testStartTime,
    }) => {
      const sellerName = `Seller-Iso-${testStartTime}`
      const user1 = DEMO_USERS.user1
      const user2 = DEMO_USERS.user2
      const billingName1 = `Iso-User1-${testStartTime}`

      // Ensure user2 exists in the database (seed data only has user1)
      ensureDemoUser(REALM_ID, user2.email, user2.password)

      await test.step('Given: admin configures seller info', async () => {
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', REALM_ID)
        await navigateToInvoiceAdminPage(page, REALM_ID)

        await page.getByTestId('seller-config-button').click()
        await expect(page.getByTestId('seller-config-form-dialog')).toBeVisible({ timeout: 5000 })
        await page.getByTestId('seller-config-name-input').fill(sellerName)
        await page.getByTestId('seller-config-address-input').fill('123 Seller Street')
        await page.getByTestId('seller-config-tax-id-input').fill('TAX123456')
        await page.getByTestId('seller-config-save-button').click()
        await expect(page.getByTestId('seller-config-form-dialog')).toBeHidden({
          timeout: 10000,
        })
      })

      await test.step('And: user1 applies for an invoice', async () => {
        await page.context().clearCookies()
        await loginPage.loginAsUser(user1.email, user1.password, REALM_ID)

        const paymentAttemptId = seedPurchaseAttempt(
          REALM_ID,
          user1.email,
          `iso-${testStartTime}`
        ).paymentAttemptId

        await openApplyInvoiceForm(page, REALM_ID, paymentAttemptId)
        await page.getByTestId('apply-billing-name-input').fill(billingName1)
        await page.getByTestId('apply-billing-address-input').fill('456 User Street')
        await page.getByTestId('apply-billing-tax-id-input').fill('TAX123456')
        await page.getByTestId('apply-due-date-input').fill(futureDueDate())
        await submitApplyInvoiceForm(page)
      })

      await test.step('Scene 3 - When: user2 logs in and views their invoices', async () => {
        await page.context().clearCookies()
        await loginPage.loginAsUser(user2.email, user2.password, REALM_ID)

        await page.goto(`/user/invoices`)
        await expect(page.getByTestId('invoice-user-page')).toBeVisible({
          timeout: 10000,
        })
      })

      await test.step('Then: user2 cannot see user1 invoice', async () => {
        await expect(page.getByTestId('invoice-user-table')).toBeVisible()
        // user2 should NOT see user1's billing name in their invoice table
        await expect(page.locator('tr').filter({ hasText: billingName1 }).first()).not.toBeVisible()
      })

      await demoLogger.testCode.log(
        'User invoice isolation verified: user2 cannot see user1 invoices'
      )
    })
  })
})
