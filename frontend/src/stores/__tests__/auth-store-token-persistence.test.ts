/**
 * Auth store + Herald SDK token-model persistence / cleanup boundaries.
 *
 * Uses the REAL `useAuthStore` and the REAL Herald SDK client (no mocks) to
 * assert the post-SDK-migration persistence contract (DEC-js-sdk-013):
 *   - the token family (in-memory AT + persisted RT) lives in the Herald SDK
 *     client's per-realm storage slot (`herald.refreshToken:{realmId}`), NOT
 *     in the Zustand persist
 *   - authentication UI state stays in memory; only a realm-bound PKCE flow
 *     persists in this tab's sessionStorage
 *   - `logout()` / `reset()` / `clearAuthStorage()` purge the store state
 *   - the storage keys stay stable (AUTH_STORAGE_KEY, SDK storage keys)
 */

import { describe, it, expect, beforeEach } from 'vitest'
import { useAuthStore, clearAuthStorage, type PersistedPkceState } from '@/stores/auth-store'
import {
  ensureHeraldClient,
  getActiveHeraldClient,
  applyTokenSet,
  heraldRefreshTokenStorageKey,
  HERALD_REFRESH_TOKEN_STORAGE_PREFIX,
} from '@/lib/herald-client'
import { getLegacyRealmId } from '@/lib/realm-routing'
import { AUTH_STORAGE_KEY } from '@/lib/constants/auth-constants'
import { TOKEN_FIXTURE } from '@/test/fixtures/browser-token'

const TEST_REALM = 'realm-1'

/** Read the pending flow snapshot written to this tab's sessionStorage. */
function readPersistedSnapshot(): Record<string, unknown> {
  const raw = window.sessionStorage.getItem(AUTH_STORAGE_KEY)
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
  realmId: TEST_REALM,
  expiresAt: Date.now() + 600000,
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
  window.sessionStorage.removeItem(AUTH_STORAGE_KEY)
  window.localStorage.removeItem(heraldRefreshTokenStorageKey(TEST_REALM))
})

describe('token family lives in the Herald SDK client, not the store persist', () => {
  it('keeps credentials in the SDK and persists only a tab-local PKCE transaction', () => {
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
    expect(snapshot).not.toHaveProperty('realmId')
    expect(snapshot).not.toHaveProperty('user')
    expect(snapshot).not.toHaveProperty('permissions')
    expect(snapshot).not.toHaveProperty('isAuthenticated')
    expect(window.localStorage.getItem(AUTH_STORAGE_KEY)).toBeNull()
    expect(snapshot.pkceState).toMatchObject(SAMPLE_PKCE)
  })

  it('the RT survives a simulated reload via the SDK storage key; no access token is persisted anywhere', () => {
    seedSession()

    // The SDK storage entry is exactly the opaque refresh token string.
    expect(window.localStorage.getItem(heraldRefreshTokenStorageKey(TEST_REALM))).toBe(
      TOKEN_FIXTURE.refreshToken
    )
    // No access token under the SDK key (it is the raw RT string, not JSON).
    expect(window.localStorage.getItem(heraldRefreshTokenStorageKey(TEST_REALM))).not.toContain(
      'accessToken'
    )
    // And none in the persisted store snapshot.
    const snapshot = readPersistedSnapshot()
    expect(snapshot).not.toHaveProperty('accessToken')
    expect(snapshot).not.toHaveProperty('access_token')
  })
})

