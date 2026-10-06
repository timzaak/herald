/**
 * Regular User OAuth Login Demo Tests
 *
 * User Stories:
 * - US-RU-003: OAuth 第三方登录
 *
 * User Stories: docs/user-stories/core/regular-user.md
 *
 * Test Coverage:
 * - OAuth Login button visibility when providers are enabled
 * - OAuth Login button visibility when no providers are enabled
 *
 * Note: This test only verifies button visibility, not actual OAuth flow.
 * OAuth authorization flow testing requires real Provider credentials.
 *
 * UnifiedLogger Usage:
 * - All tests use UnifiedLogger through 'demoLogger' fixture
 * - Logger is automatically initialized and finalized by fixture
 */

import { test, expect, cleanupTestData } from '../fixtures/demo-page.fixtures'
import { SettingsPage } from '../pages/settings-page'
import { LoginPage } from '../pages/login-page'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { logout } from '../helpers/auth'

const ADMIN_REALM = 'admin'
const ADMIN_EMAIL = 'admin@cas.com'

test.describe('[Regular User] OAuth Login Demo Tests', () => {
  test.beforeEach(async ({ page, demoLogger, loginPage }) => {
    // Verify test environment
    await verifyTestEnvironment(page, {
      requiredRealms: ['admin'],
      requiredUsers: ['admin@cas.com'],
    })

    // Login as admin to clean up OAuth providers BEFORE starting test
    await loginPage.loginAsAdmin(ADMIN_EMAIL, 'password', ADMIN_REALM)

    const settingsPage = new SettingsPage(page, demoLogger, ADMIN_REALM)
    try {
      await settingsPage.goto()
      await settingsPage.waitForReady()
      await settingsPage.switchToProvidersTab()

      // Delete all providers to ensure clean state
      const providers = ['google', 'github', 'facebook', 'apple']
      for (const provider of providers) {
        if (await settingsPage.providerExists(provider)) {
          console.log(`[BeforeEach] Deleting ${provider} provider`)
          await settingsPage.deleteProvider(provider)
        }
      }
    } catch (error) {
      console.log('[BeforeEach] Cleanup error:', error)
    }

    // Logout after cleanup so test can start fresh
    await logout(page)
  })

  test.afterEach(async ({ page, testStartTime }) => {
    await cleanupTestData(page, ADMIN_REALM, {
      keepUsers: [ADMIN_EMAIL],
      timestamp: testStartTime,
    })
  })

  test.describe('用户故事 US-RU-003：OAuth 第三方登录', () => {
    test('OAuth 登录按钮可见性测试', async ({ page, loginPage, demoLogger, testStartTime }) => {
      // Login as admin to configure OAuth providers
      await loginPage.loginAsAdmin(ADMIN_EMAIL, 'password', ADMIN_REALM)

      const settingsPage = new SettingsPage(page, demoLogger, ADMIN_REALM)

      // Phase 1: 验证有已启用 Provider 时显示 OAuth 按钮（场景 8）
      await test.step('Phase 1: 验证有已启用 Provider 时显示 OAuth 按钮', async () => {
        // Navigate to Settings and configure Google Provider
        await settingsPage.goto()
        await settingsPage.waitForReady()
        await settingsPage.switchToProvidersTab()

        // Add Google Provider
        const googleConfig = {
          providerType: 'google' as const,
          clientId: `google-client-${testStartTime}`,
          clientSecret: `google-secret-${testStartTime}`,
          scopes: ['https://www.googleapis.com/auth/userinfo.email'],
          enabled: true,
        }

        await settingsPage.addProvider(googleConfig)
        demoLogger.testCode.log('[Test] Google Provider configured and enabled')

        // Logout to test login page
        await logout(page)

        // Navigate to login page
        const loginPageObj = new LoginPage(page, demoLogger)
        await loginPageObj.goto(ADMIN_REALM)

        // Verify "Or continue with" separator is visible
        await expect(page.getByText(/or continue with/i)).toBeVisible({ timeout: 2000 })
        demoLogger.testCode.log('[Test] "Or continue with" separator is visible')

        // Verify at least one OAuth Provider button is visible
        // Using data-testid for OAuth login buttons (semantic selectors not available)
        await expect(page.getByTestId('oauth-login-button-google')).toBeVisible({ timeout: 2000 })
        demoLogger.testCode.log('[Test] Google OAuth login button is visible')

        // Verify button uses outline style (by checking variant)
        const googleButton = page.getByTestId('oauth-login-button-google')
        await expect(googleButton).toHaveClass(/outline/)
        demoLogger.testCode.log('[Test] Google OAuth button uses outline style')
      })

      // Phase 2: 验证未启用 Provider 时不显示按钮（场景 9）
      await test.step('Phase 2: 验证未启用 Provider 时不显示按钮', async () => {
        // Login as admin again to disable provider
        await loginPage.loginAsAdmin(ADMIN_EMAIL, 'password', ADMIN_REALM)

        // Navigate to Settings
        await settingsPage.goto()
        await settingsPage.waitForReady()
        await settingsPage.switchToProvidersTab()

        // Disable Google Provider
        await settingsPage.toggleProvider('google')
        demoLogger.testCode.log('[Test] Google Provider disabled')

        // Logout to test login page
        await logout(page)

        // Navigate to login page
        const loginPageObj = new LoginPage(page, demoLogger)
        await loginPageObj.goto(ADMIN_REALM)

        // Verify "Or continue with" separator is NOT visible
        await expect(page.getByText(/or continue with/i)).not.toBeVisible({ timeout: 2000 })
        demoLogger.testCode.log('[Test] "Or continue with" separator is NOT visible')

        // Verify OAuth Provider buttons are NOT visible
        await expect(page.getByTestId('oauth-login-button-google')).not.toBeVisible({ timeout: 2000 })
        demoLogger.testCode.log('[Test] Google OAuth login button is NOT visible')
      })
    })
  })
})
