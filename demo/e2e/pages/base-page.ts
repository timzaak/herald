/**
 * Base Page Object
 *
 * Provides common functionality for all Page Objects.
 * Encapsulates page navigation, loading state waiting, and element state helpers.
 *
 */

import { Page, Locator, expect } from '@playwright/test'
import { SELECTORS } from '../selectors'
import type { UnifiedLogger } from '../helpers/unified-logger'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'

/**
 * Base Page Object class
 *
 * All Page Objects should extend this class to inherit common functionality.
 */
export class BasePage {
  protected logger?: UnifiedLogger

  constructor(public readonly page: Page, logger?: UnifiedLogger) {
    this.logger = logger
  }

  /**
   * Navigate to a path and wait for page load
   *
   * @param path URL path (e.g., '/admin/users')
   * @param waitForSelector Optional selector to wait for
   */
  async goto(path: string, waitForSelector?: string): Promise<void> {
    const url = path.startsWith('http') ? path : `${BASE_URL}${path}`
    await this.page.goto(url)

    if (waitForSelector) {
      await expect(this.page.locator(waitForSelector)).toBeVisible()
    }
  }

  /**
   * Wait for page load state
   *
   * @param state Load state to wait for (default: 'domcontentloaded')
   */
  protected async waitForLoad(state: 'load' | 'domcontentloaded' | 'networkidle' = 'domcontentloaded'): Promise<void> {
    await this.page.waitForLoadState(state)
  }

  /**
   * Wait for element to be visible
   *
   * @param locator Element locator
   * @param timeout Timeout in milliseconds (default: 5000)
   */
  protected async waitForVisible(locator: Locator, timeout: number = 5000): Promise<void> {
    await expect(locator).toBeVisible({ timeout })
  }

  /**
   * Wait for element to be hidden
   *
   * @param locator Element locator
   * @param timeout Timeout in milliseconds (default: 5000)
   */
  protected async waitForHidden(locator: Locator, timeout: number = 5000): Promise<void> {
    await expect(locator).toBeHidden({ timeout })
  }

  /**
   * Smart click - waits for element to be visible before clicking
   *
   * @param element Element locator
   * @param force Whether to force click (default: false)
   */
  public async smartClick(element: Locator, force: boolean = false): Promise<void> {
    await expect(element).toBeVisible()
    await element.click({ force })
  }

  /**
   * Get current page URL
   */
  getUrl(): string {
    return this.page.url()
  }

  /**
   * Check if element is visible
   *
   * @param locator Element locator
   */
  async isVisible(locator: Locator): Promise<boolean> {
    return await locator.isVisible().catch(() => false)
  }

  /**
   * Wait for API response
   *
   * ⚠️ 如需验证状态码，请直接使用 `page.waitForResponse(predicate, () => action)`
   *
   * @param url API URL pattern
   * @param method HTTP method (default: 'GET')
   * @param timeout Timeout in milliseconds (default: 10000)
   */
  protected async waitForApiResponse(
    url: string,
    method: 'GET' | 'POST' | 'PUT' | 'DELETE' | 'PATCH' = 'GET',
    timeout: number = 10000
  ): Promise<void> {
    await this.page.waitForResponse(
      response =>
        response.url().includes(url) &&
        response.request().method() === method,
      { timeout }
    )
  }

  /**
   * Fill a form field
   *
   * Triggers blur event to ensure TanStack Form validation runs after filling.
   * This updates the form's canSubmit state and enables the submit button.
   *
   * Uses assertion-based waiting (Playwright auto-wait) instead of fixed delays.
   *
   * @param locator Element locator
   * @param value Value to fill
   * @param waitForValidation Optional - wait for validation state (default: true)
   */
  protected async fillField(locator: Locator, value: string, waitForValidation: boolean = true): Promise<void> {
    await expect(locator).toBeVisible()

    // Select all text and replace with new value
    // This works correctly with React controlled inputs
    await locator.selectText()
    await locator.fill(value)

    await locator.blur()

    // Wait for React state update by checking if value is committed.
    if (waitForValidation) {
      await expect(async () => {
        const inputValue = await locator.inputValue()
        expect(inputValue).toBe(value)
      }).toPass({ timeout: 2000 })
    }
  }

