/**
 * OAuth PKCE Happy Path Demo Tests
 *
 * Test Coverage:
 * - Test 1: Full OAuth Authorization Code + PKCE Flow
 *   (US-TP-001, US-TP-015, US-TP-016, US-RU-008, US-RU-010)
 * - Test 2: Normal Login Regression
 * - Test 3: OAuth Login Page State Verification (US-RU-010)
 * - Test 4: MCP Browser Authorization via Built-in herald-mcp Client
 *   (US-MCP-001 V4, .ai/design/mcp-server/frontend.md §8)
 *
 * @see docs/user-stories/auth/third-party-app.md（US-TP-*）、docs/user-stories/core/regular-user.md（US-RU-*）
 */

import { test, expect, cleanupTestData } from './fixtures/demo-page.fixtures'
import {
  BASE_URL,
  MCP_CLIENT_ID,
  MCP_LOOPBACK_REDIRECT_URI,
  generatePKCEPair,
  mcpAuthorize,
  mcpResourceUri,
  oauthAuthorize,
  oauthTokenExchange,
  seedOAuthClientApp,
  completeOAuthLoginAndGetAuthCode,
  blockExternalCallback,
  isLoginApiResponse,
} from './helpers/oauth-helpers'
import { verifyTestEnvironment } from './helpers/environment-setup'
import { DEMO_ADMIN, createBearerApiContext } from './helpers/auth'
import * as crypto from 'node:crypto'

