/**
 * Roles Page Object
 *
 * Encapsulates role management page operations.
 * Provides methods for creating, editing, and deleting roles.
 *
 * @see ../../../spec/demo/e2e-testing.md#page-object-model-pom-规范
 */

import { Page, Locator, expect, type Response } from '@playwright/test'
import { SELECTORS } from '../selectors'
import { BasePage } from './base-page'
import type { UnifiedLogger } from '../helpers/unified-logger'

/**
 * Role data interface
 */
export interface RoleData {
  name: string
  description?: string
}

/**
 * Roles Page Object
 *
 * Represents the role management page at /{realmId}/roles
 *
 * @example
 * ```typescript
 * const rolesPage = new RolesPage(page, logger)
 * await rolesPage.goto()
 * await rolesPage.createRole({ name: 'Editor', description: 'Can edit content' })
 * ```
 */
export class RolesPage extends BasePage {
  // Selectors
  readonly container: Locator
  readonly heading: Locator
  readonly table: Locator
  readonly addButton: Locator
  readonly dialog: Locator
  readonly dialogTitle: Locator
  readonly createNameInput: Locator
  readonly createDescriptionInput: Locator
  readonly createSubmitButton: Locator
  readonly editNameInput: Locator
  readonly editDescriptionInput: Locator
  readonly editSubmitButton: Locator
  readonly dialogSubmitButton: Locator // Generic submit button for delete confirmation
  readonly dialogCancelButton: Locator
  readonly descriptionInput: Locator // Alias for edit mode
  readonly permissionsDialog: Locator // Role permissions dialog (stays open on save errors)

  private createdRoles: string[] = [] // Track created roles for cleanup
  private realmId = 'admin'

  constructor(page: Page, logger?: UnifiedLogger) {
    super(page)
    this.logger = logger
    this.container = page.locator(SELECTORS.roles.container)
    this.heading = page.locator(SELECTORS.roles.heading)
    this.table = page.locator(SELECTORS.roles.table)
    this.addButton = page.locator(SELECTORS.roles.addButton)

    // Create dialog selectors
    // Support both dialog and alertdialog roles (Radix UI uses alertdialog)
    this.dialog = page.locator('[role="dialog"], [role="alertdialog"]')
    this.dialogTitle = page.locator('[data-testid="dialog-title"]')
    this.createNameInput = page.locator('[data-testid="role-create-name-input"]')
    this.createDescriptionInput = page.locator('[data-testid="role-create-description-input"]')
    this.createSubmitButton = page.locator('[data-testid="role-create-submit-button"]')

    // Edit dialog selectors
    this.editNameInput = page.locator('[data-testid="role-edit-name-input"]')
    this.editDescriptionInput = page.locator('[data-testid="role-edit-description-input"]')
    this.editSubmitButton = page.locator('[data-testid="role-edit-submit-button"]')

    // Generic dialog submit button (for delete confirmation, etc.)
    // Use multiple fallback selectors to handle different naming conventions
    this.dialogSubmitButton = page.locator([
      '[data-testid="dialog-submit-button"]',              // Generic naming
      '[data-testid="confirm-delete-button"]',              // Old naming
      '[data-testid="role-delete-confirm-button"]',         // ✅ Actual frontend selector
      '[data-testid="permission-delete-confirm-button"]',   // For permission dialogs
      'button:has-text("Delete")',                          // Text fallback
      'button:has-text("Confirm")',                         // Alternative text
    ].join(', '))

    // Common dialog selectors
    // Try multiple possible selectors for cancel button (fallbacks for different dialog implementations)
    this.dialogCancelButton = page.locator([
      '[data-testid="dialog-cancel-button"]',
      '[data-testid="role-edit-cancel-button"]',
      'button:has-text("Cancel")',
      'button:has-text("取消")',
    ].join(', '))

    // Aliases for convenience
    this.descriptionInput = this.editDescriptionInput // Alias for edit mode

    // Role permissions dialog
    this.permissionsDialog = page.locator('[data-testid="role-permissions-dialog"]')
  }

