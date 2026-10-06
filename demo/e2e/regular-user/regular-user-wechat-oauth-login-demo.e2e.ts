/**
 * Regular User WeChat OAuth Login Demo Tests
 *
 * User Stories:
 * - docs/user-stories/auth/wechat-oauth.md:
 *   - US-WO-001: WeChat OAuth Provider 配置（按钮随 provider 配置/启用状态显隐）
 *   - US-WO-002: WeChat Mini Program Provider 配置
 *
 * 范围声明：本 demo 只断言登录按钮可见性（provider 配置状态的渲染面），
 * 不驱动真实微信 OAuth 流（US-WO-003/004 的授权登录链路不在覆盖范围）。
 *
 * Test Coverage:
 * - WeChat login button visibility when provider is enabled
 * - WeChat login button visibility when provider is disabled
 * - WeChat login button visibility when provider is not configured
 * - Multiple provider scenario (show all enabled providers)
 * - Provider configuration validation (UI level)
 *
 * Important Notes:
 * - This test only verifies button visibility, NOT actual OAuth flow
 * - OAuth authorization flow testing requires real WeChat App ID and App Secret
 * - All operations are through UI only (no direct API calls)
 *
 * UnifiedLogger Usage:
 * - All tests use UnifiedLogger through 'demoLogger' fixture
 * - Logger is automatically initialized and finalized by fixture
 * - Logs are saved to demo/test-results/console-logs/
 */

import { test, expect, cleanupTestData } from '../fixtures/demo-page.fixtures'
import { SettingsPage } from '../pages/settings-page'
import { LoginPage } from '../pages/login-page'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { logout } from '../helpers/auth'

const ADMIN_REALM = 'admin'
const ADMIN_EMAIL = 'admin@cas.com'

