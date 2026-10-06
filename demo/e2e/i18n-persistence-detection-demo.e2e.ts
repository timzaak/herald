/**
 * i18n Persistence and Browser Detection Demo Tests
 *
 * Covers US-I18N-001 Scenarios 2, 3, 4:
 * - Scenario 2: Language preference persists after page reload
 * - Scenario 3: Browser language zh defaults to Chinese (via localStorage simulation)
 * - Scenario 4: Unsupported locale falls back to English
 *
 * @see docs/user-stories/core/i18n.md (US-I18N-001)
 */

import { test, expect, loginPageTest } from './fixtures/demo-auth.fixtures'
import { SELECTORS } from './selectors'
import {
  switchToLocale,
  getLocaleFromStorage,
  setLocaleInStorage,
  assertSidebarInEnglish,
  assertSidebarInChinese,
} from './helpers/i18n-helpers'

test.describe('[i18n] US-I18N-001: Persistence and Browser Detection', () => {
  /**
   * US-I18N-001 Scenario 2: Language preference persists after page reload
   *
   * Verifies that switching to Chinese and reloading the page preserves
   * the Chinese locale. Both the localStorage value and the sidebar text
   * must remain Chinese after reload.
   */
  test('US-I18N-001 Scenario 2: Language preference persists after page reload', async ({
    authenticatedPage,
  }) => {
    // Step 1: Switch to Chinese via language switcher
    await switchToLocale(authenticatedPage, 'zh-CN')

    // Step 2: Verify localStorage reflects the Chinese locale
    const localeBeforeReload = await getLocaleFromStorage(authenticatedPage)
    expect(localeBeforeReload).toBe('zh-CN')

    // Step 3: Reload the page
    await authenticatedPage.reload()

    // Step 4: Wait for sidebar to be visible after reload
    await expect(
      authenticatedPage.locator(SELECTORS.sidebar.container),
    ).toBeVisible()

    // Step 5: Assert sidebar dashboard text is still Chinese (persisted)
    await assertSidebarInChinese(authenticatedPage)

    // Step 6: Verify localStorage still has zh-CN after reload
    const localeAfterReload = await getLocaleFromStorage(authenticatedPage)
    expect(localeAfterReload).toBe('zh-CN')
  })

  /**
   * US-I18N-001 Scenario 3: Browser language zh defaults to Chinese
   *
   * LIMITATION: Playwright config sets --lang=en-US, so we cannot truly
   * test navigator.language-based browser detection. Instead, we simulate
   * the outcome of browser detection by setting localStorage to 'zh-CN'
   * before login. This verifies that the LocaleProvider correctly reads
   * the persisted locale and renders the UI in Chinese on fresh load.
   *
   * The explicit page.goto('/admin/auth/login') before setLocaleInStorage
   * is required because page.evaluate() needs the page to be on the correct
   * origin to access localStorage. The subsequent loginAsAdmin call will
   * navigate again, but localStorage persists by origin.
   */
  loginPageTest(
    'US-I18N-001 Scenario 3: Browser language zh defaults to Chinese',
    async ({ page, loginPage, demoLogger }) => {
      // Navigate to login page to set localStorage on the correct origin
      await page.goto('/admin/auth/login')

      // Simulate browser detection result: set localStorage to zh-CN
      await setLocaleInStorage(page, 'zh-CN')

      // Login as admin; loginAsAdmin clears cookies but not localStorage
      await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')

      // After login, the sidebar should render in Chinese
      await assertSidebarInChinese(page)
    },
  )

  /**
   * US-I18N-001 Scenario 4: Unsupported locale falls back to English
   *
   * Verifies that setting localStorage to an unsupported locale ('fr')
   * causes the LocaleProvider to fall back to English. The sidebar
   * dashboard text should display "Dashboard" (English), not broken
   * or missing text.
   *
   * The explicit page.goto('/admin/auth/login') before setLocaleInStorage
   * is required because page.evaluate() needs the page to be on the correct
   * origin to access localStorage.
   */
  loginPageTest(
    'US-I18N-001 Scenario 4: Unsupported locale falls back to English',
    async ({ page, loginPage, demoLogger }) => {
      // Navigate to login page to set localStorage on the correct origin
      await page.goto('/admin/auth/login')

      // Set localStorage to an unsupported locale (French)
      await setLocaleInStorage(page, 'fr')

      // Login as admin; loginAsAdmin clears cookies but not localStorage
      await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')

      // After login, the sidebar should fall back to English
      await assertSidebarInEnglish(page)
    },
  )
})
