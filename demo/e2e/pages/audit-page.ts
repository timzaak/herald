import { Page, Locator, expect } from '@playwright/test'
import { SELECTORS } from '../selectors'
import { BasePage } from './base-page'
import type { UnifiedLogger } from 'playwright-unified-logger'

export class AuditPage extends BasePage {
  // Page-level locators
  readonly container: Locator
  readonly heading: Locator
  readonly table: Locator
  readonly filterBar: Locator
  readonly pagination: Locator

  // Filter locators
  readonly filterCategory: Locator
  readonly filterAction: Locator
  readonly filterActorId: Locator
  readonly filterStartDate: Locator
  readonly filterEndDate: Locator
  readonly filterClear: Locator

  // Detail sheet locators
  readonly detailSheet: Locator
  readonly detailClose: Locator
  readonly detailJson: Locator
  readonly detailResult: Locator

  constructor(page: Page, logger?: UnifiedLogger) {
    super(page, logger)
    this.container = page.locator(SELECTORS.audit.container)
    this.heading = page.locator(SELECTORS.audit.heading)
    this.table = page.locator(SELECTORS.audit.table)
    this.filterBar = page.locator(SELECTORS.audit.filterBar)
    this.pagination = page.locator(SELECTORS.audit.pagination)

    this.filterCategory = page.locator(SELECTORS.audit.filterCategory)
    this.filterAction = page.locator(SELECTORS.audit.filterAction)
    this.filterActorId = page.locator(SELECTORS.audit.filterActorId)
    this.filterStartDate = page.locator(SELECTORS.audit.filterStartDate)
    this.filterEndDate = page.locator(SELECTORS.audit.filterEndDate)
    this.filterClear = page.locator(SELECTORS.audit.filterClear)

    this.detailSheet = page.locator(SELECTORS.audit.detailSheet)
    this.detailClose = page.locator(SELECTORS.audit.detailClose)
    this.detailJson = page.locator(SELECTORS.audit.detailJson)
    this.detailResult = page.locator(SELECTORS.audit.detailResult)
  }

  async goto(): Promise<void> {
    const auditMenuLink = this.page.locator(SELECTORS.sidebar.menuAuditLog)
    await this.smartClick(auditMenuLink)
    await this.waitForReady()
  }

  async waitForReady(): Promise<void> {
    await expect(this.container).toBeVisible()
    await expect(this.heading).toBeVisible()
  }

  async getRowCount(): Promise<number> {
    await expect(this.table).toBeVisible()
    // DataTable renders rows as <tr> inside tbody; header row is in thead
    const rows = this.table.locator('tbody tr')
    return await rows.count()
  }

