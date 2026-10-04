/**
 * Password Reset (Forgot Password) Demo Tests
 *
 * Covers the self-service "forgot password" → "reset password" flow that lets a
 * user regain access without an admin's help.
 *
 * Backend endpoints (already implemented in backend/api-auth/src/reset_password.rs):
 *   POST /api/auth/{realmId}/reset_password/request          — send reset email
 *   POST /api/auth/{realmId}/reset_password/confirm/{code}    — set new password
 *
 * Frontend routes (added by the forgot-password feature):
 *   /$realmId/auth/forgot-password   — email entry form
 *   /$realmId/auth/reset-password    — new password form (reads ?code= from query)
 *
 * The demo environment has no readable mailbox, so the reset code written by the
 * backend is fetched directly from the `email_verification_code` table via
 * helpers/reset-password-db-helper.ts.
 *
 * database helper only reads the code that the email link would carry.
 */

import { test, expect, cleanupTestData } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { loginWithCredentials } from '../helpers/auth'
import {
  getLatestResetCode,
  clearResetCodes,
} from '../helpers/reset-password-db-helper'

const REALM_ID = 'realm-001'
const ADMIN_EMAIL = 'admin@realm-001.com'
const ORIGINAL_PASSWORD = 'password'

test.describe('[Regular User] Password Reset Demo Tests', () => {
  let testStartTime: number

  test.beforeEach(async ({ page, testStartTime: startTime }) => {
    testStartTime = startTime

    await verifyTestEnvironment(page, {
      requiredRealms: ['realm-001'],
      requiredUsers: ['admin@realm-001.com'],
    })
  })

  test.afterEach(async ({ page }) => {
    // Ensure the seed admin password is restored to ORIGINAL_PASSWORD so other
    // demos in the suite are not affected. If a previous test changed it, walk
    // the forgot/reset flow again using the DB-read code to set it back.
    await restoreOriginalPassword(page, REALM_ID, ADMIN_EMAIL, ORIGINAL_PASSWORD)

    // Clear any leftover reset codes for the admin email.
    await clearResetCodes(ADMIN_EMAIL)

    await cleanupTestData(page, REALM_ID, {
      keepUsers: ['admin@realm-001.com'],
      timestamp: testStartTime,
      verbose: false,
    })
  })

  // --------------------------------------------------------------------------
  // Scenario 1: Login page exposes the "forgot password" entry
  // --------------------------------------------------------------------------
  test('Scenario 1: Login page has a forgot-password link that navigates to the form', async ({
    page,
  }) => {
    await test.step('Navigate to login page', async () => {
      await page.goto(`/${REALM_ID}/auth/login`)
      await expect(page.getByTestId('login-card')).toBeVisible()
    })

    await test.step('Verify forgot-password link is present', async () => {
      await expect(page.getByTestId('forgot-password-link')).toBeVisible()
    })

    await test.step('Click the link and land on the forgot-password page', async () => {
      await page.getByTestId('forgot-password-link').click()
      await page.waitForURL(`**/${REALM_ID}/auth/forgot-password`, { timeout: 5000 })
      await expect(page.getByTestId('forgot-password-card')).toBeVisible()
      await expect(page.getByTestId('forgot-password-title')).toBeVisible()
    })
  })

  // --------------------------------------------------------------------------
  // Scenario 2: Forgot-password request shows the anti-enumeration success notice
  // --------------------------------------------------------------------------
  test('Scenario 2: Submitting an email shows the reset-link-sent confirmation', async ({
    page,
  }) => {
    await test.step('Navigate to forgot-password page', async () => {
      await page.goto(`/${REALM_ID}/auth/forgot-password`)
      await expect(page.getByTestId('forgot-password-card')).toBeVisible()
    })

    await test.step('Submit the admin email', async () => {
      const responsePromise = page.waitForResponse(
        '**/api/auth/**/reset_password/request',
        { timeout: 10000 }
      )
      await page.getByTestId('forgot-password-email-input').fill(ADMIN_EMAIL)
      await page.getByTestId('forgot-password-submit-button').click()
      const response = await responsePromise
      expect(response.ok()).toBeTruthy()
    })

    await test.step('Verify success confirmation is shown', async () => {
      // Backend always returns ok (anti-enumeration); the UI shows the success view.
      await expect(page.getByTestId('forgot-password-success')).toBeVisible({
        timeout: 5000,
      })
    })
  })

  // --------------------------------------------------------------------------
  // Scenario 3: Full end-to-end reset flow and login with the new password
  // --------------------------------------------------------------------------
  test('Scenario 3: Full reset flow — request, set new password, then log in', async ({
    page,
  }) => {
    // This scenario drives the full forgot/reset flow AND logs in to the
    // dashboard; afterEach then restores the original password via another
    // reset round-trip. On a freshly rebuilt demo seed the dashboard render can
    // be slow, so allow well beyond the default 120s to keep the afterEach
    // restore from being killed mid-flow by the per-test timeout.
    test.setTimeout(240_000)

    const newPassword = 'NewPassword123!'

    await test.step('Step 1: Request reset link via the forgot-password form', async () => {
      await page.goto(`/${REALM_ID}/auth/forgot-password`)
      await expect(page.getByTestId('forgot-password-card')).toBeVisible()

      const responsePromise = page.waitForResponse(
        '**/api/auth/**/reset_password/request',
        { timeout: 10000 }
      )
      await page.getByTestId('forgot-password-email-input').fill(ADMIN_EMAIL)
      await page.getByTestId('forgot-password-submit-button').click()
      await responsePromise
      await expect(page.getByTestId('forgot-password-success')).toBeVisible({
        timeout: 5000,
      })
    })

    // The email is not readable in the demo env; read the code written by the
    // backend directly from the database (it is what the email link would carry).
    const resetCode = await getLatestResetCode(ADMIN_EMAIL)
    expect(resetCode, 'reset code must be persisted after request').not.toBeNull()

    await test.step('Step 2: Open the reset-password page with the code', async () => {
      await page.goto(`/${REALM_ID}/auth/reset-password?code=${resetCode}`)
      await expect(page.getByTestId('reset-password-card')).toBeVisible()
      await expect(page.getByTestId('reset-password-title')).toBeVisible()
    })

    await test.step('Step 3: Submit the new password', async () => {
      // The backend confirm handler returns 302 FOUND on success (it issues a
      // Location redirect to the Client App / realm public URL). The SPA's API
      // client uses `redirect: 'follow'`, so a loose glob would let the
      // auto-followed GET response race in; match strictly on the POST to the
      // confirm path. Also, 302 is NOT in Playwright's 200-299 `.ok()` range,
      // so assert on the success-or-redirect status range rather than `.ok()`.
      const confirmUrl = `/api/auth/${REALM_ID}/reset_password/confirm/`
      const responsePromise = page.waitForResponse(
        (resp) =>
          resp.request().method() === 'POST' &&
          resp.url().includes(confirmUrl),
        { timeout: 10000 }
      )
      await page.getByTestId('reset-password-new-input').fill(newPassword)
      await page.getByTestId('reset-password-confirm-input').fill(newPassword)
      await page.getByTestId('reset-password-submit-button').click()
      const response = await responsePromise
      // Success: 200-299 (if the API client ever stops following the redirect)
      // or 302 (the documented successful-confirm redirect).
      expect(
        response.status() >= 200 && response.status() < 400,
        `expected 2xx/3xx for reset confirm, got ${response.status()}`
      ).toBeTruthy()
    })

    await test.step('Step 4: Verify redirect to login page', async () => {
      await page.waitForURL(`**/${REALM_ID}/auth/login`, { timeout: 5000 })
      await expect(page.getByTestId('login-card')).toBeVisible()
    })

    await test.step('Step 5: Log in with the new password and reach the authenticated area', async () => {
      // loginWithCredentials clears the session, submits the login form, handles
      // re-consent if prompted, and finally asserts the post-login navigation to
      // the authenticated area (e.g. /realm-001/manage). A non-ok login response
      // or failure to navigate throws — proving the new password actually works.
      await loginWithCredentials(page, {
        realmId: REALM_ID,
        email: ADMIN_EMAIL,
        password: newPassword,
      })

      // Sanity-check we are no longer on the login page.
      expect(page.url()).not.toContain('/auth/login')
    })

    // afterEach restores ORIGINAL_PASSWORD so the seed user is reusable.
  })

  // --------------------------------------------------------------------------
  // Scenario 4: Mismatched passwords block submission on the reset page
  // --------------------------------------------------------------------------
  test('Scenario 4: Mismatched passwords are rejected on the reset page', async ({
    page,
  }) => {
    // Seed a reset code so the page is in the valid state.
    await seedResetCodeViaUi(page, ADMIN_EMAIL)
    const resetCode = await getLatestResetCode(ADMIN_EMAIL)
    expect(resetCode).not.toBeNull()

    await test.step('Open reset page and enter mismatched passwords', async () => {
      await page.goto(`/${REALM_ID}/auth/reset-password?code=${resetCode}`)
      await expect(page.getByTestId('reset-password-card')).toBeVisible()

      await page.getByTestId('reset-password-new-input').fill('Password123!')
      await page.getByTestId('reset-password-confirm-input').fill('Password456!')
      await page.getByTestId('reset-password-confirm-input').blur()

      // The mismatch error message should appear.
      await expect(
        page.locator('p.text-destructive').filter({ hasText: /Passwords do not match/i })
      ).toBeVisible({ timeout: 3000 })

      // Submit button must remain disabled while passwords mismatch.
      await expect(page.getByTestId('reset-password-submit-button')).toBeDisabled()
    })
  })
})

