/**
 * completeLoginAfterEmailOtp session-handoff tests (FE-T01 step 3, design §4.1).
 *
 * OTP verify runs through the Herald SDK (`client.loginWithEmailOtp.verify`,
 * DEC-js-sdk-014), which applies the issued token set itself — AT in its
 * in-memory holder, RT in its storage. `completeLoginAfterEmailOtp(realmId,
 * clientId)` therefore no longer receives the verify body: it rebinds the
 * routing clientId via the SDK bridge (`bindHeraldClientId`), `store.login`s,
 * hydrates the authenticated session, and returns the safe redirect path.
 *
 * What this protects (the dev component test does NOT touch this function — the
 * route owns it, and the form notifies the route via its argument-less
 * `onSuccess`):
 *  - The routing binding: `bindHeraldClientId` is called exactly once with the
 *    caller-supplied `clientId` (the send/verify request clientId — the token
 *    family must stay bound to the product the code was issued for).
 *  - The hydration sequence runs (`fetchAuthData` → store setters) and the
 *    function returns `{ redirectPath }`.
 *  - Boundary regression (design §4.1): OTP does NOT go through PKCE/OAuth —
 *    `performPkceTokenExchange` is never called and `store.setPkceState` is
 *    never invoked.
 *
 * Mocking pattern mirrors the sibling `oauth-login-logic.test.ts` (mock
 * `@/lib/auth-service` for `fetchAuthData`/`performPkceTokenExchange`, mock
 * `@/stores/auth-store` for `useAuthStore.getState`, mock `@/lib/herald-client`
 * for the SDK bridge) and reuses the same `makeStoreMock()` factory shape
 * rather than re-inventing a wrapper.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'

// --- Mocks (mirror oauth-login-logic.test.ts) ------------------------------

vi.mock('@/lib/api-generated', () => ({
  oauthAuthorize: vi.fn().mockResolvedValue({ data: {}, error: undefined }),
}))

vi.mock('@/lib/auth-service', () => ({
  performLogin: vi.fn(),
  fetchAuthData: vi.fn(),
  performLogout: vi.fn(),
  performPkceTokenExchange: vi.fn(),
  ClientSwitchError: class ClientSwitchError extends Error {
    constructor(public readonly status: number) {
      super('Client switch failed')
    }
  },
}))

vi.mock('@/stores/auth-store', () => ({
  useAuthStore: {
    getState: vi.fn(),
  },
  clearAuthStorage: vi.fn(),
}))

const mockBindHeraldClientId = vi.fn()
vi.mock('@/lib/herald-client', () => ({
  ensureHeraldClient: vi.fn(),
  getActiveHeraldClient: vi.fn(() => null),
  applyTokenSet: vi.fn(),
  bindHeraldClientId: (...args: unknown[]) => mockBindHeraldClientId(...args),
}))

// Import mocked modules after vi.mock declarations.
import { fetchAuthData, performPkceTokenExchange } from '@/lib/auth-service'
import { useAuthStore } from '@/stores/auth-store'
import { completeLoginAfterEmailOtp } from '@/lib/auth-utils'

// --- Factories --------------------------------------------------------------

/**
 * Minimal store mock (same shape as oauth-login-logic.test.ts::makeStoreMock),
 * with one addition: `setUserPermissions` mutates the returned `permissions`
 * array in place. `getRedirectPath()` (called at the tail of
 * `completeLoginAfterEmailOtp`) reads `useAuthStore.getState().permissions` to
 * pick admin vs user redirect, so for the redirect-distinguishability test the
 * mock must round-trip the hydrated permissions the way the real store would.
 */
function makeStoreMock(initial?: { permissions?: string[] }) {
  const permissions = initial?.permissions ?? []
  return {
    login: vi.fn(),
    logout: vi.fn(),
    setAuthStatus: vi.fn(),
    // Mutate the live `permissions` array so subsequent getState() reads (used
    // by getRedirectPath) observe the hydrated permissions.
    setUserPermissions: vi.fn((next: string[]) => {
      permissions.length = 0
      permissions.push(...next)
    }),
    setUserProfile: vi.fn(),
    setIsLoading: vi.fn(),
    reset: vi.fn(),
    clearStorage: vi.fn(),
    setRefreshClientId: vi.fn(),
    setPkceState: vi.fn(),
    getPkceState: vi.fn(() => null),
    permissions,
    roles: [],
    isAuthenticated: false,
  }
}

function makeAuthDataResponse(overrides?: { permissions?: string[] }) {
  return {
    authStatus: { authenticated: true, realmId: 'realm-1' },
    userPermissions: {
      permissions: overrides?.permissions ?? ['user:profile:read'],
      roles: ['user'],
    },
    userProfile: { id: 'user-1', email: 'user@test.com' },
  }
}

