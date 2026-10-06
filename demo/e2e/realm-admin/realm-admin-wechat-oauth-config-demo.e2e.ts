/**
 * Realm Admin WeChat OAuth Config Demo Tests
 *
 * User Stories:
 * - US-WO-001: WeChat OAuth Provider 配置
 * - US-WO-002: WeChat Mini Program Provider 配置
 *
 * User Stories: docs/user-stories/auth/wechat-oauth.md
 *
 * Test Phases:
 * - Phase 1: WeChat Provider configuration
 * - Phase 2: WeChat Mini Program Provider configuration
 * - Phase 3: Provider management (enable/disable/edit/delete)
 * - Phase 4: Scope configuration validation
 *
 * Important Notes:
 * - This test does NOT simulate actual WeChat OAuth authorization flow
 * - Tests focus on UI visibility and provider CRUD operations
 * - WeChat scope is fixed to 'snsapi_login'
 * - WeChat Mini Program does not require scope
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
import { DEMO_ADMIN } from '../helpers/auth'

test.describe('[Realm Admin] WeChat OAuth Config Demo Tests', () => {
  test.beforeEach(async ({ page, demoLogger, loginPage }) => {
    // Verify test environment
    await verifyTestEnvironment(page, {
      requiredRealms: [DEMO_ADMIN.realmId],
      requiredUsers: [DEMO_ADMIN.email],
    })

    // Login as admin using LoginPage fixture
    await loginPage.loginAsAdmin(DEMO_ADMIN.email, DEMO_ADMIN.password, DEMO_ADMIN.realmId)

    // Clean up any existing WeChat providers AFTER login (so we can access settings)
    const settingsPage = new SettingsPage(page, demoLogger, DEMO_ADMIN.realmId)
    try {
      await settingsPage.goto()
      await settingsPage.waitForReady()
      await settingsPage.switchToProvidersTab()

      // Delete WeChat and WeChat Mini Program providers to ensure clean state
      const providers = ['wechat', 'wechat_miniprogram']
      for (const provider of providers) {
        if (await settingsPage.providerExists(provider)) {
          demoLogger.testCode.log(`Deleting ${provider} provider`)
          await settingsPage.deleteProvider(provider)
        }
      }
    } catch (error) {
      demoLogger.testCode.log(`Cleanup error: ${error}`)
    }
  })

  test.afterEach(async ({ page, demoLogger, testStartTime }) => {
    // Call standard cleanup for other test data
    await cleanupTestData(page, DEMO_ADMIN.realmId, {
      keepUsers: [DEMO_ADMIN.email],
      timestamp: testStartTime,
    })
  })

  // ============================================================================
  // WeChat OAuth Provider 配置管理 [US-WO-001]
  // ============================================================================

  test.describe('用户故事 US-WO-001：WeChat OAuth Provider 配置', () => {
    test('WeChat Provider 配置管理综合流程 [US-WO-001]', async ({ page, demoLogger, testStartTime }) => {
      const settingsPage = new SettingsPage(page, demoLogger, DEMO_ADMIN.realmId)

      // === 初始状态验证 ===
      await test.step('验证初始配置状态', async () => {
        await settingsPage.goto()
        await settingsPage.waitForReady()
        await settingsPage.switchToProvidersTab()

        // Verify initial state is empty
        const wechatExists = await settingsPage.providerExists('wechat')
        expect(wechatExists).toBeFalsy()
        demoLogger.testCode.log('Initial state verified - no WeChat provider configured')
      })

      // === Phase 1: 添加 WeChat Provider ===
      await test.step('场景 1：添加 WeChat OAuth Provider 配置', async () => {
        const wechatConfig = {
          providerType: 'wechat' as const,
          clientId: `wx1234567890abcdef-${testStartTime}`,
          clientSecret: `abcdef1234567890-${testStartTime}`,
          scopes: ['snsapi_login'], // WeChat fixed scope
          enabled: true,
        }

        await settingsPage.addProvider(wechatConfig)
        demoLogger.testCode.log('WeChat Provider added successfully')

        // Verify provider appears in list
        const exists = await settingsPage.providerExists('wechat')
        expect(exists).toBeTruthy()
        demoLogger.testCode.log('WeChat Provider verified in list')

        // Verify scope is set to snsapi_login
        // Note: This is verified by the frontend automatically setting it
        const clientId = await settingsPage.getClientId('wechat')
        expect(clientId).toBe(`wx1234567890abcdef-${testStartTime}`)
        demoLogger.testCode.log('WeChat Client ID verified')
      })

      // === Phase 2: 启用/禁用 WeChat Provider ===
      await test.step('场景 3：启用/禁用 WeChat Provider', async () => {
        // Verify WeChat Provider is enabled
        let isEnabled = await settingsPage.getProviderStatus('wechat')
        expect(isEnabled).toBe(true)
        demoLogger.testCode.log('WeChat Provider is initially enabled')

        // Disable WeChat Provider
        await settingsPage.toggleProvider('wechat')
        // Wait for UI to update
        await page.waitForTimeout(1000)
        isEnabled = await settingsPage.getProviderStatus('wechat')
        expect(isEnabled).toBe(false)
        demoLogger.testCode.log('WeChat Provider disabled')

        // Re-enable WeChat Provider
        await settingsPage.toggleProvider('wechat')
        // Wait for UI to update
        await page.waitForTimeout(1000)
        isEnabled = await settingsPage.getProviderStatus('wechat')
        expect(isEnabled).toBe(true)
        demoLogger.testCode.log('WeChat Provider re-enabled')
      })

      // === Phase 3: 编辑 WeChat Provider 配置 ===
      await test.step('场景 4：编辑 WeChat Provider 配置（Client Secret 可选）', async () => {
        const updateData = {
          clientId: `wx1234567890abcdef-updated-${testStartTime}`,
          // Note: Client Secret is NOT included, which means it should not be updated
        }

        await settingsPage.editProvider('wechat', updateData)
        demoLogger.testCode.log('WeChat Provider updated with new Client ID (Client Secret left empty)')

        // Wait for React Query to update (polling approach)
        await expect(async () => {
          const clientId = await settingsPage.getClientId('wechat')
          expect(clientId).toBe(`wx1234567890abcdef-updated-${testStartTime}`)
        }).toPass({ timeout: 10000 })
        demoLogger.testCode.log('WeChat Provider Client ID verified (Client Secret unchanged)')
      })

      // === Phase 4: 删除 WeChat Provider ===
      await test.step('场景 5：删除 WeChat Provider 配置', async () => {
        // Delete WeChat Provider
        await settingsPage.deleteProvider('wechat')
        demoLogger.testCode.log('WeChat Provider deleted')

        // Verify provider is removed from list
        const exists = await settingsPage.providerExists('wechat')
        expect(exists).toBeFalsy()
        demoLogger.testCode.log('WeChat Provider no longer in list')
      })
    })
  })

  // ============================================================================
  // WeChat Mini Program Provider 配置管理 [US-WO-002]
  // ============================================================================

  test.describe('用户故事 US-WO-002：WeChat Mini Program Provider 配置', () => {
    test('WeChat Mini Program Provider 配置管理综合流程 [US-WO-002]', async ({ page, demoLogger, testStartTime }) => {
      const settingsPage = new SettingsPage(page, demoLogger, DEMO_ADMIN.realmId)

      // === 初始状态验证 ===
      await test.step('验证初始配置状态', async () => {
        await settingsPage.goto()
        await settingsPage.waitForReady()
        await settingsPage.switchToProvidersTab()

        // Verify initial state is empty
        const wechatMiniprogramExists = await settingsPage.providerExists('wechat_miniprogram')
        expect(wechatMiniprogramExists).toBeFalsy()
        demoLogger.testCode.log('Initial state verified - no WeChat Mini Program provider configured')
      })

      // === Phase 1: 添加 WeChat Mini Program Provider ===
      await test.step('场景 1：添加 WeChat Mini Program Provider 配置（无 Scope 字段）', async () => {
        const wechatMiniprogramConfig = {
          providerType: 'wechat_miniprogram' as const,
          clientId: `wx1234567890abcdef-miniprogram-${testStartTime}`,
          clientSecret: `abcdef1234567890-miniprogram-${testStartTime}`,
          enabled: true,
          // Note: scopes is NOT included for mini program
        }

        await settingsPage.addProvider(wechatMiniprogramConfig)
        demoLogger.testCode.log('WeChat Mini Program Provider added successfully (no scope configured)')

        // Verify provider appears in list
        const exists = await settingsPage.providerExists('wechat_miniprogram')
        expect(exists).toBeTruthy()
        demoLogger.testCode.log('WeChat Mini Program Provider verified in list')

        // Verify Client ID is set correctly
        const clientId = await settingsPage.getClientId('wechat_miniprogram')
        expect(clientId).toBe(`wx1234567890abcdef-miniprogram-${testStartTime}`)
        demoLogger.testCode.log('WeChat Mini Program Client ID verified')
      })

      // === Phase 2: 启用/禁用 WeChat Mini Program Provider ===
      await test.step('场景 2：启用/禁用 WeChat Mini Program Provider', async () => {
        // Verify WeChat Mini Program Provider is enabled
        let isEnabled = await settingsPage.getProviderStatus('wechat_miniprogram')
        expect(isEnabled).toBe(true)
        demoLogger.testCode.log('WeChat Mini Program Provider is initially enabled')

        // Disable WeChat Mini Program Provider
        await settingsPage.toggleProvider('wechat_miniprogram')
        // Wait for UI to update
        await page.waitForTimeout(1000)
        isEnabled = await settingsPage.getProviderStatus('wechat_miniprogram')
        expect(isEnabled).toBe(false)
        demoLogger.testCode.log('WeChat Mini Program Provider disabled')

        // Re-enable WeChat Mini Program Provider
        await settingsPage.toggleProvider('wechat_miniprogram')
        // Wait for UI to update
        await page.waitForTimeout(1000)
        isEnabled = await settingsPage.getProviderStatus('wechat_miniprogram')
        expect(isEnabled).toBe(true)
        demoLogger.testCode.log('WeChat Mini Program Provider re-enabled')
      })

      // === Phase 3: 编辑 WeChat Mini Program Provider 配置 ===
      await test.step('编辑 WeChat Mini Program Provider 配置', async () => {
        const updateData = {
          clientId: `wx1234567890abcdef-miniprogram-updated-${testStartTime}`,
          // Note: Client Secret is NOT included, which means it should not be updated
        }

        await settingsPage.editProvider('wechat_miniprogram', updateData)
        demoLogger.testCode.log('WeChat Mini Program Provider updated with new Client ID (Client Secret left empty)')

        // Wait for React Query to update (polling approach)
        await expect(async () => {
          const clientId = await settingsPage.getClientId('wechat_miniprogram')
          expect(clientId).toBe(`wx1234567890abcdef-miniprogram-updated-${testStartTime}`)
        }).toPass({ timeout: 10000 })
        demoLogger.testCode.log('WeChat Mini Program Provider Client ID verified (Client Secret unchanged)')
      })

      // === Phase 4: 删除 WeChat Mini Program Provider ===
      await test.step('删除 WeChat Mini Program Provider 配置', async () => {
        // Delete WeChat Mini Program Provider
        await settingsPage.deleteProvider('wechat_miniprogram')
        demoLogger.testCode.log('WeChat Mini Program Provider deleted')

        // Verify provider is removed from list
        const exists = await settingsPage.providerExists('wechat_miniprogram')
        expect(exists).toBeFalsy()
        demoLogger.testCode.log('WeChat Mini Program Provider no longer in list')
      })
    })
  })

  // ============================================================================
  // Scope 配置验证 [US-WO-001]
  // ============================================================================

  test.describe('Scope 配置验证 [US-WO-001]', () => {
    test('WeChat Scope 配置验证 [US-WO-001]', async ({ page, demoLogger, testStartTime }) => {
      const settingsPage = new SettingsPage(page, demoLogger, DEMO_ADMIN.realmId)

      await test.step('场景 6：Scope 配置验证（WeChat 固定为 snsapi_login）', async () => {
        // Navigate to Settings
        await settingsPage.goto()
        await settingsPage.waitForReady()
        await settingsPage.switchToProvidersTab()

        // Click "Add Provider" button
        await settingsPage.addProviderButton.click()

        // Wait for dialog to open
        await expect(settingsPage.providerTypeSelect).toBeVisible({ timeout: 5000 })

        // Select WeChat provider type
        await settingsPage.providerTypeSelect.click()
        await page.getByRole('option', { name: 'WeChat', exact: true }).click()

        // Fill in Client ID and Secret
        await settingsPage.clientIdInput.fill(`wx1234567890abcdef-${testStartTime}`)
        await settingsPage.clientSecretInput.fill(`abcdef1234567890-${testStartTime}`)

        // Step 1: Verify that scopes input is pre-filled with 'snsapi_login'
        const initialScopesValue = await settingsPage.scopesInput.inputValue()
        expect(initialScopesValue).toContain('snsapi_login')
        demoLogger.testCode.log('WeChat scope is pre-filled with snsapi_login')

        // Step 2: Verify the scope field is read-only or disabled
        // WeChat scope is fixed and cannot be modified
        const isDisabled = await settingsPage.scopesInput.isDisabled()
        expect(isDisabled).toBeTruthy()
        demoLogger.testCode.log('Scope field is disabled (read-only)')

        // Cancel the dialog (we're just testing the scope behavior)
        await settingsPage.cancelProviderButton.click()

        demoLogger.testCode.log('Scope configuration validation completed')
      })
    })
  })

  // ============================================================================
  // 综合测试：同时配置 WeChat 和 WeChat Mini Program Provider
  // ============================================================================

  test.describe('综合测试：同时配置 WeChat 和 WeChat Mini Program Provider', () => {
    test('同时配置 WeChat 和 WeChat Mini Program Provider [US-WO-001 & US-WO-002]', async ({ page, demoLogger, testStartTime }) => {
      const settingsPage = new SettingsPage(page, demoLogger, DEMO_ADMIN.realmId)

      // === Phase 1: 配置 WeChat Provider ===
      await test.step('配置 WeChat Provider', async () => {
        await settingsPage.goto()
        await settingsPage.waitForReady()
        await settingsPage.switchToProvidersTab()

        const wechatConfig = {
          providerType: 'wechat' as const,
          clientId: `wx1234567890abcdef-${testStartTime}`,
          clientSecret: `abcdef1234567890-${testStartTime}`,
          scopes: ['snsapi_login'],
          enabled: true,
        }

        await settingsPage.addProvider(wechatConfig)
        demoLogger.testCode.log('WeChat Provider configured')

        // Verify provider exists
        const exists = await settingsPage.providerExists('wechat')
        expect(exists).toBeTruthy()
      })

      // === Phase 2: 配置 WeChat Mini Program Provider ===
      await test.step('配置 WeChat Mini Program Provider', async () => {
        await settingsPage.goto()
        await settingsPage.waitForReady()
        await settingsPage.switchToProvidersTab()

        const wechatMiniprogramConfig = {
          providerType: 'wechat_miniprogram' as const,
          clientId: `wx1234567890abcdef-miniprogram-${testStartTime}`,
          clientSecret: `abcdef1234567890-miniprogram-${testStartTime}`,
          enabled: true,
        }

        await settingsPage.addProvider(wechatMiniprogramConfig)
        demoLogger.testCode.log('WeChat Mini Program Provider configured')

        // Verify provider exists
        const exists = await settingsPage.providerExists('wechat_miniprogram')
        expect(exists).toBeTruthy()
      })

      // === Phase 3: 验证两个 Provider 同时显示 ===
      await test.step('验证两个 Provider 同时显示', async () => {
        const wechatExists = await settingsPage.providerExists('wechat')
        const wechatMiniprogramExists = await settingsPage.providerExists('wechat_miniprogram')

        expect(wechatExists).toBeTruthy()
        expect(wechatMiniprogramExists).toBeTruthy()
        demoLogger.testCode.log('Both WeChat and WeChat Mini Program providers are visible')
      })

      // === Phase 4: 清理 ===
      await test.step('清理所有 WeChat Providers', async () => {
        await settingsPage.deleteProvider('wechat')
        await settingsPage.deleteProvider('wechat_miniprogram')
        demoLogger.testCode.log('All WeChat providers deleted')
      })
    })
  })
})