// ============================================================================
// Local helpers
// ============================================================================

/** Navigate to the forgot-password form and submit the email to seed a code. */
async function seedResetCodeViaUi(page: import('@playwright/test').Page, email: string) {
  await page.goto(`/${REALM_ID}/auth/forgot-password`)
  await expect(page.getByTestId('forgot-password-card')).toBeVisible()
  const responsePromise = page.waitForResponse(
    '**/api/auth/**/reset_password/request',
    { timeout: 10000 }
  )
  await page.getByTestId('forgot-password-email-input').fill(email)
  await page.getByTestId('forgot-password-submit-button').click()
  await responsePromise
}

/**
 * Ensure the seed admin password equals `originalPassword`.
 *
 * Tests in this file may change it; this helper restores it by walking the
 * forgot/reset flow once more (no bcrypt dependency) so the rest of the demo
 * suite sees the documented seed credentials.
 *
 * The reset flow is run unconditionally. Scenario 3 leaves the page on the
 * authenticated admin dashboard, which has two gotchas for cleanup:
 *   1. The SPA's root route guard redirects authenticated users away from
 *      `/auth/*` pages back to `/manage`, so the forgot-password form would
 *      never render — we tear down the session (cookies + web storage) first.
 *   2. The dashboard long-polls; clearing web storage while it is mounted can
 *      stall the page's main thread, and `page.goto` with the default `load`
 *      wait hangs. We navigate with `waitUntil: 'commit'` to drop the old
 *      document before clearing storage, then `domcontentloaded` for the form.
 */
