/**
 * OAuth PKCE API helpers for E2E tests.
 *
 * The Location header from oauthAuthorize is a **relative URL**.
 * Callers must prepend `baseUrl` before using it with `page.goto()`.
 *
 * seedOAuthClientApp uses `request` (APIRequestContext) which inherits
 * the browser context's auth cookies. Call AFTER the page is authenticated.
 */

import { type APIRequestContext, type Page, type Response, type Route, expect } from '@playwright/test'
import * as crypto from 'node:crypto'
import { BASE_URL } from './environment-setup'

export { BASE_URL }

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface OAuthAuthorizeResult {
  status: number
  redirectLocation: string | null
  errorBody?: string
}

export interface OAuthTokenResponse {
  access_token: string
  token_type: string
  expires_in: number
}

export interface OAuthTokenErrorResponse {
  error: string
  error_description: string
}

export interface OAuthPKCEPair {
  code_verifier: string
  code_challenge: string
}

export interface SeedOAuthClientAppResult {
  clientId: string
  appId: string
}

// ---------------------------------------------------------------------------
// MCP OAuth fixtures (US-MCP-001 V4)
// ---------------------------------------------------------------------------

/**
 * Per-realm built-in public MCP OAuth client, seeded by migration 0011.
 * Redirect whitelist: loopback literals with RFC 8252 any-port matching.
 */
export const MCP_CLIENT_ID = 'herald-mcp'
export const MCP_LOOPBACK_REDIRECT_URI = 'http://127.0.0.1/callback'

/**
 * Canonical MCP resource indicator (RFC 8707): `{origin}/mcp/{realmId}`.
 * Must match the PRM-advertised resource exactly or authorize rejects with
 * invalid_target.
 */
export function mcpResourceUri(baseUrl: string, realmId: string): string {
  return `${baseUrl.replace(/\/$/, '')}/mcp/${realmId}`
}

/**
 * One-shot MCP authorize probe with fresh PKCE + state per call — the common
 * shape of the V3/V4 demo assertions. The verifier is never used here (these
 * calls assert the authorize outcome only); flows that exchange a code for a
 * token go through completeOAuthLoginAndGetAuthCode instead.
 */
export async function mcpAuthorize(
  realmId: string,
  opts?: { resource?: string; scope?: string },
): Promise<OAuthAuthorizeResult> {
  const pkce = generatePKCEPair()
  return oauthAuthorize(BASE_URL, realmId, {
    client_id: MCP_CLIENT_ID,
    redirect_uri: MCP_LOOPBACK_REDIRECT_URI,
    state: crypto.randomUUID(),
    code_challenge: pkce.code_challenge,
    ...opts,
  })
}

// ---------------------------------------------------------------------------
// PKCE Cryptographic Utilities
// ---------------------------------------------------------------------------

export function generatePKCEPair(): OAuthPKCEPair {
  const code_verifier = crypto.randomBytes(32).toString('base64url')
  const code_challenge = crypto
    .createHash('sha256')
    .update(code_verifier)
    .digest('base64url')

  return { code_verifier, code_challenge }
}

// ---------------------------------------------------------------------------
// Authorize URL Builder
// ---------------------------------------------------------------------------

export function buildAuthorizeUrl(
  baseUrl: string,
  realmId: string,
  params: {
    client_id: string
    redirect_uri: string
    state: string
    code_challenge: string
    /** Optional scope; omitted → server default (MCP clients: mcp:profile:read). */
    scope?: string
    /** RFC 8707 resource indicator; required on the MCP client, rejected on others. */
    resource?: string
  },
): string {
  const query = new URLSearchParams({
    client_id: params.client_id,
    redirect_uri: params.redirect_uri,
    state: params.state,
    response_type: 'code',
    code_challenge: params.code_challenge,
    code_challenge_method: 'S256',
  })
  if (params.scope !== undefined) {
    query.set('scope', params.scope)
  }
  if (params.resource !== undefined) {
    query.set('resource', params.resource)
  }

  return `${baseUrl}/api/oauth/${encodeURIComponent(realmId)}/authorize?${query.toString()}`
}

// ---------------------------------------------------------------------------
// OAuth Authorize (capture 302 redirect)
// ---------------------------------------------------------------------------

export async function oauthAuthorize(
  baseUrl: string = BASE_URL,
  realmId: string,
  params: {
    client_id: string
    redirect_uri: string
    state: string
    code_challenge: string
    scope?: string
    resource?: string
  },
): Promise<OAuthAuthorizeResult> {
  const url = buildAuthorizeUrl(baseUrl, realmId, params)

  const response = await fetch(url, {
    method: 'GET',
    redirect: 'manual',
  })

  const status = response.status

  if (status === 302) {
    return { status, redirectLocation: response.headers.get('location') }
  }

  const errorBody = await response.text()
  return { status, redirectLocation: null, errorBody }
}

