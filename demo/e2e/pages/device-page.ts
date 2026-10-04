/**
 * Device Verification Page Object
 *
 * Encapsulates the device verification page at /{realmId}/device
 * and /{realmId}/device/{userCode}.
 *
 * Frontend states: input -> verifying -> confirmed -> result
 *
 */

import { Page, Locator, expect } from '@playwright/test'
import { SELECTORS } from '../selectors'
import { BasePage } from './base-page'
import type { UnifiedLogger } from '../helpers/unified-logger'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'

/**
 * Device Verification Page Object
 *
 * Represents the device verification page used in the OAuth Device Code Grant flow.
 *
 * @example
 * ```typescript
 * const devicePage = new DeviceVerificationPage(page, logger)
 * await devicePage.goto('demo-realm')
 * await devicePage.enterCode('BCDF-GHJK')
 * await devicePage.waitForVerified()
 * await devicePage.authorize()
 * ```
 */
export class DeviceVerificationPage extends BasePage {
  // Selectors
  readonly card: Locator
  readonly title: Locator
  readonly error: Locator
  readonly result: Locator
  readonly codeInput: Locator
  readonly codeSubmit: Locator
  readonly authorizeButton: Locator
  readonly denyButton: Locator

  constructor(page: Page, logger?: UnifiedLogger) {
    super(page, logger)
    this.card = page.locator(SELECTORS.deviceVerification.card)
    this.title = page.locator(SELECTORS.deviceVerification.title)
    this.error = page.locator(SELECTORS.deviceVerification.error)
    this.result = page.locator(SELECTORS.deviceVerification.result)
    this.codeInput = page.locator(SELECTORS.deviceVerification.codeInput)
    this.codeSubmit = page.locator(SELECTORS.deviceVerification.codeSubmit)
    this.authorizeButton = page.locator(SELECTORS.deviceVerification.authorizeButton)
    this.denyButton = page.locator(SELECTORS.deviceVerification.denyButton)
  }

  /**
   * Navigate to device verification input page
   *
   * @param realmId Realm ID
   */
  async goto(realmId: string): Promise<void> {
    await this.page.goto(`${BASE_URL}/${realmId}/device`, { waitUntil: 'domcontentloaded' })
    await expect(this.card).toBeVisible()
  }

  /**
   * Navigate to device verification with a pre-filled user code
   *
   * The page auto-submits verification on mount when a userCode is in the URL.
   *
   * @param realmId Realm ID
   * @param userCode User code in XXXX-XXXX format
   */
  async gotoWithCode(realmId: string, userCode: string): Promise<void> {
    await this.page.goto(`${BASE_URL}/${realmId}/device/${userCode}`, { waitUntil: 'domcontentloaded' })
    await expect(this.card).toBeVisible()
  }

  /**
   * Enter a device code and submit
   *
   * The CodeInput component auto-formats the code (uppercases, inserts hyphen).
   * It submits the raw code (without hyphen) to the API.
   *
   * @param code User code (will be auto-formatted, e.g., "BCDFGHJK" -> "BCDF-GHJK")
   */
  async enterCode(code: string): Promise<void> {
    await expect(this.codeInput).toBeVisible()
    await this.codeInput.fill(code)
    await this.smartClick(this.codeSubmit)
  }

  /**
   * Click authorize button and wait for result
   */
  async authorize(): Promise<void> {
    await this.smartClick(this.authorizeButton)
    await expect(this.result).toBeVisible()
  }

  /**
   * Click deny button and wait for result
   */
  async deny(): Promise<void> {
    await this.smartClick(this.denyButton)
    await expect(this.result).toBeVisible()
  }

  /**
   * Get the error message text
   *
   * @returns Error message text, or empty string if not visible
   */
  async getErrorText(): Promise<string> {
    const visible = await this.isVisible(this.error)
    if (!visible) return ''
    return await this.getText(this.error)
  }

  /**
   * Get the result message text
   *
   * @returns Result message text (e.g., "Authorization successful." or "Authorization denied.")
   */
  async getResultText(): Promise<string> {
    return await this.getText(this.result)
  }

  /**
   * Wait for the authorize/deny confirmation view to appear
   */
  async waitForVerified(): Promise<void> {
    await expect(this.authorizeButton).toBeVisible({ timeout: 10000 })
  }

  /**
   * Wait for the result view to appear
   */
  async waitForResult(): Promise<void> {
    await expect(this.result).toBeVisible({ timeout: 10000 })
  }

  /**
   * Check if authorize button is visible
   */
  async isAuthorizeButtonVisible(): Promise<boolean> {
    return await this.isVisible(this.authorizeButton)
  }

  /**
   * Check if code input is visible
   */
  async isInputVisible(): Promise<boolean> {
    return await this.isVisible(this.codeInput)
  }
}
