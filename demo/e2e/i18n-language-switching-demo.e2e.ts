/**
 * i18n Language Switching Demo Tests
 *
 * Covers US-I18N-001: Switch UI language and verify text updates
 * Partially covers US-I18N-003: Sidebar navigation text fully translated
 *
 * Test Cases:
 * - Scenario 1a: Switch English -> Chinese, verify sidebar text changes
 * - Scenario 1b: Switch Chinese -> English, verify text reverts
 * - Scenario 1c: Switched language persists across page navigation
 *
 * @see docs/user-stories/core/i18n.md (US-I18N-001, US-I18N-003)
 */

import { test, expect } from './fixtures/demo-auth.fixtures'
import { SELECTORS } from './selectors'
import {
  switchToLocale,
  getSidebarMenuText,
  assertSidebarInEnglish,
  assertSidebarInChinese,
} from './helpers/i18n-helpers'

test.describe('[i18n] US-I18N-001: Language Switching', () => {
  /**
   * US-I18N-001 Scenario 1a: Switch from English to Chinese
   *
   * Verifies that clicking the Chinese language switcher button
   * updates all visible sidebar menu text from English to Chinese.
   *
   * Expected Chinese values (from zh-CN.json nav keys):
   * - Dashboard -> 仪表盘
   * - Users -> 用户
   * - Roles -> 角色
   * - Audit Log -> 审计日志
   */
  test('US-I18N-001 Scenario 1a: Switch from English to Chinese, sidebar text updates', async ({
    authenticatedPage,
  }) => {
    // Step 1: Assert initial state is English
    await assertSidebarInEnglish(authenticatedPage)

    // Step 2: Switch to Chinese
    await switchToLocale(authenticatedPage, 'zh-CN')

    // Step 3-6: Verify sidebar text changed to Chinese
    const dashboardText = await getSidebarMenuText(
      authenticatedPage,
      SELECTORS.sidebar.menuDashboard,
    )
    expect(dashboardText).toBe('仪表盘')

    const usersText = await getSidebarMenuText(
      authenticatedPage,
      SELECTORS.sidebar.menuUsers,
    )
    expect(usersText).toBe('用户')

    const rolesText = await getSidebarMenuText(
      authenticatedPage,
      SELECTORS.sidebar.menuRoles,
    )
    expect(rolesText).toBe('角色')

    const auditLogText = await getSidebarMenuText(
      authenticatedPage,
      SELECTORS.sidebar.menuAuditLog,
    )
    expect(auditLogText).toBe('审计日志')
  })

  /**
   * US-I18N-001 Scenario 1b: Switch from Chinese back to English
   *
   * Verifies that switching back to English after being in Chinese
   * correctly reverts all sidebar text to English labels.
   */
  test('US-I18N-001 Scenario 1b: Switch from Chinese back to English, text reverts', async ({
    authenticatedPage,
  }) => {
    // Step 1: Switch to Chinese first
    await switchToLocale(authenticatedPage, 'zh-CN')

    // Step 2: Verify Chinese text is showing
    await assertSidebarInChinese(authenticatedPage)

    // Step 3: Switch back to English
    await switchToLocale(authenticatedPage, 'en')

    // Step 4-5: Verify sidebar text reverted to English
    const dashboardText = await getSidebarMenuText(
      authenticatedPage,
      SELECTORS.sidebar.menuDashboard,
    )
    expect(dashboardText).toBe('Dashboard')

    const usersText = await getSidebarMenuText(
      authenticatedPage,
      SELECTORS.sidebar.menuUsers,
    )
    expect(usersText).toBe('Users')
  })

  /**
   * US-I18N-001 Scenario 1c: Switched language persists across navigation
   *
   * Verifies that after switching to Chinese, navigating to a different
   * page and back preserves the Chinese locale setting.
   */
  test('US-I18N-001 Scenario 1c: Switched language persists across page navigation', async ({
    authenticatedPage,
  }) => {
    // Step 1: Switch to Chinese
    await switchToLocale(authenticatedPage, 'zh-CN')

    // Step 2: Navigate to users page via sidebar
    await authenticatedPage
      .locator(SELECTORS.sidebar.menuUsers)
      .click()

    // Step 3: Assert URL contains /manage/users
    await authenticatedPage.waitForURL(/\/manage\/users/)

    // Step 4: Navigate back to dashboard via sidebar
    await authenticatedPage
      .locator(SELECTORS.sidebar.menuDashboard)
      .click()

    // Step 5: Assert Chinese persists — dashboard label still in Chinese
    const dashboardText = await getSidebarMenuText(
      authenticatedPage,
      SELECTORS.sidebar.menuDashboard,
    )
    expect(dashboardText).toBe('仪表盘')
  })
})