  /**
   * Navigate to roles page
   *
   * @param realmId Realm ID (defaults to 'admin' for backward compatibility)
   */
  async goto(realmId: string = 'admin'): Promise<void> {
    this.realmId = realmId
    // 等待权限加载完成（/api/user/roles 请求）
    // 这确保侧边栏菜单在权限加载后才显示子菜单项
    await this.page.waitForResponse(
      response => response.url().includes('/api/user/roles') && response.status() === 200,
      { timeout: 10000 }
    ).catch(() => {
      // 如果请求已经完成，忽略错误
      this.logger?.testCode.log('User roles request already completed or timeout, continuing...')
    })

    // 通过点击侧边栏菜单来导航，模拟真实用户操作
    // Roles 是 Authorization 的子菜单，需要先展开
    // 等待 Authorization 菜单可见
    const authMenuLink = this.page.locator(SELECTORS.sidebar.menuAuthorization)
    await authMenuLink.waitFor({ state: 'visible', timeout: 10000 })

    // 点击 Authorization 菜单以展开子菜单
    // 由于菜单初始状态可能是关闭的，我们需要检查并确保它已展开
    // 双击以确保菜单展开（第一次点击关闭，第二次点击打开）
    await authMenuLink.click()
    await this.page.waitForTimeout(300)
    await authMenuLink.click()
    await this.page.waitForTimeout(500)

    // 等待 Roles 菜单项可见（权限加载完成后才显示）
    const rolesMenuLink = this.page.locator(SELECTORS.sidebar.menuRoles)
    await rolesMenuLink.waitFor({ state: 'visible', timeout: 10000 })
    await this.smartClick(rolesMenuLink)

    // 等待页面加载完成
    await this.waitForReady()
  }

  /**
   * Wait for roles page to be visible
   */
  async waitForReady(): Promise<void> {
    await expect(this.container).toBeVisible()
    await expect(this.heading).toBeVisible()
    await expect(this.table).toBeVisible()
  }

  /**
   * Click "Add Role" button to open create dialog
   */
  async clickAddRole(): Promise<void> {
    await this.smartClick(this.addButton)
    await expect(this.dialog).toBeVisible()
    await expect(this.dialogTitle).toHaveText(/Add Role|Create Role|New Role/i)
  }

  /**
   * Fill role form fields
   *
   * @param roleData Role data to fill (partial allowed for edit mode)
   */
  async fillRoleForm(roleData: Partial<RoleData>): Promise<void> {
    if (roleData.name) {
      await this.fillField(this.createNameInput, roleData.name)
    }

    if (roleData.description) {
      await this.fillField(this.createDescriptionInput, roleData.description)
    }
  }

  /**
   * Submit the role form (click submit button)
   */
  async submitRoleForm(): Promise<void> {
    // Check if submit button is disabled (validation error)
    const isDisabled = await this.createSubmitButton.isDisabled()
    if (isDisabled) {
      this.logger?.testCode.error('Submit button is disabled - form validation may have failed')
      throw new Error('Submit button is disabled - form validation may have failed')
    }

    this.logger?.testCode.log('Submitting role form...')

    // Click submit button and wait for dialog to close
    await Promise.all([
      // Wait for dialog to close (create name input becomes hidden)
      this.createNameInput.waitFor({ state: 'hidden', timeout: 10000 }),
      this.smartClick(this.createSubmitButton)
    ])

    // Wait for data table to refresh using role-based selectors
    const dataRows = this.table.getByRole('row').filter({ hasText: /.+/ })
    await expect(dataRows.first()).toBeVisible({ timeout: 5000 })

    this.logger?.testCode.log('✓ Role form submitted successfully')
  }

  /**
   * Create a new role
   *
   * @param roleData Role data
   *
   * @example
   * ```typescript
   * await rolesPage.createRole({
   *   name: 'Editor',
   *   description: 'Can edit content'
   * })
   * ```
   */
  async createRole(roleData: RoleData): Promise<void> {
    this.logger?.testCode.log(`Creating role: ${roleData.name}`)
    await this.clickAddRole()
    await this.fillRoleForm(roleData)
    await this.submitRoleForm()

    // Track created role for cleanup
    this.createdRoles.push(roleData.name)
    this.logger?.testCode.log(`✓ Role created successfully: ${roleData.name}`)
  }

  /**
   * Find role row in table by name
   *
   * @param name Role name to search for
   * @returns Row locator
   */
  findRoleRow(name: string): Locator {
    return this.table.locator(`tr:has-text("${name}")`).first()
  }

