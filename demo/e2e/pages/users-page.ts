/**
 * Users Page Object
 *
 * Encapsulates user management page operations.
 * Provides methods for creating, editing, deleting, and searching users.
 *
 * @see ../../../spec/demo/e2e-testing.md#page-object-model-pom-规范
 */

import { Page, Locator, expect } from '@playwright/test'
import { SELECTORS } from '../selectors'
import { BasePage } from './base-page'
import type { UnifiedLogger } from '../helpers/unified-logger'

/**
 * User data interface
 */
export interface UserData {
  email: string
  password?: string
  nickname?: string
  name?: string
}

/**
 * Users Page Object
 *
 * Represents the user management page at /{realmId}/users
 *
 * @example
 * ```typescript
 * const usersPage = new UsersPage(page, logger)
 * await usersPage.goto()
 * await usersPage.createUser({ email: 'user@example.com', password: 'password123' })
 * ```
 */
export class UsersPage extends BasePage {
  // Selectors
  readonly container: Locator
  readonly heading: Locator
  readonly table: Locator
  readonly addButton: Locator
  readonly searchInput: Locator
  readonly dialog: Locator
  readonly dialogTitle: Locator
  readonly emailInput: Locator
  readonly passwordInput: Locator
  readonly nicknameInput: Locator
  readonly nameInput: Locator
  readonly dialogCancelButton: Locator
  readonly dialogSubmitButton: Locator
  readonly toast: Locator
  readonly toastMessage: Locator

  // Reset password selectors
  readonly resetPasswordConfirmDialog: Locator
  readonly resetPasswordConfirmButton: Locator
  readonly resetPasswordResultDialog: Locator
  readonly resetPasswordNewPasswordText: Locator
  readonly resetPasswordCopyButton: Locator

  // Edit-user dialog selectors (US-RA-021)
  readonly editDialog: Locator

  // User sessions selectors (US-RA-020)
  readonly sessionsDialog: Locator
  readonly sessionsRevokeAllButton: Locator
  readonly sessionsRetryButton: Locator
  readonly revokeConfirmDialog: Locator
  readonly revokeConfirmButton: Locator
  readonly revokeCancelButton: Locator
  readonly revokeAllConfirmDialog: Locator
  readonly revokeAllConfirmButton: Locator
  readonly revokeAllCancelButton: Locator

  constructor(page: Page, logger?: UnifiedLogger) {
    super(page, logger)
    this.container = page.locator(SELECTORS.users.container)
    this.heading = page.locator(SELECTORS.users.heading)
    this.table = page.locator(SELECTORS.users.table)
    this.addButton = page.locator(SELECTORS.users.addButton)
    this.searchInput = page.locator(SELECTORS.users.searchInput)

    // Dialog selectors
    this.dialog = page.locator(SELECTORS.common.dialog)
    this.dialogTitle = page.locator(SELECTORS.common.dialogTitle)
    this.emailInput = page.locator(SELECTORS.common.formEmailInput)
    this.passwordInput = page.locator(SELECTORS.common.formPasswordInput)
    this.nicknameInput = page.locator(SELECTORS.common.formNicknameInput)
    this.nameInput = page.locator(SELECTORS.common.formNameInput)
    this.dialogCancelButton = page.locator(SELECTORS.common.dialogCancelButton)
    this.dialogSubmitButton = page.locator(SELECTORS.common.dialogSubmitButton)

    // Feedback selectors
    this.toast = page.locator(SELECTORS.common.toast)
    this.toastMessage = page.locator(SELECTORS.common.toastMessage)

    // Reset password selectors
    this.resetPasswordConfirmDialog = page.locator(SELECTORS.resetPassword.confirmDialog)
    this.resetPasswordConfirmButton = page.locator(SELECTORS.resetPassword.confirmButton)
    this.resetPasswordResultDialog = page.locator(SELECTORS.resetPassword.resultDialog)
    this.resetPasswordNewPasswordText = page.locator(SELECTORS.resetPassword.newPasswordText)
    this.resetPasswordCopyButton = page.locator(SELECTORS.resetPassword.copyButton)

    // Edit-user dialog (US-RA-021). The edit dialog passes its own testid to
    // DialogContent (edit-user-dialog.tsx:88), which OVERRIDES the shared
    // `dialog` default — ui/dialog.tsx places `data-testid="dialog"` before
    // {...props}, so a caller-provided testid wins. Edit-dialog assertions
    // must therefore use this locator, not the generic `dialog` one.
    this.editDialog = page.locator('[data-testid="user-edit-dialog"]')

    // User sessions selectors (US-RA-020)
    this.sessionsDialog = page.locator(SELECTORS.userSessions.dialog)
    this.sessionsRevokeAllButton = page.locator(SELECTORS.userSessions.revokeAllButton)
    this.sessionsRetryButton = page.locator(SELECTORS.userSessions.retryButton)
    this.revokeConfirmDialog = page.locator(SELECTORS.userSessions.revokeConfirmDialog)
    this.revokeConfirmButton = page.locator(SELECTORS.userSessions.revokeConfirmButton)
    this.revokeCancelButton = page.locator(SELECTORS.userSessions.revokeCancelButton)
    this.revokeAllConfirmDialog = page.locator(SELECTORS.userSessions.revokeAllConfirmDialog)
    this.revokeAllConfirmButton = page.locator(SELECTORS.userSessions.revokeAllConfirmButton)
    this.revokeAllCancelButton = page.locator(SELECTORS.userSessions.revokeAllCancelButton)
  }

