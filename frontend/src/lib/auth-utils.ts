/**
 * Authentication Utilities
 *
 * Helper functions for authentication flows.
 * These functions coordinate between the auth service and Zustand store
 * to provide convenient APIs for authentication operations.
 *
 * Herald FirstParty login follows OAuth Authorization Code + PKCE:
 *   generate verifier/challenge → seed OAuth state via `/authorize` → submit
 *   login with the OAuth/clientId params → on `redirectTo` extract the code →
 *   `oauthToken` PKCE exchange → store AT in memory + RT in store. 2FA detours
 *   carry the pending PKCE state through. Logout revokes the Bearer family.
 */

import type {
  LoginRequestPayload,
  LoginResponse,
  VerifyTotpResponse,
  PasskeyVerifyResponse,
  OneTapDirectResponse,
  SignupResponse,
} from '@/lib/api-generated'
import {
  fetchAuthData,
  ClientSwitchError,
  performLogin,
  performLogout,
  performPkceTokenExchange,
  switchFirstPartyClient,
} from '@/lib/auth-service'
import { useAuthStore, clearAuthStorage } from '@/stores/auth-store'
import {
  applyTokenSet,
  bindHeraldClientId,
  ensureHeraldClient,
  getActiveHeraldClient,
} from '@/lib/herald-client'
import {
  ADMIN_WEB_CONSOLE_CLIENT_ID,
  USER_ACCOUNT_CENTER_CLIENT_ID,
  type FirstPartyClientId,
  hasAdminPermission,
  DEFAULT_USER_REDIRECT,
  DEFAULT_ADMIN_REDIRECT,
  getSafeRedirectPath,
} from '@/lib/constants/auth-constants'
import { realmPath, resolvedRealmFromPath } from '@/lib/realm-routing'
import { generatePkcePair, generateStateToken, extractAuthorizationCode } from '@/lib/pkce-utils'
import { FIRST_PARTY_CLIENT_ID } from '@/lib/constants/auth-constants'

// Re-exported so existing call sites (`register-form`, `forgot-password`,
// `verify-email`) keep a single import; the canonical value lives in
// `auth-constants`, which has no module-cycle with `auth-service`.
export { FIRST_PARTY_CLIENT_ID }

/** FirstParty OAuth callback path (must match backend `FIRST_PARTY_CALLBACK_PATH`). */
const FIRST_PARTY_CALLBACK_PATH = '/callback'

export interface LoginFlowResult {
  response: LoginResponse
  redirectPath: string
}

function redirectPathForPermissions(permissions: string[]): string {
  return hasAdminPermission(permissions) ? DEFAULT_ADMIN_REDIRECT : DEFAULT_USER_REDIRECT
}

/**
 * Check if a login / verify response requires (re-)consent.
 *
 * The backend may set either the camelCase `consentRequired` or the legacy
 * snake_case `consent_required` flag; this checks both. Shared so that every
 * login path (password, TOTP, Passkey first/second factor) applies the same
 * consent interlock consistently.
 */
export function isConsentRequired(response: {
  consentRequired?: boolean | null
  consent_required?: boolean | null
}): boolean {
  return (
    !!response.consentRequired ||
    !!(response as { consent_required?: boolean | null }).consent_required
  )
}

/**
 * Tracks the realm for which auth data has already been initialized this
 * session. Module-scoped: survives in-app navigation (so the root loader does
 * not re-fetch on every page switch) but resets on a full page reload (F5).
 */
let initializedRealm: string | null = null
let initializedClientId: string | null = null

/**
 * Build the FirstParty OAuth `redirect_uri`. It must exactly equal
 * `{frontend origin}/callback` to pass backend `validate_first_party_redirect`.
 */
function firstPartyRedirectUri(): string {
  const origin = typeof window !== 'undefined' ? window.location.origin : 'http://localhost'
  return `${origin}${FIRST_PARTY_CALLBACK_PATH}`
}

