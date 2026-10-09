/**
 * Points System Test Helpers
 *
 * Helper functions for Points System demo tests.
 * All operations go through the UI.
 */

import { Page, expect } from '@playwright/test'
import { SELECTORS } from '../selectors'
import { TRANSACTION_TYPES, TransactionType, RENEWAL_PERIOD_TYPES, RenewalPeriodType, WAIT_TIMES, PLACEHOLDER_USER_ID } from './points-constants'

export { TRANSACTION_TYPES, RENEWAL_PERIOD_TYPES, WAIT_TIMES, PLACEHOLDER_USER_ID }
export type { TransactionType, RenewalPeriodType }
export async function withRetry<T>(
  operation: () => Promise<T>,
  options: {
    maxRetries?: number
    initialDelay?: number
    backoffMultiplier?: number
    retryableErrors?: string[]
  } = {}
): Promise<T> {
  const {
    maxRetries = 3,
    initialDelay = 1000,
    backoffMultiplier = 2,
    retryableErrors = ['Timeout', 'Network', 'timeout', 'network']
  } = options

  let lastError: Error | null = null
  let currentDelay = initialDelay

  for (let attempt = 0; attempt <= maxRetries; attempt++) {
    try {
      return await operation()
    } catch (error) {
      lastError = error as Error

      const errorMessage = error instanceof Error ? error.message : String(error)
      const isRetryable = retryableErrors.some(errMsg =>
        errorMessage.toLowerCase().includes(errMsg)
      )

      if (!isRetryable || attempt === maxRetries) {
        throw error
      }

      console.log(
        `[Retry] Attempt ${attempt + 1}/${maxRetries + 1} failed: ${errorMessage.substring(0, 100)}`
      )
      console.log(`[Retry] Retrying in ${currentDelay}ms...`)

      await new Promise(resolve => setTimeout(resolve, currentDelay))
      currentDelay *= backoffMultiplier
    }
  }

  throw lastError || new Error('Retry failed')
}

/** Wait for an element to reach the expected state, retrying on timeout. */
export async function waitForStableElement(
  page: Page,
  selector: string,
  options: {
    timeout?: number
    maxRetries?: number
    state?: 'visible' | 'attached' | 'hidden' | 'detached'
  } = {}
): Promise<void> {
  const {
    timeout = 10000,
    maxRetries = 2,
    state = 'visible'
  } = options

  await withRetry(
    async () => {
      const element = page.locator(selector)
      if (state === 'visible') {
        await element.waitFor({ state: 'visible', timeout })
      } else if (state === 'attached') {
        await element.waitFor({ state: 'attached', timeout })
      } else if (state === 'hidden') {
        await element.waitFor({ state: 'hidden', timeout })
      } else if (state === 'detached') {
        await element.waitFor({ state: 'detached', timeout })
      }
    },
    {
      maxRetries,
      initialDelay: 500,
      backoffMultiplier: 1.5,
      retryableErrors: ['Timeout', 'timeout']
    }
  )
}


export interface PointsWalletInfo {
  userId: string
  email: string
  balance: number
  status: string
}

export interface TransactionInfo {
  index: number
  type: string
  amount: number
  balanceAfter: number
  description: string
  time: string
}

export async function searchUserAccounts(page: Page, email: string): Promise<void> {
  await page.locator(SELECTORS.pointsAdmin.accountsSearch).clear()
  await page.locator(SELECTORS.pointsAdmin.accountsSearch).fill(email)
  await page.waitForLoadState('networkidle')
}

export async function selectUserAccount(page: Page, userId: string): Promise<void> {
  await page.locator(SELECTORS.pointsAdmin.accountRow(userId)).click()
  await expect(page.locator(SELECTORS.pointsAdmin.transactionsSection)).toBeVisible()
}

export async function applyTransactionFilters(
  page: Page,
  filters: {
    type?: TransactionType
    startTime?: string
    endTime?: string
    clientApp?: string
  }
): Promise<void> {
  if (filters.type !== undefined) {
    await page.locator(SELECTORS.pointsAdmin.filterType).selectOption(filters.type)
  }

  if (filters.startTime !== undefined) {
    await page.locator(SELECTORS.pointsAdmin.filterStartTime).fill(filters.startTime)
  }

  if (filters.endTime !== undefined) {
    await page.locator(SELECTORS.pointsAdmin.filterEndTime).fill(filters.endTime)
  }

  if (filters.clientApp !== undefined) {
    await page.locator(SELECTORS.pointsAdmin.filterClientApp).selectOption(filters.clientApp)
  }

  await page.locator(SELECTORS.pointsAdmin.applyFiltersButton).click()
  await page.waitForLoadState('networkidle')
}

export async function resetTransactionFilters(page: Page): Promise<void> {
  await page.locator(SELECTORS.pointsAdmin.resetFiltersButton).click()
  await page.waitForLoadState('networkidle')
}

export async function getUserAccountInfo(
  page: Page,
  userId: string
): Promise<PointsWalletInfo> {
  const row = page.locator(SELECTORS.pointsAdmin.accountRow(userId))

  const [nameText, email, balanceText, status] = await Promise.all([
    row.locator('.font-medium').textContent(),
    row.locator('.text-sm.text-muted-foreground').textContent(),
    row.locator('.text-2xl.font-bold').textContent(),
    row.locator('[data-slot="badge"]').textContent(),
  ])

  return {
    userId,
    email: email || nameText || '',
    balance: parseInt(balanceText?.replace(/,/g, '') || '0'),
    status: status || '',
  }
}