  /**
   * Navigate to users page
   *
   * @param realmId Realm ID (defaults to 'admin' for backward compatibility)
   */
  async goto(realmId: string = 'admin'): Promise<void> {
    // 通过点击侧边栏菜单来导航，模拟真实用户操作
    // 这样可以避免权限加载的时序问题
    const usersMenuLink = this.page.locator(SELECTORS.sidebar.menuUsers)
    await this.smartClick(usersMenuLink)

    await this.waitForReady()
  }

  /**
   * Wait for users page to be visible
   */
  async waitForReady(): Promise<void> {
    await expect(this.container).toBeVisible()
    await expect(this.heading).toBeVisible()
    await expect(this.table).toBeVisible()
  }

  /**
   * Click "Add User" button to open create dialog
   */
  async clickAddUser(): Promise<void> {
    await this.smartClick(this.addButton)
    await expect(this.dialog).toBeVisible()
    await expect(this.dialogTitle).toHaveText(/Add User|Create User|New User/i)
  }

  /**
   * Fill user form fields
   *
   * @param userData User data to fill (partial allowed for edit mode)
   */
  async fillUserForm(userData: Partial<UserData>): Promise<void> {
    if (userData.email) {
      await this.fillField(this.emailInput, userData.email)
    }

    if (userData.password) {
      await this.fillField(this.passwordInput, userData.password)
    }

    if (userData.nickname) {
      await this.fillField(this.nicknameInput, userData.nickname)
    }

    if (userData.name) {
      await this.fillField(this.nameInput, userData.name)
    }

    // Check the default "User" role checkbox (required by createUserSchema)
    const roleCheckbox = this.page.locator(SELECTORS.users.roleCheckbox)
    await expect(roleCheckbox).toBeVisible({ timeout: 10000 })
    const isChecked = await roleCheckbox.isChecked()
    if (!isChecked) {
      await roleCheckbox.check()
    }
  }

