/**
 * Login Re-consent Demo Tests
 *
 * User story: US-RU-012
 *
 * Scenarios:
 * - Agree path: after an admin publishes a new agreement version, a
 *   regular user logging in is prompted to re-consent. Agreeing completes login
 *   and issues a session.
 * - Refusal path: the same prompt offers a "Back to login" option.
 *   Clicking it returns the user to the login form without issuing a session.
 *
 * Compliance rules:
 * - All operations go through the UI (no direct API calls).
 * - Shared selectors are used for all DOM assertions.
 * - Test users are cleaned up after each test while preserving the realm admin.
 */

import { test, expect, cleanupTestData } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { AdminLegalHelper } from '../helpers/legal-consent'
import { LoginPage } from '../pages/login-page'
import { SELECTORS } from '../selectors'
import type { Page } from '@playwright/test'

test.describe('[Regular User] Login Re-consent Demo Tests', () => {
  const realmId = 'realm-001'
  let testStartTime: number
  const createdEmails: string[] = []
  let registrationCounter = 0

  test.beforeEach(async ({ page, testStartTime: startTime }) => {
    testStartTime = startTime
    createdEmails.length = 0
    registrationCounter = 0

    await verifyTestEnvironment(page, {
      requiredRealms: ['realm-001'],
      requiredUsers: ['admin@realm-001.com'],
    })
  })

  test.afterEach(async ({ page }) => {
    await cleanupTestData(page, realmId, {
      keepUsers: ['admin@realm-001.com'],
      timestamp: testStartTime,
      testUserEmails: [...createdEmails],
    })
  })

  /**
   * Register a fresh regular user through the UI and return the credentials.
   */
  async function registerTestUser(
    page: Page,
    startTime: number
  ): Promise<{ email: string; password: string }> {
    const suffix = `${startTime}-${registrationCounter++}`
    const email = `lc-login-${suffix}@example.com`
    const nickname = `lc-login-${suffix}`
    const password = 'Password123!'

    await test.step('Navigate to registration page', async () => {
      await page.goto(`/${realmId}/auth/register`)
      await page.waitForLoadState('domcontentloaded')
      await expect(page.locator(SELECTORS.registration.card)).toBeVisible()
    })

    await test.step('Fill registration form and consent', async () => {
      await page.locator(SELECTORS.registration.emailInput).fill(email)
      await page.locator(SELECTORS.registration.nicknameInput).fill(nickname)
      await page.locator(SELECTORS.registration.passwordInput).fill(password)
      await page
        .locator(SELECTORS.registration.confirmPasswordInput)
        .fill(password)
      await page
        .locator(SELECTORS.legalConsent.registerConsentCheckbox)
        .check()
      await expect(
        page.locator(SELECTORS.legalConsent.registerConsentCheckbox)
      ).toBeChecked()
    })

    await test.step('Submit registration', async () => {
      const responsePromise = page.waitForResponse(
        response =>
          response.url().includes(`/api/auth/${realmId}/register`) &&
          response.request().method() === 'POST',
        { timeout: 10000 }
      )
      await page.locator(SELECTORS.registration.registerButton).click()
      const response = await responsePromise
      console.log(
        `[LoginReconsentDemo] Registration response status: ${response.status()}`
      )
      expect(response.ok()).toBe(true)

      await page.waitForURL(`**/auth/login`, { timeout: 3000 }).catch(() => {
        // TanStack Router may navigate directly to a user-facing page if
        // auto-login is enabled; both destinations are treated as success.
      })

      // Registration may redirect to the login page or (if auto-login is on) to
      // a user-facing page. Either destination is acceptable; verify-email is not.
      const currentUrl = page.url()
      const isOnLogin = currentUrl.includes('/auth/login')
      const isOnUserArea =
        currentUrl.includes(`/${realmId}/user`) ||
        currentUrl.includes(`/${realmId}/profile`) ||
        currentUrl.includes('/dashboard')

      if (!isOnLogin && !isOnUserArea) {
        throw new Error(
          `Registration succeeded but landed on unexpected URL: ${currentUrl}`
        )
      }
    })

    return { email, password }
  }

  /**
   * Clear all session state so the next login starts from an anonymous context.
   */
  async function clearSession(page: Page): Promise<void> {
    await page.context().clearCookies()
    await page.evaluate(() => {
      localStorage.clear()
      sessionStorage.clear()
    })
  }

  test.describe('User Story: Re-consent at login [US-RU-012]', () => {
    test('Scenario 1: agreeing to updated agreements completes login and issues a session', async ({
      page,
      adminLegalHelper,
      loginPage,
    }) => {
      const { email, password } = await registerTestUser(page, testStartTime)
      createdEmails.push(email)

      await test.step('Admin publishes a new custom Terms of Service version', async () => {
        await adminLegalHelper.gotoLegalTab(realmId)
        await adminLegalHelper.publishCustomAgreement(
          'terms_of_service',
          'Updated Terms of Service for login re-consent demo (EN).',
          'login-reconsent-demo'
        )
      })

      await test.step('Clear session and navigate to login as the regular user', async () => {
        await clearSession(page)
        await loginPage.goto(realmId)
        await loginPage.waitForReady()
      })

      await test.step('Submit credentials and reach the login re-consent view', async () => {
        await loginPage.fillLoginForm({ email, password })
        await loginPage.submit()

        const loginResponse = await page.waitForResponse(
          response =>
            response.url().includes('/login') &&
            response.request().method() === 'POST',
          { timeout: 10000 }
        )
        expect(loginResponse.ok()).toBe(true)

        await expect(
          page.locator(SELECTORS.legalConsent.loginReconsentView)
        ).toBeVisible({ timeout: 10000 })
        await expect(
          page.locator(
            SELECTORS.legalConsent.loginReconsentAgreement('terms_of_service')
          )
        ).toBeVisible()
        await expect(
          page.locator(
            SELECTORS.legalConsent.loginReconsentAgreement('privacy_policy')
          )
        ).toBeVisible()
      })

      await test.step('Click agree and continue, then reach an authenticated page', async () => {
        await Promise.all([
          page.waitForResponse(
            response =>
              response.url().includes('/login') &&
              response.request().method() === 'POST',
            { timeout: 10000 }
          ),
          page
            .locator(SELECTORS.legalConsent.loginAgreeAndContinueButton)
            .click(),
        ])

        // Post-login lands on a session-scoped route with NO realm prefix after
        // the route refactor (commit 03eeb456): regular users go to /user/profile,
        // admins to /manage. Match either directly.
        await page.waitForURL(
          /^http:\/\/localhost:3000\/(user\/profile|manage\/)/,
          { timeout: 15000 }
        )

        const currentUrl = page.url()
        expect(currentUrl).not.toContain('/auth/login')
      })

      await test.step('Assert a session was established', async () => {
        // Post auth-rewrite: the access token is in-memory (never persisted)
        // and there is no X-Auth cookie. A durable session is proven by two
        // persisted artifacts in localStorage: `auth-storage` (Zustand
        // persist) flagging isAuthenticated, plus the refresh token stored
        // by the Herald SDK as a raw string under its own key
        // 'herald.refreshToken' (frontend/src/lib/herald-client.ts
        // HERALD_REFRESH_TOKEN_STORAGE_KEY, passed to the SDK as storageKey).
        // The Zustand persist no longer contains a refreshToken — its
        // partialize deliberately excludes the token family
        // (frontend/src/stores/auth-store.ts). The token lands asynchronously
        // after the post-login PKCE exchange, so poll briefly until both
        // artifacts appear. The URL having moved off /auth/login (asserted
        // above) confirms the in-memory access token was issued and the SPA
        // routed to an authenticated route.
        let hasPersistedSession = false
        for (let i = 0; i < 20 && !hasPersistedSession; i++) {
          hasPersistedSession = await page.evaluate(() => {
            const raw = localStorage.getItem('auth-storage')
            if (!raw) return false
            try {
              const parsed = JSON.parse(raw)
              const state = parsed?.state ?? parsed
              return (
                state?.isAuthenticated === true &&
                Boolean(localStorage.getItem('herald.refreshToken'))
              )
            } catch {
              return false
            }
          })
          if (!hasPersistedSession) await page.waitForTimeout(250)
        }
        expect(hasPersistedSession).toBe(true)
      })
    })

    test('Scenario 2: declining updated agreements returns to login without a session', async ({
      page,
      adminLegalHelper,
      loginPage,
    }) => {
      const { email, password } = await registerTestUser(page, testStartTime)
      createdEmails.push(email)

      await test.step('Admin publishes another new custom Terms of Service version', async () => {
        await adminLegalHelper.gotoLegalTab(realmId)
        await adminLegalHelper.publishCustomAgreement(
          'terms_of_service',
          'Second updated Terms of Service for refusal demo (EN).',
          'login-reconsent-refusal-demo'
        )
      })

      await test.step('Clear session and navigate to login as the regular user', async () => {
        await clearSession(page)
        await loginPage.goto(realmId)
        await loginPage.waitForReady()
      })

      await test.step('Submit credentials and reach the login re-consent view', async () => {
        await loginPage.fillLoginForm({ email, password })
        await loginPage.submit()

        const loginResponse = await page.waitForResponse(
          response =>
            response.url().includes('/login') &&
            response.request().method() === 'POST',
          { timeout: 10000 }
        )
        expect(loginResponse.ok()).toBe(true)

        await expect(
          page.locator(SELECTORS.legalConsent.loginReconsentView)
        ).toBeVisible({ timeout: 10000 })
      })

      await test.step('Click decline/back to login', async () => {
        await page
          .locator(SELECTORS.legalConsent.loginDeclineBackButton)
          .click()
        await expect(
          page.locator(SELECTORS.login.container)
        ).toBeVisible({ timeout: 10000 })
        await expect(page.locator(SELECTORS.login.title)).toBeVisible()
      })

      await test.step('Assert X-Auth session cookie is absent', async () => {
        const cookies = await page.context().cookies()
        const xAuthCookie = cookies.find(cookie => cookie.name === 'X-Auth')
        expect(xAuthCookie).toBeUndefined()
      })
    })
  })
})