  async getRowTexts(rowIndex: number): Promise<string[]> {
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
   * Search for a row containing specific text in a given column.
   * Accepts a single search term or an array of terms (matches if any term is found).
   */
  async hasRowWithColumnText(columnIndex: number, searchTerms: string | string[], maxRows = 10): Promise<boolean> {
    const terms = typeof searchTerms === 'string' ? [searchTerms] : searchTerms
    const rowCount = await this.getRowCount()
    for (let i = 0; i < Math.min(rowCount, maxRows); i++) {
      const rowTexts = await this.getRowTexts(i)
      const cellText = rowTexts[columnIndex].toLowerCase()
      if (terms.some(t => cellText.includes(t.toLowerCase()))) {
        return true
      }
    }
    return false
  }

  /**
   * Whether any visible audit row (up to maxRows) carries the given raw
   * action code on its action cell's `data-audit-action` attribute. Use this
   * instead of hasRowWithColumnText(3, ...) for action assertions: the action
   * cell displays a localized label, so the raw code (e.g. `user.create`) is
   * only exposed via the data attribute.
   *
   * Mirrors the same row-locator pattern (`this.table.locator('tbody
   * tr').nth(i)`) used by getRowTexts / getRowCount so the helper stays
   * consistent with the rest of the page object.
   */
  async hasRowWithAction(action: string, maxRows = 10): Promise<boolean> {
    const rowCount = await this.getRowCount()
    for (let i = 0; i < Math.min(rowCount, maxRows); i++) {
      const actionCell = this.table.locator('tbody tr').nth(i).locator('[data-audit-action]').first()
      const value = await actionCell.getAttribute('data-audit-action').catch(() => null)
      if (value === action) return true
    }
    return false
  }

  /**
   * Select an option from a Radix Select component.
   * Clicks trigger, waits for dropdown, selects by data-value or falls back to text match.
   */
  async selectRadixOption(triggerLocator: Locator, value: string): Promise<void> {
    await this.smartClick(triggerLocator)

    const listbox = this.page.locator('[data-slot="select-content"]')
    await expect(listbox).toBeVisible({ timeout: 3000 })

    const optionByValue = listbox.locator(`[data-value="${value}"]`)
    const optionCount = await optionByValue.count()

    if (optionCount > 0) {
      await optionByValue.click()
    } else {
      const optionByText = listbox.locator(`[data-slot="select-item"]`).filter({ hasText: value })
      await optionByText.first().click()
    }

    await expect(listbox).toBeHidden({ timeout: 3000 })
  }

  async filterByCategory(category: string): Promise<void> {
    await this.selectRadixOption(this.filterCategory, category)
    await expect(this.table).toBeVisible()
  }

  async filterByAction(action: string): Promise<void> {
    await this.selectRadixOption(this.filterAction, action)
    await expect(this.table).toBeVisible()
  }

  async filterByActor(actorId: string): Promise<void> {
    await this.fillField(this.filterActorId, actorId)
  }

  async filterByDateRange(start: string, end: string): Promise<void> {
    await this.fillField(this.filterStartDate, start)
    await this.fillField(this.filterEndDate, end)
  }

  async clearFilters(): Promise<void> {
    await this.smartClick(this.filterClear)
    // Button hides when no filters are active
    await expect(this.filterClear).toBeHidden({ timeout: 3000 })
  }

  async hasActiveFilters(): Promise<boolean> {
    return await this.isVisible(this.filterClear)
  }

  async openDetailSheet(rowIndex: number): Promise<void> {
    const row = this.table.locator('tbody tr').nth(rowIndex)
    await expect(row).toBeVisible()
    await row.click()
    await expect(this.detailSheet).toBeVisible({ timeout: 5000 })
  }

  async closeDetailSheet(): Promise<void> {
    await this.smartClick(this.detailClose)
    await expect(this.detailSheet).toBeHidden({ timeout: 5000 })
  }

  async isDetailSheetOpen(): Promise<boolean> {
    return await this.isVisible(this.detailSheet)
  }

  /**
   * Detail sheet uses a definition list (dl/dt/dd) layout.
   * Finds the dt matching the label, then returns the adjacent dd text.
   */
  async getDetailFieldText(label: string): Promise<string> {
    await expect(this.detailSheet).toBeVisible()
    const dt = this.detailSheet.locator('dt').getByText(label, { exact: true })
    await expect(dt).toBeVisible()
    const dd = dt.locator('..').locator('dd')
    return (await dd.textContent()) || ''
  }

  async getDetailJson(): Promise<string> {
    await expect(this.detailJson).toBeVisible()
    return (await this.detailJson.textContent()) || ''
  }

  /**
   * When DataTable has no data, the data-testid div contains the empty message
   * instead of an actual table element.
   */
  async getEmptyMessage(): Promise<string> {
    const tbody = this.table.locator('tbody')
    const hasTbody = await tbody.count()
    if (hasTbody === 0) {
      return (await this.table.textContent()) || ''
    }
    return ''
  }

  async isPaginationVisible(): Promise<boolean> {
    return await this.isVisible(this.pagination)
  }

  async navigateToPage(direction: 'next' | 'previous'): Promise<void> {
    const button = direction === 'next'
      ? this.page.locator(SELECTORS.audit.paginationNext)
      : this.page.locator(SELECTORS.audit.paginationPrevious)
    await this.smartClick(button)
    await expect(this.table).toBeVisible()
  }
}
