/**
 * Login Page Object
 *
 * Encapsulates login page operations.
 * Provides methods for user authentication.
 *
 */

import { Page, Locator, expect, type Response } from '@playwright/test'
import { SELECTORS } from '../selectors'
import { BasePage } from './base-page'
import type { UnifiedLogger } from '../helpers/unified-logger'

/**
 * Login credentials interface
 */
export interface LoginCredentials {
  email: string
  password: string
}

/**
 * Login Page Object
 *
 * Represents the login page at /{realmId}/auth/login
 *
 * @example
 * ```typescript
 * const loginPage = new LoginPage(page, logger)
 * await loginPage.goto()
 * await loginPage.login({ email: 'admin@cas.com', password: 'password' })
 * ```
 */
export class LoginPage extends BasePage {
  private accessToken: string | null = null

  // Selectors
  readonly container: Locator
  readonly title: Locator
  readonly emailInput: Locator
  readonly passwordInput: Locator
  readonly submitButton: Locator
  readonly errorMessage: Locator

  constructor(page: Page, logger?: UnifiedLogger) {
    super(page, logger)
    this.container = page.locator(SELECTORS.login.container)
    this.title = page.locator(SELECTORS.login.title)
    this.emailInput = page.locator(SELECTORS.login.emailInput)
    this.passwordInput = page.locator(SELECTORS.login.passwordInput)
    this.submitButton = page.locator(SELECTORS.login.submitButton)
    this.errorMessage = page.locator(SELECTORS.login.errorMessage)
  }

  /**
   * Navigate to login page
   *
   * @param realmId Realm ID (default: 'admin')
   * @param clientId Optional client ID for the login (default: undefined, uses frontend default)
   */
  async goto(realmId: string = 'admin', clientId?: string): Promise<void> {
    let url = `/${realmId}/auth/login`
    if (clientId) {
      url += `?clientId=${encodeURIComponent(clientId)}`
    }
    const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'
    await this.page.goto(`${BASE_URL}${url}`, { waitUntil: 'domcontentloaded' })

    // Wait a moment for the page to fully load and process any redirects
    await this.page.waitForTimeout(500)

    // Wait for navigation to complete (check if redirected to dashboard/profile/realm)
    const currentUrl = this.page.url()
    console.log(`[LoginPage] Current URL: ${currentUrl}`)

    // Check if we've been redirected to a dashboard/profile page (user is already logged in).
    // Support URLs with or without query parameters (e.g., /admin, /admin/, /admin?param=value).
    //
    // Post route-refactor (commit 03eeb456): the admin console and user account
    // center moved to TOP-LEVEL paths with NO realm prefix (/manage, /user/profile).
    // Realm-scoped variants are kept for backward-compat. Authenticated users visiting
    // an auth page are sent to /manage (admin) or /user/profile (regular user).
    const BASE_URL_NO_SLASH = BASE_URL.replace(/\/$/, '')
    const isRedirectedToDashboard = currentUrl.includes(`/${realmId}/manage`)
      || currentUrl.startsWith(`${BASE_URL_NO_SLASH}/manage`)
    const isRedirectedToProfile = currentUrl.includes(`/${realmId}/user/profile`)
      || currentUrl.startsWith(`${BASE_URL_NO_SLASH}/user/profile`)
      || currentUrl.startsWith(`${BASE_URL_NO_SLASH}/user`)
    const isRedirectedToRealm = currentUrl.match(new RegExp(`/${realmId}/?(\\?.*)?$`)) !== null

    console.log(`[LoginPage] Dashboard: ${isRedirectedToDashboard}, Profile: ${isRedirectedToProfile}, Realm: ${isRedirectedToRealm}`)

    if (isRedirectedToDashboard || isRedirectedToProfile || isRedirectedToRealm) {
      console.log(`[LoginPage] Already logged in, redirected to: ${currentUrl}`)
      return
    }

    // Wait for login page to load
    await expect(this.container).toBeVisible()
  }

  /**
   * Wait for login page to be visible
   */
  async waitForReady(): Promise<void> {
    await expect(this.container).toBeVisible()
    await expect(this.title).toBeVisible()
    await expect(this.emailInput).toBeVisible()
    await expect(this.passwordInput).toBeVisible()
    await expect(this.submitButton).toBeVisible()
  }

