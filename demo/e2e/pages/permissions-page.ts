/**
 * Permissions Page Object
 *
 * Encapsulates permission management page operations.
 * Provides methods for creating, editing, and deleting permissions.
 *
 */

import { Page, Locator, expect } from '@playwright/test'
import { SELECTORS } from '../selectors'
import { BasePage } from './base-page'
import type { UnifiedLogger } from '../helpers/unified-logger'

/**
 * Permission data interface
 *
 * Note: Permission name follows the format "resource.action" (e.g., "users.view")
 */
export interface PermissionData {
  name: string
  description?: string
}

/**
 * Permissions Page Object
 *
 * Represents the permission management page at /{realmId}/permissions
 *
 * @example
 * ```typescript
 * const permissionsPage = new PermissionsPage(page, logger)
 * await permissionsPage.goto()
 * await permissionsPage.createPermission({ name: 'users.view', description: 'View users' })
 * ```
 */
export class PermissionsPage extends BasePage {
  // Selectors
  readonly container: Locator
  readonly heading: Locator
  readonly table: Locator
  readonly createButton: Locator
  readonly dialog: Locator
  readonly dialogTitle: Locator
  readonly createNameInput: Locator
  readonly createDescriptionInput: Locator
  readonly createSubmitButton: Locator
  readonly editNameInput: Locator
  readonly editDescriptionInput: Locator
  readonly editSubmitButton: Locator
  readonly dialogCancelButton: Locator
  readonly descriptionInput: Locator // Alias for edit mode

  private createdPermissions: string[] = [] // Track created permissions for cleanup

  constructor(page: Page, logger?: UnifiedLogger) {
    super(page)
    this.logger = logger
    this.container = page.locator('[data-testid="permissions-page"]')
    this.heading = page.locator('[data-testid="permissions-heading"]')
    this.table = page.locator('[data-testid="permissions-table"]')
    this.createButton = page.locator('[data-testid="permission-create-button"]')

    // Create dialog selectors
    // Support both dialog and alertdialog roles (Radix UI uses alertdialog)
    this.dialog = page.locator('[role="dialog"], [role="alertdialog"]')
    this.dialogTitle = page.locator('[data-testid="dialog-title"]')
    this.createNameInput = page.locator('[data-testid="permission-create-name-input"]')
    this.createDescriptionInput = page.locator('[data-testid="permission-create-description-input"]')
    this.createSubmitButton = page.locator('[data-testid="permission-create-submit-button"]')

    // Edit dialog selectors
    this.editNameInput = page.locator('[data-testid="permission-edit-name-input"]')
    this.editDescriptionInput = page.locator('[data-testid="permission-edit-description-input"]')
    this.editSubmitButton = page.locator('[data-testid="permission-edit-submit-button"]')

    // Common dialog selectors
    // Try multiple possible selectors for cancel button (fallbacks for different dialog implementations)
    this.dialogCancelButton = page.locator([
      '[data-testid="dialog-cancel-button"]',
      '[data-testid="permission-edit-cancel-button"]',
      'button:has-text("Cancel")',
      'button:has-text("取消")',
    ].join(', '))

    // Aliases for convenience
    this.descriptionInput = this.editDescriptionInput // Alias for edit mode
  }

  /**
   * Navigate to permissions page
   *
   * @param realmId Realm ID (defaults to 'admin' for backward compatibility)
   */
  async goto(realmId: string = 'admin'): Promise<void> {
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
    // Permissions 是 Authorization 的子菜单，需要先展开
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

    // 等待 Permissions 菜单项可见（权限加载完成后才显示）
    const permissionsMenuLink = this.page.locator(SELECTORS.sidebar.menuPermissions)
    await permissionsMenuLink.waitFor({ state: 'visible', timeout: 10000 })
    await this.smartClick(permissionsMenuLink)

    // 等待页面加载完成
    await this.waitForReady()
  }

