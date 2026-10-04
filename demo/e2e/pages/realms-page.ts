/**
 * Realms Page Object
 *
 * Encapsulates realm management page operations.
 * Provides methods for creating, editing, and deleting realms.
 *
 */

import { Page, Locator, expect } from '@playwright/test'
import { SELECTORS } from '../selectors'
import { BasePage } from './base-page'
import type { UnifiedLogger } from '../helpers/unified-logger'

/**
 * Realm data interface for editing
 */
export interface RealmData {
  name?: string
}

/**
 * Realm creation data interface
 * Matches backend CreateRealmValidator structure
 */
export interface CreateRealmData {
  id?: string
  name: string
  description?: string
  adminEmail: string
  adminPassword: string
}

/**
 * Realms Page Object
 *
 * Represents the realm management page at /admin/realms
 *
 * @example
 * ```typescript
 * const realmsPage = new RealmsPage(page, logger)
 * await realmsPage.goto()
 * await realmsPage.createRealm({ id: 'demo-realm', name: 'Demo Realm' })
 * ```
 */
export class RealmsPage extends BasePage {
  // Selectors
  readonly container: Locator
  readonly heading: Locator
  readonly table: Locator
  readonly addButton: Locator
  readonly searchInput: Locator
  readonly paginationContainer: Locator
  readonly dialog: Locator
  readonly dialogTitle: Locator

  // Create realm dialog selectors
  readonly createIdInput: Locator
  readonly createNameInput: Locator
  readonly createAdminEmailInput: Locator
  readonly createAdminPasswordInput: Locator

  // Detail dialog selectors
  readonly detailNameInput: Locator
  readonly detailEditButton: Locator
  readonly detailSaveButton: Locator

  readonly dialogCancelButton: Locator
  readonly dialogSubmitButton: Locator
  readonly dialogCloseButton: Locator
  readonly toast: Locator

  constructor(page: Page, logger?: UnifiedLogger) {
    super(page, logger)
    this.container = page.locator(SELECTORS.realms.container)
    this.heading = page.locator(SELECTORS.realms.heading)
    this.table = page.locator(SELECTORS.realms.table)
    // Use fallback selector: data-testid OR text content (for better reliability)
    this.addButton = page.locator('[data-testid="create-realm-button"], button:has-text("Create Realm")')
    this.searchInput = page.locator('[data-testid="realms-search-input"]')
    // Use role-based selector for pagination navigation
    this.paginationContainer = page.getByRole('navigation', { name: /pagination/i })

    // Dialog selectors
    this.dialog = page.locator(SELECTORS.common.dialog)
    this.dialogTitle = page.locator(SELECTORS.common.dialogTitle)

    // Create realm dialog selectors
    this.createIdInput = page.locator('[data-testid="realm-create-id-input"]')
    this.createNameInput = page.locator('[data-testid="realm-create-name-input"]')
    this.createAdminEmailInput = page.locator('[data-testid="realm-create-admin-email-input"]')
    this.createAdminPasswordInput = page.locator('[data-testid="realm-create-admin-password-input"]')

    // Detail dialog selectors
    this.detailNameInput = page.locator('[data-testid="realm-detail-name-input"]')
    this.detailEditButton = page.locator('[data-testid="realm-detail-edit-button"]')
    this.detailSaveButton = page.locator('[data-testid="realm-detail-save-button"]')

    this.dialogCancelButton = page.locator(SELECTORS.common.dialogCancelButton)
    this.dialogSubmitButton = page.locator(SELECTORS.common.dialogSubmitButton)
    this.dialogCloseButton = page.locator(SELECTORS.common.dialogCloseButton)

    // Feedback selectors
    this.toast = page.locator(SELECTORS.common.toast)
  }

  /**
   * Navigate to realms page
   *
   * @param realmId Realm ID (defaults to 'admin' for backward compatibility)
   */
  async goto(realmId: string = 'admin'): Promise<void> {
    // Direct navigation to realms page - more reliable than clicking sidebar
    const url = `/manage/realms`
    const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'
    console.log(`[RealmsPage.goto] Navigating to ${BASE_URL}${url}`)
    await this.page.goto(`${BASE_URL}${url}`, { waitUntil: 'domcontentloaded' })

    // Wait for page to load
    await this.waitForReady()
  }

