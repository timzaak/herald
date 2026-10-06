import { Page, Locator, expect } from '@playwright/test'
import { SELECTORS } from '../selectors'
import { BasePage } from './base-page'
import type { UnifiedLogger } from '../helpers/unified-logger'

/**
 * Page Object for the Credit Bucket directory (admin Master-Detail).
 *
 * Route: /{realmId}/manage/billing/credit-buckets
 * Overview route: /{realmId}/manage/billing/credit-buckets/overview
 *
 * LOUD NOTE — navigation:
 * The sidebar entry testid `sidebar-menu-credit-buckets` is i18n-derived
 * (sidebar.tsx derives it from the localized nav label) and differs per
 * locale. This POM navigates by route, NOT by clicking the sidebar testid.
 * See docs/user-stories/billing/credit-bucket.md (bucket editor loud notes).
 *
 * User stories:
 * - US-CB-001: admin CRUD on the directory
 * - US-CB-002: bind client-app coverage set
 * - US-CB-003: entitlement mappings target a bucket via distribution rules
 *   (surfaced read-only as `ruleReferences` in the bucket editor)
 *
 * This POM wraps only navigation, list selection, editor open, and overview
 * navigation. Editor field interaction, coverage multiselect, and delete
 * confirmation live in `helpers/bucket-helpers.ts` (these are shared
 * imperative flows, not page-state concerns).
 *
 * @see docs/user-stories/billing/credit-bucket.md
 */
export class CreditBucketDirectoryPage extends BasePage {
  /** Directory page container. */
  readonly directoryPage: Locator
  /** "New Bucket" toolbar button. */
  readonly newButton: Locator
  /** Search input above the bucket list. */
  readonly searchInput: Locator
  /** Empty state (no buckets in realm). */
  readonly emptyState: Locator
  /** No-selection state (right pane before any bucket is clicked). */
  readonly noSelection: Locator
  /** Bucket editor pane (create/edit). */
  readonly editor: Locator
  /** Overview (matrix audit) page container. */
  readonly overviewPage: Locator

  constructor(page: Page, logger?: UnifiedLogger) {
    super(page, logger)
    this.directoryPage = page.locator(SELECTORS.creditBucket.directoryPage)
    this.newButton = page.locator(SELECTORS.creditBucket.newButton)
    this.searchInput = page.locator(SELECTORS.creditBucket.searchInput)
    this.emptyState = page.locator(SELECTORS.creditBucket.emptyState)
    this.noSelection = page.locator(SELECTORS.creditBucket.noSelection)
    this.editor = page.locator(SELECTORS.creditBucket.editor)
    this.overviewPage = page.locator(SELECTORS.creditBucket.overviewPage)
  }

  /**
   * Navigate to the Credit Bucket directory for a realm (by route).
   *
   * Avoids the i18n-derived sidebar testid. Waits for the directory container
   * to be visible before returning.
   */
  async gotoDirectory(realmId: string = 'admin'): Promise<void> {
    await super.goto(`/manage/billing/credit-buckets`)
    await this.waitForDirectoryReady()
  }

  /**
   * Navigate to the Credit Bucket overview (matrix audit) page.
   *
   * Route: `/{realmId}/manage/billing/credit-buckets/overview`.
   */
  async gotoOverview(realmId: string = 'admin'): Promise<void> {
    await super.goto(`/manage/billing/credit-buckets/overview`)
    await expect(this.overviewPage).toBeVisible({ timeout: 10000 })
  }

  /**
   * Wait for the directory container to be visible.
   *
   * Either the bucket list, the empty state, or the no-selection pane renders
   * depending on realm state; the container itself is the stable signal.
   */
  async waitForDirectoryReady(timeout: number = 10000): Promise<void> {
    await expect(this.directoryPage).toBeVisible({ timeout })
  }

  /**
   * Select a bucket in the left-column list (loads the editor pane).
   */
  async selectBucket(bucketId: string): Promise<void> {
    const item = this.page.locator(SELECTORS.creditBucket.listItem(bucketId))
    await expect(item).toBeVisible({ timeout: 10000 })
    await this.smartClick(item)
  }

  /**
   * Select a bucket and wait for the editor pane to render.
   */
  async openEditor(bucketId: string): Promise<void> {
    await this.selectBucket(bucketId)
    await expect(this.editor).toBeVisible({ timeout: 10000 })
  }

  /**
   * Click the "New Bucket" toolbar button and wait for the editor pane.
   */
  async clickNewBucket(): Promise<void> {
    await expect(this.newButton).toBeVisible()
    await this.smartClick(this.newButton)
    await expect(this.editor).toBeVisible({ timeout: 5000 })
  }

  /**
   * Filter the bucket list via the search input.
   */
  async search(query: string): Promise<void> {
    await expect(this.searchInput).toBeVisible()
    await this.searchInput.fill(query)
    // The list filters client-side on input; no explicit apply step.
  }

  /**
   * Check whether the realm-empty state is rendered (no buckets at all).
   */
  async isEmpty(): Promise<boolean> {
    return this.isVisible(this.emptyState)
  }

  /**
   * Check whether the no-selection pane is rendered (buckets exist, none clicked).
   */
  async isNoSelection(): Promise<boolean> {
    return this.isVisible(this.noSelection)
  }

  /**
   * Convenience locator for a single bucket list item (for caller assertions).
   */
  listItem(bucketId: string): Locator {
    return this.page.locator(SELECTORS.creditBucket.listItem(bucketId))
  }

  /**
   * Convenience locator for the disabled badge on a list item.
   */
  disabledBadge(bucketId: string): Locator {
    return this.page.locator(
      SELECTORS.creditBucket.listItemDisabledBadge(bucketId),
    )
  }
}