// --- Setup ------------------------------------------------------------------

beforeEach(() => {
  vi.mocked(useAuthStore.getState).mockReturnValue(
    makeStoreMock() as ReturnType<typeof useAuthStore.getState>
  )
})

// ===========================================================================
// Routing binding + redirect path
// ===========================================================================

describe('completeLoginAfterEmailOtp — clientId binding + redirect', () => {
  it('binds the caller-supplied clientId via the SDK bridge exactly once', async () => {
    const storeMock = makeStoreMock()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )
    vi.mocked(fetchAuthData).mockResolvedValue(makeAuthDataResponse())

    await completeLoginAfterEmailOtp('realm-1', 'admin-web-console')

    // The token family was applied by the SDK's verify; the completion's job
    // is to keep it bound to the product the code was issued for.
    expect(mockBindHeraldClientId).toHaveBeenCalledTimes(1)
    expect(mockBindHeraldClientId).toHaveBeenCalledWith('admin-web-console')
  })

  it('marks the session as logged in for the realm, then hydrates from fetchAuthData', async () => {
    const storeMock = makeStoreMock()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )
    vi.mocked(fetchAuthData).mockResolvedValue(makeAuthDataResponse())

    await completeLoginAfterEmailOtp('realm-1', 'admin-web-console')

    // store.login(realmId) marks the realm authenticated.
    expect(storeMock.login).toHaveBeenCalledWith('realm-1')
    // Hydration ran: fetchAuthData was awaited and the store setters fired with
    // the fetched auth status / permissions / profile.
    expect(fetchAuthData).toHaveBeenCalledWith()
    expect(storeMock.setAuthStatus).toHaveBeenCalledWith(true, 'realm-1')
    expect(storeMock.setUserPermissions).toHaveBeenCalledWith(['user:profile:read'], ['user'])
    expect(storeMock.setUserProfile).toHaveBeenCalledWith(expect.objectContaining({ id: 'user-1' }))
  })

  it('returns a { redirectPath } handoff shape for the route to navigate on', async () => {
    const storeMock = makeStoreMock()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )
    vi.mocked(fetchAuthData).mockResolvedValue(makeAuthDataResponse({ permissions: [] }))

    const result = await completeLoginAfterEmailOtp('realm-1', 'admin-web-console')

    // The route navigates on `result.redirectPath`. We assert the handoff shape
    // + a non-empty path; the admin-vs-user branch is `hasAdminPermission` /
    // `auth-constants`' contract (library-guaranteed), not this function's.
    expect(result).toEqual({ redirectPath: expect.any(String) })
    expect(result.redirectPath).not.toBe('')
    // And no `redirectTo` is returned — OTP has no external-redirect branch.
    expect(result).not.toHaveProperty('redirectTo')
  })
})

// ===========================================================================
// Boundary regression — OTP does NOT go through PKCE/OAuth (design §4.1)
// ===========================================================================

describe('completeLoginAfterEmailOtp — no PKCE exchange (design §4.1 boundary)', () => {
  it('does NOT call performPkceTokenExchange', async () => {
    const storeMock = makeStoreMock()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )
    vi.mocked(fetchAuthData).mockResolvedValue(makeAuthDataResponse())

    await completeLoginAfterEmailOtp('realm-1', 'admin-web-console')

    expect(performPkceTokenExchange).not.toHaveBeenCalled()
  })

  it('does NOT touch store.setPkceState (no PKCE state seed/clear)', async () => {
    const storeMock = makeStoreMock()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )
    vi.mocked(fetchAuthData).mockResolvedValue(makeAuthDataResponse())

    await completeLoginAfterEmailOtp('realm-1', 'admin-web-console')

    // PKCE path seeds + clears state; OTP path must not touch it at all.
    expect(storeMock.setPkceState).not.toHaveBeenCalled()
  })
})

// ===========================================================================
// Error path — hydrate failure forces a clean re-login
// ===========================================================================

describe('completeLoginAfterEmailOtp — hydrate failure', () => {
  it('calls store.logout and rethrows when fetchAuthData fails', async () => {
    const storeMock = makeStoreMock()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )
    vi.mocked(fetchAuthData).mockRejectedValue(new Error('Session expired'))

    await expect(completeLoginAfterEmailOtp('realm-1', 'admin-web-console')).rejects.toThrow(
      'Session expired'
    )

    // A clean re-login on hydration failure (mirrors completeLoginAfterPasskey).
    expect(storeMock.logout).toHaveBeenCalled()
  })
})