  /**
   * Wait for permissions page to be visible
   */
  async waitForReady(): Promise<void> {
    await expect(this.container).toBeVisible()
    await expect(this.heading).toBeVisible()
    await expect(this.table).toBeVisible()
  }

  /**
   * Click "Create Permission" button to open create dialog
   */
  async clickCreatePermission(): Promise<void> {
    await this.smartClick(this.createButton)
    await expect(this.dialog).toBeVisible()
    await expect(this.dialogTitle).toHaveText(/Create Permission|Add Permission|New Permission/i)
  }

  /**
   * Fill permission form fields
   *
   * @param permissionData Permission data to fill
   */
  async fillPermissionForm(permissionData: PermissionData): Promise<void> {
    await this.fillField(this.createNameInput, permissionData.name)

    if (permissionData.description) {
      await this.fillField(this.createDescriptionInput, permissionData.description)
    }
  }

  /**
   * Submit the permission form (click submit button)
   */
  async submitPermissionForm(): Promise<void> {
    // Check if submit button is disabled (validation error)
    const isDisabled = await this.createSubmitButton.isDisabled()
    if (isDisabled) {
      // Log input values for debugging
      const nameValue = await this.createNameInput.inputValue()
      this.logger?.testCode.error(`Submit button disabled. Name input: "${nameValue}"`)
      throw new Error('Submit button is disabled - form validation may have failed')
    }

    this.logger?.testCode.log('Submitting permission form...')

    // Click submit button and wait for dialog to close
    await Promise.all([
      // Wait for dialog to close (create name input becomes hidden)
      this.createNameInput.waitFor({ state: 'hidden', timeout: 10000 }),
      this.smartClick(this.createSubmitButton)
    ])

    // Wait for data table to refresh using role-based selectors
    // getByRole('row') matches both header and data rows, so we filter for content
    const dataRows = this.table.getByRole('row').filter({ hasText: /.+/ })
    await expect(dataRows.first()).toBeVisible({ timeout: 5000 })

    this.logger?.testCode.log('✓ Permission form submitted successfully')
  }

  /**
   * Create a new permission
   *
   * @param permissionData Permission data
   *
   * @example
   * ```typescript
   * await permissionsPage.createPermission({
   *   name: 'users.view',
   *   description: 'View user list'
   * })
   * ```
   */
  async createPermission(permissionData: PermissionData): Promise<void> {
    this.logger?.testCode.log(`Creating permission: ${permissionData.name}`)
    await this.clickCreatePermission()
    await this.fillPermissionForm(permissionData)
    await this.submitPermissionForm()

    // Track created permission for cleanup
    this.createdPermissions.push(permissionData.name)
    this.logger?.testCode.log(`✓ Permission created successfully: ${permissionData.name}`)
  }

  /**
   * Find permission row in table by name
   *
   * @param name Permission name to search for
   * @returns Row locator
   */
  findPermissionRow(name: string): Locator {
    return this.table.locator(`tr:has-text("${name}")`).first()
  }

  /**
   * Check if permission exists in table
   *
   * @param name Permission name to check
   */
  async permissionExists(name: string): Promise<boolean> {
    const row = this.findPermissionRow(name)
    return await this.isVisible(row)
  }

  /**
   * Click "Edit" button for a permission
   *
   * @param name Permission name
   */
  async clickEditPermission(name: string): Promise<void> {
    const row = this.findPermissionRow(name)
    await expect(row).toBeVisible()

    // Use starts-with selector to match buttons with any ID suffix
    const editButton = row.locator('[data-testid^="permission-edit-button-"]').first()
    await this.smartClick(editButton)

    await expect(this.dialog).toBeVisible()
    await expect(this.dialogTitle).toHaveText(/Edit Permission|Update Permission/i)
  }

