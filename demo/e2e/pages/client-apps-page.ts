/**
 * Client Apps Page Object
 *
 * Encapsulates Client App management page operations.
 * Updated to support the new Page-based + Tabs UI (replaces dialog-based flow).
 *
 * User Stories:
 * - US-TP-005: Client App Configuration Management
 * - US-TP-008: Configure Client App Redirect URI Whitelist
 * - US-TP-009: Manage Client App Icon
 * - US-TP-010: Enable/Disable Client App
 * - US-TP-011: Configure Session TTL Policy
 *
 */

import { Page, Locator, expect } from '@playwright/test'
import { SELECTORS } from '../selectors'
import { BasePage } from './base-page'
import type { UnifiedLogger } from '../helpers/unified-logger'

/**
 * Client App data interface
 */
export interface ClientAppData {
  id?: string
  clientId?: string
  name: string
  description?: string
  redirectUris?: string[]
  enabled?: boolean
  sessionTtl?: number
  renewalTtl?: number | null
  iconUrl?: string
  deviceCodeGrantEnabled?: boolean
}

/**
 * Tab names in the Client App form page
 */
export type DialogTabName = 'basic' | 'redirect-uris' | 'security' | 'appearance'

/**
 * Client Apps Page Object
 *
 * Represents Client App management page at /{realmId}/manage/client-apps
 * with a dedicated Page + Tabs form for creating and editing client apps.
 *
 * @example
 * ```typescript
 * const clientAppsPage = new ClientAppsPage(page, logger)
 * await clientAppsPage.goto()
 * await clientAppsPage.createClientApp({
 *   clientId: 'my-app',
 *   name: 'My Application',
 *   redirectUris: ['https://example.com/callback']
 * })
 * ```
 */
export class ClientAppsPage extends BasePage {
  // Page container
  readonly container: Locator
  readonly heading: Locator
  readonly table: Locator
  readonly addButton: Locator

  // Form page selectors
  readonly formPage: Locator
  readonly pageTitle: Locator

  // Tab triggers
  readonly tabBasic: Locator
  readonly tabRedirectUris: Locator
  readonly tabSecurity: Locator
  readonly tabAppearance: Locator

  // Basic tab fields
  readonly clientIdInput: Locator
  readonly clientIdDisplay: Locator
  readonly nameInput: Locator
  readonly descriptionInput: Locator

  // Redirect URIs tab
  readonly redirectUrisInput: Locator

  // Security tab fields
  readonly enabledSwitch: Locator
  readonly sessionTtlInput: Locator
  readonly sessionRenewalTtlInput: Locator
  readonly deviceCodeGrantSwitch: Locator
  readonly regenerateSecretSwitch: Locator

  // Appearance tab fields
  readonly iconUrlInput: Locator

  // Footer buttons
  readonly cancelButton: Locator
  readonly submitButton: Locator

  // Delete confirmation dialog
  readonly deleteConfirmDialog: Locator
  readonly deleteConfirmButton: Locator

  // Success message (toast notification)
  readonly successMessage: Locator

  // Current realm ID (stored for navigation)
  private currentRealmId: string = 'admin'