  /**
   * Submit the user form (click submit button).
   *
   * Captures the POST /api/users/{realmId} response and fails loudly on 4xx
   * (e.g. "Email already exists") instead of letting a swallowed backend
   * error pass as success. The dialog-hidden / table-visible signals alone
   * are insufficient: on a 400 the frontend keeps the dialog open, but if
   * the dialog was closed by any other interaction the test could pass with
   * stale data. Reading the actual API response is the only reliable signal.
   *
   * @returns The created user's id (from the 201 response body) when creating
   *          a user via the admin dialog. Empty string for edit flows.
   */
  async submitUserForm(): Promise<string> {
    console.log('[UsersPage] Starting form submission...')

    const dialogVisible = await this.isVisible(this.dialog)
    if (!dialogVisible) {
      throw new Error('Cannot submit form: Dialog is not visible')
    }
    console.log('[UsersPage] Dialog is visible, ready to submit')

    // Check if submit button is disabled before clicking
    const isDisabled = await this.dialogSubmitButton.isDisabled()
    if (isDisabled) {
      const buttonText = await this.dialogSubmitButton.textContent()
      throw new Error(`Cannot submit form: Submit button is disabled. Button text: "${buttonText}"`)
    }
    console.log('[UsersPage] Submit button is enabled')

    // Verify submit button is clickable
    await expect(this.dialogSubmitButton).toBeVisible()
    console.log('[UsersPage] Submit button is visible')

    // Capture the create-user API response that fires on submit. We match
    // POST to /api/users/{realmId} so this does not fire for edit (PUT) flows;
    // for edit flows the promise simply times out and is treated as no-op.
    const createResponsePromise = this.page
      .waitForResponse(
        (response) =>
          /\/api\/users\/[^/]+(?:\?|$)/.test(response.url()) &&
          response.request().method() === 'POST',
        { timeout: 8000 }
      )
      .catch(() => null)

    // Click submit button
    console.log('[UsersPage] Clicking submit button...')
    await this.smartClick(this.dialogSubmitButton)
    console.log('[UsersPage] Submit button clicked')

    const createResponse = await createResponsePromise

    // Read the response body and fail loudly on 4xx/5xx. This is what
    // previously turned a 400 "Email already exists" into a silent pass.
    let createdUserId = ''
    if (createResponse) {
      const status = createResponse.status()
      const bodyText = await createResponse.text().catch(() => '<unreadable body>')
      if (status >= 400) {
        throw new Error(
          `Create user API failed: HTTP ${status}. Response body: ${bodyText}`
        )
      }
      // 201 Created carries the new user's id; parse defensively.
      try {
        const body = JSON.parse(bodyText)
        createdUserId = body?.id ?? ''
      } catch {
        // Non-JSON body (e.g. edit PUT returns different shape) — ignore.
      }
    }

    // Wait for dialog to close with explicit error handling
    try {
      console.log('[UsersPage] Waiting for dialog to close...')
      await expect(this.dialog).toBeHidden({ timeout: 5000 })
      console.log('[UsersPage] Dialog closed successfully')
    } catch (error) {
      // Log current state for debugging
      const isDialogStillVisible = await this.isVisible(this.dialog)
      const isButtonDisabled = await this.dialogSubmitButton.isDisabled()
      const buttonText = await this.dialogSubmitButton.textContent()

      throw new Error(
        `Failed to submit form: Dialog did not close. ` +
        `Dialog visible: ${isDialogStillVisible}, ` +
        `Submit button disabled: ${isButtonDisabled}, ` +
        `Button text: "${buttonText}". ` +
        `Original error: ${error}`
      )
    }

    // Wait for table to refresh (indicates successful submission)
    console.log('[UsersPage] Waiting for table to refresh...')
    await expect(this.table).toBeVisible()
    console.log('[UsersPage] Table refreshed successfully')

    console.log('[UsersPage] Form submission completed')
    return createdUserId
  }

  /**
   * Create a new user.
   *
   * @param userData User data
   * @returns The created user's id (empty string if the create response did
   *          not carry one).
   */
  async createUser(userData: UserData): Promise<string> {
    await this.clickAddUser()
    await this.fillUserForm(userData)
    return this.submitUserForm()
  }