test.describe('[Regular User] WeChat OAuth Login Demo Tests', () => {
  test.beforeEach(async ({ page, demoLogger, loginPage }) => {
    // Verify test environment
    await verifyTestEnvironment(page, {
      requiredRealms: ['admin'],
      requiredUsers: ['admin@cas.com'],
    })

    // Login as admin to clean up WeChat providers BEFORE starting test
    await loginPage.loginAsAdmin(ADMIN_EMAIL, 'password', ADMIN_REALM)

    const settingsPage = new SettingsPage(page, demoLogger, ADMIN_REALM)
    try {
      await settingsPage.goto()
      await settingsPage.waitForReady()
      await settingsPage.switchToProvidersTab()

      // Delete WeChat providers to ensure clean state
      const providers = ['wechat', 'wechat_miniprogram', 'google']
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

  // ============================================================================
  // 微信网站应用登录 [US-RU-010]
  // ============================================================================

  test.describe('用户故事 US-RU-010：微信网站应用登录', () => {
    test('微信网站应用登录按钮可见性测试 [US-RU-010]', async ({ page, loginPage, demoLogger, testStartTime }) => {
      // Login as admin to configure WeChat providers
      await loginPage.loginAsAdmin(ADMIN_EMAIL, 'password', ADMIN_REALM)

      const settingsPage = new SettingsPage(page, demoLogger, ADMIN_REALM)

      // Phase 1: 验证有已启用 WeChat Provider 时显示微信登录按钮（场景 4 - 反向测试）
      await test.step('Phase 1: 验证有已启用 WeChat Provider 时显示微信登录按钮', async () => {
        // Navigate to Settings and configure WeChat Provider
        await settingsPage.goto()
        await settingsPage.waitForReady()
        await settingsPage.switchToProvidersTab()

        // Add WeChat Provider
        const wechatConfig = {
          providerType: 'wechat' as const,
          clientId: `wx1234567890abcdef-${testStartTime}`,
          clientSecret: `abcdef1234567890-${testStartTime}`,
          scopes: ['snsapi_login'],
          enabled: true,
        }

        await settingsPage.addProvider(wechatConfig)
        demoLogger.testCode.log('[Test] WeChat Provider configured and enabled')

        // Logout to test login page
        await logout(page)

        // Navigate to login page
        const loginPageObj = new LoginPage(page, demoLogger)
        await loginPageObj.goto(ADMIN_REALM)

        // Verify "Or continue with" separator is visible
        await expect(page.getByText(/or continue with/i)).toBeVisible({ timeout: 2000 })
        demoLogger.testCode.log('[Test] "Or continue with" separator is visible')

        // Verify WeChat OAuth login button is visible
        // Using data-testid for OAuth login buttons (semantic selectors not available)
        await expect(page.getByTestId('oauth-login-button-wechat')).toBeVisible({ timeout: 2000 })
        demoLogger.testCode.log('[Test] WeChat OAuth login button is visible')

        // Verify button uses outline style (by checking variant)
        const wechatButton = page.getByTestId('oauth-login-button-wechat')
        await expect(wechatButton).toHaveClass(/outline/)
        demoLogger.testCode.log('[Test] WeChat OAuth button uses outline style')
      })

      // Phase 2: 验证未启用 WeChat Provider 时不显示按钮（场景 4）
      await test.step('Phase 2: 验证未启用 WeChat Provider 时不显示按钮', async () => {
        // Login as admin again to disable provider
        await loginPage.loginAsAdmin(ADMIN_EMAIL, 'password', ADMIN_REALM)

        // Navigate to Settings
        await settingsPage.goto()
        await settingsPage.waitForReady()
        await settingsPage.switchToProvidersTab()

        // Disable WeChat Provider
        await settingsPage.toggleProvider('wechat')
        demoLogger.testCode.log('[Test] WeChat Provider disabled')

        // Logout to test login page
        await logout(page)

        // Navigate to login page
        const loginPageObj = new LoginPage(page, demoLogger)
        await loginPageObj.goto(ADMIN_REALM)

        // Verify "Or continue with" separator is NOT visible (if no other providers are enabled)
        // Note: This assumes no other OAuth providers are enabled
        await expect(page.getByText(/or continue with/i)).not.toBeVisible({ timeout: 2000 })
        demoLogger.testCode.log('[Test] "Or continue with" separator is NOT visible')

        // Verify WeChat OAuth Provider button is NOT visible
        await expect(page.getByTestId('oauth-login-button-wechat')).not.toBeVisible({ timeout: 2000 })
        demoLogger.testCode.log('[Test] WeChat OAuth login button is NOT visible')
      })

      // Phase 3: 验证未配置 WeChat Provider 时不显示按钮
      await test.step('Phase 3: 验证未配置 WeChat Provider 时不显示按钮', async () => {
        // Login as admin to delete provider
        await loginPage.loginAsAdmin(ADMIN_EMAIL, 'password', ADMIN_REALM)

        // Navigate to Settings
        await settingsPage.goto()
        await settingsPage.waitForReady()
        await settingsPage.switchToProvidersTab()

        // Delete WeChat Provider
        if (await settingsPage.providerExists('wechat')) {
          await settingsPage.deleteProvider('wechat')
          demoLogger.testCode.log('[Test] WeChat Provider deleted')
        }

        // Logout to test login page
        await logout(page)

        // Navigate to login page
        const loginPageObj = new LoginPage(page, demoLogger)
        await loginPageObj.goto(ADMIN_REALM)

        // Verify WeChat OAuth Provider button is NOT visible
        await expect(page.getByTestId('oauth-login-button-wechat')).not.toBeVisible({ timeout: 2000 })
        demoLogger.testCode.log('[Test] WeChat OAuth login button is NOT visible (provider not configured)')
      })
    })
  })

  // ============================================================================
  // 微信小程序登录 [US-RU-011]
  // ============================================================================

  test.describe('用户故事 US-RU-011：微信小程序登录', () => {
    test('微信小程序登录按钮可见性测试 [US-RU-011]', async ({ page, loginPage, demoLogger, testStartTime }) => {
      // Login as admin to configure WeChat Mini Program providers
      await loginPage.loginAsAdmin(ADMIN_EMAIL, 'password', ADMIN_REALM)

      const settingsPage = new SettingsPage(page, demoLogger, ADMIN_REALM)

      // Phase 1: 验证有已启用 WeChat Mini Program Provider 时显示微信登录按钮（场景 4 - 反向测试）
      await test.step('Phase 1: 验证有已启用 WeChat Mini Program Provider 时显示微信登录按钮', async () => {
        // Navigate to Settings and configure WeChat Mini Program Provider
        await settingsPage.goto()
        await settingsPage.waitForReady()
        await settingsPage.switchToProvidersTab()

        // Add WeChat Mini Program Provider
        const wechatMiniprogramConfig = {
          providerType: 'wechat_miniprogram' as const,
          clientId: `wx1234567890abcdef-miniprogram-${testStartTime}`,
          clientSecret: `abcdef1234567890-miniprogram-${testStartTime}`,
          enabled: true,
        }

        await settingsPage.addProvider(wechatMiniprogramConfig)
        demoLogger.testCode.log('[Test] WeChat Mini Program Provider configured and enabled')

        // Logout to test login page
        await logout(page)

        // Navigate to login page
        const loginPageObj = new LoginPage(page, demoLogger)
        await loginPageObj.goto(ADMIN_REALM)

        // Verify "Or continue with" separator is visible
        await expect(page.getByText(/or continue with/i)).toBeVisible({ timeout: 2000 })
        demoLogger.testCode.log('[Test] "Or continue with" separator is visible')

        // Verify WeChat Mini Program OAuth login button is visible
        // Using data-testid for OAuth login buttons (semantic selectors not available)
        await expect(page.getByTestId('oauth-login-button-wechat_miniprogram')).toBeVisible({ timeout: 2000 })
        demoLogger.testCode.log('[Test] WeChat Mini Program OAuth login button is visible')

        // Verify button uses outline style (by checking variant)
        const wechatMiniprogramButton = page.getByTestId('oauth-login-button-wechat_miniprogram')
        await expect(wechatMiniprogramButton).toHaveClass(/outline/)
        demoLogger.testCode.log('[Test] WeChat Mini Program OAuth button uses outline style')
      })

      // Phase 2: 验证未启用 WeChat Mini Program Provider 时不显示按钮（场景 4）
      await test.step('Phase 2: 验证未启用 WeChat Mini Program Provider 时不显示按钮', async () => {
        // Login as admin again to disable provider
        await loginPage.loginAsAdmin(ADMIN_EMAIL, 'password', ADMIN_REALM)

        // Navigate to Settings
        await settingsPage.goto()
        await settingsPage.waitForReady()
        await settingsPage.switchToProvidersTab()

        // Disable WeChat Mini Program Provider
        await settingsPage.toggleProvider('wechat_miniprogram')
        demoLogger.testCode.log('[Test] WeChat Mini Program Provider disabled')

        // Logout to test login page
        await logout(page)

        // Navigate to login page
        const loginPageObj = new LoginPage(page, demoLogger)
        await loginPageObj.goto(ADMIN_REALM)

        // Verify "Or continue with" separator is NOT visible (if no other providers are enabled)
        // Note: This assumes no other OAuth providers are enabled
        await expect(page.getByText(/or continue with/i)).not.toBeVisible({ timeout: 2000 })
        demoLogger.testCode.log('[Test] "Or continue with" separator is NOT visible')

        // Verify WeChat Mini Program OAuth Provider button is NOT visible
        await expect(page.getByTestId('oauth-login-button-wechat_miniprogram')).not.toBeVisible({ timeout: 2000 })
        demoLogger.testCode.log('[Test] WeChat Mini Program OAuth login button is NOT visible')
      })

      // Phase 3: 验证未配置 WeChat Mini Program Provider 时不显示按钮
      await test.step('Phase 3: 验证未配置 WeChat Mini Program Provider 时不显示按钮', async () => {
        // Login as admin to delete provider
        await loginPage.loginAsAdmin(ADMIN_EMAIL, 'password', ADMIN_REALM)

        // Navigate to Settings
        await settingsPage.goto()
        await settingsPage.waitForReady()
        await settingsPage.switchToProvidersTab()

        // Delete WeChat Mini Program Provider
        if (await settingsPage.providerExists('wechat_miniprogram')) {
          await settingsPage.deleteProvider('wechat_miniprogram')
          demoLogger.testCode.log('[Test] WeChat Mini Program Provider deleted')
        }

        // Logout to test login page
        await logout(page)

        // Navigate to login page
        const loginPageObj = new LoginPage(page, demoLogger)
        await loginPageObj.goto(ADMIN_REALM)

        // Verify WeChat Mini Program OAuth Provider button is NOT visible
        await expect(page.getByTestId('oauth-login-button-wechat_miniprogram')).not.toBeVisible({ timeout: 2000 })
        demoLogger.testCode.log('[Test] WeChat Mini Program OAuth login button is NOT visible (provider not configured)')
      })
    })
  })

  // ============================================================================
  // 多 Provider 场景测试
  // ============================================================================

  test.describe('多 Provider 场景测试', () => {
    test('同时显示多个已启用的 OAuth Provider 按钮', async ({ page, loginPage, demoLogger, testStartTime }) => {
      // Login as admin to configure multiple providers
      await loginPage.loginAsAdmin(ADMIN_EMAIL, 'password', ADMIN_REALM)

      const settingsPage = new SettingsPage(page, demoLogger, ADMIN_REALM)

      // Configure multiple providers
      await test.step('配置 WeChat 和 WeChat Mini Program Provider', async () => {
        await settingsPage.goto()
        await settingsPage.waitForReady()
        await settingsPage.switchToProvidersTab()

        // Add WeChat Provider
        const wechatConfig = {
          providerType: 'wechat' as const,
          clientId: `wx1234567890abcdef-${testStartTime}`,
          clientSecret: `abcdef1234567890-${testStartTime}`,
          scopes: ['snsapi_login'],
          enabled: true,
        }
        await settingsPage.addProvider(wechatConfig)
        demoLogger.testCode.log('[Test] WeChat Provider configured')

        // Add WeChat Mini Program Provider
        const wechatMiniprogramConfig = {
          providerType: 'wechat_miniprogram' as const,
          clientId: `wx1234567890abcdef-miniprogram-${testStartTime}`,
          clientSecret: `abcdef1234567890-miniprogram-${testStartTime}`,
          enabled: true,
        }
        await settingsPage.addProvider(wechatMiniprogramConfig)
        demoLogger.testCode.log('[Test] WeChat Mini Program Provider configured')

        // Add Google Provider for comparison
        const googleConfig = {
          providerType: 'google' as const,
          clientId: `google-client-${testStartTime}`,
          clientSecret: `google-secret-${testStartTime}`,
          scopes: ['https://www.googleapis.com/auth/userinfo.email'],
          enabled: true,
        }
        await settingsPage.addProvider(googleConfig)
        demoLogger.testCode.log('[Test] Google Provider configured')
      })

      // Verify all enabled providers show login buttons
      await test.step('验证所有已启用的 Provider 都显示登录按钮', async () => {
        // Logout to test login page
        await logout(page)

        // Navigate to login page
        const loginPageObj = new LoginPage(page, demoLogger)
        await loginPageObj.goto(ADMIN_REALM)

        // Verify "Or continue with" separator is visible
        await expect(page.getByText(/or continue with/i)).toBeVisible({ timeout: 2000 })
        demoLogger.testCode.log('[Test] "Or continue with" separator is visible')

        // Verify all three OAuth login buttons are visible
        await expect(page.getByTestId('oauth-login-button-wechat')).toBeVisible({ timeout: 2000 })
        demoLogger.testCode.log('[Test] WeChat OAuth login button is visible')

        await expect(page.getByTestId('oauth-login-button-wechat_miniprogram')).toBeVisible({ timeout: 2000 })
        demoLogger.testCode.log('[Test] WeChat Mini Program OAuth login button is visible')

        await expect(page.getByTestId('oauth-login-button-google')).toBeVisible({ timeout: 2000 })
        demoLogger.testCode.log('[Test] Google OAuth login button is visible')
      })

      // Verify disabling one provider hides its button
      await test.step('验证禁用一个 Provider 后其按钮消失', async () => {
        // Login as admin again
        await loginPage.loginAsAdmin(ADMIN_EMAIL, 'password', ADMIN_REALM)

        // Navigate to Settings
        await settingsPage.goto()
        await settingsPage.waitForReady()
        await settingsPage.switchToProvidersTab()

        // Disable WeChat Provider
        await settingsPage.toggleProvider('wechat')
        demoLogger.testCode.log('[Test] WeChat Provider disabled')

        // Logout to test login page
        await logout(page)

        // Navigate to login page
        const loginPageObj = new LoginPage(page, demoLogger)
        await loginPageObj.goto(ADMIN_REALM)

        // Verify WeChat button is NOT visible
        await expect(page.getByTestId('oauth-login-button-wechat')).not.toBeVisible({ timeout: 2000 })
        demoLogger.testCode.log('[Test] WeChat OAuth login button is NOT visible')

        // Verify other providers' buttons are still visible
        await expect(page.getByTestId('oauth-login-button-wechat_miniprogram')).toBeVisible({ timeout: 2000 })
        demoLogger.testCode.log('[Test] WeChat Mini Program OAuth login button is still visible')

        await expect(page.getByTestId('oauth-login-button-google')).toBeVisible({ timeout: 2000 })
        demoLogger.testCode.log('[Test] Google OAuth login button is still visible')
      })
    })
  })

  // ============================================================================
  // Provider 配置验证（UI 级别）
  // ============================================================================

  test.describe('Provider 配置验证（UI 级别）', () => {
    test('验证 WeChat Provider 配置的 UI 表现', async ({ page, loginPage, demoLogger, testStartTime }) => {
      // Login as admin to configure WeChat providers
      await loginPage.loginAsAdmin(ADMIN_EMAIL, 'password', ADMIN_REALM)

      const settingsPage = new SettingsPage(page, demoLogger, ADMIN_REALM)

      await test.step('验证 WeChat Provider 在列表中正确显示', async () => {
        await settingsPage.goto()
        await settingsPage.waitForReady()
        await settingsPage.switchToProvidersTab()

        // Add WeChat Provider
        const wechatConfig = {
          providerType: 'wechat' as const,
          clientId: `wx1234567890abcdef-${testStartTime}`,
          clientSecret: `abcdef1234567890-${testStartTime}`,
          scopes: ['snsapi_login'],
          enabled: true,
        }
        await settingsPage.addProvider(wechatConfig)

        // Verify provider appears in list
        const exists = await settingsPage.providerExists('wechat')
        expect(exists).toBeTruthy()

        // Verify Client ID is displayed correctly
        const clientId = await settingsPage.getClientId('wechat')
        expect(clientId).toBe(`wx1234567890abcdef-${testStartTime}`)

        // Verify status is "Enabled"
        const isEnabled = await settingsPage.getProviderStatus('wechat')
        expect(isEnabled).toBe(true)

        demoLogger.testCode.log('[Test] WeChat Provider configuration verified in UI')
      })

      await test.step('验证 WeChat Mini Program Provider 在列表中正确显示', async () => {
        // Navigate to Settings
        await settingsPage.goto()
        await settingsPage.waitForReady()
        await settingsPage.switchToProvidersTab()

        // Add WeChat Mini Program Provider
        const wechatMiniprogramConfig = {
          providerType: 'wechat_miniprogram' as const,
          clientId: `wx1234567890abcdef-miniprogram-${testStartTime}`,
          clientSecret: `abcdef1234567890-miniprogram-${testStartTime}`,
          enabled: true,
        }
        await settingsPage.addProvider(wechatMiniprogramConfig)

        // Verify provider appears in list
        const exists = await settingsPage.providerExists('wechat_miniprogram')
        expect(exists).toBeTruthy()

        // Verify Client ID is displayed correctly
        const clientId = await settingsPage.getClientId('wechat_miniprogram')
        expect(clientId).toBe(`wx1234567890abcdef-miniprogram-${testStartTime}`)

        // Verify status is "Enabled"
        const isEnabled = await settingsPage.getProviderStatus('wechat_miniprogram')
        expect(isEnabled).toBe(true)

        demoLogger.testCode.log('[Test] WeChat Mini Program Provider configuration verified in UI')
      })
    })
  })
})
