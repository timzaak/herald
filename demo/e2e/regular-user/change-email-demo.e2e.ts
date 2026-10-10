/**
 * Change Email Demo Tests
 *
 * User story: US-RU-016 (docs/user-stories/core/regular-user.md)
 *
 * Scenario A (gating): when the realm has no email channel configured, the
 * profile page hides the "Change Email" entry and shows the reason instead.
 *
 * Scenario B (main path): with a fake-Resend channel configured, the user
 * reauthenticates with their password, submits a new address, and completes
 * the change through the confirmation link. The demo environment cannot
 * deliver mail: the request handler persists the code to
 * `email_verification_code` (type='change_email', email column = the NEW
 * address) BEFORE the send attempt, and the fake Resend key then makes the
 * send fail with a 500 — which the dialog reports honestly instead of
 * pretending the mail went out. The flow reads the code from the database
 * via helpers/verification-code-db-helper.ts (exactly what the emailed link would
 * carry) and opens the confirmation page as the logged-in owner.
 *
 * Backend endpoints:
 *   POST /api/auth/{realmId}/change_email/request   — reauth ticket + new email
 *   GET  /api/auth/{realmId}/change_email/confirm/{code}
 *
 * Frontend routes:
 *   /$realmId/user/profile                      — entry + email row
 *   /$realmId/user/change-email/confirm?code=   — auto-confirm page
 */

import { test, expect } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { createAdminApiContext, loginWithCredentials } from '../helpers/auth'
import {
  ensureEmailChannelConfigured,
  ensureEmailChannelNotConfigured,
} from '../helpers/email-otp-setup'
import {
  getLatestVerificationCode,
  clearVerificationCodes,
} from '../helpers/verification-code-db-helper'
import { SELECTORS } from '../selectors'
import type { APIRequestContext } from '@playwright/test'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'
const REALM_ID = 'realm-001'
const USER_PASSWORD = 'Password123!'
// `type` column value the backend writes for change-email codes
const CHANGE_EMAIL_CODE_TYPE = 'change_email'

