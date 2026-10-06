/**
 * API Key Management Demo Tests
 *
 * 相关用户故事（同页面的在册故事，本 demo 未逐条断言其验收场景）:
 * - docs/user-stories/core/realm-admin.md:
 *   - US-RA-016: API Key 角色管理
 *   - US-RA-017: 创建 API Key 时绑定角色
 *   - US-RA-018: API Key 按 Client App 隔离
 *
 * 范围声明：本 demo 覆盖的是 API Key 页面的通用 CRUD 生命周期、过期时间与
 * 启用/禁用切换——这些行为未编入在册用户故事；上述三条故事的验收场景
 * （角色分配/创建时绑定/Client App 隔离）不在本 demo 断言范围内。
 * @see ../../../docs/user-stories/core/realm-admin.md
 *
 * Test Structure:
 * 1. Complete API Key Lifecycle (Create -> Reveal -> Copy -> Done -> Edit -> Delete)
 * 2. API Key with Expiration
 * 3. Toggle Enabled Status from List
 */

import { test, cleanupTestData, expect } from '../fixtures/demo-page.fixtures'
import { ApiKeysPage } from '../pages/api-keys-page'
import { DEMO_ADMIN } from '../helpers/auth'
import { verifyTestEnvironment } from '../helpers/environment-setup'

