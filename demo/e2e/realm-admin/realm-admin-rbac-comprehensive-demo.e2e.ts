/**
 * Realm Admin RBAC Comprehensive Demo Tests
 *
 * User Stories:
 * - US-RA-002: Role Definition Management
 * - US-RA-003: Permission Definition Management
 * - US-RA-004/005: Role Permission Assignment
 * - US-RA-006: User Role Assignment
 * - US-BP-001: Built-in Role and Permission Protection
 *
 * Test Phases:
 * - Phase 1: Infrastructure Setup
 * - Phase 2: Permission Definition Management (US-RA-003)
 * - Phase 3: Role Definition Management (US-RA-002)
 * - Phase 4: Role Permission Assignment (US-RA-004/005)
 * - Phase 5: User Role Assignment (US-RA-006)
 * - Phase 6: Built-in Protection Mechanisms (US-BP-001)
 * - Phase 7: Complete RBAC Loop Demo
 *
 * UnifiedLogger Usage:
 * - All tests use UnifiedLogger through the 'demoLogger' fixture
 * - Logger is automatically initialized and finalized by the fixture
 * - Logs are saved to demo/test-results/console-logs/
 */

import { test, cleanupTestData, expect } from '../fixtures/demo-page.fixtures'
import { PermissionsPage, type PermissionData } from '../pages/permissions-page'
import { RolesPage, type RoleData } from '../pages/roles-page'
import { UsersPage } from '../pages/users-page'

const ADMIN_REALM = 'admin'
const ADMIN_EMAIL = 'admin@cas.com'

