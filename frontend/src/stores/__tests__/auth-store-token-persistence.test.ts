/**
 * Auth store + Herald SDK token-model persistence / cleanup boundaries.
 *
 * Uses the REAL `useAuthStore` and the REAL Herald SDK client (no mocks) to
 * assert the post-SDK-migration persistence contract (DEC-js-sdk-013):
 *   - the token family (in-memory AT + persisted RT) lives in the Herald SDK
 *     client's storage (`herald.refreshToken`), NOT in the Zustand persist
 *   - the store persists UI/routing auth state (user, permissions,
 *     refreshClientId) and the PKCE state — never token material
 *   - `logout()` / `reset()` / `clearAuthStorage()` purge the store state
 *   - the storage keys stay stable (AUTH_STORAGE_KEY, SDK storage key)
 */

import { describe, it, expect, beforeEach } from 'vitest'
import { useAuthStore, clearAuthStorage, type PersistedPkceState } from '@/stores/auth-store'
import {
  ensureHeraldClient,
  getActiveHeraldClient,
  applyTokenSet,
  HERALD_REFRESH_TOKEN_STORAGE_KEY,
} from '@/lib/herald-client'
import { AUTH_STORAGE_KEY } from '@/lib/constants/auth-constants'
import { TOKEN_FIXTURE } from '@/test/fixtures/browser-token'

const TEST_REALM = 'realm-1'

/** Read the persisted snapshot written to localStorage by the persist middleware. */
function readPersistedSnapshot(): Record<string, unknown> {
  const raw = window.localStorage.getItem(AUTH_STORAGE_KEY)
  if (!raw) return {}
  try {
    const parsed = JSON.parse(raw)
    // Zustand persist wraps the state in `{ state, version }`.
    return parsed && parsed.state ? parsed.state : (parsed ?? {})
  } catch {
    return {}
  }
}

const SAMPLE_PKCE: PersistedPkceState = {
  codeVerifier: 'verifier-abc',
  clientId: TOKEN_FIXTURE.clientId,
  redirectUri: 'http://localhost/callback',
  state: 'state-xyz',
}

/** Seed a full session: token family into the SDK client, routing state into the store. */
function seedSession() {
  ensureHeraldClient(TEST_REALM)
  applyTokenSet({
    accessToken: TOKEN_FIXTURE.accessToken,
    refreshToken: TOKEN_FIXTURE.refreshToken,
    clientId: TOKEN_FIXTURE.clientId,
  })
  useAuthStore.getState().setPkceState(SAMPLE_PKCE)
  useAuthStore.getState().setAuthStatus(true, TEST_REALM)
}

beforeEach(() => {
  getActiveHeraldClient()?.tokens.clear()
  useAuthStore.getState().reset()
  window.localStorage.removeItem(AUTH_STORAGE_KEY)
  window.localStorage.removeItem(HERALD_REFRESH_TOKEN_STORAGE_KEY)
})

describe('token family lives in the Herald SDK client, not the store persist', () => {
  it('applyTokenSet stores AT in the SDK holder + RT in SDK storage, and only routing state in the store', () => {
    seedSession()

    // SDK owns the token family.
    expect(getActiveHeraldClient()?.tokens.getAccessToken()).toBe(TOKEN_FIXTURE.accessToken)
    expect(getActiveHeraldClient()?.storage.getRefreshToken()).toBe(TOKEN_FIXTURE.refreshToken)

    // The store keeps the bound product client for redirect routing…
    expect(useAuthStore.getState().refreshClientId).toBe(TOKEN_FIXTURE.clientId)
    // …but never token material.
    const snapshot = readPersistedSnapshot()
    expect(snapshot).not.toHaveProperty('refreshToken')
    expect(snapshot).not.toHaveProperty('accessToken')
    expect(snapshot.pkceState).toMatchObject(SAMPLE_PKCE)
  })

  it('the RT survives a simulated reload via the SDK storage key; no access token is persisted anywhere', () => {
    seedSession()

    // The SDK storage entry is exactly the opaque refresh token string.
    expect(window.localStorage.getItem(HERALD_REFRESH_TOKEN_STORAGE_KEY)).toBe(
      TOKEN_FIXTURE.refreshToken
    )
    // No access token under the SDK key (it is the raw RT string, not JSON).
    expect(window.localStorage.getItem(HERALD_REFRESH_TOKEN_STORAGE_KEY)).not.toContain(
      'accessToken'
    )
    // And none in the persisted store snapshot.
    const snapshot = readPersistedSnapshot()
    expect(snapshot).not.toHaveProperty('accessToken')
    expect(snapshot).not.toHaveProperty('access_token')
  })
})

describe('logout() / reset() / clearAuthStorage() purge the store state', () => {
  beforeEach(() => {
    seedSession()
  })

  it('logout() clears routing auth state + PKCE (token purge is the SDK bridge/logoutFlow job)', () => {
    useAuthStore.getState().logout()

    expect(useAuthStore.getState().refreshClientId).toBeNull()
    expect(useAuthStore.getState().pkceState).toBeNull()
    expect(useAuthStore.getState().isAuthenticated).toBe(false)
  })

  it('reset() clears routing auth state + PKCE', () => {
    useAuthStore.getState().reset()

    expect(useAuthStore.getState().refreshClientId).toBeNull()
    expect(useAuthStore.getState().pkceState).toBeNull()
  })

  it('clearAuthStorage() wipes the persisted localStorage snapshot (no routing/PKCE residue)', () => {
    clearAuthStorage()

    const snapshot = readPersistedSnapshot()
    expect(snapshot.refreshClientId).toBeUndefined()
    expect(snapshot.pkceState).toBeUndefined()
  })
})

describe('storage keys: stable shared constants (no undocumented keys)', () => {
  it('the persist middleware writes under AUTH_STORAGE_KEY', () => {
    seedSession()
    expect(window.localStorage.getItem(AUTH_STORAGE_KEY)).not.toBeNull()
    expect(AUTH_STORAGE_KEY).toBe('auth-storage')
  })

  it('the SDK persists the refresh token under its documented key', () => {
    seedSession()
    expect(HERALD_REFRESH_TOKEN_STORAGE_KEY).toBe('herald.refreshToken')
  })
})

describe('PKCE state round-trip (getPkceState reads what setPkceState wrote)', () => {
  it('persists and reads back the PKCE verifier + bound OAuth params', () => {
    useAuthStore.getState().setPkceState(SAMPLE_PKCE)
    expect(useAuthStore.getState().getPkceState()).toMatchObject(SAMPLE_PKCE)

    // Clearing returns null.
    useAuthStore.getState().setPkceState(null)
    expect(useAuthStore.getState().getPkceState()).toBeNull()
  })
})