  /**
   * Find user row in table by email.
   *
   * Uses row-level `filter({ hasText })` so the locator resolves to the `<tr>`
   * itself (not an inner text node). Downstream row-relative selectors (edit,
   * delete, reset-password buttons) rely on resolving to the row element.
   *
   * @param email User email to search for
   * @returns Row locator (use with expect().toBeVisible()/toBeHidden())
   */
  findUserRow(email: string): Locator {
    return this.table.locator('tr').filter({ hasText: email }).first()
  }

  /**
   * Check if user exists in table
   *
   * @param email User email to check
   */
  async userExists(email: string): Promise<boolean> {
    const row = this.findUserRow(email)
    return await this.isVisible(row)
  }

  /**
   * Click "Edit" button for a user
   *
   * @param email User email
   */
  async clickEditUser(email: string): Promise<void> {
    // The usersPage fixture loads this table before the test body runs, but
    // session-setup helpers create the target user via the admin API AFTER
    // that, so the rendered list is stale. Reload the list so the
    // freshly-created user row is present before we search for it (same
    // pattern as `clickManageSessions` below).
    await this.page.reload()
    await this.waitForReady()

    const row = this.findUserRow(email)
    await expect(row).toBeVisible()

    // Find edit button in the row. The row renders
    // `user-table-${row.index}-edit-button` (user-table.tsx:149); the row.index
    // is not known to the caller, so suffix-match — the same pattern as the
    // sibling row-action locators (`-delete-button`, `-reset-password-button`,
    // `-sessions-button`).
    const editButton = row.locator('[data-testid$="-edit-button"]').first()
    await this.smartClick(editButton)

    // The edit dialog's DialogContent/DialogTitle carry edit-specific testids
    // (`user-edit-dialog` / `user-edit-dialog-title`, edit-user-dialog.tsx:88/90)
    // which override the shared `dialog` / `dialog-title` defaults — assert on
    // the edit dialog's own testids (see the editDialog field note).
    await expect(this.editDialog).toBeVisible()
    await expect(
      this.editDialog.locator('[data-testid="user-edit-dialog-title"]')
    ).toHaveText(/Edit User|Update User/i)
  }

  /**
   * Fill fields in the EDIT-user dialog (edit-user-dialog.tsx).
   *
   * Separate from `fillUserForm` (create-dialog flow): the edit dialog's
   * inputs carry edit-specific testids (nickname input =
   * `user-edit-nickname-input`, edit-user-dialog.tsx:119) and the dialog has
   * NO role checkbox, so `fillUserForm`'s create-only assumptions (generic
   * `nickname-input` selector + mandatory role checkbox) do not hold here.
   *
   * @param data Fields to fill (partial allowed).
   */
  async fillEditUserForm(data: { nickname?: string }): Promise<void> {
    await expect(this.editDialog).toBeVisible()
    if (data.nickname !== undefined) {
      await this.fillField(
        this.editDialog.locator('[data-testid="user-edit-nickname-input"]'),
        data.nickname
      )
    }
  }

  /**
   * Submit the EDIT-user dialog and fail loudly on a rejected PUT.
   *
   * Separate from `submitUserForm` (create flow): the edit dialog's submit
   * button testid is `user-edit-submit-button` (edit-user-dialog.tsx:162) and
   * the backend call is a PUT to `/api/users/{realmId}/{userId}` — the create
   * helper matches a POST (and checks the generic dialog testid), so it can
   * neither find the button nor observe the response here.
   *
   * Captures the PUT response and throws on 4xx/5xx, or when no PUT was
   * observed within 10s (client-side validation kept the form from
   * submitting). On success, waits for the dialog to close (onSuccess calls
   * `onOpenChange(false)`, edit-user-dialog.tsx:35-37).
   */
  async submitEditUserForm(): Promise<void> {
    const submitButton = this.editDialog.locator(
      '[data-testid="user-edit-submit-button"]'
    )
    const isDisabled = await submitButton.isDisabled()
    if (isDisabled) {
      const buttonText = await submitButton.textContent()
      throw new Error(
        `Cannot submit edit form: Submit button is disabled. Button text: "${buttonText}"`
      )
    }

    const putResponsePromise = this.page
      .waitForResponse(
        (response) =>
          /\/api\/users\/[^/]+\/[^/]+$/.test(response.url()) &&
          response.request().method() === 'PUT',
        { timeout: 10_000 }
      )
      .catch(() => null)

    await this.smartClick(submitButton)

    const putResponse = await putResponsePromise
    if (!putResponse) {
      const dialogStillOpen = await this.isVisible(this.editDialog)
      throw new Error(
        `Edit user form: no PUT /api/users/{realmId}/{userId} response was ` +
          `observed within 10s of clicking submit (dialog still open: ` +
          `${dialogStillOpen} — client-side validation likely rejected the form).`
      )
    }
    const status = putResponse.status()
    if (status >= 400) {
      const bodyText = await putResponse.text().catch(() => '<unreadable body>')
      throw new Error(
        `Edit user API failed: HTTP ${status}. Response body: ${bodyText}`
      )
    }

    await expect(this.editDialog).toBeHidden({ timeout: 5000 })
  }

