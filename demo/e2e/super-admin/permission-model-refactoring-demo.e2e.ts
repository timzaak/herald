/**
 * Permission Model Refactoring Demo E2E Test
 *
 * Test Coverage:
 * - US-BP-001: Default role and permission protection (new permissions present and protected)
 * - US-RA-010: Dashboard user activity overview (requires dashboard.view)
 * - US-RA-012: Dashboard quick navigation (permission filtering)
 *
 * Verifies the permission model refactoring:
 * - New permissions exist: dashboard.view, audit.view, api_keys.view, realm.manage
 * - Legacy permissions absent: realm.create, realm.admin
 * - Built-in permissions are protected (badge present, delete disabled)
 * - Sidebar menus visible under full admin permissions (positive path only)
 * - QuickNav items visible under full admin permissions (positive path only)
 * - Dashboard stats render under dashboard.view
 *
 * 范围声明：本 demo 以全权限 admin 登录，只做「有权限→可见」的正向断言；
 * 「无权限→菜单/QuickNav 隐藏」的过滤负向断言不在本文件覆盖范围，
 * 且当前 demo 体系内无其他文件承接该负向路径（US-RA-012 过滤断言为已知缺口）。
 *
 * @note Uses single browser session pattern (one test with multiple steps)
 */

import { test, cleanupTestData, expect } from '../fixtures/demo-page.fixtures'
import { SELECTORS } from '../selectors'
import { DEMO_ADMIN, logout } from '../helpers/auth'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { PermissionsPage } from '../pages/permissions-page'
import { DashboardPage } from '../pages/dashboard-page'

/**
 * Permission Model Refactoring Verification
 *
 * Single browser session with multiple steps covering the
 * permission model refactoring from legacy (realm.admin, realm.create)
 * to new granular model (dashboard.view, audit.view, api_keys.view, realm.manage).
 */