/**
 * Bootstrap a FirstParty PKCE flow: generate the verifier/challenge + state,
 * persist them in the auth store, and seed the backend OAuth state by calling
 * `/authorize`. Returns the OAuth params to attach to the subsequent login
 * payload. Idempotent within a flow: if a PKCE state is already active it is
 * reused (so a re-submit after consent does not regenerate the verifier).
 *
 * Returns `null` when PKCE cannot be established (e.g. window unavailable or
 * the authorize seed call fails); the caller then falls back to a non-PKCE
 * direct login, which yields a CustomUserUi-scoped token set.
 */
export async function beginFirstPartyPkceFlow(
  realmId: string,
  clientId: string = FIRST_PARTY_CLIENT_ID
): Promise<{
  oauthClientId: string
  redirectUri: string
  state: string
} | null> {
  const existing = useAuthStore.getState().getPkceState()
  if (existing?.clientId === clientId) {
    return {
      oauthClientId: existing.clientId,
      redirectUri: existing.redirectUri,
      state: existing.state,
    }
  }

  const { codeVerifier, codeChallenge } = await generatePkcePair()
  const state = generateStateToken()
  const redirectUri = firstPartyRedirectUri()

  // Seed the backend OAuth state (stores client_id/realm/redirect_uri/code_
  // challenge in Redis under `oauth:state:{state}`). The 302 redirect body is
  // ignored — we are already on the login page.
  try {
    const { oauthAuthorize } = await import('@/lib/api-generated')
    const result = await oauthAuthorize({
      path: { realmId },
      query: {
        client_id: clientId,
        redirect_uri: redirectUri,
        state,
        response_type: 'code',
        code_challenge: codeChallenge,
        code_challenge_method: 'S256',
      },
    })
    // A 302 is the success path here; treat a hard error as PKCE-unavailable.
    if (result.error) {
      return null
    }
  } catch {
    return null
  }

  useAuthStore.getState().setPkceState({
    codeVerifier,
    clientId,
    redirectUri,
    state,
  })

  return { oauthClientId: clientId, redirectUri, state }
}

/**
 * Complete the PKCE exchange when a login / verify response carries a
 * `redirectTo` of the form `{redirect_uri}?code={code}&state={state}`.
 *
 * On success, stores the AT in memory + RT in the store and returns `true`.
 * Returns `false` when there is no PKCE state, no code, or the returned `state`
 * does not match the one sent to /authorize (CSRF) — the caller should then
 * treat the `redirectTo` as before (e.g. external nav).
 */
async function tryCompletePkceExchange(
  realmId: string,
  redirectTo: string | null | undefined
): Promise<boolean> {
  const pkce = useAuthStore.getState().getPkceState()
  if (!pkce || !redirectTo) return false
  const parsed = extractAuthorizationCode(redirectTo)
  if (!parsed) return false
  // CSRF guard (RFC 6749 §10.12): the `state` returned with the code MUST equal
  // the `state` we sent to /authorize. A mismatch means the redirect did not
  // originate from our authorize call — refuse the exchange and drop PKCE state.
  if (parsed.state !== pkce.state) {
    useAuthStore.getState().setPkceState(null)
    return false
  }
  try {
    const tokenSet = await performPkceTokenExchange(realmId, {
      code: parsed.code,
      codeVerifier: pkce.codeVerifier,
      redirectUri: pkce.redirectUri,
      clientId: pkce.clientId,
    })
    applyTokenSet({
      accessToken: tokenSet.accessToken,
      refreshToken: tokenSet.refreshToken,
      clientId: pkce.clientId,
    })
    useAuthStore.getState().setPkceState(null)
    return true
  } catch {
    // Exchange failed (bad verifier / expired code) — force a clean re-login.
    useAuthStore.getState().logout()
    useAuthStore.getState().setPkceState(null)
    throw new Error('PKCE token exchange failed; please sign in again.')
  }
}

/**
 * Initialize authentication state
 *
 * Restores the session from a persisted refresh token when present: if a
 * refresh token exists but no in-memory access token (e.g. after a full page
 * reload), this refreshes first so the subsequent status call is authenticated.
 * Then fetches auth data and populates the Zustand store on first call per
 * realm. Subsequent calls for the same realm reuse the store state.
 *
 * @param realmId - The realm ID to initialize auth for
 * @param targetClientId - Product Client App required by the destination route
 * @param forceRefresh - Force a fresh fetch even if already initialized (default: false)
 * @returns Object containing auth status and redirect path
 */