// ---------------------------------------------------------------------------
// OAuth Token Exchange
// ---------------------------------------------------------------------------

export async function oauthTokenExchange(
  baseUrl: string = BASE_URL,
  realmId: string,
  params: {
    grant_type: string
    code: string
    redirect_uri: string
    client_id: string
    code_verifier: string
  },
): Promise<OAuthTokenResponse | OAuthTokenErrorResponse> {
  const response = await fetch(`${baseUrl}/api/oauth/${encodeURIComponent(realmId)}/token`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(params),
  })

  const data = await response.json()
  return response.ok
    ? data as OAuthTokenResponse
    : data as OAuthTokenErrorResponse
}

// ---------------------------------------------------------------------------
// Client App Seed
// ---------------------------------------------------------------------------

export async function seedOAuthClientApp(
  request: APIRequestContext,
  realmId: string,
  options?: {
    appName?: string
    redirectUris?: string[]
  },
  baseUrl: string = BASE_URL,
): Promise<SeedOAuthClientAppResult> {
  const appName = options?.appName ?? 'OAuth PKCE Test App'
  const redirectUris = options?.redirectUris ?? ['https://example.com/oauth/callback']
  const clientId = `oauth-test-${Date.now()}`

  const response = await request.post(`${baseUrl}/api/client/${encodeURIComponent(realmId)}`, {
    data: {
      clientId,
      name: appName,
      description: 'Auto-created client app for OAuth PKCE demo tests',
      redirectUris,
      enabled: true,
      deviceCodeGrantEnabled: false,
    },
  })

  if (!response.ok()) {
    const body = await response.text()
    throw new Error(
      `[oauth-helpers] Failed to seed OAuth client app: ${response.status()} ${body}`,
    )
  }

  const data = await response.json()
  return { clientId, appId: data.id as string }
}

// ---------------------------------------------------------------------------
// Shared UI helpers for OAuth login flow
// ---------------------------------------------------------------------------

/** Block navigation to unreachable OAuth callback URLs in test environment. */
export async function blockExternalCallback(page: Page): Promise<void> {
  const fulfillBlocked = (route: Route) =>
    route.fulfill({ status: 200, body: '<html><body>Blocked</body></html>', contentType: 'text/html' })
  await page.route('https://example.com/**', fulfillBlocked)
  // Loopback MCP callbacks (RFC 8252) have no listener in the demo env.
  await page.route('http://127.0.0.1/**', fulfillBlocked)
}

/** Predicate matching the login API POST response. */
export function isLoginApiResponse(resp: Response): boolean {
  return resp.url().includes('/api/auth/') && resp.url().includes('/login') && resp.request().method() === 'POST'
}

/**
 * Complete the full authorize + login + extract auth code flow.
 * Returns the auth code and PKCE pair for subsequent token exchange.
 *
 * `extraAuthorizeParams` carries the MCP client's resource/scope when the
 * flow targets the built-in herald-mcp client; ordinary clients omit them.
 */
export async function completeOAuthLoginAndGetAuthCode(
  page: Page,
  baseUrl: string,
  realmId: string,
  clientId: string,
  redirectUri: string,
  state: string,
  credentials: { email: string; password: string },
  extraAuthorizeParams?: { scope?: string; resource?: string },
): Promise<{
  authCode: string
  pkce: OAuthPKCEPair
}> {
  const pkce = generatePKCEPair()

  const authorizeResult = await oauthAuthorize(baseUrl, realmId, {
    client_id: clientId,
    redirect_uri: redirectUri,
    state,
    code_challenge: pkce.code_challenge,
    ...extraAuthorizeParams,
  })

  expect(authorizeResult.status).toBe(302)
  expect(authorizeResult.redirectLocation).toBeTruthy()

  await page.context().clearCookies()
  // Under the Bearer token model auth is persisted in localStorage
  // (key 'auth-storage'), not cookies. clearCookies() alone leaves the user
  // logged in, so the authorize redirect skips the login card. Clear web
  // storage too so the redirect lands on the login page as expected.
  await page.evaluate(() => {
    localStorage.clear()
    sessionStorage.clear()
  })
  await page.goto(`${baseUrl}${authorizeResult.redirectLocation}`, { waitUntil: 'domcontentloaded' })
  await expect(page.getByTestId('login-card')).toBeVisible({ timeout: 10000 })

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

  await page.getByTestId('email-input').fill(credentials.email)
  await page.getByTestId('password-input').fill(credentials.password)
  await page.getByTestId('login-submit-button').click()

  // Wait for the intercepted response to be captured
  await page.waitForResponse(isLoginApiResponse, { timeout: 15000 })
  expect(capturedLoginBody).toBeTruthy()

  const redirectTo: string = capturedLoginBody!.redirectTo
  expect(redirectTo).toContain('code=')

  const authCode = new URL(redirectTo).searchParams.get('code')!
  expect(authCode).toBeTruthy()

  return { authCode, pkce }
}