  /**
   * Fill login form
   *
   * @param credentials Login credentials
   */
  async fillLoginForm(credentials: LoginCredentials): Promise<void> {
    await this.fillField(this.emailInput, credentials.email)
    await this.fillField(this.passwordInput, credentials.password)
  }

  /**
   * Submit login form
   */
  async submit(): Promise<void> {
    await this.smartClick(this.submitButton)
    // Note: Don't wait for networkidle here - it's handled in login() method
  }

  /**
   * Force a clean login page by clearing localStorage (auth-storage) and
   * reloading the login URL.
   *
   * Under the browser Bearer token model (commit f3b8d48a) the session is
   * persisted in localStorage under `auth-storage`, NOT in a cookie. A leftover
   * `auth-storage` from a previous test or worker makes the root loader's
   * "authenticated → redirect away from auth pages" guard fire, so the login
   * card never renders and the subsequent form-fill times out.
   *
   * localStorage can only be cleared while the page is on the app origin, so
   * this must be called AFTER goto() has navigated to localhost:3000. We
   * unconditionally clear + reload (never short-circuit on the current URL)
   * because the root loader's auth redirect is asynchronous: a URL that reads
   * as `/auth/login` immediately after goto() can still redirect to /manage a
   * tick later, and an early-return here would let that redirect win.
   */
  private async forceFreshLoginPage(realmId: string = 'admin'): Promise<void> {
    const beforeUrl = this.page.url()
    console.log(`[LoginPage] Forcing fresh login page for realm ${realmId} (was on: ${beforeUrl})`)

    try {
      await this.page.evaluate(() => {
        localStorage.clear()
        sessionStorage.clear()
      })
    } catch {
      // page not on an http origin yet — ignore; the goto below lands on origin
    }

    // Reload to the realm login page so the root loader re-evaluates auth with
    // cleared storage and renders the login card. Use load state so the SPA has
    // finished its initial render before we assert the card.
    const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'
    await this.page.goto(`${BASE_URL}/${realmId}/auth/login`, {
      waitUntil: 'load',
    })

    // The root loader may still issue an async redirect even after a cleared
    // store (e.g. a raced rehydrate). Wait for the URL to settle on the login
    // page, then assert the card. If a raced rehydrate still pulled the URL off
    // /auth/login (observed intermittently when the previous test left a stale
    // auth-storage that rehydrated after waitForURL resolved), re-navigate once
    // so the now-cleared store yields a stable login page.
    await this.page.waitForURL(/\/auth\/login/, { timeout: 10000 }).catch(() => {})
    const onLoginUrl = /\/auth\/login/.test(this.page.url())
    const cardVisible = await this.container
      .isVisible({ timeout: 3000 })
      .catch(() => false)
    if (!onLoginUrl || !cardVisible) {
      await this.page.goto(`${BASE_URL}/${realmId}/auth/login`, {
        waitUntil: 'load',
      })
      await this.page.waitForURL(/\/auth\/login/, { timeout: 10000 }).catch(() => {})
    }
    await expect(this.container).toBeVisible()
  }

  /**
   * Login with credentials
   *
   * @param credentials Login credentials
   */
  async login(credentials: LoginCredentials): Promise<Response | null> {
    await this.fillLoginForm(credentials)

    const loginResponsePromise = this.page.waitForResponse(
      response =>
        /^\/api\/auth\/[^/]+\/login$/.test(new URL(response.url()).pathname)
        && response.request().method() === 'POST',
      { timeout: 10000 }
    ).catch(() => null)
    await this.submit()
    const loginResponse = await loginResponsePromise

    if (loginResponse && !loginResponse.ok()) {
      const errorBody = await loginResponse.text().catch(() => 'Unable to read error body')
      console.error(`[LoginPage] Login API failed: ${loginResponse.status()} - ${errorBody}`)
      throw new Error(`Login failed: API returned ${loginResponse.status()}`)
    }

    if (loginResponse) {
      console.log(`[LoginPage] Login API response status: ${loginResponse.status()}`)
    }

    // Handle login-time legal re-consent. Newly created users (or users with
    // outdated agreement versions) are presented with a reconsent view after
    // successful credential check instead of being redirected to their profile.
    // The view offers "Agree and Continue" / "Decline and return to login".
    // Without accepting, the user stays on /auth/login and downstream profile
    // assertions fail. Mirrors helpers/auth.ts `acceptLoginReconsentIfPresent`.
    const reconsentView = this.page.locator(SELECTORS.legalConsent.loginReconsentView)
    const needsReconsent = await reconsentView.isVisible({ timeout: 3000 }).catch(() => false)
    if (needsReconsent) {
      console.log(`[LoginPage] Login-time re-consent required; agreeing to current agreements`)
      const agreeButton = this.page.locator(SELECTORS.legalConsent.loginAgreeAndContinueButton)
      await this.smartClick(agreeButton)
    }

    return loginResponse
  }