export async function initializeAuth(
  realmId: string,
  targetClientId: FirstPartyClientId = USER_ACCOUNT_CENTER_CLIENT_ID,
  forceRefresh: boolean = false
): Promise<{
  authenticated: boolean
  redirectPath: string
  clientId: string | null
}> {
  const store = useAuthStore.getState()

  // Reuse already-initialized store state for this realm (skip on full reload)
  if (initializedRealm === realmId && initializedClientId === targetClientId && !forceRefresh) {
    const redirectPath =
      store.refreshClientId === ADMIN_WEB_CONSOLE_CLIENT_ID && hasAdminPermission(store.permissions)
        ? DEFAULT_ADMIN_REDIRECT
        : DEFAULT_USER_REDIRECT
    return {
      authenticated: store.isAuthenticated,
      redirectPath,
      clientId: initializedClientId,
    }
  }

  store.setIsLoading(true)

  // The Herald SDK client owns the token family from here on (DEC-js-sdk-013):
  // its storage holds the refresh token, its in-memory holder the access token.
  const herald = ensureHeraldClient(realmId, targetClientId)

  try {
    // --- Startup refresh-first restore ---
    // If a refresh token is persisted but there is no in-memory access token
    // (the normal case after a full page reload), refresh before checking
    // status so the session can be restored instead of appearing logged out.
    if (herald.storage.getRefreshToken() && !herald.tokens.getAccessToken()) {
      try {
        await herald.refresh()
      } catch {
        // Refresh failed (reuse/expiry/revocation) → force full re-login.
        // (The SDK also emits session-expired, which the herald-client bridge
        // turns into a token clear + store logout.)
        store.logout()
        initializedRealm = null
        initializedClientId = null
        return {
          authenticated: false,
          redirectPath: DEFAULT_USER_REDIRECT,
          clientId: null,
        }
      }
    }

    let { authStatus, userPermissions, userProfile } = await fetchAuthData()

    if (authStatus.authenticated && authStatus.clientId !== targetClientId) {
      try {
        const tokenSet = await switchFirstPartyClient(targetClientId)
        applyTokenSet({
          accessToken: tokenSet.accessToken,
          refreshToken: tokenSet.refreshToken,
          clientId: tokenSet.clientId,
        })
        ;({ authStatus, userPermissions, userProfile } = await fetchAuthData())
      } catch (error) {
        // A denied admin switch is an authorization outcome, not a logout.
        // Keep the source product session so the root route can safely send
        // the user back to their personal center.
        if (
          targetClientId !== ADMIN_WEB_CONSOLE_CLIENT_ID ||
          !(error instanceof ClientSwitchError) ||
          error.status !== 403
        ) {
          throw error
        }
      }
    }

    store.setAuthStatus(
      authStatus.authenticated,
      authStatus.realmId || realmId,
      authStatus.clientAppId
    )
    store.setUserPermissions(userPermissions.permissions, userPermissions.roles)
    store.setUserProfile(userProfile)

    initializedRealm = realmId
    initializedClientId = authStatus.clientId

    const redirectPath =
      store.refreshClientId === ADMIN_WEB_CONSOLE_CLIENT_ID &&
      hasAdminPermission(userPermissions.permissions)
        ? DEFAULT_ADMIN_REDIRECT
        : DEFAULT_USER_REDIRECT

    return {
      authenticated: authStatus.authenticated,
      redirectPath,
      clientId: authStatus.clientId,
    }
  } catch {
    // The Herald client is guaranteed to exist here (created above the try),
    // so its storage is the source of truth for an established token family.
    if (herald.storage.getRefreshToken()) {
      // Status/client-switch failures can be transient. Clear stale UI auth
      // data, but keep the established token family so a later initialization
      // can retry or a full-page reload can use refresh-first restore.
      store.setAuthStatus(false)
      store.setUserPermissions([], [])
      store.setUserProfile(null)
    } else {
      store.reset()
    }
    initializedRealm = null
    initializedClientId = null
    return {
      authenticated: false,
      redirectPath: DEFAULT_USER_REDIRECT,
      clientId: null,
    }
  } finally {
    store.setIsLoading(false)
  }
}

