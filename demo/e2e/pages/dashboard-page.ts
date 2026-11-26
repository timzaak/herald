/**
 * Dashboard Page Object
 *
 * Encapsulates dashboard page operations including stats cards,
 * auth trend chart, quick navigation, and error state handling.
 *
 */

import { Page, Locator, expect } from '@playwright/test'
import { SELECTORS } from '../selectors'
import { BasePage } from './base-page'
import type { UnifiedLogger } from 'playwright-unified-logger'

export class DashboardPage extends BasePage {
  // Page-level locators
  readonly heading: Locator
  readonly statsRow: Locator
  readonly totalUsersCard: Locator
  readonly newUsersCard: Locator
  readonly activeUsersCard: Locator
  readonly authTrendChart: Locator
  readonly recentUsersCard: Locator
  readonly recentAuditCard: Locator
  readonly recentPointsCard: Locator
  readonly errorState: Locator
  readonly retryButton: Locator
  readonly chartSkeleton: Locator

  constructor(page: Page, logger?: UnifiedLogger) {
    super(page, logger)
    this.heading = page.locator(SELECTORS.dashboard.heading)
    this.statsRow = page.locator(SELECTORS.dashboard.statsRow)
    this.totalUsersCard = page.locator(SELECTORS.dashboard.totalUsersCard)
    this.newUsersCard = page.locator(SELECTORS.dashboard.newUsersCard)
    this.activeUsersCard = page.locator(SELECTORS.dashboard.activeUsersCard)
    this.authTrendChart = page.locator(SELECTORS.dashboard.authTrendChart)
    this.recentUsersCard = page.locator(SELECTORS.dashboard.recentUsersCard)
    this.recentAuditCard = page.locator(SELECTORS.dashboard.recentAuditCard)
    this.recentPointsCard = page.locator(SELECTORS.dashboard.recentPointsCard)
    this.errorState = page.locator(SELECTORS.dashboard.errorState)
    this.retryButton = page.locator(SELECTORS.dashboard.retryButton)
    this.chartSkeleton = page.locator(SELECTORS.dashboard.chartSkeleton)
  }

  async goto(): Promise<void> {
    const dashboardMenuLink = this.page.locator(SELECTORS.sidebar.menuDashboard)
    await this.smartClick(dashboardMenuLink)
    await this.waitForReady()
  }

  async waitForReady(): Promise<void> {
    await expect(this.heading).toBeVisible()
  }

  async waitForLoad(): Promise<void> {
    await expect(
      this.statsRow.or(this.errorState)
    ).toBeVisible()
  }

  /**
   * Extract numeric value from a stats card by its data-testid.
   * Finds the card, reads its text content, and parses the first number found.
   */
  async getStatsValue(cardTestId: string): Promise<number> {
    const card = this.page.locator(`[data-testid="${cardTestId}"]`)
    await expect(card).toBeVisible()
    const text = await card.textContent() || ''
    const match = text.match(/[\d,]+/)
    if (!match) {
      throw new Error(`No numeric value found in card "${cardTestId}". Text: "${text}"`)
    }
    return parseInt(match[0].replace(/,/g, ''), 10)
  }

  async isChartVisible(): Promise<boolean> {
    return await this.isVisible(this.authTrendChart)
  }

  async isChartEmpty(): Promise<boolean> {
    const visible = await this.isVisible(this.authTrendChart)
    if (!visible) return false
    const text = await this.authTrendChart.textContent() || ''
    return text.includes('No data')
  }

  /**
   * Follow the "view all" link of a recent-activity card into its management
   * page (the 6-card QuickNav this page used to render was replaced by these
   * cards; each one is hidden when the admin lacks its permission).
   */
  async clickRecentCardViewAll(card: Locator): Promise<void> {
    await this.smartClick(card.getByRole('link'))
  }

  async clickTotalUsersCard(): Promise<void> {
    await this.smartClick(this.totalUsersCard)
  }

  async isErrorState(): Promise<boolean> {
    return await this.isVisible(this.errorState)
  }

  async clickRetry(): Promise<void> {
    await this.smartClick(this.retryButton)
  }

  async isStatsRowVisible(): Promise<boolean> {
    const statsVisible = await this.isVisible(this.statsRow)
    if (!statsVisible) return false
    const skeletonHidden = await this.chartSkeleton.isHidden().catch(() => true)
    return skeletonHidden
  }
}