  constructor(page: Page, logger?: UnifiedLogger) {
    super(page, logger)

    // Page selectors
    this.container = this.page.locator(SELECTORS.clientApps.page)
    this.heading = this.page.locator(SELECTORS.clientApps.heading)
    this.table = this.page.locator(SELECTORS.clientApps.table)
    this.addButton = this.page.locator(SELECTORS.clientApps.addButton)

    // Form page selectors
    this.formPage = this.page.locator(SELECTORS.clientAppForm.page)
    this.pageTitle = this.page.locator(SELECTORS.clientAppForm.pageTitle)

    // Tab triggers
    this.tabBasic = this.page.locator(SELECTORS.clientAppForm.tabBasic)
    this.tabRedirectUris = this.page.locator(SELECTORS.clientAppForm.tabRedirectUris)
    this.tabSecurity = this.page.locator(SELECTORS.clientAppForm.tabSecurity)
    this.tabAppearance = this.page.locator(SELECTORS.clientAppForm.tabAppearance)

    // Basic tab fields
    this.clientIdInput = this.page.locator(SELECTORS.clientAppForm.clientIdInput)
    this.clientIdDisplay = this.page.locator(SELECTORS.clientAppForm.clientIdDisplay)
    this.nameInput = this.page.locator(SELECTORS.clientAppForm.nameInput)
    this.descriptionInput = this.page.locator(SELECTORS.clientAppForm.descriptionInput)

    // Redirect URIs tab
    this.redirectUrisInput = this.page.locator(SELECTORS.clientAppForm.redirectUrisInput)

    // Security tab fields
    this.enabledSwitch = this.page.locator(SELECTORS.clientAppForm.enabledSwitch)
    this.sessionTtlInput = this.page.locator(SELECTORS.clientAppForm.sessionTtlInput)
    this.sessionRenewalTtlInput = this.page.locator(SELECTORS.clientAppForm.sessionRenewalTtlInput)
    this.deviceCodeGrantSwitch = this.page.locator(SELECTORS.clientAppForm.deviceCodeGrantSwitch)
    this.regenerateSecretSwitch = this.page.locator(SELECTORS.clientAppForm.regenerateSecretSwitch)

    // Appearance tab fields
    this.iconUrlInput = this.page.locator(SELECTORS.clientAppForm.iconUrlInput)

    // Footer buttons
    this.cancelButton = this.page.locator(SELECTORS.clientAppForm.cancelButton)
    this.submitButton = this.page.locator(SELECTORS.clientAppForm.submitButton)

    // Delete confirmation
    this.deleteConfirmDialog = this.page.locator('[data-testid="delete-confirmation-dialog"]')
    this.deleteConfirmButton = this.page.locator('[data-testid="confirm-delete-button"]')

    // Success message (toast)
    this.successMessage = this.page.locator('[data-sonner-toast][data-type="success"]')
  }

  // ============================================================================
  // Page Navigation
  // ============================================================================

  /**
   * Navigate to Client Apps page
   *
   * @param realmId Realm ID (defaults to 'admin')
   */
  async goto(realmId: string = 'admin'): Promise<void> {
    this.currentRealmId = realmId
    const url = `/manage/client-apps`
    await this.page.goto(url)
    await this.waitForReady()
  }

  /**
   * Wait for Client Apps page to be visible
   */
  async waitForReady(): Promise<void> {
    await expect(this.container).toBeVisible()
    await expect(this.heading).toBeVisible()
    await expect(this.table).toBeVisible()
  }

  // ============================================================================
  // Form Page Navigation Methods
  // ============================================================================

  /**
   * Navigate to the Create Client App page
   *
   * Navigates to /{realmId}/manage/client-apps/new and waits for the form page.
   */
  async gotoCreatePage(): Promise<void> {
    const url = `/${this.currentRealmId}/manage/client-apps/new`
    await this.page.goto(url)
    await this.waitForFormPage()
    this.logger?.testCode.log('Create form page opened')
  }

  /**
   * Navigate to the Edit Client App page by finding the row by name
   *
   * Finds the app in the table to get its UUID, then navigates to the edit page.
   *
   * @param appName Name of the client app to edit
   */
  async gotoEditPage(appName: string): Promise<void> {
    // Ensure we are on the list page to find the app
    if (!await this.container.isVisible().catch(() => false)) {
      await this.goto(this.currentRealmId)
    }

    // Find the app ID by name
    const appId = await this.getClientIdByName(appName)
    if (!appId) {
      throw new Error(`Client App "${appName}" not found in table`)
    }

    await this.gotoEditPageById(appId)
    this.logger?.testCode.log(`Edit form page opened for "${appName}"`)
  }