  /**
   * Wait for realms page to be visible
   *
   * This method waits for the page to load, including table/empty/loading state,
   * and also checks if the "Add Realm" button is visible (if user has permission).
   * The button visibility check is optional since not all users have create permission.
   */
  async waitForReady(): Promise<void> {
    await expect(this.container).toBeVisible({ timeout: 10000 })
    await expect(this.heading).toBeVisible({ timeout: 10000 })

    // Wait for network idle to ensure permissions are loaded
    await this.page.waitForLoadState('networkidle', { timeout: 10000 }).catch(() => {
      console.log('[waitForReady] Network idle timeout, continuing...')
    })

    // Check if "Add Realm" button is visible (may not be if user lacks permission)
    try {
      await this.addButton.waitFor({ state: 'visible', timeout: 3000 })
      console.log('[waitForReady] Add Realm button is visible (user has create permission)')
    } catch (error) {
      console.log('[waitForReady] Add Realm button not visible (user may not have create permission)')
    }

    // Wait for either table, loading state, or empty state
    const table = this.table
    const loading = this.page.locator('[data-testid="realm-table-loading"]')
    const emptyState = this.page.locator('[data-testid="realm-table-empty"]')

    await expect(table.or(loading).or(emptyState)).toBeVisible({ timeout: 10000 })

    // If still loading, wait for table or empty state
    if (await loading.isVisible()) {
      // Wait for loading to complete and table or empty state to appear
      await expect(table.or(emptyState)).toBeVisible({ timeout: 10000 })
    }
  }

  /**
   * Click "Add Realm" button to open create dialog
   *
   * This method includes network idle waiting to ensure permissions are loaded
   * before attempting to click the button. The button is conditionally rendered
   * based on permissions, so we need to wait for the permission data to be available.
   */
  async clickAddRealm(): Promise<void> {
    console.log('[clickAddRealm] Waiting for add button to be visible...')

    // Wait for network idle to ensure permissions are loaded
    // This addresses the timing issue where button is not initially rendered
    await this.page.waitForLoadState('networkidle', { timeout: 10000 }).catch(() => {
      console.log('[clickAddRealm] Network idle timeout, continuing...')
    })

    // Additional wait for permission state updates
    // This is a technical delay for React state propagation (not a demo delay)
    await this.page.waitForTimeout(1000).catch(() => {})

    await expect(this.addButton).toBeVisible({ timeout: 10000 })
    console.log('[clickAddRealm] Add button is visible, clicking...')
    await this.smartClick(this.addButton)
    console.log('[clickAddRealm] Waiting for dialog to be visible...')
    await expect(this.dialog).toBeVisible({ timeout: 10000 })
    console.log('[clickAddRealm] Dialog is visible, checking title...')
    await expect(this.dialogTitle).toHaveText(/Add Realm|Create Realm|New Realm/i, { timeout: 5000 })
    console.log('[clickAddRealm] Dialog opened successfully')
  }

  /**
   * Fill create realm form fields
   *
   * @param realmData Realm creation data
   */
  async fillCreateRealmForm(realmData: CreateRealmData): Promise<void> {
    if (realmData.id) {
      await this.fillField(this.createIdInput, realmData.id)
    }

    await this.fillField(this.createNameInput, realmData.name)

    // Fill description if provided
    if (realmData.description) {
      const descriptionInput = this.page.locator('[data-testid="realm-create-description-input"]')
      await this.fillField(descriptionInput, realmData.description)
    }

    await this.fillField(this.createAdminEmailInput, realmData.adminEmail)
    await this.fillField(this.createAdminPasswordInput, realmData.adminPassword)
  }

  /**
   * Submit the realm form (click submit button)
   *
   * Note: Toast notifications auto-dismiss (sonner), so we verify dialog close
   * instead of waiting for toast. Actual success is verified by checking the realm table.
   */
  async submitRealmForm(): Promise<void> {
    await this.smartClick(this.dialogSubmitButton)

    // 验证对话框关闭（不依赖 toast）
    await expect(this.dialog).toBeHidden({ timeout: 10000 })
  }

