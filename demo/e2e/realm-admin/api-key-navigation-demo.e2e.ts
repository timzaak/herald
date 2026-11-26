/**
 * API Key Navigation and Edge Cases Demo Tests
 *
 * User Stories:
 * - Sidebar Navigation to API Keys
 * - Cancel Create Flow
 * - Cancel Edit Flow
 * - Delete Cancel (Dismiss Dialog)
 * - Back Button Navigation (Create Page)
 * - Back Button Navigation (Reveal Page)
 * - List Page Empty State
 *
 * Test Structure:
 * 1. Sidebar Navigation to API Keys
 * 2. Cancel Create Flow
 * 3. Cancel Edit Flow
 * 4. Delete Cancel (Dismiss Dialog)
 * 5. Back Button Navigation (Create Page)
 * 6. Back Button Navigation (Reveal Page)
 * 7. List Page Empty State
 */

import { test, cleanupTestData, expect } from '../fixtures/demo-page.fixtures'
import { ApiKeysPage } from '../pages/api-keys-page'
import { SELECTORS } from '../selectors'
import { DEMO_ADMIN } from '../helpers/auth'
import { verifyTestEnvironment } from '../helpers/environment-setup'

test.describe('[Realm Admin] API Key Navigation and Edge Cases Demo Tests', () => {
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
  // Test 1: Sidebar Navigation to API Keys
  // ============================================================================
  test('Sidebar Navigation to API Keys', async ({ page, loginPage, demoLogger, testStartTime }) => {
    await test.step('Given: Admin is logged in and on dashboard', async () => {
      await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')
      console.log('Admin logged in')
    })

    await test.step('When: Ensure Authorization sidebar group is expanded', async () => {
      const apiKeysMenu = page.locator(SELECTORS.sidebar.menuApiKeys)
      const isVisible = await expect(apiKeysMenu).toBeVisible({ timeout: 5000 }).then(() => true).catch(() => false)
      if (!isVisible) {
        const authorizationGroup = page.locator(SELECTORS.sidebar.menuAuthorization)
        await authorizationGroup.click()
        console.log('Clicked Authorization sidebar group to expand')
      } else {
        console.log('Authorization sidebar group already expanded')
      }
    })

    await test.step('Then: Verify API Keys menu item is visible', async () => {
      const apiKeysMenu = page.locator(SELECTORS.sidebar.menuApiKeys)
      await expect(apiKeysMenu).toBeVisible()
      console.log('API Keys menu item is visible')
    })

    await test.step('When: Click API Keys menu item', async () => {
      const apiKeysMenu = page.locator(SELECTORS.sidebar.menuApiKeys)
      await apiKeysMenu.click()
      console.log('Clicked API Keys menu item')
    })

    await test.step('Then: Verify navigated to API Keys page', async () => {
      await expect(page).toHaveURL(/\/manage\/api-keys$/)
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await expect(apiKeyPage.heading).toBeVisible()
      await expect(apiKeyPage.heading).toHaveText('API Keys')
      console.log('Navigated to /manage/api-keys with heading "API Keys"')
    })
  })

  // ============================================================================
  // Test 2: Cancel Create Flow
  // ============================================================================
  test('Cancel Create Flow', async ({ page, loginPage, demoLogger, testStartTime }) => {
    const cancelName = `Cancel Test Key ${testStartTime}`

    await test.step('Given: Admin is logged in', async () => {
      await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')
      console.log('Admin logged in')
    })

    await test.step('When: Navigate to create page and fill form', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.goto()
      await apiKeyPage.gotoCreatePage()

      await test.step('Verify title is "Create API Key"', async () => {
        await expect(apiKeyPage.pageTitle).toHaveText('Create API Key')
        console.log('Create page title verified')
      })

      await test.step('Fill name with cancel test key', async () => {
        await apiKeyPage.fillCreateForm({ name: cancelName })
        console.log(`Filled name: "${cancelName}"`)
      })
    })

    await test.step('When: Click Cancel button', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.cancelForm()
      console.log('Clicked Cancel button')
    })

    await test.step('Then: Verify back on list page and no key was created', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await expect(apiKeyPage.container).toBeVisible()
      console.log('Back on list page')

      const exists = await apiKeyPage.apiKeyExists(cancelName)
      expect(exists).toBeFalsy()
      console.log(`Verified no key named "${cancelName}" was created`)
    })
  })

  // ============================================================================
  // Test 3: Cancel Edit Flow
  // ============================================================================
  test('Cancel Edit Flow', async ({ page, loginPage, demoLogger, testStartTime }) => {
    const originalName = `Edit Cancel Key ${testStartTime}`
    const modifiedName = `Should Not Persist ${testStartTime}`

    await test.step('Given: Admin is logged in', async () => {
      await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')
      console.log('Admin logged in')
    })

    await test.step('And: Create a key for edit cancel test', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.goto()
      await apiKeyPage.gotoCreatePage()
      await apiKeyPage.fillCreateForm({ name: originalName })
      await apiKeyPage.submitForm()

      // Complete the reveal flow
      await apiKeyPage.waitForRevealPage()
      await apiKeyPage.smartClick(apiKeyPage.doneButton)
      await expect(apiKeyPage.container).toBeVisible({ timeout: 5000 })
      console.log(`Created key "${originalName}" and returned to list`)
    })

    await test.step('When: Navigate to edit page and modify name', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)

      await test.step('Navigate to edit page', async () => {
        await apiKeyPage.goto()
        await apiKeyPage.gotoEditPage(originalName)
        await expect(apiKeyPage.pageTitle).toHaveText('Edit API Key')
        console.log('Edit page opened with title "Edit API Key"')
      })

      await test.step('Clear name and fill modified name', async () => {
        await apiKeyPage.nameInput.clear()
        // The edit form has no client-app selector (immutable after creation),
        // so this is the edit-form filler, not fillCreateForm.
        await apiKeyPage.fillEditForm({ name: modifiedName })
        console.log(`Cleared name and filled: "${modifiedName}"`)
      })
    })

    await test.step('When: Click Cancel', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.cancelForm()
      console.log('Clicked Cancel on edit form')
    })

    await test.step('Then: Verify original name still exists and modified name does NOT', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)

      const originalExists = await apiKeyPage.apiKeyExists(originalName)
      expect(originalExists).toBeTruthy()
      console.log(`Original name "${originalName}" still exists`)

      const modifiedExists = await apiKeyPage.apiKeyExists(modifiedName)
      expect(modifiedExists).toBeFalsy()
      console.log(`Modified name "${modifiedName}" does NOT exist`)
    })

    await test.step('Cleanup: Delete the key', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.deleteApiKeyByName(originalName)
      const exists = await apiKeyPage.apiKeyExists(originalName)
      expect(exists).toBeFalsy()
      console.log(`Deleted key "${originalName}"`)
    })
  })

  // ============================================================================
  // Test 4: Delete Cancel (Dismiss Dialog)
  // ============================================================================
  test('Delete Cancel (Dismiss Dialog)', async ({ page, loginPage, demoLogger, testStartTime }) => {
    const keyName = `Delete Cancel Key ${testStartTime}`

    await test.step('Given: Admin is logged in', async () => {
      await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')
      console.log('Admin logged in')
    })

    await test.step('And: Create a key for delete cancel test', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.goto()
      await apiKeyPage.gotoCreatePage()
      await apiKeyPage.fillCreateForm({ name: keyName })
      await apiKeyPage.submitForm()

      // Complete the reveal flow
      await apiKeyPage.waitForRevealPage()
      await apiKeyPage.smartClick(apiKeyPage.doneButton)
      await expect(apiKeyPage.container).toBeVisible({ timeout: 5000 })
      console.log(`Created key "${keyName}" and returned to list`)
    })

    await test.step('When: Click delete button on the row', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.goto()
      const row = await apiKeyPage.findRowByName(keyName)
      expect(row).not.toBeNull()

      const deleteBtn = row!.locator(SELECTORS.apiKeys.deleteButton)
      await apiKeyPage.smartClick(deleteBtn)
      console.log('Clicked delete button')
    })

    await test.step('Then: Verify delete dialog appears', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await expect(apiKeyPage.deleteDialog).toBeVisible({ timeout: 10000 })
      console.log('Delete dialog is visible')
    })

    await test.step('When: Click Cancel in dialog to dismiss', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      // Use the cancel button directly since dialog is already open
      await apiKeyPage.smartClick(apiKeyPage.deleteCancelButton)
      console.log('Clicked Cancel in delete dialog')
    })

    await test.step('Then: Verify dialog closes and key still exists', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)

      await expect(apiKeyPage.deleteDialog).toBeHidden({ timeout: 5000 })
      console.log('Delete dialog is closed')

      const exists = await apiKeyPage.apiKeyExists(keyName)
      expect(exists).toBeTruthy()
      console.log(`Key "${keyName}" still exists after canceling delete`)
    })

    await test.step('Cleanup: Delete the key for real', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.deleteApiKeyByName(keyName)
      const exists = await apiKeyPage.apiKeyExists(keyName)
      expect(exists).toBeFalsy()
      console.log(`Deleted key "${keyName}" for cleanup`)
    })
  })

  // ============================================================================
  // Test 5: Back Button Navigation (Create Page)
  // ============================================================================
  test('Back Button Navigation (Create Page)', async ({ page, loginPage, demoLogger, testStartTime }) => {
    await test.step('Given: Admin is logged in', async () => {
      await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')
      console.log('Admin logged in')
    })

    await test.step('When: Navigate to create page', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.goto()
      await apiKeyPage.gotoCreatePage()
      await expect(apiKeyPage.pageTitle).toHaveText('Create API Key')
      console.log('Create page opened')
    })

    await test.step('When: Click back button (ArrowLeft)', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.smartClick(apiKeyPage.formBackButton)
      console.log('Clicked back button on create page')
    })

    await test.step('Then: Verify navigated to list page', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await expect(apiKeyPage.container).toBeVisible({ timeout: 5000 })
      console.log('Navigated back to list page')
    })
  })

  // ============================================================================
  // Test 6: Back Button Navigation (Reveal Page)
  // ============================================================================
  test('Back Button Navigation (Reveal Page)', async ({ page, loginPage, demoLogger, testStartTime }) => {
    const keyName = `Reveal Back Key ${testStartTime}`

    await test.step('Given: Admin is logged in', async () => {
      await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')
      console.log('Admin logged in')
    })

    await test.step('When: Create key via form and land on reveal page', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.goto()
      await apiKeyPage.gotoCreatePage()
      await apiKeyPage.fillCreateForm({ name: keyName })
      await apiKeyPage.submitForm()

      // Wait for reveal page
      await apiKeyPage.waitForRevealPage()
      console.log(`Created key "${keyName}" and landed on reveal page`)
    })

    await test.step('When: Click back button on reveal page', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.smartClick(apiKeyPage.revealBackButton)
      console.log('Clicked back button on reveal page')
    })

    await test.step('Then: Verify navigated to list page', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await expect(apiKeyPage.container).toBeVisible({ timeout: 5000 })
      console.log('Navigated back to list page')
    })

    await test.step('Cleanup: Delete the key', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.deleteApiKeyByName(keyName)
      const exists = await apiKeyPage.apiKeyExists(keyName)
      expect(exists).toBeFalsy()
      console.log(`Deleted key "${keyName}"`)
    })
  })

  // ============================================================================
  // Test 7: List Page Empty State
  // ============================================================================
  test('List Page Empty State', async ({ page, loginPage, demoLogger, testStartTime }) => {
    await test.step('Given: Admin is logged in', async () => {
      await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')
      console.log('Admin logged in')
    })

    await test.step('When: Navigate to API Keys page', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      await apiKeyPage.goto()
      console.log('Navigated to API Keys page')
    })

    await test.step('Then: Verify table loads without error', async () => {
      const apiKeyPage = new ApiKeysPage(page, demoLogger)
      // Loading, error, empty and the populated table all carry the same
      // testid (shared DataTable), so first wait for the list query to settle:
      // the empty message or at least one data row must be rendered. An error
      // state matches neither and fails here, as intended.
      const emptyMessage = apiKeyPage.table.filter({ hasText: 'No API keys found' })
      const firstRow = apiKeyPage.table.locator('tbody tr').first()
      await expect(emptyMessage.or(firstRow)).toBeVisible({ timeout: 10000 })
      console.log('Table is visible (no error state)')

      // Check if table has rows or shows empty state
      const tableText = await apiKeyPage.table.textContent()
      if (tableText?.includes('No API keys found')) {
        // Empty state
        expect(tableText).toContain('No API keys found. Create your first API key to get started.')
        console.log('Empty state message displayed')
      } else {
        // Table has data (seed data or other keys)
        const rows = apiKeyPage.table.locator('tbody tr')
        const rowCount = await rows.count()
        expect(rowCount).toBeGreaterThan(0)
        console.log(`Table has ${rowCount} row(s) of data`)
      }
    })
  })
})