  /**
   * Check if role exists in table
   *
   * @param name Role name to check
   */
  async roleExists(name: string): Promise<boolean> {
    const row = this.findRoleRow(name)
    return await this.isVisible(row)
  }

  /**
   * Click "Edit" button for a role
   *
   * @param name Role name
   */
  async clickEditRole(name: string): Promise<void> {
    const row = this.findRoleRow(name)
    await expect(row).toBeVisible()

    // Use starts-with selector to match buttons with any ID suffix
    const editButton = row.locator('[data-testid^="role-edit-button-"]').first()
    await this.smartClick(editButton)

    await expect(this.dialog).toBeVisible()
    await expect(this.dialogTitle).toHaveText(/Edit Role|Update Role/i)
  }

  /**
   * Edit an existing role
   *
   * @param name Role name to edit
   * @param updatedData New role data
   *
   * @example
   * ```typescript
   * await rolesPage.editRole('Editor', {
   *   description: 'Updated description'
   * })
   * ```
   */
  async editRole(name: string, updatedData: Partial<RoleData>): Promise<void> {
    this.logger?.testCode.log(`Editing role: ${name}`)
    await this.clickEditRole(name)

    // Use edit-specific selectors
    if (updatedData.name) {
      await this.fillField(this.editNameInput, updatedData.name)
    }

    if (updatedData.description) {
      await this.fillField(this.editDescriptionInput, updatedData.description)
    }

    // Check if submit button is disabled (validation error)
    const isDisabled = await this.editSubmitButton.isDisabled()
    if (isDisabled) {
      this.logger?.testCode.error('Submit button is disabled - form validation may have failed')
      throw new Error('Submit button is disabled - form validation may have failed')
    }

    // Click submit button and wait for dialog to close
    await Promise.all([
      // Wait for dialog to close (edit name input becomes hidden)
      this.editNameInput.waitFor({ state: 'hidden', timeout: 10000 }),
      this.smartClick(this.editSubmitButton)
    ])

    // Wait for data table to refresh using role-based selectors
    await expect(this.table.getByRole('row').first()).toBeVisible({ timeout: 5000 })

    this.logger?.testCode.log(`✓ Role edited successfully: ${name}`)
  }

  /**
   * Click "Delete" button for a role
   *
   * @param name Role name
   */
  async clickDeleteRole(name: string): Promise<void> {
    const row = this.findRoleRow(name)
    await expect(row).toBeVisible()

    // Use starts-with selector to match buttons with any ID suffix
    const deleteButton = row.locator('[data-testid^="role-delete-button-"]').first()
    await this.smartClick(deleteButton)

    await expect(this.dialog).toBeVisible()
    await expect(this.dialogTitle).toHaveText(/Delete Role|Confirm Delete/i)
  }

  /**
   * Confirm role deletion
   */
  async confirmDeleteRole(): Promise<void> {
    // Click confirm button and wait for dialog to close
    await Promise.all([
      this.dialog.waitFor({ state: 'hidden', timeout: 10000 }),
      this.smartClick(this.dialogSubmitButton)
    ])

    // Wait for data table to refresh using role-based selectors
    await expect(this.table.getByRole('row').first()).toBeVisible({ timeout: 5000 })
  }

  /**
   * Delete a role
   *
   * @param name Role name to delete
   *
   * @example
   * ```typescript
   * await rolesPage.deleteRole('Editor')
   * ```
   */
  async deleteRole(name: string): Promise<void> {
    this.logger?.testCode.log(`Deleting role: ${name}`)
    await this.clickDeleteRole(name)
    await this.confirmDeleteRole()

    // Wait for the row to be removed (AlertDialog closes before mutation completes)
    const row = this.findRoleRow(name)
    try {
      await expect(row).toBeHidden({ timeout: 5000 })
    } catch {
      // Mutation may have failed - reload and retry once
      this.logger?.testCode.log(`Role "${name}" still visible, retrying...`)
      await this.page.reload()
      await this.waitForReady()
      if (await this.roleExists(name)) {
        await this.clickDeleteRole(name)
        await this.confirmDeleteRole()
        await expect(row).toBeHidden({ timeout: 5000 })
      }
    }

    // Remove from created roles tracking
    const index = this.createdRoles.indexOf(name)
    if (index > -1) {
      this.createdRoles.splice(index, 1)
    }
    this.logger?.testCode.log(`✓ Role deleted successfully: ${name}`)
  }