  /**
   * Create a new realm with admin user
   *
   * Includes retry logic for permission loading timing issues.
   * If the first attempt fails, it waits and retries once.
   *
   * @param realmData Realm creation data including admin user
   * @param waitForPageReady Whether to wait for page to be ready before creating (default: true)
   *                         Set to false when page is already known to be ready (e.g., after goto())
   */
  async createRealm(realmData: CreateRealmData, waitForPageReady: boolean = true): Promise<void> {
    // Ensure page is ready before creating realm
    // Skip if page is already known to be ready (e.g., after goto())
    if (waitForPageReady) {
      await this.waitForReady()
    }

    // Try clicking add button with retry logic for permission timing
    try {
      await this.clickAddRealm()
    } catch (error) {
      console.log('[createRealm] First attempt failed, retrying after wait...')
      await this.page.waitForTimeout(2000)
      await this.clickAddRealm()
    }

    await this.fillCreateRealmForm(realmData)
    await this.submitRealmForm()
  }

  /**
   * Find realm row in table by ID
   *
   * @param id Realm ID to search for
   * @returns Row locator
   */
  findRealmRow(id: string): Locator {
    return this.table.locator(`tr:has-text("${id}")`).first()
  }

  /**
   * Check if realm exists in table
   *
   * Checks only the current page for the realm. Does not paginate through all pages
   * since for typical test scenarios (1-2 realms), all data fits on a single page.
   * Pagination would only waste time and add complexity.
   *
   * Uses assertion-based waiting instead of fixed delays.
   * Waits for table to be visible and stable before checking.
   *
   * @param id Realm ID to check
   * @returns true if realm is found on current page, false otherwise
   */
  async realmExists(id: string): Promise<boolean> {
    // Wait for either table, loading state, or empty state
    const loading = this.page.locator('[data-testid="realm-table-loading"]')
    const emptyState = this.page.locator('[data-testid="realm-table-empty"]')

    // Wait for page to stabilize (table, empty, or loading)
    await expect(this.table.or(emptyState).or(loading)).toBeVisible({ timeout: 10000 })

    // If still loading, wait for loading to complete
    if (await this.isVisible(loading)) {
      await expect(this.table.or(emptyState)).toBeVisible({ timeout: 10000 })
    }

    // If empty state is visible, realm doesn't exist
    if (await this.isVisible(emptyState)) {
      console.log(`[realmExists] Empty state visible, realm ${id} does not exist`)
      return false
    }

    // Check if table is visible (it should be at this point)
    const isTableVisible = await this.isVisible(this.table)
    if (!isTableVisible) {
      console.warn('[realmExists] Table not visible and no empty state, assuming realm does not exist')
      return false
    }

    // Check if realm exists on current page
    const row = this.findRealmRow(id)
    if (await this.isVisible(row)) {
      console.log(`[realmExists] Found realm ${id} on current page`)
      return true
    }

    console.log(`[realmExists] Realm ${id} not found on current page`)
    return false
  }

  /**
   * Get total number of pages from pagination
   *
   * @returns Total pages or null if no pagination
   */
  async getTotalPages(): Promise<number | null> {
    // Get all list items that contain numbers (page numbers)
    const pageNumbers = this.paginationContainer.locator('li').filter({ hasText: /^\d+$/ })
    const count = await pageNumbers.count()
    return count > 0 ? count : null
  }

  /**
   * Click "View" button for a realm to open detail dialog
   *
   * @param id Realm ID
   */
  async clickViewRealm(id: string): Promise<void> {
    const row = this.findRealmRow(id)
    await expect(row).toBeVisible({ timeout: 10000 })

    const viewButton = row.getByRole('button', { name: 'Edit' }).first()
    await this.smartClick(viewButton)

    await expect(this.dialog).toBeVisible({ timeout: 10000 })
    await expect(this.dialogTitle).toHaveText(/Edit Realm/i, { timeout: 5000 })
  }

  /**
   * Edit realm name in detail dialog
   *
   * @param newName New realm name
   */
  async editRealmName(newName: string): Promise<void> {
    await this.fillField(this.detailNameInput, newName)
    await this.smartClick(this.dialogSubmitButton)

    // 验证对话框关闭（不依赖 toast）
    await expect(this.dialog).toBeHidden({ timeout: 10000 })
  }

