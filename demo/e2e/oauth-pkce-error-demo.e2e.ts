/**
 * OAuth PKCE Error and Edge Case Demo Tests
 *
 * Test Coverage:
 * - Test 1: Authorization Code Replay (US-TP-001 scenario 4)
 * - Test 2: PKCE Verification Failure (US-TP-001 scenario 6)
 * - Test 3: redirect_uri Not in Whitelist (US-TP-001 scenario 8)
 * - Test 4: Disabled Client App (US-TP-010 scenario 1)
 * - Test 5: Invalid Authorization Code (US-TP-006 scenario 4)
 * - Test 6: Login with Mismatched State (US-TP-001 scenario 7)
 * - Test 7: Partial OAuth Params Display Error (US-RU-010 scenario 4)
 * - Test 8: MCP authorize with missing/foreign resource → invalid_target
 * - Test 9: MCP authorize with non-MCP scope token → invalid_scope
 * - Test 10: Disabled MCP client rejects authorize until re-enabled
 *   (US-MCP-001 V4 negative paths, .ai/design/mcp-server/frontend.md §8)
 *
 * Each scenario is a separate test() because error tests require
 * different setup state and must not cascade failures.
 *
 * @see docs/user-stories/auth/third-party-app.md（US-TP-*）、docs/user-stories/core/regular-user.md（US-RU-*）、docs/user-stories/auth/client-app-settings.md（US-TP-010）
 */