  /**
   * Navigate to the Edit Client App page by app UUID
   *
   * @param appId Client App UUID
   */
  async gotoEditPageById(appId: string): Promise<void> {
    const url = `/${this.currentRealmId}/manage/client-apps/${appId}/edit`
    await this.page.goto(url)
    await this.waitForFormPage()
    this.logger?.testCode.log(`Edit form page opened for app ID "${appId}"`)
  }

  /**
   * Wait for the form page to be visible
   */
  async waitForFormPage(): Promise<void> {
    await expect(this.formPage).toBeVisible()
    await expect(this.pageTitle).toBeVisible()
  }

  /**
   * Check if the form page is currently visible
   */
  async isFormPageOpen(): Promise<boolean> {
    return await this.formPage.isVisible().catch(() => false)
  }

  /**
   * Switch to a specific tab in the form page
   *
   * @param tabName Tab to switch to
   */
  private getTabLocator(tabName: DialogTabName): Locator {
    const tabMap: Record<DialogTabName, Locator> = {
      'basic': this.tabBasic,
      'redirect-uris': this.tabRedirectUris,
      'security': this.tabSecurity,
      'appearance': this.tabAppearance,
    }
    return tabMap[tabName]
  }

  async switchTab(tabName: DialogTabName): Promise<void> {
    const tab = this.getTabLocator(tabName)
    await expect(tab).toBeVisible()
    await tab.click()
    this.logger?.testCode.log(`Switched to tab: ${tabName}`)
  }

  /**
   * Fill the Basic tab fields
   *
   * @param data Basic tab data
   */
  async fillBasicTab(data: {
    clientId?: string
    name: string
    description?: string
  }): Promise<void> {
    // Client ID (create mode only)
    if (data.clientId) {
      await expect(this.clientIdInput).toBeVisible()
      await this.clientIdInput.fill(data.clientId)
      this.logger?.testCode.log(`Filled Client ID: "${data.clientId}"`)
    }

    // Name (required)
    await expect(this.nameInput).toBeVisible()
    await this.nameInput.fill(data.name)
    this.logger?.testCode.log(`Filled Name: "${data.name}"`)

    // Description (optional)
    if (data.description) {
      await this.descriptionInput.fill(data.description)
      this.logger?.testCode.log(`Filled Description: "${data.description}"`)
    }
  }

  /**
   * Fill the Redirect URIs tab
   *
   * @param uris Array of redirect URIs to add
   */
  async fillRedirectUrisTab(uris: string[]): Promise<void> {
    // Switch to Redirect URIs tab first
    await this.switchTab('redirect-uris')

    const input = this.redirectUrisInput

    for (const uri of uris) {
      // Click the input to focus it
      await input.click()

      // Fill the URI
      await input.fill(uri)

      // Press Enter to add the URI
      await this.page.keyboard.press('Enter')

      // Wait for the URI to be added to the list
      await expect(
        this.page.locator('[data-testid^="uri-item-"]').filter({ hasText: uri })
      ).toBeVisible({ timeout: 5000 })

      this.logger?.testCode.log(`Added Redirect URI: "${uri}"`)
    }
  }

  private async toggleSwitch(locator: Locator, desiredState: boolean, label: string): Promise<void> {
    const isChecked = await locator.getAttribute('aria-checked')
    const currentlyEnabled = isChecked === 'true'
    if (currentlyEnabled !== desiredState) {
      await locator.click()
      this.logger?.testCode.log(`Toggled ${label}: ${desiredState}`)
    }
  }