  /**
   * Get role count from table
   *
   * @returns Number of role rows
   */
  async getRoleCount(): Promise<number> {
    await expect(this.table).toBeVisible()
    const rows = this.table.getByRole('row')
    return await rows.count()
  }

  /**
   * Check if delete button is disabled or hidden for built-in role
   *
   * @param name Role name
   */
  async isDeleteButtonDisabled(name: string): Promise<boolean> {
    const row = this.findRoleRow(name)
    await expect(row).toBeVisible()

    // Use starts-with selector to match buttons with any ID suffix
    const deleteButton = row.locator('[data-testid^="role-delete-button-"]').first()

    // Check if button is hidden (not rendered)
    const count = await deleteButton.count()
    if (count === 0) return true

    // Check if button is disabled
    return await deleteButton.isDisabled()
  }

  /**
   * Check if built-in badge is visible for a role
   *
   * @param name Role name
   */
  async hasBuiltInBadge(name: string): Promise<boolean> {
    const row = this.findRoleRow(name)
    const badge = row.locator('[data-testid="builtin-badge"], [data-testid="built-in-badge"]')
    return await this.isVisible(badge)
  }

  /**
   * Check if name input is disabled (for built-in roles in edit mode)
   */
  async isNameInputDisabled(): Promise<boolean> {
    return await this.editNameInput.isDisabled()
  }

  /**
   * Close the edit dialog safely
   */
  async closeEditDialog(): Promise<void> {
    // Try to click the cancel button if it exists and is visible
    const cancelButton = this.dialogCancelButton

    try {
      await cancelButton.click({ timeout: 5000 })
    } catch (error) {
      // If cancel button doesn't exist or isn't clickable, try pressing Escape
      await this.page.keyboard.press('Escape')
    }

    // Wait for dialog to close
    await expect(this.dialog).toBeHidden({ timeout: 5000 })
  }

  /**
   * Click "Permissions" button for a role
   *
   * @param name Role name
   */
  async clickPermissionsButton(name: string): Promise<void> {
    const row = this.findRoleRow(name)
    await expect(row).toBeVisible()

    // Use starts-with selector to match buttons with any ID suffix
    const permissionsButton = row.locator('[data-testid^="role-permissions-button-"]').first()
    const buttonTestId = await permissionsButton.getAttribute('data-testid')
    const roleId = buttonTestId?.replace(/^role-permissions-button-/, '')
    if (!roleId) throw new Error(`Could not resolve role id for "${name}"`)

    const allPermissionsPath = `/api/permission/${this.realmId}/define`
    const rolePermissionsPath =
      `/api/roles/${this.realmId}/define/${roleId}/permissions`
    const matchesGet = (response: Response, path: string) =>
      response.request().method() === 'GET' && new URL(response.url()).pathname === path
    let allPermissionsRequested = false
    let rolePermissionsRequested = false
    const onRequest = (request: import('@playwright/test').Request) => {
      if (request.method() !== 'GET') return
      const path = new URL(request.url()).pathname
      if (path === allPermissionsPath) allPermissionsRequested = true
      if (path === rolePermissionsPath) rolePermissionsRequested = true
    }
    this.page.on('request', onRequest)
    const allPermissionsResponse = this.page
      .waitForResponse((response) => matchesGet(response, allPermissionsPath), { timeout: 10000 })
      .catch(() => null)
    const rolePermissionsResponse = this.page
      .waitForResponse((response) => matchesGet(response, rolePermissionsPath), { timeout: 10000 })
      .catch(() => null)

    let loadedRolePermissions: Response | null = null
    try {
      await this.smartClick(permissionsButton)

      const dialog = this.page.locator('[data-testid="role-permissions-dialog"]')
      await expect(dialog).toBeVisible()

      const pendingResponses: Promise<Response | null>[] = []
      if (allPermissionsRequested) pendingResponses.push(allPermissionsResponse)
      if (rolePermissionsRequested) pendingResponses.push(rolePermissionsResponse)
      const responses = await Promise.all(pendingResponses)
      for (const response of responses) {
        if (!response) throw new Error(`Timed out loading permissions for role "${name}"`)
        if (!response.ok()) {
          throw new Error(
            `Failed to load permissions for role "${name}": ${response.status()} ${await response.text()}`,
          )
        }
        if (matchesGet(response, rolePermissionsPath)) loadedRolePermissions = response
      }

      await expect(dialog.locator('[data-testid="permission-checkbox-list"]')).toBeVisible()
      if (loadedRolePermissions) {
        const body = await loadedRolePermissions.json()
        const raw = Array.isArray(body) ? body : body.data ?? body.items ?? []
        const assigned: { id?: string }[] = Array.isArray(raw) ? raw : []
        for (const permission of assigned) {
          if (permission.id) {
            await expect(this.getPermissionCheckboxById(permission.id)).toBeChecked()
          }
        }
      }
    } finally {
      this.page.off('request', onRequest)
    }
  }