  /**
   * Login as admin
   *
   * Always performs a fresh login by clearing cookies first.
   * This prevents stale authentication state from causing navigation failures.
   *
   * @param email Admin email (default: 'admin@cas.com')
   * @param password Admin password (default: 'password')
   * @param realmId Realm ID (default: 'admin')
   * @returns The userId from the login response
   */
  async loginAsAdmin(
    email: string = 'admin@cas.com',
    password: string = 'password',
    realmId: string = 'admin'
  ): Promise<string> {
    this.accessToken = null
    // Always clear cookies before login. (localStorage is cleared per-test by
    // the test's beforeEach on the app origin, so we do NOT blanket-clear it
    // here — that would log out an already-correct session established by a
    // fixture, forcing a wasteful and flaky re-login.)
    await this.page.context().clearCookies()
    console.log(`[LoginPage] Cookies cleared for fresh authentication`)

    console.log(`[LoginPage] Logging in as ${email} to realm ${realmId}`)

    // Navigate to login page. goto() detects the "already logged in" redirect
    // (top-level /manage or /user/...) and returns early in that case.
    await this.goto(realmId)

    // If goto() short-circuited because we're already on an authenticated
    // route, check whether the existing session is already the requested admin.
    // If so, skip re-login (matches legacy behavior and avoids a flaky forced
    // re-login). Only force a fresh login page when the login card did NOT
    // render (e.g. stale session redirected us off the login page).
    const onLoginCard = await this.container.isVisible({ timeout: 1000 }).catch(() => false)
    if (!onLoginCard) {
      // Not on the login card — either already authenticated or stuck. If the
      // current session user matches the requested admin email, reuse it;
      // otherwise force a fresh login.
      const sessionUser = await this.page
        .evaluate(() => {
          try {
            const raw = window.localStorage.getItem('auth-storage')
            if (!raw) return null
            const parsed = JSON.parse(raw)
            return parsed?.state?.user?.email ?? null
          } catch {
            return null
          }
        })
        .catch(() => null)

      if (sessionUser === email) {
        console.log(`[LoginPage] Already logged in as ${email}; skipping re-login`)
        return ''
      }

      // Stale/different session — force a clean login page.
      await this.forceFreshLoginPage(realmId)
    }

    const tokenResponsePromise = this.page.waitForResponse(
      response =>
        new URL(response.url()).pathname === `/api/oauth/${realmId}/token`
        && response.request().method() === 'POST',
      { timeout: 10000 }
    ).catch(() => null)

    const switchResponsePromise = this.page.waitForResponse(
      response =>
        new URL(response.url()).pathname === '/api/auth/browser-token/switch-client'
        && response.request().method() === 'POST',
      { timeout: 15000 },
    ).catch(() => null)

    const loginResponse = await this.login({ email, password })

    if (!loginResponse) {
      throw new Error('[LoginPage] Login API response was not captured')
    }
    if (!loginResponse.ok()) {
      const errorBody = await loginResponse.text().catch(() => 'Unable to read error body')
      console.error(`[LoginPage] Login API failed: ${loginResponse.status()} - ${errorBody}`)
      throw new Error(`Login failed: API returned ${loginResponse.status()}`)
    }

    const loginData = await loginResponse.json()
    const userId = loginData.userId
    const hasDirectAdminToken =
      typeof loginData.accessToken === 'string'
      && loginData.clientId === 'admin-web-console'
    let accessToken = typeof loginData.accessToken === 'string' ? loginData.accessToken : null
    if (!accessToken) {
      const tokenResponse = await tokenResponsePromise
      if (!tokenResponse) {
        throw new Error('[LoginPage] OAuth token response was not captured')
      }
      if (!tokenResponse.ok()) {
        const errorBody = await tokenResponse.text().catch(() => 'Unable to read error body')
        throw new Error(`PKCE token exchange failed: API returned ${tokenResponse.status()} ${errorBody}`)
      }
      const tokenData = await tokenResponse.json()
      accessToken = typeof tokenData.access_token === 'string' ? tokenData.access_token : null
    }

    await this.page.waitForURL(
      url => url.pathname === '/manage' || url.pathname.startsWith('/manage/'),
      { timeout: 15000 },
    )

    if (!hasDirectAdminToken) {
      const switchResponse = await switchResponsePromise
      if (!switchResponse) {
        throw new Error('[LoginPage] Admin client switch response was not captured')
      }
      if (!switchResponse.ok()) {
        const errorBody = await switchResponse.text().catch(() => 'Unable to read error body')
        throw new Error(`Admin client switch failed: API returned ${switchResponse.status()} ${errorBody}`)
      }
      const switchData = await switchResponse.json()
      if (switchData.clientId !== 'admin-web-console') {
        throw new Error(`Admin client switch returned unexpected clientId: ${switchData.clientId}`)
      }
      if (typeof switchData.accessToken !== 'string' || !switchData.accessToken) {
        throw new Error('Admin client switch response did not include an accessToken')
      }
      accessToken = switchData.accessToken
    }

    if (!accessToken) {
      throw new Error('[LoginPage] Login completed without an access token')
    }
    this.accessToken = accessToken
    console.log(`[LoginPage] Login successful, userId: ${userId}`)

    // Verify the browser-token session was established. Since commit f3b8d48a
    // replaced the X-Auth session cookie with the browser Bearer token model,
    // there is no cookie to check; the rotating refresh token is persisted in
    // localStorage under `auth-storage` (frontend Zustand `persist` store,
    // auth-store.ts:203 / auth-constants.ts:130). Its presence is the persistent
    // proof that the frontend completed login + PKCE token exchange.
    const authStorage = await this.page.evaluate(() => window.localStorage.getItem('auth-storage'))
    if (!authStorage) {
      throw new Error(`[LoginPage] Login failed: auth-storage not found in localStorage after login`)
    }

    console.log(`[LoginPage] auth-storage persisted (length=${authStorage.length})`)

    await expect(this.page).toHaveURL(/\/manage(?:\/|\?|$)/)

    return userId
  }