/**
 * After a successful authentication (login, 2FA verify, or PKCE exchange),
 * fetch the authenticated user's status / profile / permissions and hydrate
 * the store, then mark `realmId` as initialized. Returns the fetched data so
 * the caller can choose its redirect target.
 */
async function hydrateAuthenticatedSession(
  store: ReturnType<typeof useAuthStore.getState>,
  realmId: string
): Promise<Awaited<ReturnType<typeof fetchAuthData>>> {
  const fetched = await fetchAuthData()
  store.setAuthStatus(fetched.authStatus.authenticated, fetched.authStatus.realmId || realmId)
  store.setUserPermissions(fetched.userPermissions.permissions, fetched.userPermissions.roles)
  store.setUserProfile(fetched.userProfile)
  initializedRealm = realmId
  initializedClientId = fetched.authStatus.clientId
  return fetched
}

/**
 * Login flow
 * Handles the complete login process including API call and state update.
 *
 * Herald FirstParty path: bootstraps a PKCE flow, attaches the OAuth params to
 * the login payload, and on a `redirectTo` response completes the PKCE token
 * exchange (AT in memory, RT in store) instead of relying on a cookie session.
 * If PKCE could not be bootstrapped, falls back to a direct login.
 *
 * @param realmId - The realm ID to login to
 * @param credentials - Login credentials
 * @returns Login response data
 * @throws Error if login fails or requires TOTP
 */
export async function loginFlow(
  realmId: string,
  credentials: LoginRequestPayload
): Promise<LoginFlowResult> {
  const store = useAuthStore.getState()
  // True once the Bearer token family has been issued + persisted (PKCE exchange
  // succeeded). Guards the catch block: a failure AFTER this point (e.g. a
  // transient 401 while fetching the post-login auth data) must NOT destroy the
  // persisted refresh token, or a subsequent full-page navigation to a protected
  // route cannot restore the session and is wrongly bounced to login.
  let tokensEstablished = false

  try {
    // Attach FirstParty PKCE OAuth params unless the caller already supplied
    // an explicit OAuth context (third-party OAuth clients keep their own).
    let loginCredentials = credentials
    if (!credentials.oauthClientId) {
      const pkceParams = await beginFirstPartyPkceFlow(realmId, credentials.clientId)
      if (pkceParams) {
        loginCredentials = {
          ...credentials,
          oauthClientId: pkceParams.oauthClientId,
          redirectUri: pkceParams.redirectUri,
          state: pkceParams.state,
        }
      }
    }

    const loginResponse = await performLogin(realmId, loginCredentials)

    if (loginResponse.requiresTotp) {
      // 2FA detour: keep the PKCE state so the post-TOTP exchange can complete.
      return { response: loginResponse, redirectPath: DEFAULT_USER_REDIRECT }
    }

    if (isConsentRequired(loginResponse)) {
      return { response: loginResponse, redirectPath: DEFAULT_USER_REDIRECT }
    }

    // PKCE path: the backend returns `redirectTo = {redirect_uri}?code=...`.
    // Complete the exchange and store the token set instead of navigating.
    if (loginResponse.redirectTo) {
      const exchanged = await tryCompletePkceExchange(realmId, loginResponse.redirectTo)
      if (exchanged) {
        // The Bearer token family (access in memory, refresh persisted) is now
        // established. From here on, a failure must NOT wipe it — see the guard
        // in the catch block.
        tokensEstablished = true
        const userRealmId = loginResponse.realmId || realmId
        store.login(userRealmId)
        const { userPermissions } = await hydrateAuthenticatedSession(store, userRealmId)
        const redirectPath = redirectPathForPermissions(userPermissions.permissions)
        // Return the response with redirectTo nulled so the caller proceeds to
        // its post-login redirect logic instead of navigating to /callback.
        return {
          response: { ...loginResponse, redirectTo: null },
          redirectPath,
        }
      }
      // PKCE exchange not applicable (no active flow / no code) → preserve the
      // original redirectTo so the caller can still navigate (e.g. external).
      return { response: loginResponse, redirectPath: DEFAULT_USER_REDIRECT }
    }

    // Direct (non-PKCE) login path: session is established via the token set
    // returned by the login body; fetch the authenticated user data.
    const userRealmId = loginResponse.realmId || realmId
    store.login(userRealmId)

    const { userPermissions } = await hydrateAuthenticatedSession(store, userRealmId)
    const redirectPath = redirectPathForPermissions(userPermissions.permissions)

    return { response: loginResponse, redirectPath }
  } catch (error) {
    // Only tear down the session when no Bearer token family was established
    // (e.g. bad credentials, PKCE exchange failure). If the token exchange
    // already succeeded, a transient failure in the subsequent auth-data fetch
    // must not destroy the persisted refresh token: a full-page navigation to a
    // protected route can still restore the session via `initializeAuth`'s
    // refresh-first restore. tryCompletePkceExchange already calls logout() on
    // its own exchange failure, so this only short-circuits the post-exchange
    // window.
    if (!tokensEstablished) {
      store.logout()
    }
    throw error
  }
}