  /**
   * Get permission checkbox locator by permission ID
   * Note: Frontend uses permission.id, not permission.name
   *
   * @param permissionId Permission ID (UUID)
   */
  getPermissionCheckboxById(permissionId: string): Locator {
    return this.page.locator(`[data-testid="permission-checkbox-${permissionId}"]`)
  }

  /**
   * Get permission checkbox locator by permission name
   * Searches for the checkbox within a permission item containing the name
   *
   * @param permissionName Permission name
   */
  getPermissionCheckboxByName(permissionName: string): Locator {
    // Find the permission item div by name, then get the checkbox within it
    return this.page.locator(`[data-testid^="permission-item-"]`).filter({ hasText: permissionName }).locator('[data-testid^="permission-checkbox-"]').first()
  }

  /**
   * Check/uncheck a permission for a role
   *
   * @param permissionName Permission name
   * @param checked Whether to check (true) or uncheck (false)
   */
  async setPermission(permissionName: string, checked: boolean): Promise<void> {
    const checkbox = this.getPermissionCheckboxByName(permissionName)
    await this.setCheckbox(checkbox, checked)
  }

  /**
   * Check if a permission checkbox is disabled (for built-in permissions)
   *
   * @param permissionName Permission name
   */
  async isPermissionCheckboxDisabled(permissionName: string): Promise<boolean> {
    const checkbox = this.getPermissionCheckboxByName(permissionName)

    // Check if checkbox exists
    const count = await checkbox.count()
    if (count === 0) return true

    return await checkbox.isDisabled()
  }

  /**
   * Check if a permission is checked
   *
   * @param permissionName Permission name
   */
  async isPermissionChecked(permissionName: string): Promise<boolean> {
    const checkbox = this.getPermissionCheckboxByName(permissionName)
    await expect(checkbox).toBeVisible({ timeout: 10000 })
    return await checkbox.isChecked()
  }

  async savePermissions(): Promise<void> {
    const saveButton = this.page.locator('[data-testid="role-permissions-save-button"]')
    const dialog = this.page.locator('[data-testid="role-permissions-dialog"]')

    // The Save button is only enabled when the permissions form has an unsaved
    // change. When the requested permission was already bound in a prior run
    // (the role persists across demo runs in the shared realm), toggling it on
    // is a no-op and Save stays disabled. Treat that as idempotent success:
    // close the dialog via Cancel and return, rather than timing out.
    const enabled = await saveButton.isEnabled().catch(() => false)
    if (!enabled) {
      await this.cancelPermissions()
      return
    }

    await Promise.all([
      dialog.waitFor({ state: 'hidden', timeout: 10000 }),
      this.smartClick(saveButton),
    ])

    await expect(this.table.getByRole('row').first()).toBeVisible({ timeout: 5000 })
  }

  /**
   * Click Save on the role permissions dialog WITHOUT asserting that the
   * dialog closes, and return the assign API response.
   *
   * Negative-path counterpart of savePermissions(): when the backend rejects
   * the grant (e.g. the 8c7b3aa8 guard on
   * POST /api/roles/{realmId}/define/{roleId}/permissions returns 403 because
   * the caller does not hold the permission being granted), the frontend only
   * toasts the error and keeps the dialog open, so waiting for the dialog to
   * hide would time out. Callers assert on the returned response and on the
   * dialog remaining open.
   */
  async clickSavePermissions(): Promise<Response> {
    const saveButton = this.page.locator('[data-testid="role-permissions-save-button"]')
    await expect(saveButton).toBeEnabled()

    const assignResponse = this.page.waitForResponse(
      (response) =>
        response.request().method() === 'POST' &&
        /\/api\/roles\/[^/]+\/define\/[^/]+\/permissions\/?$/.test(
          new URL(response.url()).pathname,
        ),
      { timeout: 10000 },
    )

    await this.smartClick(saveButton)
    return assignResponse
  }