  /**
   * Edit an existing permission
   *
   * @param name Permission name to edit
   * @param updatedData New permission data
   *
   * @example
   * ```typescript
   * await permissionsPage.editPermission('users.view', {
   *   description: 'Updated description'
   * })
   * ```
   */
  async editPermission(name: string, updatedData: Partial<PermissionData>): Promise<void> {
    this.logger?.testCode.log(`Editing permission: ${name}`)
    await this.clickEditPermission(name)

    // Only description can be edited for existing permissions (name is disabled for built-in)
    if (updatedData.description) {
      await this.fillField(this.editDescriptionInput, updatedData.description)
    }

    // Click submit button and wait for dialog to close
    await Promise.all([
      // Wait for dialog to close (edit name input becomes hidden)
      this.editNameInput.waitFor({ state: 'hidden', timeout: 10000 }),
      this.smartClick(this.editSubmitButton)
    ])

    // Wait for data table to refresh using role-based selectors
    const dataRows = this.table.getByRole('row').filter({ hasText: /.+/ })
    await expect(dataRows.first()).toBeVisible({ timeout: 5000 })

    this.logger?.testCode.log(`✓ Permission edited successfully: ${name}`)
  }

  /**
   * Click "Delete" button for a permission
   *
   * @param name Permission name
   */
  async clickDeletePermission(name: string): Promise<void> {
    const row = this.findPermissionRow(name)
    await expect(row).toBeVisible()

    // Use starts-with selector to match buttons with any ID suffix
    const deleteButton = row.locator('[data-testid^="permission-delete-button-"]').first()
    await this.smartClick(deleteButton)

    await expect(this.dialog).toBeVisible()
    await expect(this.dialogTitle).toHaveText(/Delete Permission|Confirm Delete/i)
  }

  /**
   * Confirm permission deletion
   */
  async confirmDeletePermission(): Promise<void> {
    // Use multiple fallback selectors to handle different naming conventions
    const confirmButton = this.page.locator([
      '[data-testid="dialog-submit-button"]',              // Generic naming
      '[data-testid="confirm-delete-button"]',              // Old naming
      '[data-testid="permission-delete-confirm-button"]',   // ✅ Actual frontend selector
      'button:has-text("Delete")',                          // Text fallback
    ].join(', '))

    // Click confirm button and wait for dialog to close
    await Promise.all([
      this.dialog.waitFor({ state: 'hidden', timeout: 10000 }),
      this.smartClick(confirmButton)
    ])

    // Wait for data table to refresh using role-based selectors
    const dataRows = this.table.getByRole('row').filter({ hasText: /.+/ })
    await expect(dataRows.first()).toBeVisible({ timeout: 5000 })
  }

  /**
   * Delete a permission
   *
   * @param name Permission name to delete
   *
   * @example
   * ```typescript
   * await permissionsPage.deletePermission('users.view')
   * ```
   */
  async deletePermission(name: string): Promise<void> {
    this.logger?.testCode.log(`Deleting permission: ${name}`)
    await this.clickDeletePermission(name)
    await this.confirmDeletePermission()

    // Wait for the row to be removed (AlertDialog closes before mutation completes)
    const row = this.findPermissionRow(name)
    try {
      await expect(row).toBeHidden({ timeout: 5000 })
    } catch {
      // Mutation may have failed - reload and retry once
      this.logger?.testCode.log(`Permission "${name}" still visible, retrying...`)
      await this.page.reload()
      await this.waitForReady()
      if (await this.permissionExists(name)) {
        await this.clickDeletePermission(name)
        await this.confirmDeletePermission()
        await expect(row).toBeHidden({ timeout: 5000 })
      }
    }

    // Remove from created permissions tracking
    const index = this.createdPermissions.indexOf(name)
    if (index > -1) {
      this.createdPermissions.splice(index, 1)
    }
    this.logger?.testCode.log(`✓ Permission deleted successfully: ${name}`)
  }

  /**
   * Check if delete button is disabled or hidden for built-in permission
   *
   * @param name Permission name
   */
  async isDeleteButtonDisabled(name: string): Promise<boolean> {
    const row = this.findPermissionRow(name)
    await expect(row).toBeVisible()

    // 使用多重回退选择器提高测试健壮性
    const deleteButton = row.locator([
      '[data-testid^="permission-delete-button-"]',  // 主选择器
      '[data-testid="delete-button"]',               // 回退 1
      'button:has-text("Delete")',                   // 回退 2：文本
    ].join(', ')).first()

    // Check if button is hidden (not rendered)
    const count = await deleteButton.count()
    if (count === 0) return true

    // Check if button is disabled
    return await deleteButton.isDisabled()
  }