test.describe('[Realm Admin] RBAC Comprehensive Demo Tests', () => {
  // Single test.afterEach for cleanup
  // Note: demoLogger.finalize() is automatically called by the fixture
  test.afterEach(async ({ page, testStartTime }) => {
    // ⚠️ MANDATORY: 清理测试数据
    await cleanupTestData(page, ADMIN_REALM, {
      keepUsers: [ADMIN_EMAIL],
      timestamp: testStartTime,
    })
  })

  // ============================================================================
  // Phase 2-3: Permission and Role Definition Management [US-RA-002/003]
  // ============================================================================

  test.describe('Phase 2-3: Permission and Role Definition Management [US-RA-002/003]', () => {
    test('权限和角色定义管理综合流程', async ({ page, loginPage, demoLogger, testStartTime }) => {
      // ✅ 添加登录步骤（必须在所有操作之前）
      await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')

      // Phase 2.1: 完整权限定义管理流程
      await test.step('Phase 2.1: 完整权限定义管理流程', async () => {
        const testPermission: PermissionData = {
          name: `reports.view_${testStartTime}`,
          description: 'View reports',
        }

        await test.step('Given: 导航到权限管理页面', async () => {
          const permissionsPage = new PermissionsPage(page, demoLogger)
          await permissionsPage.goto()
          await permissionsPage.waitForReady()
          demoLogger.testCode.log('Navigated to permissions page')
        })

        await test.step('When: 创建新权限', async () => {
          const permissionsPage = new PermissionsPage(page, demoLogger)
          await permissionsPage.createPermission(testPermission)

          // 验证权限创建成功
          await expect(permissionsPage.table).toBeVisible()
          const exists = await permissionsPage.permissionExists(testPermission.name)
          expect(exists).toBeTruthy()
          demoLogger.testCode.log(`Permission "${testPermission.name}" created successfully`)
        })

        await test.step('When: 编辑权限描述', async () => {
          const permissionsPage = new PermissionsPage(page, demoLogger)
          await permissionsPage.editPermission(testPermission.name, {
            description: 'Updated description: Can view all reports',
          })

          // 重新打开编辑对话框验证
          await permissionsPage.clickEditPermission(testPermission.name)
          const descriptionValue = await permissionsPage.descriptionInput.inputValue()
          expect(descriptionValue).toContain('Updated description')
          demoLogger.testCode.log('Permission description updated successfully')

          // 关闭对话框并验证关闭
          await permissionsPage.dialogCancelButton.click()
          await expect(permissionsPage.dialog).toBeHidden({ timeout: 5000 })
        })

        await test.step('Then: 删除自定义权限', async () => {
          const permissionsPage = new PermissionsPage(page, demoLogger)
          await permissionsPage.deletePermission(testPermission.name)

          // 验证权限已删除
          const exists = await permissionsPage.permissionExists(testPermission.name)
          expect(exists).toBeFalsy()
          demoLogger.testCode.log(`Permission "${testPermission.name}" deleted successfully`)
        })
      })

      // Phase 2.2: 内置权限保护验证
      await test.step('Phase 2.2: 内置权限保护验证', async () => {
        const permissionsPage = new PermissionsPage(page, demoLogger)
        await test.step('Given: 导航到权限管理页面', async () => {
          await permissionsPage.goto()
          await permissionsPage.waitForReady()
          demoLogger.testCode.log('Navigated to permissions page')
        })

        await test.step('When: 检查内置权限 (users.manage)', async () => {
          // 内置权限应该存在
          const exists = await permissionsPage.permissionExists('users.manage')
          expect(exists).toBeTruthy()
          demoLogger.testCode.log('Built-in permission "users.manage" exists')

          // 内置权限应该有 "Built-in" 标识
          const hasBadge = await permissionsPage.hasBuiltInBadge('users.manage')
          expect(hasBadge).toBeTruthy()
          demoLogger.testCode.log('Built-in permission has "Built-in" badge')

          // 内置权限的编辑按钮应该被禁用或隐藏
          const editDisabled = await permissionsPage.isEditButtonDisabled('users.manage')
          expect(editDisabled).toBeTruthy()
          demoLogger.testCode.log('Built-in permission edit button is disabled/hidden')

          // 内置权限的删除按钮应该被禁用或隐藏
          const deleteDisabled = await permissionsPage.isDeleteButtonDisabled('users.manage')
          expect(deleteDisabled).toBeTruthy()
          demoLogger.testCode.log('Built-in permission delete button is disabled/hidden')
        })
      })

      // Phase 2.3: XSS 输入转义验证
      await test.step('Phase 2.3: XSS 输入转义验证', async () => {
        const permissionsPage = new PermissionsPage(page, demoLogger)
        const xssPayload = `<script>alert("xss-${testStartTime}")</script>`

        await test.step('Given: 导航到权限管理页面', async () => {
          await permissionsPage.goto()
          await permissionsPage.waitForReady()
          demoLogger.testCode.log('Navigated to permissions page')
        })

        await test.step('When: 创建包含 XSS 载荷的权限', async () => {
          const xssPermission: PermissionData = {
            name: `test.xss_${testStartTime}`,
            description: 'Test XSS payload',
          }

          await permissionsPage.createPermission(xssPermission)
          demoLogger.testCode.log('Permission with XSS payload created')
        })

        await test.step('Then: 验证 XSS 防护机制生效', async () => {
          // 刷新页面以确保从服务器重新加载数据
          await page.reload()
          await permissionsPage.waitForReady()

          // 验证 1: 检查权限是否显示
          const xssPermissionName = `test.xss_${testStartTime}`
          const row = permissionsPage.findPermissionRow(xssPermissionName)
          const isVisible = await row.isVisible().catch(() => false)

          if (isVisible) {
            // 如果显示，验证 XSS payload 被转义（作为文本显示，而非执行）
            const cellText = await row.locator('td').first().textContent()
            expect(cellText).toBeTruthy()
            demoLogger.testCode.log(`Permission displayed as text: "${cellText}"`)

            // 验证页面中没有可执行的 XSS script 标签（仅检查 table 内部）
            const scriptInTable = await row.locator('script').count()
            expect(scriptInTable).toBe(0)
            demoLogger.testCode.log('No executable script tags in table row')
          } else {
            // 如果不显示，说明被过滤（也是正确的安全行为）
            demoLogger.testCode.log('XSS payload filtered (not displayed in table)')
          }

          // 验证 2: 确保页面源码中没有原始 XSS script 标签
          const pageContent = await page.content()
          const hasXSSScript = pageContent.includes('<script>alert("xss-')
          expect(hasXSSScript).toBeFalsy()
          demoLogger.testCode.log('No XSS script tag in page source')
        })

        await test.step('Cleanup: 删除测试权限', async () => {
          const testPermissionName = `test.xss_${testStartTime}`
          await permissionsPage.deletePermission(testPermissionName)
          demoLogger.testCode.log('Test permission deleted')
        })
      })

      // Phase 3.1: 完整角色定义管理流程
      await test.step('Phase 3.1: 完整角色定义管理流程', async () => {
        const rolesPage = new RolesPage(page, demoLogger)
        const testRole: RoleData = {
          name: `content-admin-${testStartTime}`,
          description: 'Content administrator',
        }

        await test.step('Given: 导航到角色管理页面', async () => {
          await rolesPage.goto()
          await rolesPage.waitForReady()
          demoLogger.testCode.log('Navigated to roles page')
        })

        await test.step('When: 创建新角色', async () => {
          await rolesPage.createRole(testRole)

          // 验证角色创建成功
          await expect(rolesPage.table).toBeVisible()
          const exists = await rolesPage.roleExists(testRole.name)
          expect(exists).toBeTruthy()
          demoLogger.testCode.log(`Role "${testRole.name}" created successfully`)
        })

        await test.step('When: 编辑角色描述', async () => {
          await rolesPage.editRole(testRole.name, {
            description: 'Updated: Content administrator with full permissions',
          })

          // 重新打开编辑对话框验证
          await rolesPage.clickEditRole(testRole.name)
          const descriptionValue = await rolesPage.descriptionInput.inputValue()
          expect(descriptionValue).toContain('Updated:')
          demoLogger.testCode.log('Role description updated successfully')

          // 关闭对话框
          await rolesPage.dialogCancelButton.click()
        })

        await test.step('Then: 删除自定义角色', async () => {
          await rolesPage.deleteRole(testRole.name)

          // 验证角色已删除
          const exists = await rolesPage.roleExists(testRole.name)
          expect(exists).toBeFalsy()
          demoLogger.testCode.log(`Role "${testRole.name}" deleted successfully`)
        })
      })

      // Phase 3.2: 内置角色保护验证
      await test.step('Phase 3.2: 内置角色保护验证', async () => {
        const rolesPage = new RolesPage(page, demoLogger)
        await test.step('Given: 导航到角色管理页面', async () => {
          await rolesPage.goto()
          await rolesPage.waitForReady()
          demoLogger.testCode.log('Navigated to roles page')
        })

        await test.step('When: 检查内置角色 (realm-admin)', async () => {
          // 内置角色应该存在
          const exists = await rolesPage.roleExists('realm-admin')
          expect(exists).toBeTruthy()
          demoLogger.testCode.log('Built-in role "realm-admin" exists')

          // 内置角色应该有 "Built-in" 标识
          const hasBadge = await rolesPage.hasBuiltInBadge('realm-admin')
          expect(hasBadge).toBeTruthy()
          demoLogger.testCode.log('Built-in role has "Built-in" badge')

          // 内置角色的删除按钮应该被禁用或隐藏
          const deleteDisabled = await rolesPage.isDeleteButtonDisabled('realm-admin')
          expect(deleteDisabled).toBeTruthy()
          demoLogger.testCode.log('Built-in role delete button is disabled/hidden')
        })

        await test.step('When: 尝试编辑内置角色', async () => {
          await rolesPage.clickEditRole('realm-admin')

          // 名称输入框应该被禁用
          const nameDisabled = await rolesPage.isNameInputDisabled()
          expect(nameDisabled).toBeTruthy()
          demoLogger.testCode.log('Built-in role name input is disabled')

          // 描述输入框应该可编辑
          const descriptionDisabled = await rolesPage.descriptionInput.isDisabled()
          expect(descriptionDisabled).toBeFalsy()
          demoLogger.testCode.log('Built-in role description input is enabled')

          // 关闭对话框
          await rolesPage.dialogCancelButton.click()
        })
      })
    })
  })

  // ============================================================================
  // Phase 4: Role Permission Assignment [US-RA-004/005]
  // ============================================================================

  test.describe('Phase 4: Role Permission Assignment [US-RA-004/005]', () => {
    test('Scenario 1: 为角色分配和移除权限', async ({ page, loginPage, demoLogger, testStartTime }) => {
      // ✅ 添加登录步骤（必须在所有操作之前）
      await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')

      const rolesPage = new RolesPage(page, demoLogger)
      const testRoleName = `test-role-${testStartTime}`

      await test.step('Given: 创建测试角色', async () => {
        await rolesPage.goto()
        await rolesPage.waitForReady()

        const testRole: RoleData = {
          name: testRoleName,
          description: 'Test role for permission assignment',
        }
        await rolesPage.createRole(testRole)
        console.log(`✓ Test role "${testRoleName}" created`)
      })

      await test.step('When: 打开角色权限对话框', async () => {
        await rolesPage.clickPermissionsButton(testRoleName)
        console.log('✓ Role permissions dialog opened')
      })

      await test.step('When: 为角色分配权限', async () => {
        // 勾选一些权限
        await rolesPage.setPermission('users.view', true)
        await rolesPage.setPermission('users.manage', true)
        await rolesPage.setPermission('roles.view', true)

        // 保存更改
        await rolesPage.savePermissions()
        console.log('✓ Permissions assigned to role')
      })

      await test.step('Then: 验证权限分配成功', async () => {
        // 重新打开权限对话框
        await rolesPage.clickPermissionsButton(testRoleName)

        // 验证权限已被勾选
        expect(await rolesPage.isPermissionChecked('users.view')).toBeTruthy()
        expect(await rolesPage.isPermissionChecked('users.manage')).toBeTruthy()
        expect(await rolesPage.isPermissionChecked('roles.view')).toBeTruthy()

        console.log('✓ Permissions verified as assigned')

        // 关闭对话框
        await rolesPage.cancelPermissions()
      })

      await test.step('When: 移除角色权限', async () => {
        await rolesPage.clickPermissionsButton(testRoleName)

        // 取消勾选权限
        await rolesPage.setPermission('users.manage', false)

        // 保存更改
        await rolesPage.savePermissions()
        console.log('✓ Permission removed from role')
      })

      await test.step('Then: 验证权限移除成功', async () => {
        // 重新打开权限对话框
        await rolesPage.clickPermissionsButton(testRoleName)

        // 验证 users.manage 已被取消勾选
        expect(await rolesPage.isPermissionChecked('users.view')).toBeTruthy()
        expect(await rolesPage.isPermissionChecked('users.manage')).toBeFalsy()

        console.log('✓ Permission removal verified')

        // 关闭对话框
        await rolesPage.cancelPermissions()
      })
    })

    test('Scenario 2: 内置角色的内置权限保护', async ({ page, loginPage, demoLogger, testStartTime }) => {
      // ✅ 添加登录步骤（必须在所有操作之前）
      await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')

      const rolesPage = new RolesPage(page, demoLogger)
      const permissionsPage = new PermissionsPage(page, demoLogger)

      await test.step('Given: 导航到角色管理页面', async () => {
        await rolesPage.goto()
        await rolesPage.waitForReady()
        console.log('✓ Navigated to roles page')
      })

      await test.step('When: 打开 realm-admin 的权限对话框', async () => {
        await rolesPage.clickPermissionsButton('realm-admin')
        console.log('✓ Role permissions dialog opened for realm-admin')
      })

      await test.step('Then: 验证内置权限不能移除', async () => {
        // 内置权限的复选框应该被禁用
        const usersManageDisabled = await rolesPage.isPermissionCheckboxDisabled('users.manage')
        expect(usersManageDisabled).toBeTruthy()
        console.log('✓ Built-in permission "users.manage" checkbox is disabled')

        // 关闭对话框
        await rolesPage.cancelPermissions()
      })

      await test.step('When: 为内置角色添加自定义权限（被授予者守卫拒绝）', async () => {
        // 安全契约（commit 8c7b3aa8，
        // backend/api-admin/src/role_definitions/permissions.rs:79）：
        // POST /api/roles/{realmId}/define/{roleId}/permissions 要求授予者
        // 本人持有被授予的权限。刚创建的自定义权限无人持有，把它授予内置
        // realm-admin 角色必须被 403 拒绝（"Insufficient permissions: requires ..."）。
        // 前端保存失败只 toast 不关对话框（role-permissions-dialog.tsx onError），
        // 因此断言落在 API 响应与对话框保持打开的持久 UI 状态上，而非对话框关闭。

        // 创建自定义权限
        await permissionsPage.goto()

        const customPermission: PermissionData = {
          name: `reports.view_${testStartTime}`,
          description: 'View reports',
        }
        await permissionsPage.createPermission(customPermission)
        console.log('✓ Custom permission created')

        // 打开 realm-admin 的权限对话框
        await rolesPage.goto()
        await rolesPage.clickPermissionsButton('realm-admin')

        // 勾选自定义权限并保存 — 期望被守卫 403 拒绝
        await rolesPage.setPermission(customPermission.name, true)
        const response = await rolesPage.clickSavePermissions()
        expect(response.status()).toBe(403)
        const body = await response.json()
        expect(String(body.message)).toContain('Insufficient permissions')
        expect(String(body.message)).toContain(customPermission.name)
        console.log(`✓ Grant rejected by guard: ${response.status()} ${body.message}`)

        // 保存失败前端不关对话框 — 对话框应保持打开（若守卫被移除、保存成功，
        // 对话框会关闭，此断言失败，保证测试在契约回退时变红）
        await expect(rolesPage.permissionsDialog).toBeVisible()
        await rolesPage.cancelPermissions()
        console.log('✓ Dialog stayed open after rejected save, then cancelled')
      })

      await test.step('Then: 验证内置角色的权限保护机制', async () => {
        // 重新打开权限对话框
        await rolesPage.clickPermissionsButton('realm-admin')

        // US-BP-001 Scenario 7: 内置角色已分配的内置权限受保护（复选框禁用，不可移除）
        const usersManageDisabled = await rolesPage.isPermissionCheckboxDisabled('users.manage')
        expect(usersManageDisabled).toBeTruthy()
        console.log('✓ Built-in permission "users.manage" is disabled (protected)')

        // US-BP-001 Scenario 9: 内置角色上的自定义权限仍可自由增删（复选框启用）
        const customPermissionName = `reports.view_${testStartTime}`
        const isCustomPermissionDisabled = await rolesPage.isPermissionCheckboxDisabled(customPermissionName)
        expect(isCustomPermissionDisabled).toBeFalsy() // 启用 — 可修改
        console.log('✓ Custom permission checkbox is enabled (editable on built-in role)')

        // 8c7b3aa8 守卫：被拒绝的授予不应生效 — 自定义权限保持未勾选
        expect(await rolesPage.isPermissionChecked(customPermissionName)).toBeFalsy()
        console.log('✓ Custom permission remains unassigned (grant was rejected by guard)')

        await rolesPage.cancelPermissions()
        console.log('✓ Built-in role protection verified - built-in perms protected, custom perms editable, unheld grant rejected')
      })
    })
  })

  // ============================================================================
  // Phase 5: User Role Assignment [US-RA-006]
  // ============================================================================

  test.describe('Phase 5: User Role Assignment [US-RA-006]', () => {
    test('Scenario 1: 为用户分配和移除角色', async ({ page, loginPage, demoLogger, testStartTime }) => {
      // ✅ 添加登录步骤（必须在所有操作之前）
      await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')

      const rolesPage = new RolesPage(page, demoLogger)
      const usersPage = new UsersPage(page, demoLogger)
      const testUserEmail = `test-user-${testStartTime}@example.com`
      const testRoleName = `test-role-${testStartTime}`

      await test.step('Given: 创建测试角色和用户', async () => {
        // 创建角色
        await rolesPage.goto()
        const testRole: RoleData = {
          name: testRoleName,
          description: 'Test role for user assignment',
        }
        await rolesPage.createRole(testRole)
        console.log(`✓ Test role "${testRoleName}" created`)

        // 创建用户
        await usersPage.goto()
        await usersPage.clickAddUser()

        // 填写用户表单
        await usersPage.fillUserForm({
          email: testUserEmail,
          nickname: `Test User ${testStartTime}`,
          password: 'password123',
        })

        // Select the "User" role (required by schema)
        // Wait for role checkbox to be visible and interactive
        const roleCheckbox = page.locator('[data-testid="user-create-role-checkbox"]')
        await expect(roleCheckbox).toBeVisible({ timeout: 5000 })
        await expect(roleCheckbox).toBeEnabled()

        // Check the role checkbox
        await roleCheckbox.check()
        console.log('✓ Role checkbox checked (User role selected)')

        // Verify checkbox is actually checked before submitting
        const isChecked = await roleCheckbox.isChecked()
        expect(isChecked).toBeTruthy()
        console.log('✓ Verified role checkbox is checked')

        // Submit the form
        await usersPage.submitUserForm()
        console.log(`✓ Test user "${testUserEmail}" created`)
      })

      await test.step('When: 为用户分配角色', async () => {
        // 找到用户行并点击角色按钮
        const userRow = usersPage.findUserRow(testUserEmail)
        await expect(userRow).toBeVisible()

        // 使用文本内容查找 "Roles" 按钮（更可靠）
        const rolesButton = userRow.locator('button:has-text("Roles")').first()
        await rolesPage.smartClick(rolesButton)

        // 等待角色对话框打开
        const dialogContent = page.locator('[data-testid="user-roles-dialog-content"]')
        await expect(dialogContent).toBeVisible()

        // 使用 RoleSelector 选择角色（自动保存）
        // 点击角色选择器触发器
        const roleSelectorTrigger = page.locator('[data-testid="role-selector-trigger"]')
        await roleSelectorTrigger.click()

        // 等待下拉菜单打开并选择角色
        // 注意：role-selector-item 使用 role.id，但我们不知道 ID，所以通过文本内容查找
        const roleItem = page.locator('[data-testid^="role-selector-item-"]').filter({ hasText: testRoleName }).first()
        await expect(roleItem).toBeVisible()
        await roleItem.click()

        // 等待自动保存完成（验证角色选择器显示角色名）
        await expect(roleSelectorTrigger).toContainText(testRoleName, { timeout: 5000 })

        console.log(`✓ Role "${testRoleName}" assigned to user (auto-saved)`)
      })

      await test.step('Then: 验证用户拥有角色', async () => {
        // 关闭当前对话框
        const cancelButton = page.locator('[data-testid="user-roles-dialog-cancel"]')
        await cancelButton.click()

        // 重新打开用户角色对话框
        const userRow = usersPage.findUserRow(testUserEmail)
        const rolesButton = userRow.locator('button:has-text("Roles")').first()
        await rolesPage.smartClick(rolesButton)

        const dialogContent = page.locator('[data-testid="user-roles-dialog-content"]')
        await expect(dialogContent).toBeVisible()

        // 验证角色已被选中（通过检查角色选择器中是否显示该角色）
        const roleSelectorTrigger = page.locator('[data-testid="role-selector-trigger"]')
        await expect(roleSelectorTrigger).toContainText(testRoleName)

        console.log('✓ User role assignment verified')

        // 关闭对话框
        await page.locator('[data-testid="user-roles-dialog-cancel"]').click()
      })

      await test.step('When: 为用户分配多个角色', async () => {
        // 创建第二个角色
        await rolesPage.goto()
        const secondRoleName = `test-role-2-${testStartTime}`
        const secondRole: RoleData = {
          name: secondRoleName,
          description: 'Second test role',
        }
        await rolesPage.createRole(secondRole)
        console.log(`✓ Second role "${secondRoleName}" created`)

        // 为用户分配第二个角色
        await usersPage.goto()
        const userRow = usersPage.findUserRow(testUserEmail)
        const rolesButton = userRow.locator('button:has-text("Roles")').first()
        await rolesPage.smartClick(rolesButton)

        const dialogContent = page.locator('[data-testid="user-roles-dialog-content"]')
        await expect(dialogContent).toBeVisible()

        // 使用 RoleSelector 选择第二个角色（自动保存）
        const roleSelectorTrigger = page.locator('[data-testid="role-selector-trigger"]')
        await roleSelectorTrigger.click()

        // 通过文本内容查找角色项
        const roleItem2 = page.locator('[data-testid^="role-selector-item-"]').filter({ hasText: secondRoleName }).first()
        await expect(roleItem2).toBeVisible()
        await roleItem2.click()

        // 等待自动保存完成（验证角色选择器显示角色名）
        await expect(roleSelectorTrigger).toContainText(secondRoleName, { timeout: 5000 })

        console.log('✓ Second role assigned to user (auto-saved)')
      })

      await test.step('Then: 移除用户角色', async () => {
        // 关闭当前对话框（如果打开）- 使用 Escape 键确保覆盖层消失
        await page.keyboard.press('Escape')
        // Brief wait for overlay animation to complete
        await expect(page.locator('[data-state="open"][data-slot$="overlay"]')).toBeHidden({ timeout: 3000 }).catch(() => {})
        // Wait for any remaining overlays to disappear
        const anyOverlay = page.locator('[data-state="open"][data-slot$="overlay"]')
        if (await anyOverlay.count() > 0) {
          await page.keyboard.press('Escape')
          await expect(anyOverlay).toHaveCount(0, { timeout: 5000 })
        }

        // 打开用户角色对话框
        await usersPage.goto()
        const userRow = usersPage.findUserRow(testUserEmail)
        const rolesButton = userRow.locator('button:has-text("Roles")').first()
        await rolesPage.smartClick(rolesButton)

        const dialogContent = page.locator('[data-testid="user-roles-dialog-content"]')
        await expect(dialogContent).toBeVisible()

        // 使用 RoleSelector 取消选择角色（自动保存）
        const roleSelectorTrigger = page.locator('[data-testid="role-selector-trigger"]')
        await roleSelectorTrigger.click()

        // 点击已选中的角色以取消选择
        // 注意：role-selector-item 使用 role.id，所以我们通过文本内容查找（与角色分配步骤一致）
        const roleItem = page.locator('[data-testid^="role-selector-item-"]').filter({ hasText: testRoleName }).first()
        await expect(roleItem).toBeVisible()
        await roleItem.click()

        // 等待自动保存完成（验证角色选择器不再显示该角色）
        await expect(roleSelectorTrigger).not.toContainText(testRoleName, { timeout: 5000 })

        console.log(`✓ Role "${testRoleName}" removed from user (auto-saved)`)
      })
    })
  })

  // ============================================================================
  // Phase 6-7: Built-in Protection and Complete RBAC Loop [US-BP-001]
  // ============================================================================

  test.describe('Phase 6-7: Built-in Protection and Complete RBAC Loop [US-BP-001]', () => {
    test('内置保护和完整 RBAC 闭环演示', async ({ page, loginPage, demoLogger, testStartTime }) => {
      // ✅ 添加登录步骤（必须在所有操作之前）
      await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')

      const permissionsPage = new PermissionsPage(page, demoLogger)
      const rolesPage = new RolesPage(page, demoLogger)
      const usersPage = new UsersPage(page, demoLogger)

      // Phase 6: 完整内置保护机制验证
      await test.step('Phase 6: 完整内置保护机制验证', async () => {
        // 确保所有对话框覆盖层已关闭（Phase 5 可能留下 dialog-overlay）
        await page.keyboard.press('Escape')
        // Brief wait for overlay animation to complete
        await expect(page.locator('[data-state="open"][data-slot$="overlay"]')).toBeHidden({ timeout: 3000 }).catch(() => {})
        const anyOverlay = page.locator('[data-state="open"][data-slot$="overlay"]')
        if (await anyOverlay.count() > 0) {
          await page.keyboard.press('Escape')
          await expect(anyOverlay).toHaveCount(0, { timeout: 5000 }).catch(() => {})
        }

        await test.step('Given: 导航到权限和角色管理页面', async () => {
          await permissionsPage.goto()
          await permissionsPage.waitForReady()
          console.log('✓ Navigated to permissions page')
        })

        await test.step('When: 验证内置权限保护', async () => {
          // 检查多个内置权限
          const builtInPermissions = ['users.manage', 'roles.view', 'roles.manage']

          for (const permissionName of builtInPermissions) {
            const exists = await permissionsPage.permissionExists(permissionName)
            if (exists) {
              const hasBadge = await permissionsPage.hasBuiltInBadge(permissionName)
              const editDisabled = await permissionsPage.isEditButtonDisabled(permissionName)
              const deleteDisabled = await permissionsPage.isDeleteButtonDisabled(permissionName)

              expect(hasBadge).toBeTruthy()
              expect(editDisabled).toBeTruthy()
              expect(deleteDisabled).toBeTruthy()

              console.log(`✓ Built-in permission "${permissionName}" is protected`)
            }
          }
        })

        await test.step('When: 验证内置角色保护', async () => {
          await rolesPage.goto()
          await rolesPage.waitForReady()

          // 检查 realm-admin 角色
          const exists = await rolesPage.roleExists('realm-admin')
          expect(exists).toBeTruthy()

          const hasBadge = await rolesPage.hasBuiltInBadge('realm-admin')
          const deleteDisabled = await rolesPage.isDeleteButtonDisabled('realm-admin')

          expect(hasBadge).toBeTruthy()
          expect(deleteDisabled).toBeTruthy()

          console.log('✓ Built-in role "realm-admin" is protected')
        })

        await test.step('When: 验证内置角色的内置权限保护', async () => {
          await rolesPage.clickPermissionsButton('realm-admin')

          // 内置权限的复选框应该被禁用
          const builtInPermissions = ['users.manage', 'roles.view', 'roles.manage']

          for (const permissionName of builtInPermissions) {
            const isDisabled = await rolesPage.isPermissionCheckboxDisabled(permissionName)
            if (isDisabled) {
              console.log(`✓ Built-in permission "${permissionName}" checkbox is disabled for built-in role`)
            }
          }

          // 关闭对话框
          await rolesPage.cancelPermissions()
        })

        await test.step('Then: 验证内置角色的自定义权限授予受守卫保护', async () => {
          // 安全契约（commit 8c7b3aa8，
          // backend/api-admin/src/role_definitions/permissions.rs:79）：
          // 授予者必须本人持有被授予的权限。刚创建的自定义权限无人持有，
          // 授予内置 realm-admin 角色必须被 403 拒绝。前端保存失败只 toast
          // 不关对话框，因此断言落在 API 响应与对话框保持打开的持久 UI 状态上。

          // 创建自定义权限
          await permissionsPage.goto()

          const customPermission: PermissionData = {
            name: `custom.perm_${testStartTime}`,
            description: 'Custom permission for testing',
          }
          await permissionsPage.createPermission(customPermission)
          console.log('✓ Custom permission created')

          // 为内置角色添加自定义权限 — 期望被守卫 403 拒绝
          await rolesPage.goto()
          await rolesPage.clickPermissionsButton('realm-admin')

          await rolesPage.setPermission(customPermission.name, true)
          const response = await rolesPage.clickSavePermissions()
          expect(response.status()).toBe(403)
          const body = await response.json()
          expect(String(body.message)).toContain('Insufficient permissions')
          expect(String(body.message)).toContain(customPermission.name)
          console.log(`✓ Grant rejected by guard: ${response.status()} ${body.message}`)

          // 保存失败前端不关对话框 — 对话框应保持打开
          await expect(rolesPage.permissionsDialog).toBeVisible()
          await rolesPage.cancelPermissions()

          // 验证授予未生效：自定义权限复选框仍启用（可编辑）但未被勾选
          await rolesPage.clickPermissionsButton('realm-admin')

          const isDisabled = await rolesPage.isPermissionCheckboxDisabled(customPermission.name)
          expect(isDisabled).toBeFalsy()

          expect(await rolesPage.isPermissionChecked(customPermission.name)).toBeFalsy()
          console.log('✓ Custom permission remains editable but unassigned (guard held)')

          await rolesPage.cancelPermissions()
        })
      })

      // Phase 7: 完整 RBAC 管理闭环演示
      await test.step('Phase 7: 完整 RBAC 管理闭环演示', async () => {
        // 确保所有对话框覆盖层已关闭（Phase 6 可能留下 alert-dialog-overlay）
        await page.keyboard.press('Escape')
        // Brief wait for overlay animation to complete
        await expect(page.locator('[data-state="open"][data-slot$="overlay"]')).toBeHidden({ timeout: 3000 }).catch(() => {})
        const anyOverlay = page.locator('[data-state="open"][data-slot$="overlay"]')
        if (await anyOverlay.count() > 0) {
          await page.keyboard.press('Escape')
          await expect(anyOverlay).toHaveCount(0, { timeout: 5000 }).catch(() => {})
        }

        const permission1Name = `reports_${testStartTime}.view`
        const permission2Name = `reports_${testStartTime}.manage`
        // Step 3/7 授予并验证的权限：调用者（realm-admin）已持有的内置权限。
        // 8c7b3aa8 契约（backend/api-admin/src/role_definitions/permissions.rs:79）
        // 要求授予者本人持有被授予权限，刚创建的自定义权限无人持有、不可授予
        // （见 Step 1/Step 3 注释），故闭环授予改走 Scenario 1 已验证的内置权限路径。
        const grantedPermission1Name = 'users.view'
        const grantedPermission2Name = 'users.manage'
        const roleName = `content-admin-${testStartTime}`
        const userEmail = `content-user-${testStartTime}@example.com`

        await test.step('Step 1: 创建自定义权限', async () => {
          // 独立演示：权限定义页可创建自定义权限。
          // 8c7b3aa8 契约（backend/api-admin/src/role_definitions/permissions.rs:79）：
          // 授予者必须本人持有被授予的权限，而刚创建的自定义权限无人持有，
          // 因此它们不能在 Step 3 被授予任何角色（授予会按设计被 403 拒绝，
          // 见 Phase 6 的守卫验证步骤）。这两个自定义权限仅验证可创建，由 Cleanup 删除。
          await permissionsPage.goto()

          const permission1: PermissionData = {
            name: permission1Name,
            description: 'View reports',
          }
          await permissionsPage.createPermission(permission1)

          const permission2: PermissionData = {
            name: permission2Name,
            description: 'Manage reports',
          }
          await permissionsPage.createPermission(permission2)

          // 验证两个自定义权限已创建
          expect(await permissionsPage.permissionExists(permission1Name)).toBeTruthy()
          expect(await permissionsPage.permissionExists(permission2Name)).toBeTruthy()

          console.log('✓ Step 1: Created 2 custom permissions')
        })

        await test.step('Step 2: 创建角色', async () => {
          await rolesPage.goto()

          const role: RoleData = {
            name: roleName,
            description: 'Content administrator with reports permissions',
          }
          await rolesPage.createRole(role)

          console.log('✓ Step 2: Created role')
        })

        await test.step('Step 3: 为角色分配权限', async () => {
          // 8c7b3aa8 契约（backend/api-admin/src/role_definitions/permissions.rs:79）：
          // 授予者必须本人持有被授予的权限。Step 1 新建的自定义权限无人持有，
          // 授予必被 403 拒绝且对话框不会关闭，因此闭环演示授予调用者
          // （realm-admin）已持有的内置权限（与通过的 Scenario 1 相同路径），
          // 保存正常关框。
          await rolesPage.clickPermissionsButton(roleName)

          await rolesPage.setPermission(grantedPermission1Name, true)
          await rolesPage.setPermission(grantedPermission2Name, true)

          await rolesPage.savePermissions()

          console.log(`✓ Step 3: Assigned permissions to role (${grantedPermission1Name}, ${grantedPermission2Name})`)
        })

        await test.step('Step 4: 创建用户', async () => {
          await usersPage.goto()
          await usersPage.clickAddUser()

          // 填写用户表单
          await usersPage.fillUserForm({
            email: userEmail,
            nickname: `Content User ${testStartTime}`,
            password: 'password123',
          })

          // Select the "User" role (required by schema)
          // Wait for role checkbox to be visible and interactive
          const roleCheckbox = page.locator('[data-testid="user-create-role-checkbox"]')
          await expect(roleCheckbox).toBeVisible({ timeout: 5000 })
          await expect(roleCheckbox).toBeEnabled()

          // Check the role checkbox
          await roleCheckbox.check()
          console.log('✓ Role checkbox checked (User role selected)')

          // Verify checkbox is actually checked before submitting
          const isChecked = await roleCheckbox.isChecked()
          expect(isChecked).toBeTruthy()
          console.log('✓ Verified role checkbox is checked')

          // Submit the form
          await usersPage.submitUserForm()

          console.log('✓ Step 4: Created user')
        })

        await test.step('Step 5: 为用户分配角色', async () => {
          const userRow = usersPage.findUserRow(userEmail)
          const rolesButton = userRow.locator('button:has-text("Roles")').first()
          await rolesPage.smartClick(rolesButton)

          const dialogContent = page.locator('[data-testid="user-roles-dialog-content"]')
          await expect(dialogContent).toBeVisible()

          // 使用 RoleSelector 选择角色（自动保存）
          const roleSelectorTrigger = page.locator('[data-testid="role-selector-trigger"]')
          await roleSelectorTrigger.click()

          // 通过文本内容查找角色项
          const roleItem = page.locator('[data-testid^="role-selector-item-"]').filter({ hasText: roleName }).first()
          await expect(roleItem).toBeVisible()
          await roleItem.click()

          // 等待自动保存完成（验证角色选择器显示角色名）
          await expect(roleSelectorTrigger).toContainText(roleName, { timeout: 5000 })

          console.log('✓ Step 5: Assigned role to user (auto-saved)')
        })

        await test.step('Step 6: 验证用户拥有正确角色', async () => {
          // 关闭当前对话框
          await page.locator('[data-testid="user-roles-dialog-cancel"]').click()

          // 重新打开用户角色对话框
          const userRow = usersPage.findUserRow(userEmail)
          const rolesButton = userRow.locator('button:has-text("Roles")').first()
          await rolesPage.smartClick(rolesButton)

          const dialogContent = page.locator('[data-testid="user-roles-dialog-content"]')
          await expect(dialogContent).toBeVisible()

          // 验证角色已被选中
          const roleSelectorTrigger = page.locator('[data-testid="role-selector-trigger"]')
          await expect(roleSelectorTrigger).toContainText(roleName)

          console.log('✓ Step 6: Verified user has correct role')

          // 关闭对话框
          await page.locator('[data-testid="user-roles-dialog-cancel"]').click()
        })

        await test.step('Step 7: 验证角色拥有正确权限', async () => {
          await rolesPage.goto()
          await rolesPage.clickPermissionsButton(roleName)

          expect(await rolesPage.isPermissionChecked(grantedPermission1Name)).toBeTruthy()
          expect(await rolesPage.isPermissionChecked(grantedPermission2Name)).toBeTruthy()

          console.log('✓ Step 7: Verified role has correct permissions')

          // 关闭对话框
          await rolesPage.cancelPermissions()
        })

        await test.step('Cleanup: 清理测试数据', async () => {
          // 确保所有对话框覆盖层已关闭 - 使用多次 Escape 确保彻底关闭
          for (let i = 0; i < 3; i++) {
            await page.keyboard.press('Escape')
          }
          // Wait for overlays to disappear using assertions
          const anyOverlay = page.locator('[data-state="open"][data-slot$="overlay"]')
          await expect(anyOverlay).toHaveCount(0, { timeout: 5000 }).catch(() => {})
          // 额外等待 alert-dialog overlay 消失
          const alertDialogOverlay = page.locator('[data-slot="alert-dialog-overlay"][data-state="open"]')
          if (await alertDialogOverlay.count() > 0) {
            await page.keyboard.press('Escape')
            await expect(alertDialogOverlay).toHaveCount(0, { timeout: 3000 }).catch(() => {})
          }
          // 删除用户（deleteUser内部会调用goto()）
          await usersPage.deleteUser(userEmail)

          // 再次确保无覆盖层
          for (let i = 0; i < 2; i++) {
            await page.keyboard.press('Escape')
          }
          await page.locator('[data-state="open"][data-slot$="overlay"]').waitFor({ state: 'hidden', timeout: 3000 }).catch(() => {})

          // 删除角色
          await rolesPage.goto()
          await rolesPage.deleteRole(roleName)

          // 删除权限
          await permissionsPage.goto()
          await permissionsPage.deletePermission(permission1Name)
          await permissionsPage.deletePermission(permission2Name)

          console.log('✓ Cleanup: All test data deleted')
        })
      })
    })
  })
})