async function restoreOriginalPassword(
  page: import('@playwright/test').Page,
  realmId: string,
  email: string,
  originalPassword: string
) {
  console.log('[PasswordReset Demo] Restoring original password via reset flow')

  // Tear down any leftover session so the SPA's root route guard lets us onto
  // the public forgot-password page (otherwise it redirects authenticated
  // users back to /manage). Scenario 3 leaves the page on the admin dashboard,
  // which long-polls and whose main thread can stall a `page.evaluate`, so:
  //   - clear cookies at the context level (always safe), then
  //   - navigate with `waitUntil: 'commit'` — this resolves as soon as the
  //     response is received, before the SPA mounts/polls, so the old
  //     dashboard is torn down, and
  //   - clear web storage from that not-yet-mounted document.
  await page.context().clearCookies()
  await page.goto(`/${realmId}/auth/forgot-password`, { waitUntil: 'commit' })
  try {
    await page.evaluate(() => {
      localStorage.clear()
      sessionStorage.clear()
    })
  } catch {
    // On some documents (e.g. an opaque-origin blank page) web storage is
    // unavailable; clearing cookies + a fresh navigation below is enough for
    // the public page to render.
  }

  // Request a fresh reset code. Reload the now-unauthenticated public page and
  // wait for the form. Use `domcontentloaded` (the SPA keeps long-polling
  // under `load`).
  await page.goto(`/${realmId}/auth/forgot-password`, { waitUntil: 'domcontentloaded' })
  await page.getByTestId('forgot-password-email-input').fill(email)
  const requestPromise = page.waitForResponse(
    '**/api/auth/**/reset_password/request',
    { timeout: 10000 }
  )
  await page.getByTestId('forgot-password-submit-button').click()
  await requestPromise

  const code = await getLatestResetCode(email)
  if (!code) {
    console.warn('[PasswordReset Demo] Could not obtain reset code to restore password')
    return
  }

  // Reset back to the original password. Match the POST to the confirm path
  // specifically: the backend returns 302 and the SPA's API client follows the
  // redirect, so a loose glob could match the auto-followed GET instead.
  await page.goto(`/${realmId}/auth/reset-password?code=${code}`, {
    waitUntil: 'domcontentloaded',
  })
  await page.getByTestId('reset-password-new-input').fill(originalPassword)
  await page.getByTestId('reset-password-confirm-input').fill(originalPassword)
  const confirmPromise = page.waitForResponse(
    (resp) =>
      resp.request().method() === 'POST' &&
      resp.url().includes(`/api/auth/${realmId}/reset_password/confirm/`),
    { timeout: 10000 }
  )
  await page.getByTestId('reset-password-submit-button').click()
  await confirmPromise
  console.log('[PasswordReset Demo] Original password restored')
}