  getAccessToken(): string {
    if (!this.accessToken) {
      throw new Error('[LoginPage] Access token unavailable; a fresh login is required')
    }
    return this.accessToken
  }

  /**
   * Login as a regular user in the target realm.
   *
   * Unlike admin login, regular users are expected to land on user-facing routes
   * such as profile or points pages instead of admin console routes.
   *
   * @param email User email
   * @param password User password
   * @param realmId Realm ID
   * @param clientId Optional client ID (default: undefined, uses frontend default)
   */
  async loginAsUser(
    email: string,
    password: string,
    realmId: string,
    clientId?: string
  ): Promise<void> {
    this.accessToken = null
    await this.page.context().clearCookies()
    console.log(`[LoginPage] Cookies cleared for fresh user authentication`)

    console.log(`[LoginPage] Logging in as user ${email} to realm ${realmId}`)

    await this.goto(realmId, clientId)

    // If the login card did not render (e.g. a stale session redirected us off
    // the login page), force a clean login page. See loginAsAdmin.
    const onLoginCard = await this.container.isVisible({ timeout: 1000 }).catch(() => false)
    if (!onLoginCard) {
      await this.forceFreshLoginPage(realmId)
    }

    const tokenResponsePromise = this.page.waitForResponse(
      response =>
        new URL(response.url()).pathname === `/api/oauth/${realmId}/token`
        && response.request().method() === 'POST',
      { timeout: 10000 },
    ).catch(() => null)

    // The frontend ALWAYS runs the client-switch gate on login
    // (frontend/src/lib/auth-utils.ts:278): when the authenticated session's
    // clientId !== targetClientId, it calls switchFirstPartyClient(targetClientId),
    // which the backend implements (backend/api-auth/src/browser_token.rs:127-228)
    // by creating a NEW first-party token family and REVOKING the source token
    // family (revoke_family(context.family_id) at line 182). So the OAuth/PKCE
    // access_token captured above is the SOURCE token, which is then revoked.
    // getAccessToken() must therefore return the POST-switch token, which
    // supersedes the revoked source token. Mirrors loginAsAdmin's handling.
    const switchResponsePromise = this.page.waitForResponse(
      response =>
        new URL(response.url()).pathname === '/api/auth/browser-token/switch-client'
        && response.request().method() === 'POST',
      { timeout: 15000 },
    ).catch(() => null)

    const loginResponse = await this.login({ email, password })
    if (!loginResponse) {
      throw new Error('[LoginPage] Login API response was not captured')
    }
    if (!loginResponse.ok()) {
      const errorBody = await loginResponse.text().catch(() => 'Unable to read error body')
      throw new Error(`Login failed: API returned ${loginResponse.status()} ${errorBody}`)
    }

    const loginData = await loginResponse.json()
    let accessToken = typeof loginData.accessToken === 'string' ? loginData.accessToken : null
    if (!accessToken) {
      const tokenResponse = await tokenResponsePromise
      if (!tokenResponse) {
        throw new Error('[LoginPage] OAuth token response was not captured')
      }
      if (!tokenResponse.ok()) {
        const errorBody = await tokenResponse.text().catch(() => 'Unable to read error body')
        throw new Error(`PKCE token exchange failed: API returned ${tokenResponse.status()} ${errorBody}`)
      }
      const tokenData = await tokenResponse.json()
      accessToken = typeof tokenData.access_token === 'string' ? tokenData.access_token : null
    }

    if (!accessToken) {
      throw new Error('[LoginPage] Login completed without an access token')
    }

    // Post route-refactor (commit 03eeb456): regular users land on the top-level
    // /user/profile (NO realm prefix). Accept /user/... or the legacy realm root
    // /{realmId} for backward-compat.
    await this.page.waitForURL(
      new RegExp(`^http://localhost:3000/(user|${realmId})(/|\\?|$)`),
      { timeout: 15000 }
    )

    // The client-switch (if any) runs after the authenticated route settles.
    // Tolerate its absence: if no switch happened (response null) the OAuth/
    // direct accessToken remains authoritative; if the switch succeeded it
    // supersedes the now-revoked source token. Do NOT hard-fail on a missing
    // switch — the gate is conditional on clientId mismatch.
    const switchResponse = await switchResponsePromise
    if (switchResponse) {
      if (!switchResponse.ok()) {
        const errorBody = await switchResponse.text().catch(() => 'Unable to read error body')
        console.warn(`[LoginPage] User client switch failed (keeping source token): ${switchResponse.status()} - ${errorBody}`)
      } else {
        const switchData = await switchResponse.json().catch(() => null)
        const switchToken = typeof switchData?.accessToken === 'string' ? switchData.accessToken : null
        if (switchToken) {
          console.log(`[LoginPage] User client switch succeeded; using post-switch access token`)
          accessToken = switchToken
        } else {
          console.warn(`[LoginPage] User client switch response did not include an accessToken; keeping source token`)
        }
      }
    } else {
      console.log(`[LoginPage] No user client switch response captured; keeping source access token`)
    }

    this.accessToken = accessToken

    // Browser Bearer token model (commit f3b8d48a): no X-Auth cookie. The
    // rotating refresh token is persisted in localStorage under `auth-storage`
    // (frontend Zustand `persist` store). Its presence proves the session was
    // established. See loginAsAdmin for the full rationale.
    const authStorage = await this.page.evaluate(() => window.localStorage.getItem('auth-storage'))
    if (!authStorage) {
      throw new Error(`[LoginPage] User login failed: auth-storage not found in localStorage after login`)
    }

    console.log(`[LoginPage] User login successful, auth-storage persisted (length=${authStorage.length})`)
  }

  /**
   * Get error message text
   *
   * @returns Error message text (empty string if no error)
   */
  async getErrorMessage(): Promise<string> {
    const visible = await this.isVisible(this.errorMessage)
    if (!visible) return ''
    return await this.getText(this.errorMessage)
  }

  /**
   * Check if error message is visible
   *
   * @returns true if error message is visible
   */
  async hasError(): Promise<boolean> {
    return await this.isVisible(this.errorMessage)
  }

  /**
   * Verify we are on the login page
   */
  async isOnLoginPage(): Promise<boolean> {
    const url = this.getUrl()
    return url.includes('/login') && await this.isVisible(this.container)
  }
}