test.describe('Permission Model Refactoring', () => {
  test.beforeEach(async ({ page }) => {
    await verifyTestEnvironment(page, {
      requiredRealms: ['admin'],
      requiredUsers: [DEMO_ADMIN.email],
    })
  })

  test.afterEach(async ({ page, testStartTime }) => {
    await cleanupTestData(page, 'admin', {
      keepUsers: [DEMO_ADMIN.email],
      timestamp: testStartTime,
    })
  })

  test('Permission Model Refactoring Verification', async ({ loginPage, demoLogger, testStartTime }) => {
    const page = loginPage.page

    // ==========================================================================
    // Prerequisite: Login as admin
    // ==========================================================================
    await test.step('Login as admin', async () => {
      await loginPage.loginAsAdmin(DEMO_ADMIN.email, DEMO_ADMIN.password, DEMO_ADMIN.realmId)
      demoLogger.testCode.log('Logged in as admin@cas.com')
    })

    // ==========================================================================
    // Step 1: Verify new permissions exist in permissions management page (US-BP-001)
    // ==========================================================================
    await test.step('Step 1: Verify new permissions exist in permissions management page', async () => {
      const permissionsPage = new PermissionsPage(page, demoLogger)
      await permissionsPage.goto()

      await test.step('dashboard.view permission exists', async () => {
        const exists = await permissionsPage.permissionExists('dashboard.view')
        expect(exists).toBe(true)
        demoLogger.testCode.log('dashboard.view permission found')
      })

      await test.step('audit.view permission exists', async () => {
        const exists = await permissionsPage.permissionExists('audit.view')
        expect(exists).toBe(true)
        demoLogger.testCode.log('audit.view permission found')
      })

      await test.step('api_keys.view permission exists', async () => {
        const exists = await permissionsPage.permissionExists('api_keys.view')
        expect(exists).toBe(true)
        demoLogger.testCode.log('api_keys.view permission found')
      })

      await test.step('realm.manage permission exists', async () => {
        const exists = await permissionsPage.permissionExists('realm.manage')
        expect(exists).toBe(true)
        demoLogger.testCode.log('realm.manage permission found')
      })

      await test.step('Legacy realm.create does NOT exist', async () => {
        const exists = await permissionsPage.permissionExists('realm.create')
        expect(exists).toBe(false)
        demoLogger.testCode.log('realm.create correctly absent')
      })

      await test.step('Legacy realm.admin does NOT exist', async () => {
        const exists = await permissionsPage.permissionExists('realm.admin')
        expect(exists).toBe(false)
        demoLogger.testCode.log('realm.admin correctly absent')
      })

      await test.step('Built-in badge present for new permissions', async () => {
        const dashboardBadge = await permissionsPage.hasBuiltInBadge('dashboard.view')
        expect(dashboardBadge).toBe(true)

        const auditBadge = await permissionsPage.hasBuiltInBadge('audit.view')
        expect(auditBadge).toBe(true)

        const apiKeysBadge = await permissionsPage.hasBuiltInBadge('api_keys.view')
        expect(apiKeysBadge).toBe(true)

        const realmBadge = await permissionsPage.hasBuiltInBadge('realm.manage')
        expect(realmBadge).toBe(true)

        demoLogger.testCode.log('All new permissions have built-in badges')
      })

      await test.step('Delete button disabled for built-in permissions', async () => {
        const dashboardDeleteDisabled = await permissionsPage.isDeleteButtonDisabled('dashboard.view')
        expect(dashboardDeleteDisabled).toBe(true)

        const auditDeleteDisabled = await permissionsPage.isDeleteButtonDisabled('audit.view')
        expect(auditDeleteDisabled).toBe(true)

        const apiKeysDeleteDisabled = await permissionsPage.isDeleteButtonDisabled('api_keys.view')
        expect(apiKeysDeleteDisabled).toBe(true)

        const realmDeleteDisabled = await permissionsPage.isDeleteButtonDisabled('realm.manage')
        expect(realmDeleteDisabled).toBe(true)

        demoLogger.testCode.log('Delete buttons correctly disabled for built-in permissions')
      })
    })

    // ==========================================================================
    // Step 2: Verify sidebar shows all menus for admin user (US-RA-010, US-BP-001)
    // ==========================================================================
    await test.step('Step 2: Verify sidebar shows all menus for admin user', async () => {
      const sidebar = page.locator(SELECTORS.sidebar.container)
      await expect(sidebar).toBeVisible()

      await test.step('Dashboard menu visible (requires dashboard.view)', async () => {
        await expect(page.locator(SELECTORS.sidebar.menuDashboard)).toBeVisible()
        demoLogger.testCode.log('Dashboard menu visible')
      })

      await test.step('API Keys menu visible (requires api_keys.view)', async () => {
        await expect(page.locator(SELECTORS.sidebar.menuApiKeys)).toBeVisible()
        demoLogger.testCode.log('API Keys menu visible')
      })

      await test.step('Audit Log menu visible (requires audit.view)', async () => {
        await expect(page.locator(SELECTORS.sidebar.menuAuditLog)).toBeVisible()
        demoLogger.testCode.log('Audit Log menu visible')
      })

      await test.step('Settings menu visible (requires settings.view)', async () => {
        await expect(page.locator(SELECTORS.sidebar.menuSettings)).toBeVisible()
        demoLogger.testCode.log('Settings menu visible')
      })

      await test.step('Users menu visible', async () => {
        await expect(page.locator(SELECTORS.sidebar.menuUsers)).toBeVisible()
        demoLogger.testCode.log('Users menu visible')
      })

      await test.step('Authorization group expandable with Roles/Permissions', async () => {
        const authMenu = page.locator(SELECTORS.sidebar.menuAuthorization)
        await expect(authMenu).toBeVisible()

        // Only click to expand if children are not already visible
        const rolesVisible = await page.locator(SELECTORS.sidebar.menuRoles).isVisible().catch(() => false)
        if (!rolesVisible) {
          await authMenu.click()
          await page.waitForTimeout(300)
        }

        await expect(page.locator(SELECTORS.sidebar.menuRoles)).toBeVisible()
        await expect(page.locator(SELECTORS.sidebar.menuPermissions)).toBeVisible()
        demoLogger.testCode.log('Authorization group expanded with Roles and Permissions visible')
      })
    })

    // ==========================================================================
    // Step 3: Verify QuickNav shows expected items for admin user (US-RA-012)
    // ==========================================================================
    await test.step('Step 3: Verify QuickNav shows expected items for admin user', async () => {
      // Navigate to dashboard via sidebar
      const dashboardPage = new DashboardPage(page, demoLogger)
      await dashboardPage.goto()

      await test.step('QuickNav container visible on dashboard', async () => {
        await expect(page.locator(SELECTORS.dashboard.quickNav)).toBeVisible()
        demoLogger.testCode.log('QuickNav container visible')
      })

      await test.step('Users quick nav card visible', async () => {
        await expect(page.locator(SELECTORS.dashboard.quickNavUsers)).toBeVisible()
        demoLogger.testCode.log('Users card visible')
      })

      await test.step('Roles quick nav card visible', async () => {
        await expect(page.locator(SELECTORS.dashboard.quickNavRoles)).toBeVisible()
        demoLogger.testCode.log('Roles card visible')
      })

      await test.step('Permissions quick nav card visible', async () => {
        await expect(page.locator(SELECTORS.dashboard.quickNavPermissions)).toBeVisible()
        demoLogger.testCode.log('Permissions card visible')
      })

      await test.step('Client Apps quick nav card visible', async () => {
        await expect(page.locator(SELECTORS.dashboard.quickNavClientApps)).toBeVisible()
        demoLogger.testCode.log('Client Apps card visible')
      })

      await test.step('Realms quick nav card visible (admin realm user has realm.view)', async () => {
        await expect(page.locator(SELECTORS.dashboard.quickNavRealms)).toBeVisible()
        demoLogger.testCode.log('Realms card visible')
      })

      await test.step('Settings quick nav card visible', async () => {
        await expect(page.locator(SELECTORS.dashboard.quickNavSettings)).toBeVisible()
        demoLogger.testCode.log('Settings card visible')
      })
    })

    // ==========================================================================
    // Step 4: Verify dashboard stats load (US-RA-010)
    // ==========================================================================
    await test.step('Step 4: Verify dashboard stats load', async () => {
      const dashboardPage = new DashboardPage(page, demoLogger)
      // Already on dashboard from Step 3

      await test.step('Stats row visible (total users, new users, active users)', async () => {
        await expect(page.locator(SELECTORS.dashboard.statsRow)).toBeVisible()
        demoLogger.testCode.log('Stats row visible')
      })

      await test.step('Auth trend chart area visible', async () => {
        await expect(page.locator(SELECTORS.dashboard.authTrendChart)).toBeVisible()
        demoLogger.testCode.log('Auth trend chart area visible')
      })
    })
  })
})