test.describe('[Realm Admin] API Key Management Demo Tests', () => {
  test.beforeEach(async ({ page }) => {
    await verifyTestEnvironment(page, {
      requiredRealms: [DEMO_ADMIN.realmId],
      requiredUsers: [DEMO_ADMIN.email],
    })
  })

  test.afterEach(async ({ page, testStartTime }) => {
    await cleanupTestData(page, DEMO_ADMIN.realmId, {
      keepUsers: [DEMO_ADMIN.email],
      timestamp: testStartTime,
    })
  })

  // ============================================================================
  // Test 1: Complete API Key Lifecycle (Create -> Reveal -> Copy -> Done -> Edit -> Delete)
  // ============================================================================
  test('Complete API Key Lifecycle', async ({ page, loginPage, demoLogger, testStartTime }) => {
    const originalName = `Lifecycle Key ${testStartTime}`
    const updatedName = `Updated Lifecycle Key ${testStartTime}`

    await test.step('Given: Admin is logged in', async () => {
      await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')
      console.log('Admin logged in')
    })

    await test.step('When: Create API Key', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)

      await test.step('Step 1: Navigate to API Keys page and open create form', async () => {
        await apiKeyPage.goto()
        await apiKeyPage.gotoCreatePage()
        await expect(apiKeyPage.pageTitle).toHaveText('Create API Key')
        console.log('Create form page opened')
      })

      await test.step('Step 2: Fill and submit create form', async () => {
        await apiKeyPage.fillCreateForm({ name: originalName })
        await apiKeyPage.submitForm()
        console.log('API Key creation submitted')
      })
    })

    await test.step('When: Verify reveal page', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)

      await test.step('Step 1: Wait for reveal page', async () => {
        await apiKeyPage.waitForRevealPage()
        console.log('Reveal page visible')
      })

      await test.step('Step 2: Get revealed key value', async () => {
        const keyValue = await apiKeyPage.getRevealedKeyValue()
        expect(keyValue.length).toBeGreaterThan(0)
        console.log('Revealed API key value is non-empty')
      })

      await test.step('Step 3: Copy key and verify button shows Copied', async () => {
        await apiKeyPage.smartClick(apiKeyPage.copyButton)
        await expect(apiKeyPage.copyButton).toContainText('Copied')
        console.log('Copy button shows "Copied"')
      })

      await test.step('Step 4: Click Done and verify list page', async () => {
        await apiKeyPage.smartClick(apiKeyPage.doneButton)
        await expect(apiKeyPage.container).toBeVisible({ timeout: 5000 })
        console.log('Returned to list page')
      })
    })

    await test.step('Then: Verify key appears in list', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.waitForApiKeyByName(originalName)
      console.log(`API Key "${originalName}" verified in list`)
    })

    await test.step('When: Edit API Key', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)

      await test.step('Step 1: Navigate to edit page', async () => {
        await apiKeyPage.goto()
        await apiKeyPage.gotoEditPage(originalName)
        await expect(apiKeyPage.pageTitle).toHaveText('Edit API Key')
        console.log('Edit form page opened')
      })

      await test.step('Step 2: Verify name is pre-populated', async () => {
        const currentName = await apiKeyPage.nameInput.inputValue()
        expect(currentName).toBe(originalName)
        console.log('Name is pre-populated with original value')
      })

      await test.step('Step 3: Fill edit form with updated name and disabled', async () => {
        await apiKeyPage.fillEditForm({ name: updatedName, enabled: false })
        await apiKeyPage.submitForm()
        console.log('Edit form submitted')
      })
    })

    await test.step('Then: Verify edited key in list', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.goto()
      await apiKeyPage.waitForApiKeyByName(updatedName)

      // Verify "Disabled" badge
      const row = await apiKeyPage.findRowByName(updatedName)
      expect(row).not.toBeNull()
      const badge = row!.locator('[data-testid="api-key-status-badge"]')
      await expect(badge).toHaveText('Disabled')
      console.log(`API Key "${updatedName}" shows Disabled badge`)
    })

    await test.step('When: Delete API Key', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.deleteApiKeyByName(updatedName)
      console.log(`Delete action completed for "${updatedName}"`)
    })

    await test.step('Then: Verify key is deleted', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      const exists = await apiKeyPage.apiKeyExists(updatedName)
      expect(exists).toBeFalsy()
      console.log(`API Key "${updatedName}" deleted successfully`)
    })
  })

  // ============================================================================
  // Test 2: API Key with Expiration
  // ============================================================================
  test('API Key with Expiration', async ({ page, loginPage, demoLogger, testStartTime }) => {
    const keyName = `Expiry Key ${testStartTime}`

    await test.step('Given: Admin is logged in and on API Keys page', async () => {
      await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.goto()
      console.log('Admin logged in and navigated to API Keys page')
    })

    await test.step('When: Create API Key with expiration date', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.gotoCreatePage()
      await apiKeyPage.fillCreateForm({
        name: keyName,
        expiresAt: '2026-12-31T23:59',
      })
      await apiKeyPage.submitForm()
      console.log('API Key with expiration submitted')
    })

    await test.step('When: Verify reveal page and return to list', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.waitForRevealPage()

      const keyValue = await apiKeyPage.getRevealedKeyValue()
      expect(keyValue.length).toBeGreaterThan(0)
      console.log('Revealed key value verified')

      // Click Done to return to list
      await apiKeyPage.smartClick(apiKeyPage.doneButton)
      await expect(apiKeyPage.container).toBeVisible({ timeout: 5000 })
      console.log('Returned to list page')
    })

    await test.step('Then: Verify key appears in list with expiration date', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.waitForApiKeyByName(keyName)

      // Verify "Expires" column shows a formatted date (not "Never")
      const row = await apiKeyPage.findRowByName(keyName)
      expect(row).not.toBeNull()
      const expiresCell = row!.locator('[data-testid="api-key-expires"]')
      const expiresText = await expiresCell.textContent()
      expect(expiresText).not.toBe('Never')
      expect(expiresText!.length).toBeGreaterThan(0)
      console.log(`API Key "${keyName}" has expiration date: ${expiresText}`)
    })

    await test.step('Cleanup: Delete API Key', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.deleteApiKeyByName(keyName)
      const exists = await apiKeyPage.apiKeyExists(keyName)
      expect(exists).toBeFalsy()
      console.log(`API Key "${keyName}" deleted`)
    })
  })

  // ============================================================================
  // Test 3: Toggle Enabled Status from List
  // ============================================================================
  test('Toggle Enabled Status from List', async ({ page, loginPage, demoLogger, testStartTime }) => {
    const keyName = `Toggle Key ${testStartTime}`

    await test.step('Given: Admin is logged in', async () => {
      await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')
      console.log('Admin logged in')
    })

    await test.step('When: Create API Key and return to list', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.goto()
      await apiKeyPage.gotoCreatePage()
      await apiKeyPage.fillCreateForm({ name: keyName })
      await apiKeyPage.submitForm()

      // Wait for reveal page and click Done
      await apiKeyPage.waitForRevealPage()
      await apiKeyPage.smartClick(apiKeyPage.doneButton)
      await expect(apiKeyPage.container).toBeVisible({ timeout: 5000 })
      console.log('API Key created and returned to list')
    })

    await test.step('Then: Verify Enabled badge in list', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.waitForApiKeyByName(keyName)

      const row = await apiKeyPage.findRowByName(keyName)
      expect(row).not.toBeNull()
      const badge = row!.locator('[data-testid="api-key-status-badge"]')
      await expect(badge).toHaveText('Enabled')
      console.log(`API Key "${keyName}" shows Enabled badge`)
    })

    await test.step('When: Click enabled switch to disable', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)

      const row = await apiKeyPage.findRowByName(keyName)
      expect(row).not.toBeNull()

      const toggleSwitch = row!.locator('[data-testid="api-key-enabled-switch"]')
      await apiKeyPage.smartClick(toggleSwitch)
      console.log('Clicked enabled switch to disable')
    })

    await test.step('Then: Verify badge changes to Disabled', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      const row = await apiKeyPage.findRowByName(keyName)
      expect(row).not.toBeNull()
      const badge = row!.locator('[data-testid="api-key-status-badge"]')
      await expect(badge).toHaveText('Disabled')
      console.log('Badge changed to Disabled')
    })

    await test.step('When: Click switch again to re-enable', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      const row = await apiKeyPage.findRowByName(keyName)
      expect(row).not.toBeNull()

      const toggleSwitch = row!.locator('[data-testid="api-key-enabled-switch"]')
      await apiKeyPage.smartClick(toggleSwitch)
      console.log('Clicked enabled switch to re-enable')
    })

    await test.step('Then: Verify badge changes back to Enabled', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)

      const row = await apiKeyPage.findRowByName(keyName)
      expect(row).not.toBeNull()
      const badge = row!.locator('[data-testid="api-key-status-badge"]')
      await expect(badge).toHaveText('Enabled')
      console.log('Badge changed back to Enabled')
    })

    await test.step('Cleanup: Delete API Key', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.deleteApiKeyByName(keyName)
      const exists = await apiKeyPage.apiKeyExists(keyName)
      expect(exists).toBeFalsy()
      console.log(`API Key "${keyName}" deleted`)
    })
  })
})
