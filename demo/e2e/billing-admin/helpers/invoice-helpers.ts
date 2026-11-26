/**
 * Invoice Admin Helpers
 *
 * Shared helper functions for admin invoice lifecycle demo tests.
 * Covers: create, edit, issue, void, mark-paid, and table verification.
 *
 * All amounts are in smallest currency unit (cents).
 * unitPrice in the form is an integer (cents).
 * Fee value inputs are in display/major-currency units (e.g. "5" means 5.00 = 500 cents for fixed,
 * "6" means 6% for percent).
 */

import { Page, expect } from '@playwright/test'

export interface InvoiceLineItemData {
  name: string
  quantity?: string   // default '1'
  unitPrice?: number  // in cents, default 0
}

export interface InvoiceCreateData {
  accountId: string
  billingName: string
  billingEmail?: string
  billingAddress?: string
  billingTaxId?: string
  sellerName: string
  sellerEmail?: string
  sellerAddress?: string
  sellerTaxId?: string
  lineItems: InvoiceLineItemData[]
  dueDate: string           // ISO date string, e.g. '2026-12-31'
  discountMode?: 'fixed' | 'percent' | null
  discountValue?: string | null
  taxMode?: 'fixed' | 'percent' | null
  taxValue?: string | null
  shippingMode?: 'fixed' | null
  shippingValue?: string | null
}

export interface InvoiceEditChanges {
  billingName?: string
  billingTaxId?: string
  sellerName?: string
  lineItems?: InvoiceLineItemData[]
  dueDate?: string
  discountMode?: 'fixed' | 'percent' | null
  discountValue?: string | null
  taxMode?: 'fixed' | 'percent' | null
  taxValue?: string | null
}

export async function navigateToInvoiceAdminPage(
  page: Page,
  realmId: string,
): Promise<void> {
  await page.goto(`/${realmId}/manage/billing/invoices`)
  await expect(page.getByTestId('invoice-admin-page')).toBeVisible({ timeout: 10000 })
}

export async function createInvoice(
  page: Page,
  realmId: string,
  data: InvoiceCreateData,
): Promise<string> {
  await page.getByTestId('create-invoice-button').click()
  await expect(page.getByTestId('invoice-form-page')).toBeVisible({ timeout: 5000 })

  await page.getByTestId('invoice-account-id').fill(data.accountId)

  await page.getByTestId('invoice-billing-name').fill(data.billingName)
  if (data.billingEmail) {
    await page.getByTestId('invoice-billing-email').fill(data.billingEmail)
  }
  await page.getByTestId('invoice-billing-address').fill(data.billingAddress ?? '123 Billing St')
  await page.getByTestId('invoice-billing-tax-id').fill(data.billingTaxId ?? 'N/A')

  await page.getByTestId('invoice-seller-name').fill(data.sellerName)
  if (data.sellerEmail) {
    await page.getByTestId('invoice-seller-email').fill(data.sellerEmail)
  }
  await page.getByTestId('invoice-seller-address').fill(data.sellerAddress ?? '456 Seller Ave')
  await page.getByTestId('invoice-seller-tax-id').fill(data.sellerTaxId ?? 'N/A')

  const firstItem = data.lineItems[0]
  await page.getByTestId('invoice-line-item-name-0').fill(firstItem.name)
  if (firstItem.quantity !== undefined) {
    await page.getByTestId('invoice-line-item-quantity-0').fill(firstItem.quantity)
  }
  if (firstItem.unitPrice !== undefined) {
    await page.getByTestId('invoice-line-item-unit-price-0').fill((firstItem.unitPrice / 100).toFixed(2))
  }

  for (let i = 1; i < data.lineItems.length; i++) {
    await page.getByTestId('invoice-add-line-item').click()
    const item = data.lineItems[i]
    await page.getByTestId(`invoice-line-item-name-${i}`).fill(item.name)
    if (item.quantity !== undefined) {
      await page.getByTestId(`invoice-line-item-quantity-${i}`).fill(item.quantity)
    }
    if (item.unitPrice !== undefined) {
      await page.getByTestId(`invoice-line-item-unit-price-${i}`).fill((item.unitPrice / 100).toFixed(2))
    }
  }

  if (data.discountMode) {
    await selectFeeMode(page, 'invoice-discount', data.discountMode)
    if (data.discountValue != null) {
      await page.getByTestId('invoice-discount-value').fill(data.discountValue)
    }
  }

  if (data.taxMode) {
    await selectFeeMode(page, 'invoice-tax', data.taxMode)
    if (data.taxValue != null) {
      await page.getByTestId('invoice-tax-value').fill(data.taxValue)
    }
  }

  if (data.shippingMode) {
    await selectFeeMode(page, 'invoice-shipping', data.shippingMode)
    if (data.shippingValue != null) {
      await page.getByTestId('invoice-shipping-value').fill(data.shippingValue)
    }
  }

  await page.getByTestId('invoice-due-date').fill(data.dueDate)

  await page.getByTestId('invoice-form-submit-button').click()

  await page.waitForURL(`**/manage/billing/invoices`, { timeout: 10000 })
  await expect(page.getByTestId('invoice-admin-page')).toBeVisible({ timeout: 10000 })

  // New invoices are always in draft status, so filtering by status avoids matching
  // stale rows left behind from previous failed test runs.
  const row = page.locator('tr').filter({ hasText: data.billingName }).filter({ hasText: 'Draft' }).first()
  await expect(row).toBeVisible({ timeout: 10000 })
  const invoiceNumber = await row.locator('td').nth(1).textContent()
  if (!invoiceNumber) {
    throw new Error('Could not extract invoice number from table')
  }
  return invoiceNumber.trim()
}