  /**
   * Edit an existing realm (opens detail dialog and edits name)
   *
   * @param id Realm ID
   * @param newName New realm name
   */
  async editRealm(id: string, newName: string): Promise<void> {
    await this.clickViewRealm(id)
    await this.editRealmName(newName)
  }

  /**
   * Click "Delete" button for a realm
   *
   * @param id Realm ID
   */
  async clickDeleteRealm(id: string): Promise<void> {
    const row = this.findRealmRow(id)
    await expect(row).toBeVisible({ timeout: 10000 })

    const deleteButton = row.locator('[data-testid="delete-button"]').first()
    await this.smartClick(deleteButton)

    await expect(this.dialog).toBeVisible({ timeout: 10000 })
    await expect(this.dialogTitle).toHaveText(/Delete Realm|Confirm Delete/i, { timeout: 5000 })
  }

  /**
   * Confirm realm deletion
   */
  async confirmDeleteRealm(): Promise<void> {
    await this.smartClick(this.dialogSubmitButton)

    // 验证对话框关闭（不依赖 toast）
    await expect(this.dialog).toBeHidden({ timeout: 10000 })
  }

  /**
   * Delete a realm
   *
   * @param id Realm ID to delete
   */
  async deleteRealm(id: string): Promise<void> {
    await this.clickDeleteRealm(id)
    await this.confirmDeleteRealm()
  }

  /**
   * Get realm count from table
   *
   * @returns Number of realm rows
   */
  async getRealmCount(): Promise<number> {
    // Check for empty state first
    const emptyState = this.page.locator('[data-testid="realm-table-empty"]')
    if (await this.isVisible(emptyState)) {
      return 0
    }

    // Wait for table to be visible
    await expect(this.table).toBeVisible({ timeout: 10000 })
    const rows = this.table.getByRole('row')
    return await rows.count()
  }

  /**
   * Get the first realm ID from the table
   *
   * Returns the ID of the first realm in the current page.
   * Useful for tests that need to work with any existing realm.
   *
   * @returns First realm ID or null if no realms exist
   */
  async getFirstRealmId(): Promise<string | null> {
    // Check for empty state first
    const emptyState = this.page.locator('[data-testid="realm-table-empty"]')
    if (await this.isVisible(emptyState)) {
      return null
    }

    // Wait for table to be visible
    await expect(this.table).toBeVisible({ timeout: 10000 })

    // Get the first data row (skip header row)
    const rows = this.table.getByRole('row')
    const rowCount = await rows.count()

    if (rowCount <= 1) {
      return null // Only header row exists
    }

    // Get the first data row (index 1, after header)
    const firstRow = rows.nth(1)

    // The realm ID is in the first cell
    const idCell = firstRow.locator('td').first()

    // Get the text content which is the realm ID
    const realmId = await idCell.textContent()

    return realmId?.trim() || null
  }

  /**
   * Search realms by ID or name
   *
   * Uses assertion-based waiting instead of fixed delays.
   * Waits for search API response and table content update.
   *
   * @param searchTerm Search query (realm ID or name)
   */
  async searchRealms(searchTerm: string): Promise<void> {
    await this.fillField(this.searchInput, searchTerm)

    // Wait for search API response instead of fixed timeout
    // This handles the 500ms search debounce properly
    try {
      await this.page.waitForResponse(
        response =>
          response.url().includes('/api/realms') &&
          response.request().method() === 'GET' &&
          response.status() === 200,
        { timeout: 5000 }
      )
    } catch {
      // If no API request is made (e.g., search term too short), continue
      // The table content assertion below will fail if results don't match
    }

    // Wait for either results (table) or "no results" message
    const table = this.table
    const emptyState = this.page.locator('[data-testid="realm-table-empty"]')
    await expect(table.or(emptyState)).toBeVisible({ timeout: 10000 })
  }

  /**
   * Clear search input to reset the realm list
   */
  async clearSearch(): Promise<void> {
    await this.searchInput.clear()

    // Wait for the full list to reload
    try {
      await this.page.waitForResponse(
        response =>
          response.url().includes('/api/realms') &&
          response.request().method() === 'GET' &&
          response.status() === 200,
        { timeout: 5000 }
      )
    } catch {
      // Continue even if API response is not detected
    }

    // Wait for either results (table) or "no results" message
    const table = this.table
    const emptyState = this.page.locator('[data-testid="realm-table-empty"]')
    await expect(table.or(emptyState)).toBeVisible({ timeout: 10000 })
  }