test.describe('[Regular User] Change Email Demo Tests', () => {
  let testStartTime: number
  const createdEmails: string[] = []
  const createdUserIds: string[] = []
  // Held for the whole test so afterEach can always restore the seed email
  // channel state and delete the throwaway users, even when the test body
  // fails midway.
  let adminApi: APIRequestContext | null = null

  test.beforeEach(async ({ page, testStartTime: startTime }) => {
    testStartTime = startTime
    createdEmails.length = 0
    createdUserIds.length = 0
    adminApi = null

    await verifyTestEnvironment(page, {
      requiredRealms: ['realm-001'],
      requiredUsers: ['admin@realm-001.com'],
    })
  })

  test.afterEach(async ({ demoLogger }) => {
    if (!adminApi) {
      return
    }

    // The fake Resend config must never outlive the run: the rest of the demo
    // suite (email-otp series, email-config clean-state assertions) depends on
    // realm-001's email channel staying unconfigured. Best-effort — a teardown
    // failure is logged, not thrown, so it never masks the original failure.
    try {
      await ensureEmailChannelNotConfigured(adminApi, demoLogger, REALM_ID)
    } catch (error) {
      console.warn('[ChangeEmail Demo] Failed to restore unconfigured email channel:', error)
    }

    // Throwaway users are deleted via the admin API rather than the UI-based
    // cleanupTestData: each test runs in a fresh browser context, so the admin
    // console's first-login onboarding dialog (localStorage-driven) is always
    // back and reliably blocks the cleanup's sidebar navigation. Deleting by id
    // is also immune to the email change scenario B performs.
    for (const userId of createdUserIds) {
      try {
        const response = await adminApi.delete(`${BASE_URL}/api/users/${userId}`)
        // 404 = already gone (e.g. a prior teardown got interrupted mid-loop).
        if (!response.ok() && response.status() !== 404) {
          const body = await response.text().catch(() => '')
          console.warn(
            `[ChangeEmail Demo] Failed to delete throwaway user ${userId}: ` +
              `${response.status()} ${body}`
          )
        }
      } catch (error) {
        console.warn(`[ChangeEmail Demo] Failed to delete throwaway user ${userId}:`, error)
      }
    }

    // Codes reference one-off addresses; delete the rows so nothing leaks
    // between runs (the user rows themselves are gone after the loop above).
    for (const email of createdEmails) {
      try {
        await clearVerificationCodes(email, CHANGE_EMAIL_CODE_TYPE)
      } catch (error) {
        console.warn(`[ChangeEmail Demo] Failed to clear codes for ${email}:`, error)
      }
    }

    await adminApi.dispose()
  })

  /**
   * Create a throwaway user with a known password via the admin API
   * (POST /api/users — camelCase body per UserCreateRequest) and return its
   * id for teardown. The seed accounts are never touched: the email is the
   * account's login credential, and changing it on a seed user would break
   * every other demo.
   */
  async function createThrowawayUser(api: APIRequestContext, email: string): Promise<string> {
    const response = await api.post(`${BASE_URL}/api/users`, {
      data: {
        email,
        password: USER_PASSWORD,
        nickname: email.split('@')[0],
        status: 1, // Active
        roleIds: [],
      },
    })
    if (!response.ok()) {
      const body = await response.text().catch(() => '')
      throw new Error(
        `[ChangeEmail Demo] Failed to create throwaway user "${email}": ` +
          `${response.status()} ${body}`
      )
    }
    const user: { id: string } = await response.json()
    return user.id
  }

  test('Scenario A: profile hides the Change Email entry when the realm has no email channel', async ({
    page,
    demoLogger,
  }) => {
    const email = `ce-gate-${testStartTime}@example.com`
    createdEmails.push(email)

    await test.step('Ensure the email channel is unconfigured (seed state)', async () => {
      adminApi = await createAdminApiContext(page, demoLogger, REALM_ID)
      await ensureEmailChannelNotConfigured(adminApi, demoLogger, REALM_ID)
      createdUserIds.push(await createThrowawayUser(adminApi, email))
    })

    await test.step('Log in as the throwaway user and open the profile page', async () => {
      await loginWithCredentials(page, {
        realmId: REALM_ID,
        email,
        password: USER_PASSWORD,
      })
      await page.goto(`/${REALM_ID}/user/profile`)
      // Profile data must be loaded before the gating assertions below:
      // while the config query is still loading the entry is not rendered
      // either, so the "hidden" assertion is only meaningful once the page
      // has settled.
      await expect(page.locator(SELECTORS.changeEmail.emailDisplay)).toBeVisible()
    })

    await test.step('Entry is hidden and the unavailable reason is shown', async () => {
      await expect(page.locator(SELECTORS.changeEmail.entryButton)).not.toBeVisible()
      await expect(page.locator(SELECTORS.changeEmail.unavailableNote)).toBeVisible()
    })
  })

  test('Scenario B: reauth, submit new email, confirm via link, profile shows the new address', async ({
    page,
    demoLogger,
  }) => {
    // Several logins (admin setup + user flow) plus a confirmation round-trip;
    // the password-reset full-flow demo uses the same allowance.
    test.setTimeout(240_000)

    const oldEmail = `ce-main-${testStartTime}@example.com`
    const newEmail = `ce-new-${testStartTime}@example.com`
    createdEmails.push(oldEmail, newEmail)

    await test.step('Provision the fake Resend channel, then the throwaway user', async () => {
      adminApi = await createAdminApiContext(page, demoLogger, REALM_ID)
      // The change-email request endpoint rejects realms without a configured
      // email channel, so the channel stays configured for the whole scenario
      // (afterEach restores the unconfigured seed state).
      await ensureEmailChannelConfigured(adminApi, demoLogger, REALM_ID)
      createdUserIds.push(await createThrowawayUser(adminApi, oldEmail))
    })

    await test.step('Log in and open the profile — the entry is available', async () => {
      await loginWithCredentials(page, {
        realmId: REALM_ID,
        email: oldEmail,
        password: USER_PASSWORD,
      })
      await page.goto(`/${REALM_ID}/user/profile`)
      await expect(page.locator(SELECTORS.changeEmail.emailDisplay)).toBeVisible()
      await expect(page.locator(SELECTORS.changeEmail.emailDisplay)).toContainText(oldEmail)
      await expect(page.locator(SELECTORS.changeEmail.entryButton)).toBeVisible()
    })

    await test.step('Open the dialog and reauth with the password factor', async () => {
      await page.locator(SELECTORS.changeEmail.entryButton).click()
      await expect(page.locator(SELECTORS.changeEmail.dialog)).toBeVisible()
      // The throwaway user has only a password: single-factor accounts skip
      // the factor radio group and go straight to the password input.
      await expect(page.locator(SELECTORS.changeEmail.passwordInput)).toBeVisible({
        timeout: 10000,
      })
      await page.locator(SELECTORS.changeEmail.passwordInput).fill(USER_PASSWORD)
      await page.locator(SELECTORS.changeEmail.verifyButton).click()
      await expect(page.locator(SELECTORS.changeEmail.newEmailInput)).toBeVisible({
        timeout: 10000,
      })
    })

    await test.step('Submit the new address — the fake key fails the send honestly', async () => {
      await page.locator(SELECTORS.changeEmail.newEmailInput).fill(newEmail)
      const responsePromise = page.waitForResponse(
        (resp) =>
          resp.request().method() === 'POST' &&
          resp.url().includes(`/api/auth/${REALM_ID}/change_email/request`),
        { timeout: 15000 }
      )
      await page.locator(SELECTORS.changeEmail.submitButton).click()
      const response = await responsePromise
      // The demo channel uses a fake Resend key, so the send 500s — the
      // honest-failure UX (no fake "link sent" success) is the assertion.
      expect(
        response.status(),
        `expected the send to fail (500) with the fake Resend key, got ${response.status()}`
      ).toBe(500)
      await expect(page.locator(SELECTORS.changeEmail.errorMessage)).toBeVisible()
      await expect(page.locator(SELECTORS.changeEmail.errorMessage)).toContainText(
        /failed to send the confirmation email/i
      )
    })

    // No readable mailbox in the demo env: the code the confirmation link
    // would carry was persisted BEFORE the send attempt failed — read it
    // straight from the database.
    const code = await getLatestVerificationCode(newEmail, CHANGE_EMAIL_CODE_TYPE)
    expect(code, 'change-email code must be persisted before the send attempt').not.toBeNull()

    await test.step('Open the confirmation link as the logged-in owner', async () => {
      await page.goto(`/${REALM_ID}/user/change-email/confirm?code=${code}`)
      await expect(page.locator(SELECTORS.changeEmail.confirmCard)).toBeVisible()
      await expect(page.locator(SELECTORS.changeEmail.confirmSuccess)).toBeVisible({
        timeout: 15000,
      })
    })

    await test.step('Back on the profile page the account shows the new address', async () => {
      await page.locator(SELECTORS.changeEmail.confirmBackLink).click()
      await expect(page.locator(SELECTORS.changeEmail.emailDisplay)).toBeVisible()
      await expect(page.locator(SELECTORS.changeEmail.emailDisplay)).toContainText(newEmail)
    })
  })
})