  async fillSecurityTab(data: {
    enabled?: boolean
    sessionTtlSeconds?: number
    sessionRenewalTtlSeconds?: number | null
    deviceCodeGrantEnabled?: boolean
  }): Promise<void> {
    await this.switchTab('security')

    if (data.enabled !== undefined) {
      await this.toggleSwitch(this.enabledSwitch, data.enabled, 'Enabled')
    }

    // Session TTL - use preset button if available, otherwise type value.
    // NOTE: this field is the frontend's `browserRefreshAbsoluteTtlSeconds`
    // (input testid `browser-refresh-ttl-input`). Presets mirror the frontend
    // BROWSER_REFRESH_TTL_PRESETS: 1d/7d/14d/30d/60d/90d. Values must satisfy
    // min 86400 / max 7776000.
    if (data.sessionTtlSeconds !== undefined) {
      const presetMap: Record<number, string> = {
        86400: '1d',
        604800: '7d',
        1209600: '14d',
        2592000: '30d',
        5184000: '60d',
        7776000: '90d',
      }
      const presetLabel = presetMap[data.sessionTtlSeconds]
      if (presetLabel) {
        const presetButton = this.page.locator(
          SELECTORS.clientAppForm.sessionTtlPreset(presetLabel)
        )
        await presetButton.click()
        this.logger?.testCode.log(`Selected Session TTL preset: ${presetLabel}`)
      } else {
        await this.sessionTtlInput.fill(data.sessionTtlSeconds.toString())
        this.logger?.testCode.log(`Filled custom Session TTL: ${data.sessionTtlSeconds}s`)
      }
    }

    // Session Renewal TTL — field was removed in the browser bearer-token model
    // (no `sessionRenewalTtlSeconds` input exists on the security tab anymore).
    // Keep the parameter for call-site compatibility but no-op; backend rejects
    // the field anyway (not in CreateClientAppRequest).
    if (data.sessionRenewalTtlSeconds !== undefined) {
      this.logger?.testCode.log(
        `Session Renewal TTL ignored (field removed from UI): ${data.sessionRenewalTtlSeconds}`,
      )
    }

    if (data.deviceCodeGrantEnabled !== undefined) {
      await this.toggleSwitch(this.deviceCodeGrantSwitch, data.deviceCodeGrantEnabled, 'Device Code Grant')
    }
  }

  /**
   * Fill the Appearance tab
   *
   * @param iconUrl Icon URL
   */
  async fillAppearanceTab(iconUrl?: string): Promise<void> {
    // Switch to Appearance tab first
    await this.switchTab('appearance')

    if (iconUrl) {
      await this.iconUrlInput.fill(iconUrl)
      this.logger?.testCode.log(`Filled Icon URL: "${iconUrl}"`)
    }
  }

  /**
   * Submit the form page
   *
   * Clicks the submit button and waits for success toast and navigation back to list page.
   */
  async submitForm(): Promise<void> {
    await expect(this.submitButton).toBeEnabled()
    await this.smartClick(this.submitButton)
    this.logger?.testCode.log('Clicked submit button')

    // Wait for success toast
    await expect(this.successMessage.first()).toBeVisible({ timeout: 10000 })
    this.logger?.testCode.log('Success message appeared')

    // Wait for navigation back to list page
    await expect(this.container).toBeVisible({ timeout: 10000 })
    this.logger?.testCode.log('Navigated back to list page')
  }

  /**
   * Cancel the form and return to list page
   *
   * Clicks the cancel button and waits for navigation back to list page.
   */
  async cancelForm(): Promise<void> {
    await this.smartClick(this.cancelButton)
    this.logger?.testCode.log('Clicked cancel button')

    // Wait for navigation back to list page
    await expect(this.container).toBeVisible({ timeout: 5000 })
  }

  // ============================================================================
  // Convenience Methods
  // ============================================================================

  /**
   * Create a new Client App using the page + tabs flow
   *
   * Navigates to page, opens create page, fills all tabs, and submits.
   *
   * @param data Client App data
   * @param realmId Realm ID
   */
  async createClientApp(data: ClientAppData, realmId: string = 'admin'): Promise<void> {
    // Navigate to list page if not already there
    await this.goto(realmId)

    // Open create form page
    await this.gotoCreatePage()

    // Fill Basic tab
    await this.fillBasicTab({
      clientId: data.clientId,
      name: data.name,
      description: data.description,
    })

    // Fill Redirect URIs tab
    if (data.redirectUris && data.redirectUris.length > 0) {
      await this.fillRedirectUrisTab(data.redirectUris)
    }

    // Fill Security tab
    await this.fillSecurityTab({
      enabled: data.enabled,
      sessionTtlSeconds: data.sessionTtl,
      sessionRenewalTtlSeconds: data.renewalTtl,
      deviceCodeGrantEnabled: data.deviceCodeGrantEnabled,
    })

    // Fill Appearance tab
    if (data.iconUrl) {
      await this.fillAppearanceTab(data.iconUrl)
    }

    // Submit
    await this.submitForm()

    // Verify the Client App appears in the list
    await this.waitForClientAppByName(data.name)
    this.logger?.testCode.log(`Client App "${data.name}" created and verified`)
  }

