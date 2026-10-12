/**
 * Bare-origin realm resolution honors the persisted last-visited realm.
 *
 * The realm for a URL without a realm prefix used to be a hardcoded 'admin'.
 * But the session store already persists the realm the user last visited —
 * `initializeAuth` writes it on every realm page entry (anonymous visits
 * included), and `logoutFlow` re-persists it after logout. These tests pin
 * the resolution contract:
 *   - the bare '/' reads that memory, so a returning user lands back on
 *     their realm's login page (and a live session restores against the
 *     right realm-scoped Herald client) instead of the admin default;
 *   - 'admin' remains the fallback for a fresh browser with no memory;
 *   - a realm in the URL always wins over the memory.
 */

import { describe, it, expect, beforeEach, vi } from 'vitest'
import { getLegacyRealmId, resolveRealmContext } from '@/lib/realm-routing'
import { useAuthStore } from '@/stores/auth-store'

// On the main domain the custom-domain probe deterministically 404s — that is
// the path the bare '/' takes through resolveRealmContext.
vi.mock('@/lib/api-generated', () => ({
  resolveCustomDomain: vi.fn(async () => ({ data: null, error: { status: 404 } })),
}))

beforeEach(() => {
  useAuthStore.getState().reset()
})

describe('bare-root / resolves to the persisted last-visited realm', () => {
  it('returns the remembered realm of a live session', () => {
    useAuthStore.getState().setAuthStatus(true, 'realm-042')

    expect(getLegacyRealmId('/')).toBe('realm-042')
  })

  it('returns the post-logout memory — the exact shape logoutFlow leaves behind (no session, only the realm)', () => {
    useAuthStore.getState().setAuthStatus(false, 'realm-001')

    expect(getLegacyRealmId('/')).toBe('realm-001')
  })

  it("falls back to the default 'admin' realm on a fresh browser with no memory", () => {
    expect(useAuthStore.getState().realmId).toBeNull()

    expect(getLegacyRealmId('/')).toBe('admin')
  })
})

describe('a realm in the URL always wins over the memory', () => {
  beforeEach(() => {
    useAuthStore.getState().setAuthStatus(true, 'realm-001')
  })

  it('realm-prefixed paths use the URL segment even when the store remembers another realm', () => {
    expect(getLegacyRealmId('/acme/auth/login')).toBe('acme')
  })

  it('session-scoped roots keep reading the session store', () => {
    expect(getLegacyRealmId('/user/profile')).toBe('realm-001')
  })

  it('prefix-less public roots still resolve to admin (legacy entry, deliberately unchanged)', () => {
    expect(getLegacyRealmId('/auth/login')).toBe('admin')
  })
})

describe('resolveRealmContext on a 404ing custom-domain probe (main domain)', () => {
  // resolveRealmContext caches the fallback context for the whole page
  // session (cachedCustomDomainContext), so exactly one test here may call
  // it — a second call would read the first test's cache.
  it('initializes the bare root with the remembered realm so the root loader runs the session restore against the right realm client', async () => {
    useAuthStore.getState().setAuthStatus(true, 'realm-001')

    const context = await resolveRealmContext('/')

    expect(context.realmId).toBe('realm-001')
    expect(context.isCustomDomain).toBe(false)
  })
})
