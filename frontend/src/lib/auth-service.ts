/**
 * Authentication Service
 *
 * Direct API calls for authentication and authorization.
 * This service bypasses React Query to provide direct, synchronous
 * access to auth data for Zustand store updates.
 *
 * Login-family calls (login, logout, status) go through the `herald-auth-web` SDK
 * client (`lib/herald-client.ts`), which owns the token family and its
 * silent-refresh transport. Everything else (permissions, profile,
 * switch-client, PKCE exchange) uses the generated `@hey-api` client, which
 * (after `initBearerClient()` in `main.tsx`) injects
 * `Authorization: Bearer` from the SDK's token holder and silently refreshes
 * on a single 401.
 */

import {
  getCurrentUserPermissions,
  getUserRoles,
  getProfile,
  switchClient,
  oauthToken,
} from '@/lib/api-generated'
import type {
  StatusResponse,
  LoginRequestPayload,
  LoginResponse,
  UserProfile,
  BrowserTokenResponse,
  SwitchClientResponse,
} from '@/lib/api-generated'
import type { ConsentAgreement, HeraldClient, LoginResult } from 'herald-auth-web'
import { ensureHeraldClient, assertActiveHeraldClient } from '@/lib/herald-client'

/**
 * Extended status response with permissions
 */
export interface ExtendedStatusResponse extends StatusResponse {
  permissions?: string[]
}

/**
 * Fetch authentication status from the API (via the Herald SDK client).
 *
 * @returns The authentication status
 */
export async function fetchAuthStatus(herald: HeraldClient): Promise<StatusResponse> {
  const data = await herald.getStatus()
  assertActiveHeraldClient(herald)
  return data as StatusResponse
}

function authHeaders(herald: HeraldClient): HeadersInit {
  assertActiveHeraldClient(herald)
  const token = herald.tokens.getAccessToken()
  return token ? { Authorization: `Bearer ${token}` } : {}
}

/**
 * Fetch user roles and permissions from the API
 *
 * @returns Object containing permissions and roles arrays
 */
export async function fetchUserPermissions(
  herald: HeraldClient
): Promise<{ permissions: string[]; roles: string[] }> {
  const [permissionsResult, rolesResult] = await Promise.all([
    getCurrentUserPermissions({ headers: authHeaders(herald) }),
    getUserRoles({ headers: authHeaders(herald) }),
  ])
  assertActiveHeraldClient(herald)

  if (permissionsResult.error || rolesResult.error) {
    return { permissions: [], roles: [] }
  }

  return {
    permissions: permissionsResult.data?.permissions || [],
    roles: rolesResult.data?.roles || [],
  }
}

/**
 * Fetch user profile from the API
 *
 * @returns The user profile or null if not authenticated
 */
export async function fetchUserProfile(herald: HeraldClient): Promise<UserProfile | null> {
  const { data, error } = await getProfile({ headers: authHeaders(herald) })
  assertActiveHeraldClient(herald)
  if (error || !data) {
    return null
  }
  return data
}

/**
 * Map the SDK's `LoginResult` discriminated union back onto the legacy
 * `LoginResponse` branch shape consumed by `loginFlow` and the login page
 * (`secondFactors` / `requiresTotp` / `consentRequired` / `agreements` /
 * `redirectTo` / `realmId`). Shared with the TOTP / passkey login forms, whose
 * verify endpoints return the same multi-branch bodies. The consent branch
 * restores the raw snake_case agreement summaries from the SDK's passthrough
 * so the consent UI and `toAuthConsentAgreements` keep working unchanged.
 */
export function mapLoginResultToResponse(result: LoginResult): LoginResponse {
  switch (result.kind) {
    case 'success':
      // Tokens are already applied inside the SDK; callers of the direct
      // (non-PKCE) path only read `realmId` / the absence of branch flags.
      return { realmId: result.session.realmId ?? '' } as unknown as LoginResponse
    case 'requires-second-factor':
      return {
        requiresTotp: true,
        secondFactors: result.secondFactors,
        tempToken: result.tempToken,
        expiresInSeconds: result.expiresInSeconds,
        userId: result.userId,
        realmId: result.realmId,
        message: '',
      } as unknown as LoginResponse
    case 'consent-required':
      return {
        consentRequired: true,
        agreements: result.agreements.map(
          (a) => a.raw ?? { agreement_type: a.agreementType, version_id: a.versionId }
        ),
      } as unknown as LoginResponse
    case 'oauth-redirect':
      return { redirectTo: result.redirectTo } as unknown as LoginResponse
  }
}

/**
 * Shared first-factor context spread: the optional Turnstile/consent/OAuth
 * fields pass through identically for password and corporate-directory (LDAP)
 * logins — new context fields must land here once, not per performer.
 */
function firstFactorContext(credentials: LoginRequestPayload) {
  return {
    turnstileToken: credentials.turnstileToken ?? undefined,
    ...(credentials.agreements ? { agreements: credentials.agreements as ConsentAgreement[] } : {}),
    ...(credentials.oauthClientId ? { oauthClientId: credentials.oauthClientId } : {}),
    ...(credentials.redirectUri ? { redirectUri: credentials.redirectUri } : {}),
    ...(credentials.state ? { state: credentials.state } : {}),
  }
}

/**
 * Perform login with credentials (via the Herald SDK client).
 *
 * The SDK applies the issued token set itself on the success branch; the
 * PKCE/OAuth context is passed through so the backend answers with
 * `redirectTo` (the code exchange stays with the caller, DEC-js-sdk-008).
 *
 * @param realmId - The realm ID to login to
 * @param credentials - Login credentials
 * @returns Login response data
 */
