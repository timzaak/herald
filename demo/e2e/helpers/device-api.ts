/**
 * Device Code API helpers for E2E tests.
 *
 * Low-level HTTP helpers that interact with device code grant endpoints.
 * These wrap the raw API calls so demo tests can set up device code state
 * without going through the UI for the "CLI tool" side of the flow.
 *
 * Usage:
 *   import { deviceAuthorize, deviceTokenPoll } from '../helpers/device-api'
 *
 *   const { device_code, user_code } = await deviceAuthorize('demo-realm', 'my-client-id')
 *   // ... user verifies via UI ...
 *   const token = await deviceTokenPoll('demo-realm', device_code)
 */

import { type APIRequestContext, type Page } from '@playwright/test'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface DeviceAuthorizeResponse {
  device_code: string
  user_code: string
  verification_uri: string
  verification_uri_complete: string
  expires_in: number
  interval: number
}

export interface DeviceTokenResponse {
  access_token: string
  token_type: string
  expires_in: number
}

export interface DeviceTokenErrorResponse {
  error: string
  error_description: string
}

export interface SeedDeviceCodeClientAppResult {
  /** OAuth client_id used in deviceAuthorize calls */
  clientId: string
  /** UUID of the client app (for edit/delete via admin UI) */
  appId: string
}

// ---------------------------------------------------------------------------
// Cookie extraction
// ---------------------------------------------------------------------------

/**
 * Extract the X-Auth session cookie from the browser context.
 *
 * @param page Playwright Page (must be authenticated)
 * @returns The cookie value string
 * @throws If X-Auth cookie is not found
 */
export async function getSessionCookie(page: Page): Promise<string> {
  const cookies = await page.context().cookies()
  const xAuth = cookies.find((c) => c.name === 'X-Auth')
  if (!xAuth) {
    throw new Error('[device-api] X-Auth cookie not found. Is the user logged in?')
  }
  return xAuth.value
}

// ---------------------------------------------------------------------------
// Client App Seed
// ---------------------------------------------------------------------------

/**
 * Create a Client App with device code grant enabled via API.
 *
 * Uses `request` (APIRequestContext) which inherits the browser context's
 * auth cookies. Call AFTER the page is authenticated.
 *
 * @param request APIRequestContext (e.g., page.request)
 * @param realmId Realm ID
 * @param appName Optional app name (default: "Device Code Test App")
 * @returns Object with clientId (OAuth client_id) and appId (UUID)
 */
export async function seedDeviceCodeClientApp(
  request: APIRequestContext,
  realmId: string,
  appName: string = 'Device Code Test App',
): Promise<SeedDeviceCodeClientAppResult> {
  const clientId = `device-test-${Date.now()}`

  const response = await request.post(`${BASE_URL}/api/client`, {
    data: {
      clientId,
      name: appName,
      description: 'Auto-created client app for device code demo tests',
      redirectUris: ['https://example.com/callback'],
      enabled: true,
      deviceCodeGrantEnabled: true,
    },
  })

  if (!response.ok()) {
    const body = await response.text()
    throw new Error(
      `[device-api] Failed to seed device code client app: ${response.status()} ${body}`,
    )
  }

  const data = await response.json()
  const appId: string = data.id

  console.log(
    `[device-api] Seeded client app "${appName}" (clientId=${clientId}, appId=${appId}) in realm "${realmId}"`,
  )

  return { clientId, appId }
}

// ---------------------------------------------------------------------------
// Device Authorization (RFC 8628 §3.1)
// ---------------------------------------------------------------------------

/**
 * POST /api/device/{realmId}/authorize
 *
 * Initiates a device code authorization request.
 * **Content-Type must be application/x-www-form-urlencoded** per RFC 8628.
 *
 * @param baseUrl Base URL (default: http://localhost:3000)
 * @param realmId Realm ID
 * @param clientId OAuth client_id of the device code enabled client app
 * @returns Device authorization response with device_code and user_code
 */