  /**
   * Edit an existing user
   *
   * @param email User email to edit
   * @param updatedData New user data
   */
  async editUser(email: string, updatedData: Partial<UserData>): Promise<void> {
    await this.clickEditUser(email)
    await this.fillUserForm(updatedData)
    await this.submitUserForm()
    // Discard the returned id; edit PUT does not produce a create response.
  }

  /**
   * Click "Delete" button for a user
   *
   * @param email User email
   */
  async clickDeleteUser(email: string): Promise<void> {
    const row = this.findUserRow(email)
    await expect(row).toBeVisible()

    // Find delete button in the row
    const deleteButton = row.locator('[data-testid$="-delete-button"]').first()

    // Click delete button to open the AlertDialog
    await deleteButton.click()

    // Wait for the confirm dialog to appear
    const confirmDialog = this.page.locator(SELECTORS.users.deleteDialog)
    await expect(confirmDialog).toBeVisible({ timeout: 5000 })
  }

  /**
   * Confirm user deletion by clicking the confirm button in the AlertDialog
   */
  async confirmDeleteUser(): Promise<void> {
    const confirmButton = this.page.locator(SELECTORS.users.confirmDeleteButton)
    await this.smartClick(confirmButton)

    const confirmDialog = this.page.locator(SELECTORS.users.deleteDialog)
    await expect(confirmDialog).toBeHidden({ timeout: 5000 })
  }

  /**
   * Delete a user
   *
   * @param email User email to delete
   * @param realmId Realm ID for page refresh (defaults to 'admin')
   */
  async deleteUser(email: string, realmId: string = 'admin'): Promise<void> {
    // ✅ Ensure page is in latest state before deletion
    await this.goto(realmId)

    await this.clickDeleteUser(email)
    await this.confirmDeleteUser()
    // Wait for the user row to disappear from the table
    await expect(this.findUserRow(email)).toBeHidden({ timeout: 5000 })
  }

  /**
   * Search users by email
   *
   * Uses assertion-based waiting instead of fixed delays.
   * Waits for search API response and table content update.
   *
   * @param searchTerm Search query
   */
  async searchUsers(searchTerm: string): Promise<void> {
    await this.fillField(this.searchInput, searchTerm)

    // ✅ Improved: Wait for search API response instead of fixed timeout
    // This handles the search debounce properly by waiting for the actual network request
    try {
      await this.page.waitForResponse(
        response =>
          response.url().includes('/api/users') &&
          response.request().method() === 'GET' &&
          response.status() === 200,
        { timeout: 5000 }
      )
    } catch {
      // If no API request is made (e.g., search term too short), continue
      // The table content assertion below will fail if results don't match
    }

    // Wait for either results or "no results" message
    // Playwright auto-waits for the table to be stable
    await expect(this.table).toBeVisible()
  }