describe('per-realm refresh-token slots (one browser, coexisting realm sessions)', () => {
  // WHY: the OAuth consent flow can send a cross-realm user — e.g. someone
  // holding an admin-console session — to the flow realm's login. That login
  // must not evict the first realm's session, or authorizing a third-party
  // app would silently log the user out of the console they came from.
  const REALM_A = 'realm-a'
  const REALM_B = 'realm-b'

  function seedRealmSession(realmId: string, refreshToken: string) {
    ensureHeraldClient(realmId)
    applyTokenSet({
      accessToken: TOKEN_FIXTURE.accessToken,
      refreshToken,
      clientId: TOKEN_FIXTURE.clientId,
    })
  }

  it("a second realm's login writes its own slot and leaves the first realm's token intact", () => {
    seedRealmSession(REALM_A, 'rt-realm-a')
    seedRealmSession(REALM_B, 'rt-realm-b')

    expect(window.localStorage.getItem(heraldRefreshTokenStorageKey(REALM_A))).toBe('rt-realm-a')
    expect(window.localStorage.getItem(heraldRefreshTokenStorageKey(REALM_B))).toBe('rt-realm-b')
  })

  it("the active realm's token purge (logoutFlow's SDK clear) leaves the other realm's slot untouched", () => {
    seedRealmSession(REALM_A, 'rt-realm-a')
    seedRealmSession(REALM_B, 'rt-realm-b')

    // REALM_B's client is active; logoutFlow clears only through it.
    getActiveHeraldClient()?.tokens.clear()

    expect(window.localStorage.getItem(heraldRefreshTokenStorageKey(REALM_A))).toBe('rt-realm-a')
    expect(window.localStorage.getItem(heraldRefreshTokenStorageKey(REALM_B))).toBeNull()
  })

  it('drops the pre-per-realm shared key on client creation (no compat: the stale token is a live credential nothing can revoke via the UI)', () => {
    window.localStorage.setItem(HERALD_REFRESH_TOKEN_STORAGE_PREFIX, 'legacy-shared-rt')
    seedRealmSession('realm-c', 'rt-realm-c')

    expect(window.localStorage.getItem(HERALD_REFRESH_TOKEN_STORAGE_PREFIX)).toBeNull()
    expect(window.localStorage.getItem(heraldRefreshTokenStorageKey('realm-c'))).toBe('rt-realm-c')
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

  it('clearAuthStorage() wipes the pending flow snapshot and legacy shared authentication data', () => {
    clearAuthStorage()

    const snapshot = readPersistedSnapshot()
    expect(snapshot.refreshClientId).toBeUndefined()
    expect(snapshot.pkceState).toBeUndefined()
  })
})

describe('logout cannot change another tabs route identity', () => {
  it('the logoutFlow cleanup does not write a shared realm or authentication snapshot', () => {
    seedSession()

    // logoutFlow clears this realm's SDK token and this tab's pending flow.
    getActiveHeraldClient()?.tokens.clear()
    useAuthStore.getState().reset()
    clearAuthStorage()
    useAuthStore.getState().setAuthStatus(false, TEST_REALM)

    // Neither UI authentication nor a remembered realm is shared with tabs.
    const snapshot = readPersistedSnapshot()
    expect(snapshot.realmId).toBeUndefined()
    expect(snapshot.isAuthenticated).toBeUndefined()
    expect(snapshot.refreshClientId).toBeUndefined()
    expect(snapshot.pkceState).toBeNull()
    expect(snapshot.user).toBeUndefined()
    expect(getActiveHeraldClient()?.storage.getRefreshToken()).toBeNull()

    // Another tab's explicit URL still identifies its own realm.
    expect(getLegacyRealmId('/oncewise/manage/settings')).toBe('oncewise')
    expect(window.localStorage.getItem(AUTH_STORAGE_KEY)).toBeNull()
  })
})

describe('storage keys: stable shared constants (no undocumented keys)', () => {
  it('the persist middleware writes under AUTH_STORAGE_KEY', () => {
    seedSession()
    expect(window.sessionStorage.getItem(AUTH_STORAGE_KEY)).not.toBeNull()
    expect(AUTH_STORAGE_KEY).toBe('auth-storage')
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

  it('ignores persisted UI authentication state and restores only an unexpired realm-bound flow', async () => {
    useAuthStore.getState().reset()
    sessionStorage.setItem(
      AUTH_STORAGE_KEY,
      JSON.stringify({
        state: {
          isAuthenticated: true,
          realmId: 'admin',
          user: { id: 'admin-user' },
          permissions: ['realm.manage'],
          pkceState: SAMPLE_PKCE,
        },
        version: 0,
      })
    )
    await useAuthStore.persist.rehydrate()
    expect(useAuthStore.getState()).toMatchObject({
      isAuthenticated: false,
      realmId: null,
      user: null,
      permissions: [],
      pkceState: SAMPLE_PKCE,
    })
  })

  it.each([
    { ...SAMPLE_PKCE, expiresAt: Date.now() - 1000 },
    { ...SAMPLE_PKCE, realmId: undefined },
  ])('does not restore an expired or legacy realm-less PKCE flow', async (pkceState) => {
    sessionStorage.setItem(AUTH_STORAGE_KEY, JSON.stringify({ state: { pkceState }, version: 0 }))
    await useAuthStore.persist.rehydrate()
    expect(useAuthStore.getState().pkceState).toBeNull()
  })
})