/**
 * Logout flow
 * Handles the complete logout process including API call (Bearer family
 * revocation), state reset, storage cleanup, and navigation.
 *
 * @param realmId - The realm ID to logout from
 */
export async function logoutFlow(realmId: string): Promise<void> {
  const store = useAuthStore.getState()
  store.setIsLoading(true)

  try {
    // Perform logout API call — revokes the Bearer access/refresh token family.
    await performLogout()
  } catch (error) {
    // Log the error but continue with state cleanup
    console.error('Logout API call failed:', error)
  } finally {
    // Always reset the store, clear the SDK's token family and persisted
    // storage (refresh token + PKCE state).
    getActiveHeraldClient()?.tokens.clear()
    store.reset()
    initializedRealm = null
    initializedClientId = null
    store.setIsLoading(false)

    clearAuthStorage()

    // Navigate to login page - use window.location for simple redirect
    // since we need to reload the page to properly clear in-memory state.
    const realmContext = resolvedRealmFromPath(window.location.pathname)
    window.location.href = realmPath({ ...realmContext, realmId }, '/auth/login')
  }
}

export function checkAdminPermission(): boolean {
  const { permissions } = useAuthStore.getState()
  return hasAdminPermission(permissions)
}

export function getRedirectPath(): string {
  return redirectPathForPermissions(useAuthStore.getState().permissions)
}

/**
 * Get safe redirect path
 * Validates redirect path and returns a safe fallback if invalid
 *
 * @param redirectPath - The requested redirect path
 * @param fallback - The fallback path (defaults to user profile)
 * @returns The safe redirect path
 */
export function getSafeRedirect(
  redirectPath: string | undefined,
  fallback: string = DEFAULT_USER_REDIRECT
): string {
  return getSafeRedirectPath(redirectPath, fallback)
}

/**
 * Complete login after TOTP verification
 *
 * Carries the pending PKCE state through the 2FA detour: when the verify
 * response carries a `redirectTo` with a PKCE code, the exchange completes here.
 *
 * @param realmId - The realm ID
 * @param verifyResponse - The TOTP verification response from the API
 */