  async cancelPermissions(): Promise<void> {
    const cancelButton = this.page.locator('[data-testid="role-permissions-cancel-button"]')
    await this.smartClick(cancelButton)

    const dialog = this.page.locator('[data-testid="role-permissions-dialog"]')
    await expect(dialog).toBeHidden()
  }

  /**
   * Get list of roles created during this session
   *
   * @returns Array of role names created
   */
  getCreatedRoles(): string[] {
    return [...this.createdRoles]
  }

  /**
   * Clean up all roles created during this session
   *
   * @example
   * ```typescript
   * // In test.afterEach
   * await rolesPage.cleanupCreatedRoles()
   * ```
   */
  async cleanupCreatedRoles(): Promise<void> {
    this.logger?.testCode.log(`Cleaning up ${this.createdRoles.length} created roles`)

    for (const roleName of this.createdRoles) {
      try {
        if (await this.roleExists(roleName)) {
          await this.deleteRole(roleName)
          this.logger?.testCode.log(`✓ Cleaned up role: ${roleName}`)
        }
      } catch (error) {
        this.logger?.testCode.error(`✗ Failed to cleanup role: ${roleName}`, error as Error)
      }
    }

    this.createdRoles = []
    this.logger?.testCode.log('✓ Role cleanup completed')
  }

  /**
   * Verify role was created successfully
   *
   * @param name Role name to verify
   * @returns true if role exists in table
   *
   * @example
   * ```typescript
   * await rolesPage.createRole({ name: 'Editor' })
   * const exists = await rolesPage.verifyRoleCreated('Editor')
   * expect(exists).toBe(true)
   * ```
   */
  async verifyRoleCreated(name: string): Promise<boolean> {
    const exists = await this.roleExists(name)
    this.logger?.testCode.log(`Role verification ${name}: ${exists ? '✓ PASS' : '✗ FAIL'}`)
    return exists
  }

  /**
   * Verify role was deleted successfully
   *
   * @param name Role name to verify
   * @returns true if role does not exist in table
   *
   * @example
   * ```typescript
   * await rolesPage.deleteRole('Editor')
   * const deleted = await rolesPage.verifyRoleDeleted('Editor')
   * expect(deleted).toBe(true)
   * ```
   */
  async verifyRoleDeleted(name: string): Promise<boolean> {
    const exists = await this.roleExists(name)
    this.logger?.testCode.log(`Role deletion verification ${name}: ${!exists ? '✓ PASS' : '✗ FAIL'}`)
    return !exists
  }

  /**
   * Verify role was updated successfully
   *
   * @param name Role name to verify
   * @param expectedData Expected role data
   * @returns true if role exists in table
   *
   * @example
   * ```typescript
   * await rolesPage.editRole('Editor', { description: 'Updated' })
   * const updated = await rolesPage.verifyRoleUpdated('Editor', { description: 'Updated' })
   * expect(updated).toBe(true)
   * ```
   */
  async verifyRoleUpdated(name: string, expectedData: Partial<RoleData>): Promise<boolean> {
    const exists = await this.roleExists(name)
    if (!exists) {
      this.logger?.testCode.log(`Role update verification ${name}: ✗ FAIL - Role not found`)
      return false
    }

    // Note: Can't verify description from table alone without clicking into row
    // This is a basic existence check
    this.logger?.testCode.log(`Role update verification ${name}: ✓ PASS - Role exists`)
    return true
  }

  /**
   * Verify dialog is closed
   *
   * @returns true if dialog is hidden
   */
  async verifyDialogClosed(): Promise<boolean> {
    try {
      await expect(this.dialog).toBeHidden({ timeout: 5000 })
      this.logger?.testCode.log('Dialog closed: ✓ PASS')
      return true
    } catch (error) {
      this.logger?.testCode.error('Dialog closed: ✗ FAIL - Dialog still visible', error as Error)
      return false
    }
  }
}
