/**
 * Email-OTP Login Demo Tests
 *
 * Covers the two P0 user stories for Email-OTP login as ONE
 * registered/unregistered user login-flow file (they share the OTP send/verify
 * UI closure and the Redis helper), plus the graceful-degradation assertion
 * (US-EO-003 scenario 3: Realm disables OTP → entry hidden, registered user
 * still password-logs-in), which closes the same login-route visibility state
 * machine.
 *
 * User stories:
 *   `docs/user-stories/auth/email-otp-login.md`
 *   - US-EO-001: registered user logs in with email OTP
 *   - US-EO-002: unregistered email auto-registers after consent
 *   - US-EO-003 scenario 3: disabling OTP hides the entry; password login still works
 *
 * Backend (already implemented):
 *   POST /api/auth/{realmId}/login/email-otp/send   — issue code (anti-enumeration: uniform ok)
 *   POST /api/auth/{realmId}/login/email-otp/verify — verify code → Bearer session
 *
 * The code is stored in Redis as PLAINTEXT under
 * `emailotp:{realm_id}:{sha256_hex(normalize_email(email))}`. The demo env has
 * no readable mailbox, so the code is read back directly from Redis via
 * `email-otp-redis-helper.ts` to complete the UI — the UI itself is fully
 * driven (send → fill code → verify), the helper only reads what the email
 * would carry.
 *
 * Anti-enumeration (design §4.2): `send` returns a uniform "code sent" outcome
 * for known AND unknown email; only `verify` distinguishes outcomes. The send
 * step here asserts ONLY the uniform success UI, never per-email differences.
 *
 * Compliance: guides/demo/e2e-testing.md + selector-strategy.md — UI interactions
 * drive the flow; the Redis helper only reads the code. Acceptance assertions
 * land on stable observable results (URL navigation, entry visibility), NOT on
 * sonner/toast.
 */

import { test, expect, cleanupTestData } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { loginWithCredentials } from '../helpers/auth'
import {
  enableEmailOtpForRealm,
  disableEmailOtpForRealm,
} from '../helpers/email-otp-setup'
import {
  getLatestOtpCode,
  clearOtpCode,
} from '../helpers/email-otp-redis-helper'
import { SELECTORS } from '../selectors'

const REALM_ID = 'realm-001'
const REGISTERED_EMAIL = 'user@realm-001.com'
const PASSWORD = 'password'
const CODE_LENGTH = 6

