/**
 * LDAP corporate-directory login demo tests.
 *
 * Covers the corporate-login user stories (docs/user-stories/auth/support-ldap.md)
 * as ONE login-flow file (they share the ldap-toggle → ldap-login-form closure
 * and the realm setup helper):
 *   - US-LD-001: employee signs in with the corporate directory account
 *     (happy path + anti-enumeration failures + directory-unavailable)
 *   - US-LD-002: first login JIT-provisions the Herald account with zero
 *     registration steps (consent, when the realm requires it, comes before
 *     the account is created)
 *   - US-LD-003 (login side): admin disables LDAP → entry hidden, password
 *     login unaffected (regression guard for the shared login state machine)
 *
 * The directory is the REAL demo OpenLDAP container (seeded users: alice with
 * mail, a duplicate uid under two OUs). The realm's config — including the
 * private-CA trust the v1 Settings UI does not manage — is seeded via the
 * admin configs API by helpers/ldap-setup.ts; every user-visible step below
 * is driven through the UI. This mirrors the email-otp demos reading the OTP
 * code from Redis: the flow is real, the out-of-band input is injected.
 *
 * Anti-enumeration is asserted as EQUALITY: wrong password, unknown user and
 * multi-match (the duplicate uid) must produce the exact same error text —
 * never per-cause differences.
 *
 * NOT-COVERED (explicit): disabled-account rejection, directory-email
 * matching an existing account, no-mail JIT provisioning (placeholder), the
 * TOTP/OAuth co-existence flows and audit visibility — all covered by backend
 * scenario tests; the no-mail user also needs an uncleanable placeholder
 * account. Cross-realm config isolation is covered by backend tests.
 */

import { test, expect, cleanupTestData, type Page } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { loginWithCredentials } from '../helpers/auth'
import {
  DEMO_LDAP,
  enableLdapForRealm,
  disableLdapForRealm,
} from '../helpers/ldap-setup'
import type { Response } from '@playwright/test'
import { SELECTORS } from '../selectors'

const REALM_ID = 'realm-001'
const REGISTERED_EMAIL = 'user@realm-001.com'
const PASSWORD = 'password'
// Nothing listens here — the backend classifies the dead directory as
// unavailable (503) and the UI maps it to the localized message.
const UNREACHABLE_LDAPS_URL = 'ldaps://127.0.0.1:6363'