export async function deviceAuthorize(
  baseUrl: string = BASE_URL,
  realmId: string,
  clientId: string,
): Promise<DeviceAuthorizeResponse> {
  const params = new URLSearchParams()
  params.append('client_id', clientId)

  const response = await fetch(`${baseUrl}/api/device/${realmId}/authorize`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/x-www-form-urlencoded',
    },
    body: params.toString(),
  })

  if (!response.ok) {
    const body = await response.text()
    throw new Error(
      `[device-api] deviceAuthorize failed: ${response.status} ${body}`,
    )
  }

  const data: DeviceAuthorizeResponse = await response.json()
  console.log(
    `[device-api] Authorized: user_code=${data.user_code}, device_code=${data.device_code.substring(0, 8)}...`,
  )
  return data
}

// ---------------------------------------------------------------------------
// Device Token Poll (RFC 8628 §3.4, §3.5)
// ---------------------------------------------------------------------------

/**
 * POST /api/device/{realmId}/token
 *
 * Single polling request to check if the user has authorized the device code.
 * **Content-Type must be application/x-www-form-urlencoded** per RFC 8628.
 *
 * For repeated polling, call this function in a loop with appropriate interval
 * and error handling for "authorization_pending" / "slow_down" responses.
 *
 * @param baseUrl Base URL (default: http://localhost:3000)
 * @param realmId Realm ID
 * @param deviceCode The device_code from deviceAuthorize
 * @returns Token response on success, or error response object
 */
export async function deviceTokenPoll(
  baseUrl: string = BASE_URL,
  realmId: string,
  deviceCode: string,
): Promise<DeviceTokenResponse | DeviceTokenErrorResponse> {
  const params = new URLSearchParams()
  params.append('grant_type', 'urn:ietf:params:oauth:grant-type:device_code')
  params.append('device_code', deviceCode)

  const response = await fetch(`${baseUrl}/api/device/${realmId}/token`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/x-www-form-urlencoded',
    },
    body: params.toString(),
  })

  const data = await response.json()

  if (response.ok) {
    return data as DeviceTokenResponse
  }

  return data as DeviceTokenErrorResponse
}

// ---------------------------------------------------------------------------
// Device Verify (user enters code)
// ---------------------------------------------------------------------------

/**
 * POST /api/device/{realmId}/verify
 *
 * Verifies a user_code, transitioning the device state from "pending" to "verified".
 * Requires an authenticated session (X-Auth cookie).
 *
 * @param baseUrl Base URL (default: http://localhost:3000)
 * @param realmId Realm ID
 * @param userCode User code in XXXX-XXXX format
 * @param sessionCookie X-Auth session cookie value
 * @returns Verify response with client_app_name and client_app_icon_url
 */
export async function deviceVerify(
  baseUrl: string = BASE_URL,
  realmId: string,
  userCode: string,
  sessionCookie: string,
): Promise<{ client_app_name: string; client_app_icon_url: string | null }> {
  const response = await fetch(`${baseUrl}/api/device/${realmId}/verify`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Cookie: `X-Auth=${sessionCookie}`,
    },
    body: JSON.stringify({ user_code: userCode }),
  })

  if (!response.ok) {
    const body = await response.text()
    throw new Error(
      `[device-api] deviceVerify failed: ${response.status} ${body}`,
    )
  }

  return await response.json()
}

// ---------------------------------------------------------------------------
// Device Confirm (user approves/denies)
// ---------------------------------------------------------------------------

/**
 * POST /api/device/{realmId}/confirm
 *
 * Confirms or denies a device authorization after verification.
 * Requires an authenticated session (X-Auth cookie).
 *
 * @param baseUrl Base URL (default: http://localhost:3000)
 * @param realmId Realm ID
 * @param userCode User code in XXXX-XXXX format
 * @param approved Whether to authorize (true) or deny (false)
 * @param sessionCookie X-Auth session cookie value
 * @returns Confirm response with status ("authorized" or "denied")
 */
export async function deviceConfirm(
  baseUrl: string = BASE_URL,
  realmId: string,
  userCode: string,
  approved: boolean,
  sessionCookie: string,
): Promise<{ status: string }> {
  const response = await fetch(`${baseUrl}/api/device/${realmId}/confirm`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Cookie: `X-Auth=${sessionCookie}`,
    },
    body: JSON.stringify({ user_code: userCode, approved }),
  })

  if (!response.ok) {
    const body = await response.text()
    throw new Error(
      `[device-api] deviceConfirm failed: ${response.status} ${body}`,
    )
  }

  return await response.json()
}
