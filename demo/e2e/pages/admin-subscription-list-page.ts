import { Page, Locator, expect } from '@playwright/test'
import { SELECTORS } from '../selectors'
import { BasePage } from './base-page'
import type { UnifiedLogger } from 'playwright-unified-logger'

/**
 * Page Object for the Admin Subscription List page.
 *
 * Route: /{realmId}/manage/billing/subscriptions
 *
 * User stories:
 * - US-EM-006: View subscription projection list
 *
 * @see docs/user-stories/billing/entitlement-mapping.md
 */
export class AdminSubscriptionListPage extends BasePage {
  // Page-level locators
  readonly container: Locator
  readonly heading: Locator
  readonly table: Locator
  readonly pagination: Locator

  // Filter locators
  readonly entitlementKeyFilterInput: Locator
  readonly statusFilterSelect: Locator
  readonly paymentProviderFilterSelect: Locator

  // Empty state
  readonly emptyState: Locator

  constructor(page: Page, logger?: UnifiedLogger) {
    super(page, logger)
    this.container = page.locator(SELECTORS.adminSubscriptionList.page)
    this.heading = page.locator(SELECTORS.adminSubscriptionList.heading)
    this.table = page.locator(SELECTORS.adminSubscriptionList.table)
    this.pagination = page.locator(SELECTORS.adminSubscriptionList.pagination)

    this.entitlementKeyFilterInput = page.locator(SELECTORS.adminSubscriptionList.entitlementKeyFilterInput)
    this.statusFilterSelect = page.locator(SELECTORS.adminSubscriptionList.statusFilterSelect)
    this.paymentProviderFilterSelect = page.locator(SELECTORS.adminSubscriptionList.paymentProviderFilterSelect)

    this.emptyState = page.locator(SELECTORS.adminSubscriptionList.emptyState)
  }

  /**
   * Navigate to the admin subscription list page for a given realm.
   */
  async goto(realmId: string = 'admin'): Promise<void> {
    await super.goto(`/${realmId}/manage/billing/subscriptions`)
    await this.waitForReady()
  }

  /**
   * Wait for the page container and heading to be visible.
   */
  async waitForReady(): Promise<void> {
    await expect(this.container).toBeVisible()
    await expect(this.heading).toBeVisible()
  }

  /**
   * Wait for data to finish loading (table or empty state becomes visible).
   *
   * The frontend renders a loading skeleton while the API call is in flight.
   * Neither the table nor the empty state has its data-testid set during loading,
   * so tests must wait for data to settle before checking table/empty state.
   */
  async waitForDataLoaded(timeout: number = 10000): Promise<void> {
    await this.page.locator(
      `${SELECTORS.adminSubscriptionList.table}, ${SELECTORS.adminSubscriptionList.emptyState}`
    ).first().waitFor({ state: 'visible', timeout })
  }

  /**
   * Get the number of visible subscription rows in the table.
   */
  async getSubscriptionRowCount(): Promise<number> {
    await expect(this.table).toBeVisible()
    const rows = this.table.locator('tbody tr')
    return await rows.count()
  }

  /**
   * Get text content of all cells in a subscription row by index.
   */
  async getSubscriptionRowTexts(rowIndex: number): Promise<string[]> {
    const row = this.table.locator('tbody tr').nth(rowIndex)
    await expect(row).toBeVisible()
    const cells = row.locator('td')
    const count = await cells.count()
    const texts: string[] = []
    for (let i = 0; i < count; i++) {
      texts.push((await cells.nth(i).textContent()) || '')
    }
    return texts
  }

  /**
   * Filter subscriptions by entitlement key using the text input.
   */
  async filterByEntitlementKey(key: string): Promise<void> {
    await this.fillField(this.entitlementKeyFilterInput, key)
  }

  /**
   * Filter subscriptions by status using the Radix Select dropdown.
   */
  async filterByStatus(status: string): Promise<void> {
    await this.selectRadixOption(this.statusFilterSelect, status)
    await this.page.waitForLoadState('domcontentloaded')
  }

  /**
   * Filter subscriptions by payment provider using the Radix Select dropdown.
   */
  async filterByProvider(provider: string): Promise<void> {
    await this.selectRadixOption(this.paymentProviderFilterSelect, provider)
    await this.page.waitForLoadState('domcontentloaded')
  }

  /**
   * Check if the empty state card is visible (no subscriptions).
   */
  async isTableEmpty(): Promise<boolean> {
    return await this.isVisible(this.emptyState)
  }

  /**
   * Get the empty state message text.
   */
  async getEmptyStateText(): Promise<string> {
    await expect(this.emptyState).toBeVisible()
    return (await this.emptyState.textContent()) || ''
  }

  /**
   * Check if pagination controls are visible.
   */
  async isPaginationVisible(): Promise<boolean> {
    return await this.isVisible(this.pagination)
  }

  /**
   * Get all table header text content.
   */
  async getTableHeaders(): Promise<string[]> {
    const headers = this.table.locator('thead th')
    const count = await headers.count()
    const texts: string[] = []
    for (let i = 0; i < count; i++) {
      texts.push((await headers.nth(i).textContent()) || '')
    }
    return texts
  }
}