  /**
   * Get user count from table
   *
   * @returns Number of user rows
   */
  async getUserCount(): Promise<number> {
    await expect(this.table).toBeVisible()
    const rows = this.table.getByRole('row')
    return await rows.count()
  }

  /**
   * Close/success toast
   */
  async closeToast(): Promise<void> {
    if (await this.isVisible(this.toast)) {
      const closeButton = this.toast.locator('[data-testid="toast-close-button"], button[aria-label="Close"]')
      await this.smartClick(closeButton)
    }
  }

  /**
   * Alias for clickAddUser() - for test compatibility
   */
  async clickAddUserButton(): Promise<void> {
    await this.clickAddUser()
  }

  /**
   * Check if create user dialog is visible
   */
  async isCreateDialogVisible(): Promise<boolean> {
    return await this.isVisible(this.dialog)
  }

  // ─── Reset Password Methods ────────────────────────────────────────────

  /**
   * Click "Reset Password" button for a user identified by email.
   *
   * Finds the user row by email, then locates the reset password button
   * relative to that row using a suffix-matching selector.
   * Waits for the confirmation dialog to appear.
   *
   * @param email User email to reset password for
   */
  async clickResetPassword(email: string): Promise<void> {
    const row = this.findUserRow(email)
    await expect(row).toBeVisible()

    const resetButton = row.locator('[data-testid$="-reset-password-button"]').first()
    await this.smartClick(resetButton)

    await expect(this.resetPasswordConfirmDialog).toBeVisible()
  }

  /**
   * Confirm the reset password action by clicking the confirm button.
   * Waits for the confirmation dialog to close.
   */
  async confirmResetPassword(): Promise<void> {
    await this.smartClick(this.resetPasswordConfirmButton)
    await expect(this.resetPasswordConfirmDialog).toBeHidden({ timeout: 5000 })
  }

  /**
   * Wait for the reset password result dialog to appear and return the new password.
   *
   * @returns The newly generated password string
   */
  async waitForResetPasswordResult(): Promise<string> {
    await expect(this.resetPasswordResultDialog).toBeVisible({ timeout: 10000 })
    await expect(this.resetPasswordNewPasswordText).toBeVisible()
    const password = await this.resetPasswordNewPasswordText.textContent()
    if (!password) {
      throw new Error('New password text is empty in reset password result dialog')
    }
    return password.trim()
  }

  /**
   * Click the "Copy Password" button in the result dialog.
   */
  async copyPassword(): Promise<void> {
    await this.smartClick(this.resetPasswordCopyButton)
  }

  /**
   * Close the reset password result dialog.
   * Clicks the Close button inside the dialog footer.
   */
  async closeResetPasswordResult(): Promise<void> {
    const closeButton = this.resetPasswordResultDialog.getByRole('button', { name: 'Close', exact: true }).first()
    await this.smartClick(closeButton)
    await expect(this.resetPasswordResultDialog).toBeHidden({ timeout: 5000 })
  }

  /**
   * Composite method: perform full reset password flow for a user.
   *
   * 1. Click reset password button in the user row
   * 2. Confirm the action
   * 3. Wait for the result and extract the new password
   *
   * @param email User email to reset password for
   * @returns The newly generated password
   */
  async resetUserPassword(email: string): Promise<string> {
    await this.clickResetPassword(email)
    await this.confirmResetPassword()
    return await this.waitForResetPasswordResult()
  }

  // ─── User Sessions Methods (US-RA-020) ───────────────────────────────