export async function getTransactionInfo(
  page: Page,
  index: number
): Promise<TransactionInfo> {
  const row = page.locator(SELECTORS.pointsAdmin.transactionRow(index))

  const [type, amountText, balanceText, description, time] = await Promise.all([
    row.locator(SELECTORS.pointsAdmin.transactionType(index)).textContent(),
    row.locator(SELECTORS.pointsAdmin.transactionAmount(index)).textContent(),
    row.locator(SELECTORS.pointsAdmin.transactionBalance(index)).textContent(),
    row.locator(SELECTORS.pointsAdmin.transactionDescription(index)).textContent(),
    row.locator(SELECTORS.pointsAdmin.transactionTime(index)).textContent(),
  ])

  return {
    index,
    type: type || '',
    amount: parseInt(amountText?.replace(/[^0-9-]/g, '') || '0'),
    balanceAfter: parseInt(balanceText?.replace(/,/g, '') || '0'),
    description: description || '',
    time: time || '',
  }
}

export async function navigateToPointsUser(page: Page, realmId: string): Promise<void> {
  await page.goto(`/user/points`)
  await expect(page.locator(SELECTORS.pointsUser.page)).toBeVisible()
}

async function parseNumberFromSelector(page: Page, selector: string): Promise<number> {
  const text = await page.locator(selector).textContent()
  return parseInt(text?.replace(/,/g, '') || '0')
}

export async function getUserBalance(page: Page): Promise<number> {
  return parseNumberFromSelector(page, SELECTORS.pointsUser.balanceAmount)
}

export async function getWalletStatus(page: Page): Promise<string> {
  return await page.locator(SELECTORS.pointsUser.accountStatus).textContent() || ''
}

export async function applyUserTransactionFilters(
  page: Page,
  filters: {
    type?: TransactionType
    startTime?: string
    endTime?: string
  }
): Promise<void> {
  if (filters.type !== undefined) {
    await page.locator(SELECTORS.pointsUser.filterType).selectOption(filters.type)
  }

  if (filters.startTime !== undefined) {
    await page.locator(SELECTORS.pointsUser.filterStartTime).fill(filters.startTime)
  }

  if (filters.endTime !== undefined) {
    await page.locator(SELECTORS.pointsUser.filterEndTime).fill(filters.endTime)
  }

  await page.locator(SELECTORS.pointsUser.applyFiltersButton).click()
  await page.waitForLoadState('networkidle')
}

export async function resetUserTransactionFilters(page: Page): Promise<void> {
  await page.locator(SELECTORS.pointsUser.resetFiltersButton).click()
  await page.waitForLoadState('networkidle')
}

export async function viewTransactionDetail(page: Page, index: number): Promise<void> {
  await page.locator(SELECTORS.pointsUser.transactionRow(index)).click()
  await expect(page.locator('[data-testid="transaction-detail-dialog"]')).toBeVisible()
}

export const POINTS_ROUTES = {
  FREE_USERS: (realmId: string) => `/manage/points/free-users`,
  USER_POINTS: (realmId: string) => `/user/points`,
  REGISTER: (realmId: string) => `/${realmId}/auth/register`,
} as const

export function generateTestEmail(testStartTime: number, prefix: string = 'test-user'): string {
  return `${prefix}-${testStartTime}@example.com`
}

export async function updateRealmConfig(
  page: Page,
  selector: string,
  value: string
): Promise<void> {
  await page.locator(selector).fill(value)
  await page.locator(SELECTORS.points.saveButton).click()
  await expect(page.getByText(/配置已更新|configuration.*updated/i).first()).toBeVisible()
  await expect(page.locator(selector)).toHaveValue(value)
}

export async function verifyConfigValidation(
  page: Page,
  selector: string,
  invalidValue: string,
  expectedErrorPattern: RegExp
): Promise<void> {
  await page.locator(selector).clear()
  await page.locator(selector).fill(invalidValue)
  // Client-side Zod validation shows the error immediately and disables save.
  await expect(page.getByText(expectedErrorPattern)).toBeVisible()
  await expect(page.locator(SELECTORS.points.saveButton)).toBeDisabled()
}

export async function verifyChartDisplayed(
  page: Page,
  chartSelector: string,
  verifyLegend?: string[]
): Promise<void> {
  const chart = page.getByTestId(chartSelector)
  await expect(chart).toBeVisible()
  await expect(chart.locator('canvas')).toBeVisible()

  if (verifyLegend) {
    for (const legend of verifyLegend) {
      await expect(page.getByText(new RegExp(legend, 'i'))).toBeVisible()
    }
  }
}

export async function registerUser(
  page: Page,
  realmId: string,
  email: string,
  password: string = 'password123'
): Promise<void> {
  await page.goto(POINTS_ROUTES.REGISTER(realmId))

  await page.getByTestId('register-email-input').waitFor({ state: 'visible', timeout: 5000 })
  await page.getByTestId('register-email-input').fill(email)
  // Nickname is required on the register form; derive it from the email local
  // part so points demos don't need an extra parameter.
  await page
    .getByTestId('register-nickname-input')
    .fill(email.split('@')[0] || 'points-user')
  await page.getByTestId('register-password-input').fill(password)
  await page.getByTestId('register-confirm-password-input').fill(password)

  const consentCheckbox = page.locator(SELECTORS.legalConsent.registerConsentCheckbox)
  if (await consentCheckbox.isVisible().catch(() => false)) {
    await consentCheckbox.check()
  }

  const responsePromise = page.waitForResponse(
    response => response.url().includes('/register') && response.request().method() === 'POST',
    { timeout: 10000 }
  )

  await page.getByTestId('register-submit-button').click()
  await responsePromise
}

export async function navigateToPointsPageAndVerify(
  page: Page,
  realmId: string
): Promise<void> {
  await page.goto(POINTS_ROUTES.USER_POINTS(realmId))
  await expect(page.locator(SELECTORS.pointsUser.page)).toBeVisible()
}