test.describe('[OAuth PKCE] Happy Path Demo Tests', () => {
  let testStartTime: number

  test.beforeEach(async ({ page, testStartTime: startTime }) => {
    testStartTime = startTime

    await verifyTestEnvironment(page, {
      requiredRealms: ['admin'],
      requiredUsers: ['admin@cas.com'],
    })
  })

  test.afterEach(async ({ page }) => {
    await cleanupTestData(page, DEMO_ADMIN.realmId, {
      timestamp: testStartTime,
    })
  })

  test('Full OAuth Authorization Code + PKCE Flow', async ({ page, loginPage }) => {
    const realmId = DEMO_ADMIN.realmId
    const redirectUri = 'https://example.com/oauth/callback'
    const state = crypto.randomUUID()

    let clientId: string

    await test.step('Given: Admin is logged in', async () => {
      await loginPage.loginAsAdmin(DEMO_ADMIN.email, DEMO_ADMIN.password, realmId)
    })

    await test.step('And: OAuth client app is seeded', async () => {
      const adminApiContext = await createBearerApiContext(loginPage.getAccessToken())
      const result = await seedOAuthClientApp(adminApiContext, realmId, {
        appName: `PKCE Flow Test ${Date.now()}`,
        redirectUris: [redirectUri],
      })
      clientId = result.clientId
    })

    const pkce = generatePKCEPair()
    expect(pkce.code_verifier).toHaveLength(43)
    expect(pkce.code_challenge).toBeTruthy()

    let redirectLocation: string

    await test.step('When: Third-party SPA calls authorize endpoint', async () => {
      const result = await oauthAuthorize(BASE_URL, realmId, {
        client_id: clientId,
        redirect_uri: redirectUri,
        state,
        code_challenge: pkce.code_challenge,
      })

      expect(result.status).toBe(302)
      expect(result.redirectLocation).toBeTruthy()
      redirectLocation = result.redirectLocation!

      expect(redirectLocation).toContain(`/${realmId}/auth/login`)
      expect(redirectLocation).toContain('oauthClientId=')
      expect(redirectLocation).toContain('redirectUri=')
      expect(redirectLocation).toContain('state=')
    })

    let authCode: string

    await test.step('And: User follows redirect to login page', async () => {
      // Clear cookies + web storage so the browser appears unauthenticated;
      // otherwise the frontend detects the existing admin session (persisted
      // in localStorage under the Bearer model) and redirects to dashboard.
      await page.context().clearCookies()
      await page.evaluate(() => {
        localStorage.clear()
        sessionStorage.clear()
      })
      await page.goto(`${BASE_URL}${redirectLocation}`, { waitUntil: 'domcontentloaded' })
      await expect(page.getByTestId('login-card')).toBeVisible({ timeout: 10000 })
      await expect(page.getByTestId('email-input')).toBeVisible()
      await expect(page.getByTestId('password-input')).toBeVisible()
      await expect(page.getByTestId('login-submit-button')).toBeVisible()
    })

    await test.step('And: User submits credentials', async () => {
      // Block navigation to the unreachable callback URL
      await blockExternalCallback(page)

      // Intercept the login API response at the network level to capture the body
      // before the page navigation (window.location.href = redirectTo) destroys
      // the Playwright response context.
      let capturedLoginBody: { redirectTo: string } | undefined
      await page.route('**/api/auth/*/login', async (route) => {
        const response = await route.fetch()
        const body = await response.text()
        try { capturedLoginBody = JSON.parse(body) } catch { /* not JSON */ }
        await route.fulfill({ response, body })
      })

      await page.getByTestId('email-input').fill(DEMO_ADMIN.email)
      await page.getByTestId('password-input').fill(DEMO_ADMIN.password)
      await page.getByTestId('login-submit-button').click()

      // Wait for the intercepted response to be captured
      await page.waitForResponse(isLoginApiResponse, { timeout: 15000 })
      expect(capturedLoginBody).toBeTruthy()

      const redirectTo: string = capturedLoginBody!.redirectTo
      expect(redirectTo).toContain(redirectUri)
      expect(redirectTo).toContain('code=')
      expect(redirectTo).toContain(`state=${state}`)

      authCode = new URL(redirectTo).searchParams.get('code')!
      expect(authCode).toBeTruthy()
      expect(authCode).toMatch(/^ac_/)
    })

    await test.step('Then: Third-party backend exchanges code for token', async () => {
      const tokenResult = await oauthTokenExchange(BASE_URL, realmId, {
        grant_type: 'authorization_code',
        code: authCode!,
        redirect_uri: redirectUri,
        client_id: clientId,
        code_verifier: pkce.code_verifier,
      })

      expect('access_token' in tokenResult).toBe(true)
      const tokenResponse = tokenResult as { access_token: string; token_type: string; expires_in: number }
      expect(tokenResponse.access_token).toBeTruthy()
      expect(tokenResponse.token_type).toBe('Bearer')
      expect(tokenResponse.expires_in).toBeGreaterThan(0)
    })
  })

  test('Normal Login Regression', async ({ page, loginPage }) => {
    const realmId = DEMO_ADMIN.realmId

    await test.step('Given: User navigates to login page', async () => {
      await page.goto(`${BASE_URL}/${realmId}/auth/login`, { waitUntil: 'domcontentloaded' })
      await expect(page.getByTestId('login-card')).toBeVisible({ timeout: 10000 })
    })

    await test.step('When: User submits credentials without OAuth params', async () => {
      const loginResponsePromise = page.waitForResponse(isLoginApiResponse, { timeout: 15000 })
      await page.getByTestId('email-input').fill(DEMO_ADMIN.email)
      await page.getByTestId('password-input').fill(DEMO_ADMIN.password)
      await page.getByTestId('login-submit-button').click()
      const loginResponse = await loginResponsePromise
      expect(loginResponse.ok()).toBe(true)
    })

    await test.step('Then: Login succeeds and redirects to dashboard', async () => {
      await page.waitForURL(new RegExp(`/${realmId}(/|$|\\?)`), { timeout: 15000 })
      expect(page.url()).toMatch(new RegExp(`/${realmId}(/|$|\\?)`))
    })

    await test.step('And: Bearer token is persisted in localStorage', async () => {
      // The refresh token is persisted by the Herald SDK as a raw string under
      // its own key 'herald.refreshToken'
      // (frontend/src/lib/herald-client.ts HERALD_REFRESH_TOKEN_STORAGE_KEY,
      // passed to the SDK as storageKey). The Zustand 'auth-storage' persist
      // no longer contains a refreshToken — its partialize deliberately
      // excludes the token family (frontend/src/stores/auth-store.ts: "The
      // token family itself ... lives in the Herald SDK client"). The token
      // lands asynchronously after the post-login PKCE exchange, so poll
      // briefly until it appears in localStorage.
      let refreshToken = ''
      for (let i = 0; i < 20 && !refreshToken; i++) {
        refreshToken = await page.evaluate(
          () => window.localStorage.getItem('herald.refreshToken') ?? ''
        )
        if (!refreshToken) await page.waitForTimeout(250)
      }
      expect(refreshToken).toBeTruthy()
    })
  })

  test('OAuth Login Page State Verification', async ({ page }) => {
    const realmId = DEMO_ADMIN.realmId
    const fakeOAuthClientId = 'test-oauth-client'
    const fakeRedirectUri = 'https://example.com/callback'
    const fakeState = crypto.randomUUID()

    await test.step('When: User navigates with full OAuth params', async () => {
      const url = `${BASE_URL}/${realmId}/auth/login?oauthClientId=${encodeURIComponent(fakeOAuthClientId)}&redirectUri=${encodeURIComponent(fakeRedirectUri)}&state=${encodeURIComponent(fakeState)}`
      await page.goto(url, { waitUntil: 'domcontentloaded' })
    })

    await test.step('Then: Login form is visible and submit button is enabled', async () => {
      await expect(page.getByTestId('login-card')).toBeVisible({ timeout: 10000 })
      await expect(page.getByTestId('login-form')).toBeVisible()
      await expect(page.getByTestId('email-input')).toBeVisible()
      await expect(page.getByTestId('password-input')).toBeVisible()

      const submitButton = page.getByTestId('login-submit-button')
      await expect(submitButton).toBeVisible()
      // Submit button should NOT be disabled when OAuth params are complete
      await expect(submitButton).toBeEnabled()
    })

    await test.step('When: User navigates with partial OAuth params (only oauthClientId)', async () => {
      const url = `${BASE_URL}/${realmId}/auth/login?oauthClientId=${encodeURIComponent(fakeOAuthClientId)}`
      await page.goto(url, { waitUntil: 'domcontentloaded' })
    })

    await test.step('Then: OAuth incomplete error is visible', async () => {
      await expect(page.getByTestId('oauth-incomplete-error')).toBeVisible({ timeout: 10000 })
    })

    await test.step('And: Submit button is disabled', async () => {
      const submitButton = page.getByTestId('login-submit-button')
      await expect(submitButton).toBeVisible()
      await expect(submitButton).toBeDisabled()
    })
  })

  // ---------------------------------------------------------------------------
  // Test 4: MCP Browser Authorization via Built-in herald-mcp Client (V4)
  // ---------------------------------------------------------------------------
  test('MCP Browser Authorization via Built-in herald-mcp Client', async ({ page }) => {
    const realmId = DEMO_ADMIN.realmId
    const redirectUri = MCP_LOOPBACK_REDIRECT_URI
    const resource = mcpResourceUri(BASE_URL, realmId)

    let abandonedLoginUrl: string

    await test.step('When: An MCP authorize URL opens a pending browser session', async () => {
      const result = await mcpAuthorize(realmId, { resource })

      // The resource indicator is accepted: authorize 302s to the login page
      // with a server-side pending state instead of erroring back.
      expect(result.status).toBe(302)
      expect(result.redirectLocation).toBeTruthy()
      expect(result.redirectLocation).toContain(`/${realmId}/auth/login`)
      abandonedLoginUrl = result.redirectLocation!
    })

    await test.step('And: Abandoning before login yields no code and blocks nothing', async () => {
      // The agent user walks away from the login page without submitting
      // credentials: the pending state simply goes unused. A brand-new
      // authorize works immediately afterwards — abandonment leaves no
      // residue. (Stay on the app origin; navigating to about:blank would
      // make WebStorage unreachable for the login helper below.)
      await page.context().clearCookies()
      await page.goto(`${BASE_URL}${abandonedLoginUrl}`, { waitUntil: 'domcontentloaded' })
      await expect(page.getByTestId('login-card')).toBeVisible({ timeout: 10000 })

      const fresh = await mcpAuthorize(realmId, { resource })
      expect(fresh.status).toBe(302)
      expect(fresh.redirectLocation).toContain(`/${realmId}/auth/login`)
    })

    let authCode: string
    let pkce: ReturnType<typeof generatePKCEPair>

    await test.step('When: A fresh full MCP authorization completes in the browser', async () => {
      // blockExternalCallback (called inside the helper) also fulfills the
      // loopback callback so the login redirect does not hit a connection
      // error.
      const flowResult = await completeOAuthLoginAndGetAuthCode(
        page,
        BASE_URL,
        realmId,
        MCP_CLIENT_ID,
        redirectUri,
        crypto.randomUUID(),
        { email: DEMO_ADMIN.email, password: DEMO_ADMIN.password },
        { resource }
      )
      authCode = flowResult.authCode
      pkce = flowResult.pkce
      expect(authCode).toMatch(/^ac_/)
    })

    await test.step('Then: The PKCE code exchanges for an MCP access token', async () => {
      const tokenResult = await oauthTokenExchange(BASE_URL, realmId, {
        grant_type: 'authorization_code',
        code: authCode!,
        redirect_uri: redirectUri,
        client_id: MCP_CLIENT_ID,
        code_verifier: pkce.code_verifier,
      })

      // The public MCP client authenticates with PKCE alone (no secret) and
      // the issued token is bound to the MCP resource (DEC-mcp-server-003).
      expect('access_token' in tokenResult).toBe(true)
      const tokenResponse = tokenResult as {
        access_token: string
        token_type: string
        expires_in: number
      }
      expect(tokenResponse.access_token).toBeTruthy()
      expect(tokenResponse.token_type).toBe('Bearer')
      expect(tokenResponse.expires_in).toBeGreaterThan(0)
    })
  })
})