test.describe('[Regular User] LDAP corporate-directory login demo tests', () => {
  let testStartTime: number

  test.beforeEach(async ({ page, testStartTime: startTime }) => {
    testStartTime = startTime

    await verifyTestEnvironment(page, {
      requiredRealms: ['realm-001'],
      requiredUsers: ['admin@realm-001.com'],
    })
  })

  test.afterEach(async ({ page, demoLogger }) => {
    // Best-effort restore: leave LDAP disabled so other demos are unaffected
    // (disableLdapForRealm is itself try/catch-wrapped).
    try {
      await disableLdapForRealm(page, demoLogger, REALM_ID)
    } catch (error) {
      console.warn('[Ldap Demo] teardown disableLdapForRealm failed:', error)
    }

    // The JIT-provisioned directory user (alice) must be deleted so
    // subsequent runs start with no linked identity.
    await cleanupTestData(page, REALM_ID, {
      keepUsers: ['admin@realm-001.com', 'user@realm-001.com'],
      testUserEmails: usedEmails,
      timestamp: testStartTime,
      verbose: false,
    })
  })

  // Per-test list of JIT-created accounts to delete in afterEach.
  let usedEmails: string[] = []

  /**
   * Drive the corporate login UI: fresh session → login page → switch into
   * the corporate form → fill credentials → submit. Returns the
   * POST /login/ldap response so callers can assert its status.
   */
  async function submitLdapLogin(page: Page, username: string, password: string): Promise<Response> {
    await page.context().clearCookies()
    await page.evaluate(() => {
      localStorage.clear()
      sessionStorage.clear()
    })
    await page.goto(`/${REALM_ID}/auth/login`)
    await expect(page.locator(SELECTORS.login.container)).toBeVisible()

    const toggle = page.locator(SELECTORS.ldap.loginRouteToggle)
    await expect(toggle).toBeVisible({ timeout: 10000 })
    await toggle.click()
    await expect(page.locator(SELECTORS.ldap.form)).toBeVisible({ timeout: 5000 })

    const responsePromise = page.waitForResponse('**/api/auth/**/login/ldap', {
      timeout: 15000,
    })
    await page.locator(SELECTORS.ldap.usernameInput).fill(username)
    await page.locator(SELECTORS.ldap.passwordInput).fill(password)
    await page.locator(SELECTORS.ldap.submitButton).click()
    return responsePromise
  }

  /**
   * Submit corporate credentials and return the shared login error region's
   * text once it renders (the failure surface for every LDAP login error).
   *
   * The rendered 401 text appends a per-request correlation id in parentheses
   * (frontend getErrorMessage behavior for support); the anti-enumeration
   * invariant lives in the message copy itself, so the id is stripped before
   * callers compare texts for equality.
   */
  async function submitLdapLoginAndReadError(page: Page, username: string, password: string): Promise<string> {
    const response = await submitLdapLogin(page, username, password)
    expect(response.status()).toBe(401)
    const errorRegion = page.locator(SELECTORS.login.errorMessage)
    await expect(errorRegion).toBeVisible({ timeout: 10000 })
    const text = (await errorRegion.innerText()).trim()
    return text.replace(/\s*\([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\)$/i, '')
  }

  // --------------------------------------------------------------------------
  // US-LD-001 + US-LD-002 — corporate first login: JIT-provisioned, no friction
  // --------------------------------------------------------------------------
  test('US-LD-001+US-LD-002: first corporate login JIT-provisions the account and signs in', async ({ page, demoLogger }) => {
    usedEmails = [DEMO_LDAP.users.alice.email]

    await test.step('Given: LDAP is enabled for realm-001', async () => {
      await enableLdapForRealm(page, demoLogger, REALM_ID)
    })

    await test.step('When: the employee submits corporate credentials through the UI', async () => {
      const response = await submitLdapLogin(page, DEMO_LDAP.users.alice.username, DEMO_LDAP.users.alice.password)
      expect(response.ok()).toBeTruthy()

      // Consent gate (when the realm requires agreements): consent is
      // expressed by clicking agree-and-continue — there is no checkbox, and
      // the account is only provisioned after consent.
      const agreeButton = page.locator(SELECTORS.legalConsent.loginAgreeAndContinueButton)
      const consentSurfaced = await agreeButton.isVisible({ timeout: 5000 }).catch(() => false)
      if (consentSurfaced) {
        const consentResponsePromise = page.waitForResponse('**/api/auth/**/login/ldap', {
          timeout: 15000,
        })
        await agreeButton.click()
        const consentResponse = await consentResponsePromise
        expect(consentResponse.ok()).toBeTruthy()
      }
    })

    await test.step('Then: the login completes with NO registration steps — a Bearer session is established', async () => {
      // Successful login IS the JIT proof: alice exists only in the directory
      // (no Herald account was pre-created), so reaching an authenticated URL
      // means the backend provisioned the account on the fly. No registration
      // link was clicked and no registration-policy prompt ever appeared.
      await page.waitForURL((url) => !url.pathname.endsWith('/auth/login'), {
        timeout: 15000,
      })
      expect(page.url()).not.toContain('/auth/login')
    })
  })

  // --------------------------------------------------------------------------
  // US-LD-001 — anti-enumeration: wrong password / unknown user / multi-match
  // all show the SAME generalized error
  // --------------------------------------------------------------------------
  test('US-LD-001: wrong password, unknown user and multi-match show the same generalized error', async ({ page, demoLogger }) => {
    usedEmails = []

    await test.step('Given: LDAP is enabled for realm-001', async () => {
      await enableLdapForRealm(page, demoLogger, REALM_ID)
    })

    // Reference error text; the two follow-up cases must match it exactly.
    let wrongPasswordError = ''

    await test.step('Wrong password for a directory user → generalized failure', async () => {
      wrongPasswordError = await submitLdapLoginAndReadError(
        page,
        DEMO_LDAP.users.alice.username,
        'not-alices-password'
      )
      expect(wrongPasswordError.length).toBeGreaterThan(0)
    })

    await test.step('Unknown directory user → the exact same text (no "no such user" leak)', async () => {
      const unknownUserError = await submitLdapLoginAndReadError(page, 'ghost', 'whatever')
      expect(unknownUserError).toBe(wrongPasswordError)
    })

    await test.step('Ambiguous multi-match uid → the exact same text (no guess-binding)', async () => {
      const multiMatchError = await submitLdapLoginAndReadError(page, 'dup', 'duppass')
      expect(multiMatchError).toBe(wrongPasswordError)
    })
  })

  // --------------------------------------------------------------------------
  // US-LD-001 — directory unavailable: localized message, other entries intact
  // --------------------------------------------------------------------------
  test('US-LD-001: unreachable directory shows the localized unavailable message and other entries keep working', async ({ page, demoLogger }) => {
    usedEmails = []

    await test.step('Given: LDAP is enabled but pointed at an unreachable directory', async () => {
      await enableLdapForRealm(page, demoLogger, REALM_ID, {
        url: UNREACHABLE_LDAPS_URL,
      })
    })

    await test.step('When: the employee submits corporate credentials → 503 directory unavailable', async () => {
      const response = await submitLdapLogin(page, DEMO_LDAP.users.alice.username, DEMO_LDAP.users.alice.password)
      expect(response.status()).toBe(503)
    })

    await test.step('Then: the shared error region shows the localized message (no directory details leaked)', async () => {
      const errorRegion = page.locator(SELECTORS.login.errorMessage)
      await expect(errorRegion).toBeVisible({ timeout: 10000 })
      await expect(errorRegion).toContainText(
        'Login is temporarily unavailable, please try again later'
      )
    })

    await test.step('And: back to password login — other entries are unaffected', async () => {
      await page.locator(SELECTORS.ldap.backButton).click()
      await expect(page.locator(SELECTORS.login.usernameInput)).toBeVisible({ timeout: 5000 })
      await expect(page.locator(SELECTORS.login.submitButton)).toBeVisible()
    })
  })

  // --------------------------------------------------------------------------
  // US-LD-003-degradation — disabling LDAP hides the entry; password login
  // still works (shared login state machine regression guard)
  // --------------------------------------------------------------------------
  test('US-LD-003-degradation: disabling LDAP hides the entry; password login still works', async ({ page, demoLogger }) => {
    usedEmails = []

    await test.step('Given: LDAP is enabled — the corporate entry is visible', async () => {
      await enableLdapForRealm(page, demoLogger, REALM_ID)

      await page.context().clearCookies()
      await page.evaluate(() => {
        localStorage.clear()
        sessionStorage.clear()
      })
      await page.goto(`/${REALM_ID}/auth/login`)
      await expect(page.locator(SELECTORS.login.container)).toBeVisible()
      await expect(page.locator(SELECTORS.ldap.loginRouteToggle)).toBeVisible({
        timeout: 10000,
      })
    })

    await test.step('When: admin disables LDAP for realm-001', async () => {
      await disableLdapForRealm(page, demoLogger, REALM_ID)
    })

    await test.step('Then: reload login — the corporate entry is NOT visible', async () => {
      // Clear any cached public ldap-status (React Query) before reloading so
      // a stale enabled:true does not keep the toggle mounted.
      await page.context().clearCookies()
      await page.evaluate(() => {
        localStorage.clear()
        sessionStorage.clear()
      })
      await page.goto(`/${REALM_ID}/auth/login`)
      await expect(page.locator(SELECTORS.login.container)).toBeVisible()
      await expect(page.locator(SELECTORS.ldap.loginRouteToggle)).toHaveCount(0, {
        timeout: 10000,
      })
    })

    await test.step('And: the registered user can still password-login (password entry survives LDAP-off)', async () => {
      // loginWithCredentials clears the session, submits the password login
      // form, handles re-consent if prompted, and asserts post-login
      // navigation — proving the shared login state machine is unaffected by
      // the corporate-entry integration.
      await loginWithCredentials(page, {
        realmId: REALM_ID,
        email: REGISTERED_EMAIL,
        password: PASSWORD,
      })
      expect(page.url()).not.toContain('/auth/login')
    })
  })
})