test.describe('[Regular User] Email-OTP Login Demo Tests', () => {
  let testStartTime: number

  test.beforeEach(async ({ page, testStartTime: startTime }) => {
    testStartTime = startTime

    await verifyTestEnvironment(page, {
      requiredRealms: ['realm-001'],
      requiredUsers: ['admin@realm-001.com'],
    })
  })

  test.afterEach(async ({ page, demoLogger }) => {
    // Best-effort restore: leave Email-OTP DISABLED so other demos are
    // unaffected (mirrors resetWhiteLabelConfig's try/catch intent). The
    // disableEmailOtpForRealm helper is itself wrapped in try/catch, but we
    // wrap once more at the suite level so a Redis-clear failure cannot mask
    // the disable step.
    try {
      await disableEmailOtpForRealm(page, demoLogger, REALM_ID)
    } catch (error) {
      console.warn('[EmailOtp Demo] teardown disableEmailOtpForRealm failed:', error)
    }

    // Clear any leftover OTP codes for the emails this suite touches. Best-effort.
    try {
      await clearOtpCode(REALM_ID, REGISTERED_EMAIL)
      // `usedEmails` carries the per-test unregistered email (set inside each
      // test body). It is undefined for tests that never reached the send step.
      for (const email of usedEmails) {
        await clearOtpCode(REALM_ID, email)
      }
    } catch (error) {
      console.warn('[EmailOtp Demo] teardown clearOtpCode failed:', error)
    }

    // cleanupDemoTestData only deletes users explicitly listed in testUserEmails
    // (NOT timestamp-based), so the auto-registered email MUST be passed in.
    await cleanupTestData(page, REALM_ID, {
      keepUsers: ['admin@realm-001.com', 'user@realm-001.com'],
      testUserEmails: usedEmails,
      timestamp: testStartTime,
      verbose: false,
    })
  })

  // Shared, mutable per-test list of emails whose Redis keys + accounts must be
  // cleaned up in afterEach. Reset is implicit because each test runs in its
  // own worker/fixture scope, but we reset defensively at the top of each test.
  let usedEmails: string[] = []

  // --------------------------------------------------------------------------
  // US-EO-001 — registered user OTP login
  // --------------------------------------------------------------------------
  test('US-EO-001: registered user logs in with email OTP', async ({ page, demoLogger }) => {
    usedEmails = []

    await test.step('Given: Email-OTP is enabled for realm-001 (autoRegister on for shared setup)', async () => {
      await enableEmailOtpForRealm(page, demoLogger, REALM_ID, { autoRegister: true })
    })

    await test.step('Navigate to login and switch to OTP mode', async () => {
      await page.goto(`/${REALM_ID}/auth/login`)
      await expect(page.getByTestId('login-card')).toBeVisible()

      // The OTP entry toggle only renders when the public OTP-status query
      // reports `enabled`. Asserting it visible proves the realm config took.
      const otpToggle = page.locator(SELECTORS.emailOtp.loginRouteToggle)
      await expect(otpToggle).toBeVisible({ timeout: 10000 })
      await otpToggle.click()

      // The OTP email form should now be mounted.
      await expect(page.locator(SELECTORS.emailOtp.form)).toBeVisible({ timeout: 5000 })
    })

    await test.step('Fill the registered email and send a code', async () => {
      // Clear any session so the post-verify navigation leaves /auth/login.
      await page.context().clearCookies()
      await page.evaluate(() => {
        localStorage.clear()
        sessionStorage.clear()
      })
      await page.goto(`/${REALM_ID}/auth/login`)
      const otpToggle = page.locator(SELECTORS.emailOtp.loginRouteToggle)
      await expect(otpToggle).toBeVisible({ timeout: 10000 })
      await otpToggle.click()

      const sendResponsePromise = page.waitForResponse(
        '**/api/auth/**/login/email-otp/send',
        { timeout: 10000 }
      )
      await page.locator(SELECTORS.emailOtp.emailInput).fill(REGISTERED_EMAIL)
      await page.locator(SELECTORS.emailOtp.sendButton).click()
      const sendResponse = await sendResponsePromise
      // Anti-enumeration: a uniform "code sent" outcome for known/unknown email.
      // Assert ONLY the uniform success status, never per-email differences.
      expect(sendResponse.ok()).toBeTruthy()

      // The code step should now be visible (uniform post-send UI).
      await expect(page.locator(SELECTORS.emailOtp.codeInput)).toBeVisible({ timeout: 5000 })
    })

    await test.step('Read the code out-of-band from Redis and fill the 6 digits', async () => {
      const code = await getLatestOtpCode(REALM_ID, REGISTERED_EMAIL)
      expect(code, 'OTP code must be persisted after send').not.toBeNull()
      expect(code!.length).toBe(CODE_LENGTH)

      // The code input is SIX per-digit <input>s (react-otp-input). Per-digit
      // fill is the robust fallback (wrapper-fill auto-advance is not assumed).
      for (let i = 0; i < CODE_LENGTH; i++) {
        await page.locator(SELECTORS.emailOtp.codeDigit(i)).fill(code![i])
      }
    })

    await test.step('Verify the code and assert a Bearer session is established', async () => {
      const verifyResponsePromise = page.waitForResponse(
        '**/api/auth/**/login/email-otp/verify',
        { timeout: 10000 }
      )
      await page.locator(SELECTORS.emailOtp.verifyButton).click()
      const verifyResponse = await verifyResponsePromise
      expect(verifyResponse.ok()).toBeTruthy()

      // Session is Bearer (design §1.1) — assert navigation leaves the login
      // page to an authenticated realm URL, NOT a cookie assertion.
      await page.waitForURL((url) => !url.pathname.endsWith('/auth/login'), {
        timeout: 15000,
      })
      expect(page.url()).not.toContain('/auth/login')
    })
  })

  // --------------------------------------------------------------------------
  // US-EO-002 — unregistered email auto-register (incl. consent)
  // --------------------------------------------------------------------------
  test('US-EO-002: unregistered email auto-registers after consent', async ({ page, demoLogger }) => {
    // A clearly-unique unregistered email per run; cleaned up in afterEach via
    // `testUserEmails` (the auto-registered account is created by the backend
    // on verify and must be deleted so subsequent runs stay clean).
    const unregisteredEmail = `otp-autoreg-${Date.now()}@realm-001.com`
    usedEmails = [unregisteredEmail]

    await test.step('Given: Email-OTP + autoRegister are enabled for realm-001', async () => {
      await enableEmailOtpForRealm(page, demoLogger, REALM_ID, { autoRegister: true })
    })

    await test.step('Switch to OTP mode and send a code for the unregistered email', async () => {
      // Clean session before driving the login UI.
      await page.context().clearCookies()
      await page.evaluate(() => {
        localStorage.clear()
        sessionStorage.clear()
      })
      await page.goto(`/${REALM_ID}/auth/login`)
      await expect(page.getByTestId('login-card')).toBeVisible()

      const otpToggle = page.locator(SELECTORS.emailOtp.loginRouteToggle)
      await expect(otpToggle).toBeVisible({ timeout: 10000 })
      await otpToggle.click()
      await expect(page.locator(SELECTORS.emailOtp.form)).toBeVisible({ timeout: 5000 })

      const sendResponsePromise = page.waitForResponse(
        '**/api/auth/**/login/email-otp/send',
        { timeout: 10000 }
      )
      await page.locator(SELECTORS.emailOtp.emailInput).fill(unregisteredEmail)
      await page.locator(SELECTORS.emailOtp.sendButton).click()
      const sendResponse = await sendResponsePromise
      // Anti-enumeration: assert ONLY the uniform send outcome (ok or the
      // consent conflict, which is the auto-register path). Do NOT assert any
      // per-email content difference here.
      expect([200, 409]).toContain(sendResponse.status())
    })

    await test.step('Express consent via agree-and-continue (no checkbox) when surfaced', async () => {
      // Per design §4.2, send may first return 409 `consent_required` and the
      // UI surfaces the agreements list. Consent is expressed by CLICKING
      // `email-otp-agree-and-continue-button` — there is NO consent checkbox.
      const agreeButton = page.locator(SELECTORS.emailOtp.agreeAndContinueButton)

      const consentSurfaced = await agreeButton.isVisible({ timeout: 5000 }).catch(() => false)
      if (consentSurfaced) {
        // The agree click triggers a re-send with the agreements built from the
        // conflict list; wait for that re-send to resolve before reading code.
        const resendPromise = page.waitForResponse(
          '**/api/auth/**/login/email-otp/send',
          { timeout: 10000 }
        )
        await agreeButton.click()
        const resend = await resendPromise
        expect(resend.ok()).toBeTruthy()
      }

      // After consent (or if the realm had no pending agreements), the code
      // step must be visible — proves the send ultimately succeeded.
      await expect(page.locator(SELECTORS.emailOtp.codeInput)).toBeVisible({ timeout: 10000 })
    })

    await test.step('Read the code out-of-band from Redis and fill the 6 digits', async () => {
      const code = await getLatestOtpCode(REALM_ID, unregisteredEmail)
      expect(code, 'OTP code must be persisted after send/consent').not.toBeNull()
      expect(code!.length).toBe(CODE_LENGTH)

      for (let i = 0; i < CODE_LENGTH; i++) {
        await page.locator(SELECTORS.emailOtp.codeDigit(i)).fill(code![i])
      }
    })

    await test.step('Verify the code — successful login proves account creation + activation', async () => {
      const verifyResponsePromise = page.waitForResponse(
        '**/api/auth/**/login/email-otp/verify',
        { timeout: 10000 }
      )
      await page.locator(SELECTORS.emailOtp.verifyButton).click()
      const verifyResponse = await verifyResponsePromise
      expect(verifyResponse.ok()).toBeTruthy()

      // The successful login itself is the proof of account creation +
      // activation (backend does `create_user_without_password` +
      // `activate_user` on verify). Assert a Bearer session is established
      // (URL leaves /auth/login) rather than a cookie.
      await page.waitForURL((url) => !url.pathname.endsWith('/auth/login'), {
        timeout: 15000,
      })
      expect(page.url()).not.toContain('/auth/login')
    })
  })

  // --------------------------------------------------------------------------
  // US-EO-003-degradation — disabling OTP hides the entry; password login works
  // --------------------------------------------------------------------------
  test('US-EO-003-degradation: disabling OTP hides the entry; password login still works', async ({
    page,
    demoLogger,
  }) => {
    usedEmails = []

    await test.step('Given: Email-OTP is enabled — the entry toggle is visible', async () => {
      await enableEmailOtpForRealm(page, demoLogger, REALM_ID, { autoRegister: true })

      await page.context().clearCookies()
      await page.evaluate(() => {
        localStorage.clear()
        sessionStorage.clear()
      })
      await page.goto(`/${REALM_ID}/auth/login`)
      await expect(page.getByTestId('login-card')).toBeVisible()
      await expect(page.locator(SELECTORS.emailOtp.loginRouteToggle)).toBeVisible({
        timeout: 10000,
      })
    })

    await test.step('When: admin disables Email-OTP for realm-001', async () => {
      await disableEmailOtpForRealm(page, demoLogger, REALM_ID)
    })

    await test.step('Then: reload login — the OTP entry toggle is NOT visible', async () => {
      // Clear any cached public OTP-status (React Query) before reloading so
      // the stale `enabled:true` does not keep the toggle mounted.
      await page.context().clearCookies()
      await page.evaluate(() => {
        localStorage.clear()
        sessionStorage.clear()
      })
      await page.goto(`/${REALM_ID}/auth/login`)
      await expect(page.getByTestId('login-card')).toBeVisible()

      // Entry hidden: the toggle must not be in the DOM (the route only mounts
      // it when the public OTP-status query reports `enabled`).
      await expect(page.locator(SELECTORS.emailOtp.loginRouteToggle)).toHaveCount(0, {
        timeout: 10000,
      })
    })

    await test.step('And: the registered user can still password-login (password entry survives OTP-off)', async () => {
      // loginWithCredentials clears the session, submits the login form,
      // handles re-consent if prompted, and asserts post-login navigation to
      // the authenticated area. A non-ok login or failure to navigate throws
      // — proving the password entry still works with OTP off.
      await loginWithCredentials(page, {
        realmId: REALM_ID,
        email: REGISTERED_EMAIL,
        password: PASSWORD,
      })

      // Sanity-check we reached an authenticated realm URL.
      expect(page.url()).not.toContain('/auth/login')
    })
  })
})
