/**
 * i18n Error Message Translation Demo Tests
 *
 * Covers US-I18N-002: Verify error messages follow current language
 *
 * Test Cases:
 * - Scenario 1: Chinese UI shows Chinese validation errors
 * - Scenario 2: English UI shows English validation errors
 * - Mid-session: Error language follows locale switch mid-session
 *
 * Trigger: Zod validation error from roles.name_required key
 * - English: "Role name is required"
 * - Chinese: "角色名称为必填项"
 *
 * @see docs/user-stories/core/i18n.md (US-I18N-002)
 */

import { test, expect } from './fixtures/demo-page.fixtures'
import { switchToLocale } from './helpers/i18n-helpers'
import { RolesPage } from './pages/roles-page'

/**
 * Locator for validation error messages rendered inside the dialog.
 *
 * CreateResourceDialog renders Zod validation errors as:
 *   <p className="text-sm text-destructive" role="alert">
 *
 * Using role="alert" as the primary selector — it targets persistent
 * form validation errors, NOT toast/Sonner auto-dismiss notifications.
 */
const validationErrorLocator = (page: import('@playwright/test').Page) =>
  page.locator('[data-testid="dialog"] [role="alert"]')

test.describe('[i18n] US-I18N-002: Error Message Translation', () => {
  /**
   * US-I18N-002 Scenario 1: Chinese UI shows Chinese validation error
   *
   * After switching locale to zh-CN, submitting an empty role creation
   * form must show the Chinese Zod validation message:
   * "角色名称为必填项" (zh-CN.json roles.name_required)
   */
  test('US-I18N-002 Scenario 1: Chinese UI shows Chinese validation error', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    // Step 1: Login as admin
    await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')

    // Step 2: Switch to Chinese
    await switchToLocale(page, 'zh-CN')

    // Step 3: Navigate to roles page
    const rolesPage = new RolesPage(page, demoLogger)
    await rolesPage.goto()

    // Step 4: Open create dialog (NOT clickAddRole — it has English-only title assertion)
    await rolesPage.addButton.click()
    await expect(rolesPage.dialog).toBeVisible()

    // Step 5: Submit empty form to trigger validation
    await rolesPage.createSubmitButton.click()

    // Step 6: Assert Chinese validation error
    const errorElement = validationErrorLocator(page)
    await expect(errorElement).toBeVisible()
    await expect(errorElement).toContainText('角色名称为必填项')
  })

  /**
   * US-I18N-002 Scenario 2: English UI shows English validation error
   *
   * With the default English locale, submitting an empty role creation
   * form must show the English Zod validation message:
   * "Role name is required" (en.json roles.name_required)
   */
  test('US-I18N-002 Scenario 2: English UI shows English validation error', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    // Step 1: Login as admin (English is default)
    await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')

    // Step 2: Navigate to roles page
    const rolesPage = new RolesPage(page, demoLogger)
    await rolesPage.goto()

    // Step 3: Open create dialog
    await rolesPage.addButton.click()
    await expect(rolesPage.dialog).toBeVisible()

    // Step 4: Submit empty form to trigger validation
    await rolesPage.createSubmitButton.click()

    // Step 5: Assert English validation error
    const errorElement = validationErrorLocator(page)
    await expect(errorElement).toBeVisible()
    await expect(errorElement).toContainText('Role name is required')
  })

  /**
   * US-I18N-002: Error language follows locale switch mid-session
   *
   * Proves error messages dynamically follow the current locale, not the
   * initial one. Starts in English, triggers an error, switches to Chinese,
   * then triggers the error again and verifies it is now in Chinese.
   */
  test('US-I18N-002: Error language follows locale switch mid-session', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    // Step 1: Login as admin (English)
    await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')

    // Step 2: Navigate to roles page
    const rolesPage = new RolesPage(page, demoLogger)
    await rolesPage.goto()

    // Step 3: Open create dialog, submit empty, verify English error
    await rolesPage.addButton.click()
    await expect(rolesPage.dialog).toBeVisible()
    await rolesPage.createSubmitButton.click()

    const englishError = validationErrorLocator(page)
    await expect(englishError).toBeVisible()
    await expect(englishError).toContainText('Role name is required')

    // Step 4: Close dialog via cancel button
    await rolesPage.dialogCancelButton.click()
    await expect(rolesPage.dialog).toBeHidden()

    // Step 5: Switch to Chinese
    await switchToLocale(page, 'zh-CN')

    // Step 6: Open create dialog again, submit empty, verify Chinese error
    await rolesPage.addButton.click()
    await expect(rolesPage.dialog).toBeVisible()
    await rolesPage.createSubmitButton.click()

    const chineseError = validationErrorLocator(page)
    await expect(chineseError).toBeVisible()
    await expect(chineseError).toContainText('角色名称为必填项')
  })
})
