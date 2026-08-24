/**
 * Pluggable refresh-token storage (design §5.6 / DEC-js-sdk-006).
 *
 * The access token NEVER passes through `TokenStorage` — it lives only in the
 * in-memory holder (`session.ts`). The default implementation persists the
 * (rotating, reuse-detected) refresh token to `localStorage`, matching the
 * Herald own-frontend risk posture. Non-browser / SSR integrators must inject
 * an adapter or use `memoryStorage()`.
 */

import { HeraldError } from './errors'

export interface TokenStorage {
  getRefreshToken(): string | null
  setRefreshToken(token: string | null): void
}

/** In-memory storage: nothing survives a page reload. */
export function memoryStorage(): TokenStorage {
  let token: string | null = null
  return {
    getRefreshToken: () => token,
    setRefreshToken: (t) => {
      token = t
    },
  }
}

/**
 * `localStorage`-backed storage (browser default). Throws `HeraldError
 * { kind: 'ssr-no-storage' }` when `localStorage` is unavailable so SSR/Node
 * misuse fails fast instead of silently no-op'ing.
 */
export function localStorageStorage(key: string): TokenStorage {
  if (typeof localStorage === 'undefined' || localStorage === null) {
    throw new HeraldError({
      kind: 'ssr-no-storage',
      message:
        'localStorage is unavailable in this environment. Inject a TokenStorage adapter via `storage`, or use memoryStorage().',
    })
  }
  return {
    getRefreshToken: () => localStorage.getItem(key),
    setRefreshToken: (t) => {
      if (t === null) {
        localStorage.removeItem(key)
      } else {
        localStorage.setItem(key, t)
      }
    },
  }
}