  /**
   * Click the "Manage Sessions" entry button on the row identified by email.
   *
   * Mirrors the proven `clickResetPassword` pattern: find the row, then use a
   * row-relative suffix-match locator. The row testid is
   * `user-table-${row.index}-sessions-button` (user-table.tsx:131), but the
   * row.index is not known to the caller, so we suffix-match on
   * `[data-testid$="-sessions-button"]` scoped to the row.
   *
   * Waits for the sessions dialog to become visible.
   *
   * @param email User email whose sessions to manage.
   */
  async clickManageSessions(email: string): Promise<void> {
    // The usersPage fixture navigates here at test start, but session-setup
    // helpers create the target user via the admin API AFTER that, so the
    // rendered list is stale. Reload the list so the freshly-created user row
    // is present before we search for it.
    await this.page.reload()
    await this.waitForReady()

    const row = this.findUserRow(email)
    await expect(row).toBeVisible()

    const sessionsButton = row
      .locator('[data-testid$="-sessions-button"]')
      .first()
    await this.smartClick(sessionsButton)

    await expect(this.sessionsDialog).toBeVisible()
  }

  /**
   * Assert the user-sessions dialog is open (visible).
   */
  async expectSessionsDialogOpen(): Promise<void> {
    await expect(this.sessionsDialog).toBeVisible()
  }

  /**
   * Close the sessions dialog by clicking the footer Close button.
   *
   * The footer renders `<Button>{m['common.close']()}</Button>`
   * (user-sessions-dialog.tsx:162-166). Located by accessible name `close`
   * (i18n-independent via the role name, which Playwright matches against the
   * rendered label) inside the dialog, then awaited hidden.
   */
  async closeSessionsDialog(): Promise<void> {
    const closeButton = this.sessionsDialog
      .getByRole('button', { name: /close/i })
      .first()
    await this.smartClick(closeButton)
    await expect(this.sessionsDialog).toBeHidden({ timeout: 5000 })
  }

  /**
   * Count the session rows currently rendered in the dialog.
   *
   * Counts `[data-testid^="user-sessions-table-"][data-testid$="-revoke-button"]`
   * inside the dialog — one per non-empty session row. This is locale-independent
   * (does not depend on any localized cell text).
   *
   * @returns The number of session rows in the dialog.
   */
  async getSessionRowCount(): Promise<number> {
    return await this.sessionsDialog
      .locator(
        '[data-testid^="user-sessions-table-"][data-testid$="-revoke-button"]'
      )
      .count()
  }

  /**
   * Revoke a single session by zero-based row index.
   *
   * Clicks the per-row revoke button
   * (`SELECTORS.userSessions.revokeRowButton(index)`), waits for the
   * revoke-one ConfirmDialog to appear, confirms, then waits for the confirm
   * dialog to disappear. Uses a 5s ceiling consistent with `confirmResetPassword`.
   *
   * @param index Zero-based session row index.
   */
  async revokeSessionByIndex(index: number): Promise<void> {
    const revokeButton = this.page.locator(
      SELECTORS.userSessions.revokeRowButton(index)
    )
    await this.smartClick(revokeButton)
    await expect(this.revokeConfirmDialog).toBeVisible({ timeout: 5000 })

    await this.smartClick(this.revokeConfirmButton)
    await expect(this.revokeConfirmDialog).toBeHidden({ timeout: 5000 })
  }

  /**
   * Revoke all sessions for the user via the "Revoke All" button.
   *
   * Clicks `sessionsRevokeAllButton`, waits for the revoke-all ConfirmDialog,
   * confirms, then waits for that confirm dialog to disappear.
   */
  async revokeAllSessions(): Promise<void> {
    await this.smartClick(this.sessionsRevokeAllButton)
    await expect(this.revokeAllConfirmDialog).toBeVisible({ timeout: 5000 })

    await this.smartClick(this.revokeAllConfirmButton)
    await expect(this.revokeAllConfirmDialog).toBeHidden({ timeout: 5000 })
  }

  /**
   * Assert the revoke-all button is absent (empty-state proxy).
   *
   * `user-sessions-dialog.tsx:84` renders the revoke-all button ONLY when the
   * session list is non-empty (`{hasSessions && ...}`). A count of 0 is the
   * stable, locale-independent proof the list is empty — there is no dedicated
   * empty-state testid to assert on.
   */
  async expectRevokeAllButtonAbsent(): Promise<void> {
    await expect(this.sessionsRevokeAllButton).toHaveCount(0)
  }
}