export async function editInvoice(
  page: Page,
  realmId: string,
  invoiceNumber: string,
  changes: InvoiceEditChanges,
): Promise<void> {
  const row = page.locator('tr').filter({ hasText: invoiceNumber }).first()
  await expect(row).toBeVisible({ timeout: 5000 })

  const menuButton = row.getByRole('button', { name: 'Open menu' })
  await menuButton.click()
  await page.getByRole('menuitem', { name: 'Edit' }).click()

  await expect(page.getByTestId('invoice-form-page')).toBeVisible({ timeout: 5000 })

  if (changes.billingName) {
    await page.getByTestId('invoice-billing-name').clear()
    await page.getByTestId('invoice-billing-name').fill(changes.billingName)
  }

  const billingTaxIdInput = page.getByTestId('invoice-billing-tax-id')
  const billingTaxId = changes.billingTaxId ?? 'N/A'
  if (changes.billingTaxId || (await billingTaxIdInput.inputValue()) === '') {
    await billingTaxIdInput.clear()
    await billingTaxIdInput.fill(billingTaxId)
  }

  if (changes.sellerName) {
    await page.getByTestId('invoice-seller-name').clear()
    await page.getByTestId('invoice-seller-name').fill(changes.sellerName)
  }

  if (changes.lineItems) {
    const existingCount = await page.locator('[data-testid^="invoice-line-item-name-"]').count()

    for (let i = 0; i < changes.lineItems.length; i++) {
      if (i >= existingCount) {
        await page.getByTestId('invoice-add-line-item').click()
        await page.waitForTimeout(200)
      }

      const item = changes.lineItems[i]
      const nameInput = page.getByTestId(`invoice-line-item-name-${i}`)
      await nameInput.fill(item.name)
      await expect(nameInput).toHaveValue(item.name)

      if (item.quantity !== undefined) {
        const quantityInput = page.getByTestId(`invoice-line-item-quantity-${i}`)
        await quantityInput.fill(item.quantity)
        await expect(quantityInput).toHaveValue(item.quantity)
      }
      if (item.unitPrice !== undefined) {
        const unitPriceStr = (item.unitPrice / 100).toFixed(2)
        const unitPriceInput = page.getByTestId(`invoice-line-item-unit-price-${i}`)
        await unitPriceInput.fill(unitPriceStr)
        await expect(unitPriceInput).toHaveValue(unitPriceStr)
      }
    }
  }

  if (changes.dueDate) {
    await page.getByTestId('invoice-due-date').fill(changes.dueDate)
  }

  if (changes.discountMode) {
    await selectFeeMode(page, 'invoice-discount', changes.discountMode)
    if (changes.discountValue != null) {
      await page.getByTestId('invoice-discount-value').clear()
      await page.getByTestId('invoice-discount-value').fill(changes.discountValue)
    }
  }

  if (changes.taxMode) {
    await selectFeeMode(page, 'invoice-tax', changes.taxMode)
    if (changes.taxValue != null) {
      await page.getByTestId('invoice-tax-value').clear()
      await page.getByTestId('invoice-tax-value').fill(changes.taxValue)
    }
  }

  await page.getByTestId('invoice-form-submit-button').click()

  await page.waitForURL(`**/manage/billing/invoices`, { timeout: 10000 })
  await expect(page.getByTestId('invoice-admin-page')).toBeVisible({ timeout: 10000 })
}

