import { expect, type Page } from '@playwright/test'
import { BasePage } from '../../pages/base-page'
import { SELECTORS } from '../../selectors'
import type { UnifiedLogger } from 'playwright-unified-logger'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'

/**
 * Helper for executing the self-service account deletion flow from the user
 * Security page.
 */
export class DeleteAccountHelper extends BasePage {
  constructor(page: Page, logger?: UnifiedLogger) {
    super(page, logger)
  }

  /**
   * Navigate to the user Security page.
   */
  async gotoSecurityPage(realmId: string): Promise<void> {
    await this.page.goto(`${BASE_URL}/${realmId}/user/security`, {
      waitUntil: 'domcontentloaded',
    })
    await expect(this.page.locator(SELECTORS.security.pageTitle)).toBeVisible({ timeout: 10000 })
  }

  /**
   * Open the delete account dialog.
   */
  async openDeleteDialog(): Promise<void> {
    const openButton = this.page.locator(SELECTORS.legalConsent.deleteAccountOpenButton)
    await this.smartClick(openButton)
    await expect(this.page.locator(SELECTORS.legalConsent.deleteAccountDialog)).toBeVisible()
  }

  /**
   * Fill the password confirmation field.
   */
  async fillPassword(password: string): Promise<void> {
    const input = this.page.locator(SELECTORS.legalConsent.deleteAccountPasswordInput)
    await this.fillField(input, password)
  }

  /**
   * Submit the delete account form.
   */
  async submitDelete(): Promise<void> {
    const submitButton = this.page.locator(SELECTORS.legalConsent.deleteAccountSubmitButton)
    await this.smartClick(submitButton)
  }

  /**
   * Assert that the user is redirected to the login page after deletion.
   */
  async expectSuccessRedirectToLogin(realmId: string): Promise<void> {
    await this.page.waitForURL(`**/${realmId}/auth/login`, { timeout: 15000 })
    await expect(this.page.locator(SELECTORS.login.container)).toBeVisible()
  }

  /**
   * Assert that the delete account dialog shows an error, optionally checking
   * the error message text.
   */
  async expectError(message?: string): Promise<void> {
    const alert = this.page.locator(SELECTORS.legalConsent.deleteAccountErrorAlert)
    await expect(alert).toBeVisible({ timeout: 5000 })

    if (message !== undefined) {
      const alertMessage = this.page.locator(SELECTORS.legalConsent.deleteAccountErrorMessage)
      await expect(alertMessage).toContainText(message)
    }
  }
}