  /**
   * Edit an existing Client App using the page + tabs flow
   *
   * @param appName Current name of the client app
   * @param data Updated fields
   * @param realmId Realm ID
   */
  async editClientApp(
    appName: string,
    data: Partial<ClientAppData>,
    realmId: string = 'admin'
  ): Promise<void> {
    // Navigate to list page if not already there
    await this.goto(realmId)

    // Open edit form page by finding the row by name
    await this.gotoEditPage(appName)

    // Update Basic tab fields (if provided)
    if (data.name || data.description) {
      // We are on Basic tab by default when form page opens
      if (data.name) {
        await this.nameInput.clear()
        await this.nameInput.fill(data.name)
        this.logger?.testCode.log(`Updated Name: "${data.name}"`)
      }
      if (data.description) {
        await this.descriptionInput.clear()
        await this.descriptionInput.fill(data.description)
        this.logger?.testCode.log(`Updated Description: "${data.description}"`)
      }
    }

    // Update Redirect URIs tab (if provided)
    if (data.redirectUris) {
      await this.fillRedirectUrisTab(data.redirectUris)
    }

    // Update Security tab (if provided)
    if (data.enabled !== undefined || data.sessionTtl !== undefined || data.deviceCodeGrantEnabled !== undefined) {
      await this.fillSecurityTab({
        enabled: data.enabled,
        sessionTtlSeconds: data.sessionTtl,
        sessionRenewalTtlSeconds: data.renewalTtl,
        deviceCodeGrantEnabled: data.deviceCodeGrantEnabled,
      })
    }

    // Update Appearance tab (if provided)
    if (data.iconUrl) {
      await this.fillAppearanceTab(data.iconUrl)
    }

    // Submit
    await this.submitForm()

    // Verify update if name changed
    const verifyName = data.name || appName
    await this.waitForClientAppByName(verifyName)
    this.logger?.testCode.log(`Client App "${verifyName}" updated and verified`)
  }

  // ============================================================================
  // Table Interaction Methods
  // ============================================================================

  async deleteClientApp(appId: string): Promise<void> {
    const deleteButton = this.page.locator(SELECTORS.clientApps.deleteButton(appId))

    // Click delete button to open confirmation dialog
    await this.smartClick(deleteButton)

    // Wait for delete confirmation dialog to appear
    await expect(this.deleteConfirmDialog).toBeVisible({ timeout: 10000 })

    // Click confirm button to delete
    await this.smartClick(this.deleteConfirmButton)

    // Wait for dialog to close
    await expect(this.deleteConfirmDialog).toBeHidden({ timeout: 10000 })

    // Wait for success toast to ensure mutation completed and cache invalidated
    await expect(this.successMessage.first()).toBeVisible({ timeout: 10000 })
  }

  async deleteClientAppByName(appName: string): Promise<void> {
    const appId = await this.getClientIdByName(appName)
    if (!appId) {
      throw new Error(`Client App "${appName}" not found`)
    }
    await this.deleteClientApp(appId)
  }

  async clientAppExists(appIdOrName: string): Promise<boolean> {
    const nameCell = this.page.getByText(appIdOrName, { exact: true }).first()
    return await nameCell.isVisible({ timeout: 5000 }).catch(() => false)
  }

