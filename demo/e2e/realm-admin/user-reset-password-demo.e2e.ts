/**
 * Reset Password Demo Tests
 *
 * User Story: docs/prd/core/users.md
 * - Admin can reset a user's password from the user list
 * - After reset, the new password is displayed in a result dialog
 * - Admin can copy the new password and close the dialog
 * - Cancel path dismisses the confirmation dialog without resetting
 *
 * Selector calibration: All selectors verified against
 *   frontend/src/components/users/user-table.tsx (row-level reset button)
 *   frontend/src/routes/$realmId/manage/users.tsx (confirm dialog)
 *   frontend/src/components/users/reset-password-result-dialog.tsx (result dialog)
 */

import { APIRequestContext } from '@playwright/test'
import { test, expect, cleanupTestData } from '../fixtures/demo-page.fixtures'
import { createBearerApiContext } from '../helpers/auth'
import type { LoginPage } from '../pages/login-page'

const TEST_REALM = 'admin'

/**
 * Per-test unique email for the user this suite creates.
 *
 * `testStartTime` is Date.now() captured per test by the demo-page fixtures,
 * so every test in every run gets a distinct address. This keeps the two
 * cases (and repeated runs) collision-free EVEN IF a previous cleanup
 * failed — the create-user API returns 400 "Email already exists" on
 * collision, which used to break the second case when both shared the fixed
 * `reset-pw-test@demo.com` and the afterEach delete silently failed (see
 * backendBaseUrl note below).
 *
 * afterEach recomputes the SAME address from the same per-test fixture value,
 * so no cross-scope tracking variable is needed.
 */
const testUserEmailFor = (testStartTime: number) =>
  `reset-pw-${testStartTime}@demo.com`

/**
 * Backend base URL for the admin user API (search/delete).
 *
 * Mirrors the resolution used in billing-admin/points-grant-sdk-demo.e2e.ts
 * and helpers/api-validator.ts.
 *
 * ⚠️ Auth note: the `/api/users/{realmId}` search/delete endpoints are gated
 * on `Authorization: Bearer` (inject_token_identity,
 * backend/api-base/.../identity_middleware.rs:38-41 reads ONLY that header —
 * cookies are ignored), and this project's access token lives purely in SPA
 * memory (frontend/src/stores/auth-store.ts), so `page.request.*` (cookies
 * only) gets 401 "missing bearer token". The delete helpers below therefore
 * use a Bearer-authenticated APIRequestContext built from the shared
 * `loginPage` fixture's post-switch admin-console token — the same pattern as
 * user-sessions-management-demo.e2e.ts / user-forbidden-session-revoke-demo.e2e.ts,
 * minus their disposable-context login (the usersPage fixture has already
 * logged the admin in on the same loginPage instance).
 */
function backendBaseUrl(): string {
  return (
    process.env.API_BASE_URL ||
    process.env.BASE_URL?.replace(/:\d+$/, ':8080') ||
    'http://localhost:8080'
  )
}

/**
 * Resolve a user's id by email via the admin user list API.
 *
 * GET /api/users/{realmId}?search={email} → PageResponse<{ id, email, ... }>
 *
 * @returns The matching user id, or '' when the user does not exist.
 */
async function findUserIdByEmail(
  apiContext: APIRequestContext,
  email: string
): Promise<string> {
  const url = `${backendBaseUrl()}/api/users/${TEST_REALM}?search=${encodeURIComponent(email)}`
  const response = await apiContext.get(url)
  if (!response.ok()) {
    // Non-200 here is unexpected; surface it rather than silently returning ''.
    const body = await response.text().catch(() => '<unreadable>')
    throw new Error(
      `findUserIdByEmail: list users failed (HTTP ${response.status()}): ${body}`
    )
  }
  const body = await response.json()
  const items = (body?.items ?? []) as Array<{ id: string; email: string }>
  const match = items.find((u) => u.email === email)
  return match?.id ?? ''
}

/**
 * Idempotent setup: ensure no user with the given email exists before the
 * test creates one. A previous run may have left a user behind (the
 * create-user API returns 400 "Email already exists" on collision), which
 * historically turned into a silent pass because the page object did not
 * read the create response. Deleting up-front guarantees each run starts
 * from a clean state.
 *
 * Uses the admin user delete API:
 *   DELETE /api/users/{realmId}/{userId}
 *
 * Failures are non-fatal (logged) — the create step will surface any
 * remaining conflict loudly via submitUserForm's response check.
 */
async function deleteExistingUser(
  apiContext: APIRequestContext,
  email: string
): Promise<void> {
  try {
    const userId = await findUserIdByEmail(apiContext, email)
    if (!userId) {
      return
    }
    const url = `${backendBaseUrl()}/api/users/${TEST_REALM}/${userId}`
    const response = await apiContext.delete(url)
    if (response.status() >= 400 && response.status() !== 404) {
      const body = await response.text().catch(() => '<unreadable>')
      console.warn(
        `[reset-pw setup] delete existing user ${email} (${userId}) ` +
          `returned HTTP ${response.status()}: ${body}`
      )
    } else {
      console.log(`[reset-pw setup] deleted stale user ${email} (${userId})`)
    }
  } catch (error) {
    console.warn(`[reset-pw setup] deleteExistingUser error (non-fatal):`, error)
  }
}