export async function completeLoginAfterTotp(
  realmId: string,
  verifyResponse: VerifyTotpResponse
): Promise<{ redirectPath?: string; redirectTo?: string | null }> {
  // PKCE flow: when redirectTo carries a code, complete the token exchange.
  if (verifyResponse.redirectTo) {
    const exchanged = await tryCompletePkceExchange(realmId, verifyResponse.redirectTo)
    if (exchanged) {
      const store = useAuthStore.getState()
      const { userPermissions } = await hydrateAuthenticatedSession(store, realmId)
      return { redirectPath: redirectPathForPermissions(userPermissions.permissions) }
    }
    // Not a PKCE redirect — return it so the caller can navigate externally.
    return { redirectTo: verifyResponse.redirectTo }
  }

  if (isConsentRequired(verifyResponse)) {
    return {}
  }

  const store = useAuthStore.getState()

  try {
    const { userPermissions } = await hydrateAuthenticatedSession(store, realmId)
    return { redirectPath: redirectPathForPermissions(userPermissions.permissions) }
  } catch (error) {
    store.logout()
    throw error
  }
}

/**
 * Complete login after Passkey verification (first or second factor).
 *
 * Behaviour mirrors `completeLoginAfterTotp` (PKCE exchange on redirectTo,
 * otherwise fetch auth data). `PasskeyVerifyResponse` is structurally the same
 * as `VerifyTotpResponse` (camelCase `consentRequired`/`agreements`/
 * `redirectTo`/`token`). Kept as a separate, strongly-typed entry point so
 * call sites don't need an unsafe cast and the consent interlock is applied
 * uniformly across all login paths.
 *
 * @param realmId - The realm ID
 * @param verifyResponse - The Passkey verification response from the API
 */
export async function completeLoginAfterPasskey(
  realmId: string,
  verifyResponse: PasskeyVerifyResponse
): Promise<{ redirectPath?: string; redirectTo?: string | null }> {
  // PKCE flow: when redirectTo carries a code, complete the token exchange.
  if (verifyResponse.redirectTo) {
    const exchanged = await tryCompletePkceExchange(realmId, verifyResponse.redirectTo)
    if (exchanged) {
      const store = useAuthStore.getState()
      const { userPermissions } = await hydrateAuthenticatedSession(store, realmId)
      return { redirectPath: redirectPathForPermissions(userPermissions.permissions) }
    }
    return { redirectTo: verifyResponse.redirectTo }
  }

  if (isConsentRequired(verifyResponse)) {
    return {}
  }

  const store = useAuthStore.getState()

  try {
    const { userPermissions } = await hydrateAuthenticatedSession(store, realmId)
    return { redirectPath: redirectPathForPermissions(userPermissions.permissions) }
  } catch (error) {
    store.logout()
    throw error
  }
}

/**
 * Complete a login after an Email-OTP verify.
 *
 * OTP login does NOT go through PKCE/OAuth and the
 * verify response carries no `redirectTo` and no consent step (the send-phase
 * consent gate is the only consent for auto-register; login-as-consent for
 * existing users is enforced server-side). The verify call runs through the
 * Herald SDK (`client.loginWithEmailOtp.verify`), which applies the issued
 * token set itself — so this function only rebinds the routing clientId,
 * marks the realm initialized via the shared `hydrateAuthenticatedSession`,
 * and returns the safe redirect path.
 *
 * `clientId` is the Client App the OTP code was issued for (the send/verify
 * request `clientId`), persisted alongside the SDK-owned token family so a
 * later refresh stays bound to the same product, exactly as the PKCE path
 * persists `FIRST_PARTY_CLIENT_ID`.
 *
 * @param realmId - The realm ID
 * @param clientId - The Client App id used for send/verify
 */
export async function completeLoginAfterEmailOtp(
  realmId: string,
  clientId: string
): Promise<{ redirectPath: string }> {
  const store = useAuthStore.getState()

  try {
    bindHeraldClientId(clientId)
    store.login(realmId)

    const { userPermissions } = await hydrateAuthenticatedSession(store, realmId)
    return { redirectPath: redirectPathForPermissions(userPermissions.permissions) }
  } catch (error) {
    store.logout()
    throw error
  }
}