import { test, expect, cleanupTestData } from './fixtures/demo-page.fixtures'
import {
  BASE_URL,
  MCP_CLIENT_ID,
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
import { ClientAppsPage } from './pages/client-apps-page'
import * as crypto from 'node:crypto'

test.describe('[OAuth PKCE] Error and Edge Case Demo Tests', () => {
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

  // ---------------------------------------------------------------------------
  // Test 1: Authorization Code Replay
  // ---------------------------------------------------------------------------

  test('Authorization code replay fails on second exchange', async ({
    page,
    loginPage,
  }) => {
    const realmId = DEMO_ADMIN.realmId
    const redirectUri = 'https://example.com/oauth/callback'
    const state = crypto.randomUUID()
    const appName = `Replay Test ${Date.now()}`

    let clientId: string

    await test.step('Given: Admin is logged in and OAuth client app is seeded', async () => {
      await loginPage.loginAsAdmin(DEMO_ADMIN.email, DEMO_ADMIN.password, realmId)
      const adminApiContext = await createBearerApiContext(loginPage.getAccessToken())
      const result = await seedOAuthClientApp(adminApiContext, realmId, {
        appName,
        redirectUris: [redirectUri],
      })
      clientId = result.clientId
    })

    let authCode: string
    let pkce: ReturnType<typeof generatePKCEPair>

    await test.step('And: Full PKCE flow completes producing an auth code', async () => {
      const flowResult = await completeOAuthLoginAndGetAuthCode(
        page, BASE_URL, realmId, clientId, redirectUri, state,
        { email: DEMO_ADMIN.email, password: DEMO_ADMIN.password },
      )
      authCode = flowResult.authCode
      pkce = flowResult.pkce
    })

    await test.step('When: First token exchange succeeds', async () => {
      const firstResult = await oauthTokenExchange(BASE_URL, realmId, {
        grant_type: 'authorization_code',
        code: authCode,
        redirect_uri: redirectUri,
        client_id: clientId,
        code_verifier: pkce.code_verifier,
      })

      expect('access_token' in firstResult).toBe(true)
      const tokenResp = firstResult as { access_token: string }
      expect(tokenResp.access_token).toBeTruthy()
    })

    await test.step('Then: Second token exchange with same code fails', async () => {
      const secondResult = await oauthTokenExchange(BASE_URL, realmId, {
        grant_type: 'authorization_code',
        code: authCode,
        redirect_uri: redirectUri,
        client_id: clientId,
        code_verifier: pkce.code_verifier,
      })

      const errorResp = secondResult as unknown as Record<string, unknown>
      expect('message' in errorResp || 'error' in errorResp).toBe(true)
      const message = (errorResp.message as string) || (errorResp.error as string) || ''
      expect(message).toContain('Invalid or expired authorization code')
    })
  })

  // ---------------------------------------------------------------------------
  // Test 2: PKCE Verification Failure
  // ---------------------------------------------------------------------------

  test('Wrong code_verifier produces PKCE verification failure', async ({
    page,
    loginPage,
  }) => {
    const realmId = DEMO_ADMIN.realmId
    const redirectUri = 'https://example.com/oauth/callback'
    const state = crypto.randomUUID()
    const appName = `PKCE Mismatch Test ${Date.now()}`

    let clientId: string

    await test.step('Given: Admin is logged in and OAuth client app is seeded', async () => {
      await loginPage.loginAsAdmin(DEMO_ADMIN.email, DEMO_ADMIN.password, realmId)
      const adminApiContext = await createBearerApiContext(loginPage.getAccessToken())
      const result = await seedOAuthClientApp(adminApiContext, realmId, {
        appName,
        redirectUris: [redirectUri],
      })
      clientId = result.clientId
    })

    let authCode: string

    await test.step('And: Full authorize + login flow produces an auth code', async () => {
      const flowResult = await completeOAuthLoginAndGetAuthCode(
        page, BASE_URL, realmId, clientId, redirectUri, state,
        { email: DEMO_ADMIN.email, password: DEMO_ADMIN.password },
      )
      authCode = flowResult.authCode
    })

    await test.step('When: Token exchange uses a wrong code_verifier', async () => {
      const wrongPkce = generatePKCEPair()

      const result = await oauthTokenExchange(BASE_URL, realmId, {
        grant_type: 'authorization_code',
        code: authCode,
        redirect_uri: redirectUri,
        client_id: clientId,
        code_verifier: wrongPkce.code_verifier,
      })

      await test.step('Then: Response contains PKCE verification failure', async () => {
        const errorResp = result as unknown as Record<string, unknown>
        const message = (errorResp.message as string) || (errorResp.error as string) || ''
        expect(message).toContain('PKCE verification failed')
      })
    })
  })

  // ---------------------------------------------------------------------------
  // Test 3: redirect_uri Not in Whitelist
  // ---------------------------------------------------------------------------

  test('Non-whitelisted redirect_uri is rejected at authorize', async ({
    page,
    loginPage,
  }) => {
    const realmId = DEMO_ADMIN.realmId
    const whitelistedUri = 'https://example.com/oauth/callback'
    const evilUri = 'https://evil.com/callback'
    const state = crypto.randomUUID()
    const appName = `Whitelist Test ${Date.now()}`

    let clientId: string

    await test.step('Given: OAuth client app is seeded with whitelist containing only example.com', async () => {
      await loginPage.loginAsAdmin(DEMO_ADMIN.email, DEMO_ADMIN.password, realmId)
      const adminApiContext = await createBearerApiContext(loginPage.getAccessToken())
      const result = await seedOAuthClientApp(adminApiContext, realmId, {
        appName,
        redirectUris: [whitelistedUri],
      })
      clientId = result.clientId
    })

    await test.step('When: Authorize is called with non-whitelisted redirect_uri', async () => {
      const pkce = generatePKCEPair()

      const result = await oauthAuthorize(BASE_URL, realmId, {
        client_id: clientId,
        redirect_uri: evilUri,
        state,
        code_challenge: pkce.code_challenge,
      })

      await test.step('Then: Response returns 400 with whitelist error', async () => {
        expect(result.status).toBe(400)
        expect(result.errorBody).toBeTruthy()
        expect(result.errorBody!).toContain('not in the whitelist')
      })
    })
  })

  // ---------------------------------------------------------------------------
  // Test 4: Disabled Client App
  // ---------------------------------------------------------------------------

  test('Disabled client app returns 403 at authorize', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    const realmId = DEMO_ADMIN.realmId
    const redirectUri = 'https://example.com/oauth/callback'
    const state = crypto.randomUUID()
    const appName = `Disabled Client Test ${Date.now()}`

    let clientId: string

    await test.step('Given: Admin is logged in and OAuth client app is seeded', async () => {
      await loginPage.loginAsAdmin(DEMO_ADMIN.email, DEMO_ADMIN.password, realmId)
      const adminApiContext = await createBearerApiContext(loginPage.getAccessToken())
      const result = await seedOAuthClientApp(adminApiContext, realmId, {
        appName,
        redirectUris: [redirectUri],
      })
      clientId = result.clientId
    })

    await test.step('And: Client app is disabled via admin UI', async () => {
      const clientAppsPage = new ClientAppsPage(page, demoLogger)
      await clientAppsPage.goto(realmId)
      await clientAppsPage.editClientApp(appName, { enabled: false }, realmId)
    })

    await test.step('When: Authorize is called with the disabled client_id', async () => {
      const pkce = generatePKCEPair()

      const result = await oauthAuthorize(BASE_URL, realmId, {
        client_id: clientId,
        redirect_uri: redirectUri,
        state,
        code_challenge: pkce.code_challenge,
      })

      await test.step('Then: Response returns 403 with disabled error', async () => {
        expect(result.status).toBe(403)
        expect(result.errorBody).toBeTruthy()
        expect(result.errorBody!).toContain('disabled')
      })
    })
  })

  // ---------------------------------------------------------------------------
  // Test 5: Invalid Authorization Code
  // ---------------------------------------------------------------------------

  test('Fabricated authorization code produces 400 at token exchange', async () => {
    const realmId = DEMO_ADMIN.realmId

    await test.step('When: Token exchange is called with a fabricated code', async () => {
      const result = await oauthTokenExchange(BASE_URL, realmId, {
        grant_type: 'authorization_code',
        code: 'ac_nonexistent_fabricated_code',
        redirect_uri: 'https://example.com/oauth/callback',
        client_id: 'some-client-id',
        code_verifier: 'fabricated_verifier_that_does_not_matter',
      })

      await test.step('Then: Response contains invalid code error', async () => {
        const errorResp = result as unknown as Record<string, unknown>
        const message = (errorResp.message as string) || (errorResp.error as string) || ''
        expect(message).toContain('Invalid or expired authorization code')
      })
    })
  })

  // ---------------------------------------------------------------------------
  // Test 6: Login with Mismatched State (state never stored in Redis)
  // ---------------------------------------------------------------------------

  test('Login with fabricated state returns 400 from login API', async ({
    page,
  }) => {
    const realmId = DEMO_ADMIN.realmId
    const fabricatedState = `fabricated-state-${crypto.randomUUID()}`
    const fabricatedClientId = `fabricated-client-${Date.now()}`

    await test.step('When: User navigates to login page with fabricated OAuth params (state never in Redis)', async () => {
      // Do NOT call authorize first -- the fabricated state was never stored in Redis.
      const loginUrl =
        `${BASE_URL}/${realmId}/auth/login?` +
        `oauthClientId=${encodeURIComponent(fabricatedClientId)}` +
        `&redirectUri=${encodeURIComponent('https://example.com/oauth/callback')}` +
        `&state=${encodeURIComponent(fabricatedState)}`

      await page.goto(loginUrl, { waitUntil: 'domcontentloaded' })
      await expect(page.getByTestId('login-card')).toBeVisible({ timeout: 10000 })
      await blockExternalCallback(page)
    })

    await test.step('And: User submits credentials', async () => {
      const loginResponsePromise = page.waitForResponse(isLoginApiResponse, { timeout: 15000 })

      await page.getByTestId('email-input').fill(DEMO_ADMIN.email)
      await page.getByTestId('password-input').fill(DEMO_ADMIN.password)
      await page.getByTestId('login-submit-button').click()

      const loginResponse = await loginResponsePromise

      await test.step('Then: Login API returns 400 with state error', async () => {
        expect(loginResponse.status()).toBe(400)
        const body = await loginResponse.text()
        expect(body).toContain('state')
      })
    })
  })

  // ---------------------------------------------------------------------------
  // Test 7: Partial OAuth Params Display Error
  // ---------------------------------------------------------------------------

  test('Partial OAuth params show error message and disable submit', async ({ page }) => {
    const realmId = DEMO_ADMIN.realmId

    await test.step('When: User navigates to login with only oauthClientId (partial params)', async () => {
      const url =
        `${BASE_URL}/${realmId}/auth/login?` +
        `oauthClientId=${encodeURIComponent('test-client')}`

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
  // Test 8: MCP authorize with missing / foreign resource (V4 negative)
  // ---------------------------------------------------------------------------

  test('MCP authorize with missing or foreign resource returns invalid_target', async () => {
    const realmId = DEMO_ADMIN.realmId

    await test.step('When: Authorize omits the resource indicator', async () => {
      const result = await mcpAuthorize(realmId)

      await test.step('Then: 302 back to the loopback callback with invalid_target, no code', async () => {
        expect(result.status).toBe(302)
        expect(result.redirectLocation).toContain('error=invalid_target')
        expect(result.redirectLocation).not.toContain('code=')
      })
    })

    await test.step('When: Authorize points the resource at another realm', async () => {
      const result = await mcpAuthorize(realmId, {
        resource: mcpResourceUri(BASE_URL, 'realm1'),
      })

      await test.step('Then: Also invalid_target — an agent cannot hop tenants', async () => {
        expect(result.status).toBe(302)
        expect(result.redirectLocation).toContain('error=invalid_target')
        expect(result.redirectLocation).not.toContain('code=')
      })
    })
  })

  // ---------------------------------------------------------------------------
  // Test 9: MCP authorize with a non-MCP scope token (V4 negative)
  // ---------------------------------------------------------------------------

  test('MCP authorize with a non-MCP scope token returns invalid_scope', async () => {
    const realmId = DEMO_ADMIN.realmId

    await test.step('When: Authorize requests openid alongside an MCP scope', async () => {
      const result = await mcpAuthorize(realmId, {
        resource: mcpResourceUri(BASE_URL, realmId),
        // openid is not one of the four mcp:* wire scopes — the MCP client
        // only ever receives MCP credentials, never identity tokens.
        scope: 'openid mcp:profile:read',
      })

      await test.step('Then: 302 back with invalid_scope, no code', async () => {
        expect(result.status).toBe(302)
        expect(result.redirectLocation).toContain('error=invalid_scope')
        expect(result.redirectLocation).not.toContain('code=')
      })
    })
  })

  // ---------------------------------------------------------------------------
  // Test 10: Disabled MCP client rejects authorize until re-enabled (V4/V3)
  // ---------------------------------------------------------------------------

  test('Disabled MCP client rejects authorization until re-enabled', async ({
    page,
    loginPage,
  }) => {
    const realmId = DEMO_ADMIN.realmId
    const resource = mcpResourceUri(BASE_URL, realmId)

    let adminApiContext: Awaited<ReturnType<typeof createBearerApiContext>>
    let mcpAppId: string

    await test.step('Given: Admin locates the built-in MCP client via the admin API', async () => {
      await loginPage.loginAsAdmin(DEMO_ADMIN.email, DEMO_ADMIN.password, realmId)
      adminApiContext = await createBearerApiContext(loginPage.getAccessToken())
      const listResponse = await adminApiContext.get(`${BASE_URL}/api/client?page=0&pageSize=50`)
      expect(listResponse.ok()).toBe(true)
      const items = (await listResponse.json()).items as Array<{
        id: string
        clientId: string
      }>
      mcpAppId = items.find((item) => item.clientId === MCP_CLIENT_ID)!.id
      expect(mcpAppId).toBeTruthy()
    })

    try {
      await test.step('When: The MCP client is disabled via the admin API', async () => {
        const response = await adminApiContext.put(`${BASE_URL}/api/client/${mcpAppId}`, {
          data: { enabled: false },
        })
        expect(response.ok()).toBe(true)
      })

      await test.step('Then: Authorize returns 403 with the disabled error', async () => {
        const result = await mcpAuthorize(realmId, { resource })
        expect(result.status).toBe(403)
        expect(result.errorBody).toContain('disabled')
      })
    } finally {
      // Shared preset cleanup: restore the enabled state no matter what.
      await adminApiContext
        .put(`${BASE_URL}/api/client/${mcpAppId}`, { data: { enabled: true } })
        .catch(() => undefined)
    }

    await test.step('And: A fresh authorization is accepted again after re-enabling', async () => {
      const result = await mcpAuthorize(realmId, { resource })
      // Old grants staying dead after re-enable is the backend contract
      // (mcp_scenarios); here a brand-new authorization is possible again.
      expect(result.status).toBe(302)
      expect(result.redirectLocation).toContain(`/${realmId}/auth/login`)
    })
  })
})