/**
 * {@link deleteExistingUser} with a Bearer-authenticated admin API context
 * built from the shared `loginPage` fixture's access token (the usersPage
 * fixture logs the admin in on that same instance, so no extra login is
 * needed). The context is disposed afterwards.
 */
async function deleteExistingUserAsAdmin(
  loginPage: LoginPage,
  email: string
): Promise<void> {
  const apiContext = await createBearerApiContext(loginPage.getAccessToken())
  try {
    await deleteExistingUser(apiContext, email)
  } finally {
    await apiContext.dispose().catch(() => {})
  }
}

test.describe('[ResetPassword] Admin resets user password', () => {
  test.afterEach(async ({ usersPage, loginPage, testStartTime }) => {
    // Prefer targeted API cleanup by id (the generic timestamp-based
    // cleanupTestData only clears subscription plans, not users, so users
    // survived across runs). Delete the specific test user this run created
    // (same per-test email derived from testStartTime the test used), then
    // run the shared cleanup for any other test data.
    await deleteExistingUserAsAdmin(
      loginPage,
      testUserEmailFor(testStartTime)
    ).catch((error) => {
      console.warn('[reset-pw afterEach] cleanup error (non-fatal):', error)
    })

    await cleanupTestData(usersPage.page, TEST_REALM, {
      timestamp: testStartTime,
      keepUsers: [],
    })
  })

  test('should reset password and show new password in result dialog', async ({
    usersPage,
    loginPage,
    testStartTime,
  }) => {
    // Per-test unique email: no collision with the cancel-path case below or
    // with any previous run, even if a cleanup failed.
    const testUserEmail = testUserEmailFor(testStartTime)

    // Given: a user exists in the admin realm
    await test.step('Given a test user exists', async () => {
      // Idempotent: clear any stale user from a previous run before creating.
      await deleteExistingUserAsAdmin(loginPage, testUserEmail)
      await usersPage.createUser({
        email: testUserEmail,
        password: 'TestPass123!',
        nickname: 'resetpw',
      })
      await expect(usersPage.findUserRow(testUserEmail)).toBeVisible()
    })

    // When: admin clicks Reset Password button
    let newPassword = ''
    await test.step('When admin clicks Reset Password and confirms', async () => {
      await usersPage.clickResetPassword(testUserEmail)

      // Then: confirmation dialog appears
      await expect(usersPage.resetPasswordConfirmDialog).toBeVisible()

      // Click confirm
      await usersPage.confirmResetPassword()
    })

    // Then: result dialog appears with a new password
    await test.step('Then result dialog shows new password', async () => {
      await expect(usersPage.resetPasswordResultDialog).toBeVisible()

      newPassword = await usersPage.waitForResetPasswordResult()

      // Password should be non-empty and at least 16 chars (backend generates 16-char passwords)
      expect(newPassword.length).toBeGreaterThanOrEqual(16)
      expect(newPassword).toMatch(/[A-Z]/)     // has uppercase
      expect(newPassword).toMatch(/[a-z]/)     // has lowercase
      expect(newPassword).toMatch(/[0-9]/)     // has digit
      expect(newPassword).toMatch(/[!@#$%^&*]/) // has special
    })

    // And: admin can copy the password
    await test.step('And admin can copy the password', async () => {
      await usersPage.copyPassword()

      // Verify button text changed to "Copied!" -- this is a stable DOM assertion, not a toast
      await expect(usersPage.resetPasswordCopyButton).toHaveText(/Copied/)
    })

    // And: admin can close the result dialog
    await test.step('And admin can close the result dialog', async () => {
      await usersPage.closeResetPasswordResult()
      await expect(usersPage.resetPasswordResultDialog).toBeHidden()
    })
  })

  test('should cancel reset password without changing password', async ({
    usersPage,
    loginPage,
    testStartTime,
  }) => {
    // Per-test unique email: no collision with the reset-path case above or
    // with any previous run, even if a cleanup failed.
    const testUserEmail = testUserEmailFor(testStartTime)

    // Given: a user exists
    await test.step('Given a test user exists', async () => {
      // Idempotent: clear any stale user from a previous run before creating.
      await deleteExistingUserAsAdmin(loginPage, testUserEmail)
      await usersPage.createUser({
        email: testUserEmail,
        password: 'TestPass123!',
        nickname: 'cancel-reset',
      })
      await expect(usersPage.findUserRow(testUserEmail)).toBeVisible()
    })

    // When: admin clicks Reset Password then cancels
    await test.step('When admin clicks Reset Password then cancels', async () => {
      await usersPage.clickResetPassword(testUserEmail)
      await expect(usersPage.resetPasswordConfirmDialog).toBeVisible()

      // Cancel by pressing Escape (AlertDialog supports Escape to dismiss)
      await usersPage.page.keyboard.press('Escape')
    })

    // Then: dialog closes, no result dialog appears
    await test.step('Then confirmation dialog closes without result dialog', async () => {
      await expect(usersPage.resetPasswordConfirmDialog).toBeHidden()
      // Result dialog should NOT appear
      await expect(usersPage.resetPasswordResultDialog).toBeHidden()
    })
  })
})
