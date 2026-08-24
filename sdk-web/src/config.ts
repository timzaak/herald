/**
 * Client factory (design §5.1 / §4.4.1 / US-JS-001).
 *
 * `createHeraldClient` wires the in-memory access-token holder, the pluggable
 * refresh-token storage, the session store, the transport interceptors, and the
 * auth-orchestration methods into a single client object.
 */

import { createAuth } from './auth'
import { HeraldError } from './errors'
import { createAccessTokenHolder, createSessionStore } from './session'
import { createTransport } from './transport'
import { localStorageStorage } from './storage'
import type { TokenStorage } from './storage'
import type { HeraldSession, SessionEvent } from './types'

export interface HeraldClientConfig {
  /** Herald API origin (e.g. `https://auth.example.com`). */
  baseUrl: string
  /** Realm the integration belongs to. */
  realmId: string
  /** Client App identifier; injected into request bodies. */
  clientId: string
  /** Refresh-token storage. Defaults to `localStorage`; inject in SSR. */
  storage?: TokenStorage
  /** `localStorage` key for the refresh token (default `herald.refreshToken`). */
  storageKey?: string
  /** Session lifecycle callback (`authenticated` / `session-expired` / `logged-out`). */
  onSessionChange?: (event: SessionEvent) => void
}

export type HeraldClient = ReturnType<typeof createAuth> & {
  /** The resolved refresh-token storage. */
  readonly storage: TokenStorage
  /** Current session snapshot + per-instance event subscription. */
  readonly session: {
    getSession(): HeraldSession
    subscribe(listener: (event: SessionEvent) => void): () => void
  }
  /** First-party token bridge: inspect / inject the token family from the host app. */
  readonly tokens: TokenBridge
}

/** Token set obtained outside the SDK (PKCE exchange, switch-client, direct-issue responses). */
export interface SetTokensPayload {
  accessToken: string
  refreshToken: string
  /**
   * When provided, rebinds the client's request-body `clientId` (e.g. after a
   * first-party PKCE exchange or a switch-client performed by the host app).
   */
  clientId?: string
}

export interface TokenBridge {
  /** Current in-memory access token (null after a reload, until refreshed). */
  getAccessToken(): string | null
  /**
   * Inject a token set obtained outside the SDK. Pure state update — no session
   * event is emitted; call `getStatus()` afterwards to hydrate the session.
   */
  setTokens(tokens: SetTokensPayload): void
  /** Clear the access + refresh tokens without calling the server or emitting events. */
  clear(): void
  /**
   * Rebind the request-body `clientId` without touching tokens. First-party
   * hosts pick between built-in products (e.g. console vs account center) per
   * flow, before any tokens exist.
   */
  bindClientId(clientId: string): void
}

/**
 * Create a Herald browser client. Each client owns its own generated HTTP
 * client instance, so multiple clients are fully isolated.
 *
 * @throws {HeraldError} `kind: 'ssr-no-storage'` when no `storage` is injected
 *   and `localStorage` is unavailable (SSR / Node).
 */
export function createHeraldClient(config: HeraldClientConfig): HeraldClient {
  const hasLocalStorage = typeof localStorage !== 'undefined' && localStorage !== null

  let storage = config.storage
  if (!storage) {
    if (!hasLocalStorage) {
      throw new HeraldError({
        kind: 'ssr-no-storage',
        message:
          'No TokenStorage adapter was provided and localStorage is unavailable (SSR/Node). Inject a `storage` adapter or use memoryStorage().',
      })
    }
    storage = localStorageStorage(config.storageKey ?? 'herald.refreshToken')
  }

  const accessTokenHolder = createAccessTokenHolder()
  const session = createSessionStore(config.onSessionChange)

  const transport = createTransport({
    baseUrl: config.baseUrl,
    accessTokenHolder,
    storage,
    session,
  })

  // Mutable on purpose: `tokens.setTokens({ clientId })` rebinds the client's
  // request-body clientId after a host-app switch-client / PKCE exchange.
  const authDeps = {
    realmId: config.realmId,
    clientId: config.clientId,
    client: transport.client,
    accessTokenHolder,
    storage,
    session,
    refreshTokens: transport.refreshTokens,
  }

  const auth = createAuth(authDeps)

  return {
    ...auth,
    storage,
    session: {
      getSession: () => session.getSession(),
      subscribe: (listener) => session.subscribe(listener),
    },
    tokens: {
      getAccessToken: () => accessTokenHolder.get(),
      setTokens: (tokens) => {
        accessTokenHolder.set(tokens.accessToken)
        storage.setRefreshToken(tokens.refreshToken)
        if (tokens.clientId !== undefined) {
          authDeps.clientId = tokens.clientId
        }
      },
      clear: () => {
        accessTokenHolder.clear()
        storage.setRefreshToken(null)
      },
      bindClientId: (clientId) => {
        authDeps.clientId = clientId
      },
    },
  }
}
