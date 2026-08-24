/**
 * In-memory access-token holder + session state + events.
 *
 * Mirrors the Herald own-frontend pattern (`frontend/src/stores/auth-store.ts`
 * `accessTokenHolder` + persist), with all framework (Zustand/React) coupling
 * removed.
 */

import type { HeraldSession, SessionEvent } from './types'

/**
 * Non-persisted, module-instance holder for the access token. A page reload
 * clears it; the transport restores it via a silent refresh on the next 401.
 */
export interface AccessTokenHolder {
  get(): string | null
  set(token: string | null): void
  clear(): void
}

export function createAccessTokenHolder(): AccessTokenHolder {
  let token: string | null = null
  return {
    get: () => token,
    set: (t) => {
      token = t
    },
    clear: () => {
      token = null
    },
  }
}

export const UNAUTHENTICATED_SESSION: HeraldSession = {
  authenticated: false,
  realmId: null,
  userId: null,
  clientAppId: null,
  clientId: null,
  credentialClass: null,
  permissions: [],
  scopes: [],
}

export type SessionListener = (event: SessionEvent) => void

export interface SessionStore {
  getSession(): HeraldSession
  setSession(session: HeraldSession | null): void
  subscribe(listener: SessionListener): () => void
  emit(event: SessionEvent): void
}

/**
 * Create a session store. `onChange` is the optional callback supplied via
 * `createHeraldClient({ onSessionChange })`; per-instance listeners can also be
 * added with `subscribe`.
 */
export function createSessionStore(onChange?: (event: SessionEvent) => void): SessionStore {
  let session: HeraldSession = { ...UNAUTHENTICATED_SESSION }
  const listeners = new Set<SessionListener>()

  return {
    getSession: () => session,
    setSession: (s) => {
      session = s ?? { ...UNAUTHENTICATED_SESSION }
    },
    subscribe: (fn) => {
      listeners.add(fn)
      return () => {
        listeners.delete(fn)
      }
    },
    emit: (event) => {
      if (event.type === 'authenticated') {
        session = event.session
      } else if (event.type === 'session-expired' || event.type === 'logged-out') {
        session = { ...UNAUTHENTICATED_SESSION }
      }
      for (const fn of listeners) {
        fn(event)
      }
      onChange?.(event)
    },
  }
}