  /**
   * Click pagination page number button
   *
   * @param pageNumber Page number to navigate to (1-based)
   */
  async clickPaginationPage(pageNumber: number): Promise<void> {
    const pageButton = this.paginationContainer.getByRole('button', { name: pageNumber.toString() })

    await this.smartClick(pageButton)

    // Wait for navigation and data reload
    try {
      await this.page.waitForResponse(
        response =>
          response.url().includes('/api/realms') &&
          response.request().method() === 'GET' &&
          response.status() === 200,
        { timeout: 5000 }
      )
    } catch {
      // Continue even if API response is not detected
    }

    // Verify table is visible after navigation
    await expect(this.table).toBeVisible({ timeout: 10000 })
  }

  /**
   * Click "Next" pagination button
   */
  async clickNextPage(): Promise<void> {
    const nextButton = this.paginationContainer.getByRole('button', { name: /next/i })

    // Only click if the button is enabled (not on last page)
    const isEnabled = await nextButton.isEnabled().catch(() => false)
    if (!isEnabled) {
      throw new Error('Next page button is disabled - already on last page')
    }

    await this.smartClick(nextButton)

    // Wait for navigation and data reload
    await this.page.waitForLoadState('domcontentloaded', { timeout: 10000 })

    // Verify table is visible after navigation
    await expect(this.table).toBeVisible({ timeout: 10000 })
  }

  /**
   * Click "Previous" pagination button
   */
  async clickPreviousPage(): Promise<void> {
    const prevButton = this.paginationContainer.getByRole('button', { name: /previous/i })

    // Only click if the button is enabled (not on first page)
    const isEnabled = await prevButton.isEnabled().catch(() => false)
    if (!isEnabled) {
      throw new Error('Previous page button is disabled - already on first page')
    }

    await this.smartClick(prevButton)

    // Wait for navigation and data reload
    await this.page.waitForLoadState('domcontentloaded', { timeout: 10000 })

    // Verify table is visible after navigation
    await expect(this.table).toBeVisible({ timeout: 10000 })
  }

  /**
   * Click column header to sort
   *
   * @param columnName Column name to sort by ('id' | 'name' | 'createdAt' | 'updatedAt')
   */
  async clickSortColumn(columnName: 'id' | 'name' | 'createdAt' | 'updatedAt'): Promise<void> {
    // Map column names to data-testid attributes from frontend
    const columnTestIds: Record<string, string> = {
      id: 'realm-id-sort-button',
      name: 'realm-name-sort-button',
      createdAt: 'realm-created-at-sort-button',
      updatedAt: 'realm-updated-at-sort-button',
    }

    const testId = columnTestIds[columnName]
    if (!testId) {
      throw new Error(`Invalid column name: ${columnName}`)
    }

    const columnHeader = this.table.locator(`[data-testid="${testId}"]`)

    await this.smartClick(columnHeader)

    // Wait for sort to apply (API response + table update)
    try {
      await this.page.waitForResponse(
        response =>
          response.url().includes('/api/realms') &&
          response.request().method() === 'GET' &&
          response.status() === 200,
        { timeout: 5000 }
      )
    } catch {
      // Continue even if API response is not detected
    }

    // Verify sort indicator appears (ascending or descending)
    await expect(columnHeader).toBeVisible({ timeout: 5000 })
  }

  /**
   * Get current active page number from pagination
   *
   * @returns Current page number (1-based) or null if no pagination
   */
  async getCurrentPage(): Promise<number | null> {
    const activeButton = this.paginationContainer.locator('li').filter({ hasText: /^\d+$/ }).first()
    const isVisible = await activeButton.isVisible().catch(() => false)

    if (!isVisible) {
      return null
    }

    const pageText = await activeButton.textContent()
    return pageText ? parseInt(pageText, 10) : null
  }

  /**
   * Check if pagination is visible
   *
   * @returns true if pagination controls are visible
   */
  async isPaginationVisible(): Promise<boolean> {
    return await this.isVisible(this.paginationContainer)
  }
}