  /**
   * Wait for Client App to appear in table by name
   *
   * @param name Client App name
   * @param timeout Timeout in milliseconds (default: 10000)
   */
  async waitForClientAppByName(name: string, timeout: number = 10000): Promise<void> {
    await expect(this.page.getByText(name, { exact: true })).toBeVisible({ timeout })
  }

  /**
   * Get Client App UUID by name
   *
   * @param name Client App name
   * @returns Client App UUID (data-app-id) or empty string if not found
   */
  async getClientIdByName(name: string): Promise<string> {
    const nameCell = this.page.getByText(name, { exact: true }).first()
    const isVisible = await nameCell.isVisible({ timeout: 5000 }).catch(() => false)

    if (!isVisible) {
      return ''
    }

    const row = nameCell.locator('xpath=ancestor::tr[1]')
    const appId = await row.getAttribute('data-app-id')

    return appId || ''
  }

  /**
   * Search for Client App by name or Client ID
   *
   * @param query Search query
   */
  async search(query: string): Promise<void> {
    const searchInput = this.page.getByPlaceholder(/search/i)
    await searchInput.fill(query)
  }

  /**
   * Get list of Client Apps from table
   *
   * @returns Array of Client App data with UUID, client_id, and name
   */
  async getClientAppsList(): Promise<Array<{ id: string; clientId: string; name: string }>> {
    await this.waitForReady()

    const rows = this.page.locator('[data-app-id]')
    const count = await rows.count()

    const apps: Array<{ id: string; clientId: string; name: string }> = []

    for (let i = 0; i < count; i++) {
      const row = rows.nth(i)
      const id = await row.getAttribute('data-app-id')
      const clientId = await row.getAttribute('data-client-id')
      const name = await row.locator('[data-field="name"]').textContent()

      if (id && clientId && name) {
        apps.push({ id, clientId, name })
      }
    }

    return apps
  }

  // ============================================================================
  // Validation Methods
  // ============================================================================

  /**
   * Get validation errors visible in the form page
   *
   * @returns Array of validation error messages
   */
  async getValidationErrors(): Promise<string[]> {
    const errors: string[] = []

    // Get error messages within the form page
    const errorElements = this.formPage.locator('[role="alert"], .text-destructive')
    const count = await errorElements.count()

    for (let i = 0; i < count; i++) {
      const errorText = await errorElements.nth(i).textContent()
      if (errorText && errorText.trim()) {
        errors.push(errorText.trim())
      }
    }

    return errors
  }

  /**
   * Verify page title text
   *
   * @param expectedTitle Expected title text ("Create Client App" or "Edit Client App")
   */
  async verifyPageTitle(expectedTitle: string): Promise<void> {
    await expect(this.pageTitle).toHaveText(expectedTitle)
    this.logger?.testCode.log(`Page title verified: "${expectedTitle}"`)
  }

  /**
   * Verify tab is active (has data-state="active")
   *
   * @param tabName Tab name to verify
   */
  async verifyActiveTab(tabName: DialogTabName): Promise<void> {
    const tab = this.getTabLocator(tabName)
    await expect(tab).toHaveAttribute('data-state', 'active')
    this.logger?.testCode.log(`Active tab verified: ${tabName}`)
  }

  /**
   * Verify submit button text
   *
   * @param expectedText Expected button text ("Create" or "Save Changes")
   */
  async verifySubmitButtonText(expectedText: string): Promise<void> {
    await expect(this.submitButton).toHaveText(expectedText)
    this.logger?.testCode.log(`Submit button text verified: "${expectedText}"`)
  }

  /**
   * Verify a field value in the form page
   *
   * @param fieldSelector Selector for the field
   * @param expectedValue Expected value
   */
  async verifyFieldValue(fieldSelector: string, expectedValue: string): Promise<void> {
    const field = this.formPage.locator(fieldSelector)
    const value = await field.inputValue()
    expect(value).toBe(expectedValue)
    this.logger?.testCode.log(`Field value verified: "${expectedValue}"`)
  }
}