export async function performLogin(
  realmId: string,
  credentials: LoginRequestPayload
): Promise<LoginResponse> {
  const herald = ensureHeraldClient(realmId)
  // The login page targets a product client (console vs account center) per
  // flow — rebind the SDK's request-body clientId accordingly.
  herald.tokens.bindClientId(credentials.clientId)
  const result = await herald.login({
    email: credentials.email ?? undefined,
    username: credentials.username ?? undefined,
    password: credentials.password,
    ...firstFactorContext(credentials),
  })
  return mapLoginResultToResponse(result)
}

/**
 * Perform a corporate-directory (LDAP) login (via the Herald SDK client).
 *
 * The directory username is NOT split into email/username — it is a directory
 * login identifier passed through verbatim. Otherwise mirrors `performLogin`:
 * the SDK applies the issued token set itself on the success branch, and the
 * OAuth/PKCE context passes through so the backend can answer with
 * `redirectTo`.
 */
export async function performLdapLogin(
  realmId: string,
  credentials: LoginRequestPayload
): Promise<LoginResponse> {
  const herald = ensureHeraldClient(realmId)
  herald.tokens.bindClientId(credentials.clientId)
  const result = await herald.loginWithLdap({
    username: credentials.username ?? '',
    password: credentials.password,
    ...firstFactorContext(credentials),
  })
  return mapLoginResultToResponse(result)
}

/**
 * Perform logout — revokes the Bearer access/refresh token family via the
 * Herald SDK client (which also clears its token state and emits
 * `logged-out`).
 */
export async function performLogout(herald: HeraldClient): Promise<void> {
  await herald.logout()
}

/**
 * Replace the active first-party token family with one bound to another
 * built-in Herald product.
 */
export class ClientSwitchError extends Error {
  constructor(
    public readonly status: number,
    cause?: unknown
  ) {
    super('Client switch failed', { cause })
  }
}

export async function switchFirstPartyClient(
  targetClientId: string,
  herald: HeraldClient
): Promise<SwitchClientResponse> {
  const { data, error, response } = await switchClient({
    body: { targetClientId },
    headers: authHeaders(herald),
  })
  assertActiveHeraldClient(herald)
  if (!data) {
    throw new ClientSwitchError(response.status, error)
  }
  return data
}

/**
 * Input for the FirstParty PKCE token exchange.
 */
export interface PkceTokenExchangeInput {
  /** The authorization `code` returned in the login `redirectTo` URL. */
  code: string
  /** The PKCE `code_verifier` paired with the challenge sent to authorize. */
  codeVerifier: string
  /** The pre-registered `redirect_uri` the code was issued for. */
  redirectUri: string
  /** First-party product Client App the authorization code was issued for. */
  clientId: string
}

/**
 * Exchange an OAuth authorization code for a FirstParty Bearer token set.
 *
 * Wraps the generated `oauthToken` SDK function (`POST /api/oauth/{realmId}/
 * token`). The token endpoint verifies the PKCE `code_verifier` against the
 * stored S256 challenge, then issues a `FirstParty` token set for the selected
 * built-in product Client App. The response uses OAuth
 * snake_case field names (`access_token`, `refresh_token`, ...) per RFC 6749.
 *
 * @param realmId - The realm ID the code was issued in.
 * @param input   - The code + PKCE verifier + redirect URI.
 * @returns The new access + refresh token set (normalized to camelCase).
 */
export async function performPkceTokenExchange(
  realmId: string,
  input: PkceTokenExchangeInput
): Promise<BrowserTokenResponse> {
  const { data, error } = await oauthToken({
    path: { realmId },
    body: {
      grant_type: 'authorization_code',
      code: input.code,
      code_verifier: input.codeVerifier,
      redirect_uri: input.redirectUri,
      client_id: input.clientId,
    },
  })
  if (error) {
    throw error
  }
  if (!data) {
    throw new Error('PKCE token exchange failed: no response data')
  }
  // Normalize OAuth snake_case → the shared BrowserTokenResponse shape so the
  // store and API client deal with one token-set contract everywhere.
  return {
    accessToken: data.access_token,
    refreshToken: data.refresh_token,
    tokenType: data.token_type,
    expiresIn: data.expires_in,
    refreshExpiresIn: data.refresh_expires_in,
  }
}

/**
 * Fetch auth data based on authentication status.
 * First checks auth status, then conditionally fetches user data in parallel.
 *
 * @returns Object containing auth status, user permissions, and profile
 */
export async function fetchAuthData(
  herald: HeraldClient,
  realmId: string
): Promise<{
  authStatus: StatusResponse
  userPermissions: { permissions: string[]; roles: string[] }
  userProfile: UserProfile | null
}> {
  // First, check authentication status
  const authStatus = await fetchAuthStatus(herald)
  if (authStatus.authenticated && authStatus.realmId !== realmId) {
    herald.tokens.clear()
    throw new Error('Authenticated session does not match the route realm')
  }

  // Fetch user data in parallel since they have no dependency on each other
  const [userPermissions, userProfile] = authStatus.authenticated
    ? await Promise.all([fetchUserPermissions(herald), fetchUserProfile(herald).catch(() => null)])
    : [{ permissions: [], roles: [] }, null]

  return {
    authStatus,
    userPermissions,
    userProfile,
  }
}