  /**
   * Check/uncheck a checkbox
   *
   * Uses click() with explicit wait instead of setChecked() to avoid
   * "Clicking the checkbox did not change its state" errors with custom
   * checkbox implementations (e.g., Radix UI).
   *
   * Uses assertion-based waiting (Playwright auto-wait) instead of fixed delays.
   *
   * @param locator Element locator
   * @param checked Whether to check (true) or uncheck (false)
   *
   * @see .claude/mistakes/demo-repair.md - CHECKBOX_STATE_CHANGE_FAILURE
   */
  protected async setCheckbox(locator: Locator, checked: boolean): Promise<void> {
    await expect(locator).toBeVisible()

    const isChecked = await locator.isChecked()

    if (isChecked !== checked) {
      // Use click() instead of setChecked() to avoid state detection issues
      await locator.click({ force: true })

      await expect(async () => {
        const newCheckedState = await locator.isChecked()
        expect(newCheckedState).toBe(checked)
      }).toPass({ timeout: 5000 })
    }
  }

  /**
   * Toggle a Radix `Switch` (role="switch") to a desired on/off state.
   *
   * Radix Switch renders a `<button role="switch">` with a `data-state`
   * attribute (`checked` | `unchecked`) and `aria-checked`. Unlike native
   * checkboxes, toggling it reliably requires:
   *   - reading `data-state` (NOT `isChecked()`, which can race with the
   *     React state update), and
   *   - a plain `click()` (NOT `click({ force: true })`), which lets Playwright
   *     run its normal actionability checks and hit the real interactive layer.
   *
   * Mirrors the proven pattern in helpers/bucket-helpers.ts: click only when
   * the current state differs from the target, then assert the `data-state`
   * transitioned before returning (assertion-based wait, no fixed sleeps).
   *
   * @param locator Locator pointing at the Radix Switch button
   * @param on Desired state: true = checked, false = unchecked
   */
  protected async setSwitch(locator: Locator, on: boolean): Promise<void> {
    await expect(locator).toBeVisible()

    const wantState = on ? 'checked' : 'unchecked'

    const state = await locator.getAttribute('data-state')
    if (state === wantState) {
      return
    }

    await locator.click()

    await expect(async () => {
      const newState = await locator.getAttribute('data-state')
      expect(newState).toBe(wantState)
    }).toPass({ timeout: 5000 })
  }

  /**
   * Select an option from a select dropdown
   *
   * @param locator Element locator
   * @param value Option value to select
   */
  protected async selectOption(locator: Locator, value: string): Promise<void> {
    await expect(locator).toBeVisible()
    await locator.selectOption(value)
  }

  /**
   * Get text content of an element
   *
   * @param locator Element locator
   */
  protected async getText(locator: Locator): Promise<string> {
    await expect(locator).toBeVisible()
    return await locator.textContent() || ''
  }

  /**
   * Get attribute value of an element
   *
   * @param locator Element locator
   * @param attribute Attribute name
   */
  protected async getAttribute(locator: Locator, attribute: string): Promise<string | null> {
    return await locator.getAttribute(attribute)
  }

  /**
   * Take screenshot (for debugging)
   *
   * @param name Screenshot name
   */
  protected async screenshot(name: string): Promise<void> {
    await this.page.screenshot({ path: `test-results/screenshots/${name}.png` })
  }

  /**
   * Select an option from a Radix Select component.
   *
   * Clicks the trigger, waits for the dropdown content to appear,
   * then selects by data-value or falls back to text match.
   *
   * @param triggerLocator Locator for the SelectTrigger element
   * @param value The option value to select (matches data-value attribute)
   */
  protected async selectRadixOption(triggerLocator: Locator, value: string): Promise<void> {
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
}