export async function issueInvoice(
  page: Page,
  invoiceNumber: string,
): Promise<void> {
  const row = page.locator('tr').filter({ hasText: invoiceNumber }).first()
  await expect(row).toBeVisible({ timeout: 5000 })

  const menuButton = row.getByRole('button', { name: 'Open menu' })
  await menuButton.click()
  await page.getByRole('menuitem', { name: 'Issue' }).click()

  await expect(page.getByTestId('issue-confirm-dialog')).toBeVisible({ timeout: 5000 })
  await page.getByTestId('issue-confirm-button').click()

  await expect(page.getByTestId('issue-confirm-dialog')).toBeHidden({ timeout: 10000 })
  await page.waitForLoadState('networkidle')
  await expect(row.getByText('Issued', { exact: true })).toBeVisible({ timeout: 10000 })
  // Technical delay: allow table to refresh
  await page.waitForTimeout(300)
}

export async function voidInvoice(
  page: Page,
  invoiceNumber: string,
  reason?: string,
): Promise<void> {
  const row = page.locator('tr').filter({ hasText: invoiceNumber }).first()
  await expect(row).toBeVisible({ timeout: 5000 })

  const menuButton = row.getByRole('button', { name: 'Open menu' })
  await menuButton.click()
  await page.getByRole('menuitem', { name: 'Void' }).click()

  await expect(page.getByTestId('void-confirm-dialog')).toBeVisible({ timeout: 5000 })

  if (reason) {
    await page.getByTestId('void-reason-input').fill(reason)
  }

  await page.getByTestId('void-confirm-button').click()

  await expect(page.getByTestId('void-confirm-dialog')).toBeHidden({ timeout: 10000 })
  await page.waitForLoadState('networkidle')
  // Technical delay: allow table to refresh
  await page.waitForTimeout(300)
}

export async function markPaidInvoice(
  page: Page,
  invoiceNumber: string,
): Promise<void> {
  const row = page.locator('tr').filter({ hasText: invoiceNumber }).first()
  await expect(row).toBeVisible({ timeout: 5000 })

  const menuButton = row.getByRole('button', { name: 'Open menu' })
  await menuButton.click()
  await page.getByRole('menuitem', { name: 'Mark Paid' }).click()

  await expect(page.getByTestId('mark-paid-confirm-dialog')).toBeVisible({ timeout: 5000 })
  await page.getByTestId('mark-paid-confirm-button').click()

  await expect(page.getByTestId('mark-paid-confirm-dialog')).toBeHidden({ timeout: 10000 })
  await page.waitForLoadState('networkidle')
  // Technical delay: allow table to refresh
  await page.waitForTimeout(300)
}

/**
 * Verify that a specific invoice appears in the table with the expected status.
 *
 * Status labels (from InvoiceStatusBadge): Draft, Issued, Paid, Void, Overdue
 */
export async function verifyInvoiceInTable(
  page: Page,
  invoiceNumber: string,
  expectedStatus: string,
): Promise<void> {
  const row = page.locator('tr').filter({ hasText: invoiceNumber }).first()
  await expect(row).toBeVisible({ timeout: 10000 })

  const statusLabel = expectedStatus.charAt(0).toUpperCase() + expectedStatus.slice(1).toLowerCase()
  await expect(row.getByText(statusLabel, { exact: true })).toBeVisible()
}

export async function verifyInvoiceNotInTable(
  page: Page,
  invoiceNumber: string,
): Promise<void> {
  await expect(page.locator('tr').filter({ hasText: invoiceNumber })).not.toBeVisible()
}

export async function selectFeeMode(
  page: Page,
  testIdPrefix: string,
  mode: 'fixed' | 'percent' | 'none',
): Promise<void> {
  const trigger = page.getByTestId(`${testIdPrefix}-mode-trigger`)
  await trigger.click()
  await page.getByRole('option', { name: mode === 'fixed' ? 'Fixed' : mode === 'percent' ? 'Percent' : 'None' }).click()
}
