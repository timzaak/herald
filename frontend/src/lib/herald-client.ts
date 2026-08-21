/**
 * Herald SDK binding — the own frontend as the first-party consumer of
 * `@herald/web` (DEC-js-sdk-013).
 *
 * One client instance per realm (a browser session operates in a single realm
 * context at a time). The SDK owns the token family end-to-end:
 *   - in-memory access token (its holder replaces the former store-side
 *     `accessTokenHolder`);
 *   - persisted refresh token via the SDK's storage adapter (replaces the
 *     Zustand-persisted refresh token);
 *   - single-flight 401 refresh (its transport core replaces api-client's
 *     local `refreshOnce`).
 *
 * The app's generated client (profile / admin / points APIs) keeps its own
 * interceptors in `api-client.ts`, but reads the access token and delegates
 * the refresh through this bridge. First-party PKCE exchange and switch-client
 * results are injected via `applyTokenSet` — the exchanges themselves stay in
 * the app (DEC-js-sdk-008).
 */

import { createHeraldClient, type HeraldClient, type SessionEvent } from '@herald/web'
import { useAuthStore } from '@/stores/auth-store'
import { FIRST_PARTY_CLIENT_ID } from '@/lib/constants/auth-constants'

/** localStorage key the SDK persists the refresh token under. */
export const HERALD_REFRESH_TOKEN_STORAGE_KEY = 'herald.refreshToken'

let baseUrlOverride: string | null = null
let active: { realmId: string; client: HeraldClient } | null = null

function handleSessionEvent(event: SessionEvent, client: HeraldClient): void {
  if (event.type === 'session-expired') {
    // Mirror the pre-SDK refresh-failure path: clear the stale token family so
    // it is not retried, and reset the UI auth state. Navigation stays with
    // the route loaders, exactly as before.
    client.tokens.clear()
    useAuthStore.getState().logout()
  }
  // 'authenticated' → hydration stays with initializeAuth / fetchAuthData (the
  //   login-success session is a placeholder until getStatus + profile run).
  // 'logged-out'    → logoutFlow owns the store reset + navigation.
}

/**
 * Ensure the SDK client exists for `realmId` (recreated on realm change; the
 * shared storage key keeps the refresh token across instances). The initial
 * `clientId` binding only applies at creation — flows that target a specific
 * product rebind explicitly via `tokens.bindClientId`.
 */
export function ensureHeraldClient(
  realmId: string,
  clientId: string = FIRST_PARTY_CLIENT_ID
): HeraldClient {
  if (!active || active.realmId !== realmId) {
    const client = createHeraldClient({
      baseUrl: baseUrlOverride ?? window.location.origin,
      realmId,
      clientId,
      storageKey: HERALD_REFRESH_TOKEN_STORAGE_KEY,
      onSessionChange: (event) => handleSessionEvent(event, client),
    })
    active = { realmId, client }
  }
  return active.client
}

/** The current SDK client, or null before any realm was initialized. */
export function getActiveHeraldClient(): HeraldClient | null {
  return active?.client ?? null
}

/** Access token for the generated-client request interceptor. */
export function getHeraldAccessToken(): string | null {
  return active?.client.tokens.getAccessToken() ?? null
}

/**
 * Inject a token set obtained outside the SDK (PKCE exchange, switch-client,
 * direct-issue responses) and remember the bound client for redirect routing.
 */
export function applyTokenSet(tokens: {
  accessToken: string
  refreshToken: string
  clientId?: string
}): void {
  const client = active?.client
  if (!client) {
    throw new Error('applyTokenSet called before the Herald client was initialized for a realm')
  }
  client.tokens.setTokens(tokens)
  if (tokens.clientId) {
    useAuthStore.getState().setRefreshClientId(tokens.clientId)
  }
}

/**
 * Rebind the SDK's request-body clientId and record the routing binding —
 * for flows whose tokens the SDK already applied itself (email-OTP verify).
 */
export function bindHeraldClientId(clientId: string): void {
  const client = active?.client
  if (!client) {
    throw new Error(
      'bindHeraldClientId called before the Herald client was initialized for a realm'
    )
  }
  client.tokens.bindClientId(clientId)
  useAuthStore.getState().setRefreshClientId(clientId)
}

/** Test hook: point the SDK client at the MSW-intercepted origin. */
export function setHeraldBaseUrlOverride(url: string | null): void {
  baseUrlOverride = url
  active = null
}