  /**
   * Check if edit button is disabled or hidden for built-in permission
   *
   * @param name Permission name
   */
  async isEditButtonDisabled(name: string): Promise<boolean> {
    const row = this.findPermissionRow(name)
    await expect(row).toBeVisible()

    // 使用多重回退选择器提高测试健壮性
    const editButton = row.locator([
      '[data-testid^="permission-edit-button-"]',  // 主选择器
      '[data-testid="edit-button"]',               // 回退 1
      'button:has-text("Edit")',                   // 回退 2：文本
    ].join(', ')).first()

    // Check if button is hidden (not rendered)
    const count = await editButton.count()
    if (count === 0) return true

    // Check if button is disabled
    return await editButton.isDisabled()
  }

  /**
   * Check if built-in badge is visible for a permission
   *
   * @param name Permission name
   */
  async hasBuiltInBadge(name: string): Promise<boolean> {
    const row = this.findPermissionRow(name)
    const badge = row.locator('[data-testid="builtin-badge"], [data-testid="built-in-badge"]')
    return await this.isVisible(badge)
  }

  /**
   * Get permission count from table
   *
   * @returns Number of permission rows
   */
  async getPermissionCount(): Promise<number> {
    await expect(this.table).toBeVisible()
    const rows = this.table.getByRole('row')
    return await rows.count()
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
   * Get list of permissions created during this session
   *
   * @returns Array of permission names created
   */
  getCreatedPermissions(): string[] {
    return [...this.createdPermissions]
  }

  /**
   * Clean up all permissions created during this session
   *
   * @example
   * ```typescript
   * // In test.afterEach
   * await permissionsPage.cleanupCreatedPermissions()
   * ```
   */
  async cleanupCreatedPermissions(): Promise<void> {
    this.logger?.testCode.log(`Cleaning up ${this.createdPermissions.length} created permissions`)

    for (const permissionName of this.createdPermissions) {
      try {
        if (await this.permissionExists(permissionName)) {
          await this.deletePermission(permissionName)
          this.logger?.testCode.log(`✓ Cleaned up permission: ${permissionName}`)
        }
      } catch (error) {
        this.logger?.testCode.error(`✗ Failed to cleanup permission: ${permissionName}`, error as Error)
      }
    }

    this.createdPermissions = []
    this.logger?.testCode.log('✓ Permission cleanup completed')
  }

  /**
   * Verify permission was created successfully
   *
   * @param name Permission name to verify
   * @returns true if permission exists in table
   *
   * @example
   * ```typescript
   * await permissionsPage.createPermission({ name: 'users.view' })
   * const exists = await permissionsPage.verifyPermissionCreated('users.view')
   * expect(exists).toBe(true)
   * ```
   */
  async verifyPermissionCreated(name: string): Promise<boolean> {
    const exists = await this.permissionExists(name)
    this.logger?.testCode.log(`Permission verification ${name}: ${exists ? '✓ PASS' : '✗ FAIL'}`)
    return exists
  }

  /**
   * Verify permission was deleted successfully
   *
   * @param name Permission name to verify
   * @returns true if permission does not exist in table
   *
   * @example
   * ```typescript
   * await permissionsPage.deletePermission('users.view')
   * const deleted = await permissionsPage.verifyPermissionDeleted('users.view')
   * expect(deleted).toBe(true)
   * ```
   */
  async verifyPermissionDeleted(name: string): Promise<boolean> {
    const exists = await this.permissionExists(name)
    this.logger?.testCode.log(`Permission deletion verification ${name}: ${!exists ? '✓ PASS' : '✗ FAIL'}`)
    return !exists
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