/**
 * Complete a login after a Google One Tap direct-session exchange.
 *
 * One Tap on Herald's own login page is the first-party (non-PKCE) variant: the
 * backend `POST /api/oauth/{realmId}/google/one-tap` handler, when no
 * `downstreamState` is supplied, calls `issue_callback_token_response` and
 * returns a flattened `BrowserTokenSet` (`OneTapDirectResponse`) — the same
 * shape the Email-OTP verify endpoint returns. This therefore mirrors
 * `completeLoginAfterEmailOtp` exactly: persist the token set via the shared
 * store helper, mark the realm initialized via the shared
 * `hydrateAuthenticatedSession`, and return the safe redirect path. Token
 * storage is reused (`store.setTokens`) — never duplicated.
 *
 * `clientId` is the Herald Client App id the One Tap request was issued for
 * (`FIRST_PARTY_CLIENT_ID` on the first-party login page), persisted alongside
 * the refresh token so a later refresh rebinds to the same Client App, exactly
 * as the OTP/PKCE paths do. The `OneTapDirectResponse` body itself does not
 * carry `clientId`, so the caller (which owns the resolved Client App id) must
 * supply it.
 *
 * @param realmId - The realm ID
 * @param tokenResponse - The direct `OneTapDirectResponse` from the One Tap endpoint
 * @param clientId - The Herald Client App id used for the One Tap request
 */
export async function completeLoginAfterOneTap(
  realmId: string,
  tokenResponse: OneTapDirectResponse,
  clientId: string
): Promise<{ redirectPath: string }> {
  const store = useAuthStore.getState()

  try {
    applyTokenSet({
      accessToken: tokenResponse.accessToken,
      refreshToken: tokenResponse.refreshToken,
      clientId,
    })
    store.login(realmId)

    const { userPermissions } = await hydrateAuthenticatedSession(store, realmId)
    return { redirectPath: redirectPathForPermissions(userPermissions.permissions) }
  } catch (error) {
    store.logout()
    throw error
  }
}

/**
 * Complete a self-service realm signup session.
 *
 * The signup endpoint issues a first-party `admin-web-console` token set for the
 * newly created realm directly (DEC-012), so — like the OTP / One-Tap direct
 * paths — it does not go through PKCE. This mirrors
 * `completeLoginAfterEmailOtp` / `completeLoginAfterOneTap`: persist the token
 * set via the shared store helper (passing the admin-web-console `clientId`),
 * mark the new realm initialized via the shared `hydrateAuthenticatedSession`,
 * and return the safe redirect path. The caller then navigates to the new
 * realm's `/manage`.
 *
 * `realmId` is the NEWLY created realm (from `SignupResponse.realmId`). The
 * `SignupResponse` body carries `accessToken`/`refreshToken` but not `clientId`,
 * so the caller (which knows the console Client App) supplies it.
 */
export async function completeSignup(
  realmId: string,
  tokenResponse: Pick<SignupResponse, 'accessToken' | 'refreshToken'>,
  clientId: string
): Promise<{ redirectPath: string }> {
  const store = useAuthStore.getState()

  try {
    applyTokenSet({
      accessToken: tokenResponse.accessToken,
      refreshToken: tokenResponse.refreshToken,
      clientId,
    })
    store.login(realmId)

    const { userPermissions } = await hydrateAuthenticatedSession(store, realmId)
    return { redirectPath: redirectPathForPermissions(userPermissions.permissions) }
  } catch (error) {
    store.logout()
    throw error
  }
}

/**
 * Validate OAuth search params for completeness
 *
 * All 3 params (oauthClientId, redirectUri, state) must be present together.
 * Partial params indicate a misconfigured OAuth flow.
 *
 * @param search - Search params from URL
 * @returns oauthParams if complete, hasPartialOAuth flag for error display
 */
export function validateOAuthParams(search: {
  oauthClientId?: string
  redirectUri?: string
  state?: string
}): {
  oauthParams: { oauthClientId: string; redirectUri: string; state: string } | null
  hasPartialOAuth: boolean
} {
  const oauthParams =
    search.oauthClientId && search.redirectUri && search.state
      ? {
          oauthClientId: search.oauthClientId,
          redirectUri: search.redirectUri,
          state: search.state,
        }
      : null
  const hasPartialOAuth =
    !oauthParams && (!!search.oauthClientId || !!search.redirectUri || !!search.state)
  return { oauthParams, hasPartialOAuth }
}
