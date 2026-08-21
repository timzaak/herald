/**
 * Tests for OAuth-related login logic (Herald FirstParty PKCE):
 * - loginSearchSchema parsing (backward compatibility + OAuth fields)
 * - loginFlow redirectTo branching
 * - completeLoginAfterTotp redirectTo branching
 * - validateOAuthParams completeness check
 * - loginFlow passes OAuth fields to performLogin
 * - PKCE S256 code_verifier/code_challenge generation (pure functions)
 * - loginFlow Herald FirstParty PKCE exchange (token family in the Herald SDK)
 * - 2FA detour carrying pending PKCE state (TOTP / Passkey completion)
 * - error-code distinguishability across the login/PKCE paths
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { loginSearchSchema } from '@/lib/schemas/search-params'
import type {
  LoginResponse,
  VerifyTotpResponse,
  LoginRequestPayload,
  PasskeyVerifyResponse,
} from '@/lib/api-generated'

// --- Mocks ---

// The PKCE bootstrap calls `oauthAuthorize` to seed backend state; stub it so
// the unit tests do not hit the network. `beginFirstPartyPkceFlow` only runs
// when the caller did not already supply an `oauthClientId`, so most tests
// pass one explicitly to exercise the non-PKCE / explicit-OAuth branches.
vi.mock('@/lib/api-generated', () => ({
  oauthAuthorize: vi.fn().mockResolvedValue({ data: {}, error: undefined }),
}))

vi.mock('@/lib/auth-service', () => ({
  performLogin: vi.fn(),
  fetchAuthData: vi.fn(),
  performLogout: vi.fn(),
  performPkceTokenExchange: vi.fn(),
  switchFirstPartyClient: vi.fn(),
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

// The Herald SDK client owns the token family (DEC-js-sdk-013). The hoisted
// fake carries the mutable token state the flows read/write; `applyTokenSet`
// (the bridge the flows call) is a vi.fn so call assertions work, and it
// mirrors the real bridge by writing through to the fake's state.
const heraldMock = vi.hoisted(() => {
  const state = {
    accessToken: null as string | null,
    refreshToken: null as string | null,
    refreshError: null as Error | null,
  }
  return {
    state,
    resetState(initial?: {
      accessToken?: string | null
      refreshToken?: string | null
      refreshError?: Error | null
    }) {
      state.accessToken = initial?.accessToken ?? null
      state.refreshToken = initial?.refreshToken ?? null
      state.refreshError = initial?.refreshError ?? null
    },
    storage: {
      getRefreshToken: () => state.refreshToken,
    },
    tokens: {
      getAccessToken: () => state.accessToken,
      clear: () => {
        state.accessToken = null
        state.refreshToken = null
      },
    },
    refresh: () => (state.refreshError ? Promise.reject(state.refreshError) : Promise.resolve({})),
    logout: () => Promise.resolve({ message: 'Logged out' }),
  }
})

vi.mock('@/lib/herald-client', () => ({
  ensureHeraldClient: () => heraldMock,
  getActiveHeraldClient: () => heraldMock,
  applyTokenSet: vi.fn((tokens: { accessToken: string; refreshToken: string }) => {
    heraldMock.state.accessToken = tokens.accessToken
    heraldMock.state.refreshToken = tokens.refreshToken
  }),
}))

// Import mocked modules after vi.mock declarations
import {
  performLogin,
  fetchAuthData,
  performPkceTokenExchange,
  switchFirstPartyClient,
} from '@/lib/auth-service'
import { useAuthStore } from '@/stores/auth-store'
import { applyTokenSet } from '@/lib/herald-client'
import {
  initializeAuth,
  loginFlow,
  completeLoginAfterTotp,
  validateOAuthParams,
} from '@/lib/auth-utils'

// --- Factories ---

function makeLoginResponse(overrides?: Partial<LoginResponse>): LoginResponse {
  return {
    expiresInSeconds: 3600,
    message: 'Login successful',
    realmId: 'realm1',
    userId: 'user-1',
    requiresTotp: false,
    ...overrides,
  }
}

function makeVerifyTotpResponse(overrides?: Partial<VerifyTotpResponse>): VerifyTotpResponse {
  return {
    expiresInSeconds: 3600,
    message: 'TOTP verified',
    token: 'tok-1',
    userId: 'user-1',
    ...overrides,
  }
}

function makeStoreMock() {
  return {
    login: vi.fn(),
    logout: vi.fn(),
    setAuthStatus: vi.fn(),
    setUserPermissions: vi.fn(),
    setUserProfile: vi.fn(),
    setIsLoading: vi.fn(),
    reset: vi.fn(),
    clearStorage: vi.fn(),
    setRefreshClientId: vi.fn(),
    setPkceState: vi.fn(),
    getPkceState: vi.fn(() => null),
    permissions: [],
    roles: [],
    isAuthenticated: false,
  }
}

/**
 * A store mock whose PKCE actions round-trip through in-memory holders,
 * mirroring the real store contract (`setPkceState`→`getPkceState`). Needed
 * for tests that exercise the full `beginFirstPartyPkceFlow` →
 * `tryCompletePkceExchange` round-trip inside a single `loginFlow` call, where
 * the seed writes PKCE state and the later exchange must read it back. Token
 * material round-trips through the herald mock instead (see `applyTokenSet`).
 */
function makeStatefulStoreMock(initial?: {
  pkceState?: ReturnType<NonNullable<ReturnType<typeof makeStoreMock>['getPkceState']>>
}) {
  let pkceState = initial?.pkceState ?? null
  return {
    login: vi.fn(),
    logout: vi.fn(() => {
      pkceState = null
    }),
    setAuthStatus: vi.fn(),
    setUserPermissions: vi.fn(),
    setUserProfile: vi.fn(),
    setIsLoading: vi.fn(),
    reset: vi.fn(() => {
      pkceState = null
    }),
    clearStorage: vi.fn(),
    setRefreshClientId: vi.fn(),
    setPkceState: vi.fn((state: unknown) => {
      pkceState = state as (typeof initial)['pkceState']
    }),
    getPkceState: vi.fn(() => pkceState),
    permissions: [],
    roles: [],
    isAuthenticated: false,
  }
}

function makeAuthDataResponse(overrides?: { permissions?: string[] }) {
  return {
    authStatus: { authenticated: true, realmId: 'realm1' },
    userPermissions: {
      permissions: overrides?.permissions ?? ['user:profile:read'],
      roles: ['user'],
    },
    userProfile: { id: 'user-1', email: 'user@test.com' },
  }
}

function baseCredentials(): LoginRequestPayload {
  return {
    clientId: 'client-1',
    // Pass an explicit oauthClientId so `loginFlow` does NOT auto-start the
    // Herald FirstParty PKCE bootstrap (which would call `oauthAuthorize` +
    // mutate PKCE state). This keeps these branching tests focused on the
    // redirectTo / consent logic; the PKCE exchange path has its own suite.
    oauthClientId: 'oauth-client-1',
    username: 'user@test.com',
    password: 'password123',
  }
}

// --- Setup ---

beforeEach(() => {
  heraldMock.resetState()
  vi.mocked(applyTokenSet).mockClear()
  vi.mocked(useAuthStore.getState).mockReturnValue(
    makeStoreMock() as ReturnType<typeof useAuthStore.getState>
  )
})

// --- Tests ---

describe('loginSearchSchema', () => {
  it('parses empty {} — backward compatible', () => {
    const result = loginSearchSchema.parse({})
    expect(result).toEqual({
      redirect: undefined,
      clientId: undefined,
      oauthClientId: undefined,
      redirectUri: undefined,
      state: undefined,
    })
  })

  it('parses { redirect: "/manage" } — existing field works', () => {
    const result = loginSearchSchema.parse({ redirect: '/manage' })
    expect(result.redirect).toBe('/manage')
    expect(result.oauthClientId).toBeUndefined()
  })

  it('parses all 3 OAuth fields together', () => {
    const result = loginSearchSchema.parse({
      oauthClientId: 'my-client',
      redirectUri: 'https://app.example.com/callback',
      state: 'abc123',
    })
    expect(result.oauthClientId).toBe('my-client')
    expect(result.redirectUri).toBe('https://app.example.com/callback')
    expect(result.state).toBe('abc123')
  })

  it('parses redirect + all 3 OAuth fields together', () => {
    const result = loginSearchSchema.parse({
      redirect: '/manage',
      oauthClientId: 'my-client',
      redirectUri: 'https://app.example.com/callback',
      state: 'xyz789',
    })
    expect(result.redirect).toBe('/manage')
    expect(result.oauthClientId).toBe('my-client')
    expect(result.redirectUri).toBe('https://app.example.com/callback')
    expect(result.state).toBe('xyz789')
  })

  it('treats all fields as optional — missing values are undefined', () => {
    const result = loginSearchSchema.parse({ oauthClientId: 'only-one' })
    expect(result.oauthClientId).toBe('only-one')
    expect(result.redirectUri).toBeUndefined()
    expect(result.state).toBeUndefined()
    expect(result.redirect).toBeUndefined()
    expect(result.clientId).toBeUndefined()
  })
})

describe('loginFlow redirectTo logic', () => {
  it('when redirectTo present and no TOTP, returns redirectTo without calling store.login or fetchAuthData', async () => {
    const storeMock = makeStoreMock()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )

    vi.mocked(performLogin).mockResolvedValue(
      makeLoginResponse({ redirectTo: 'https://app.example.com/callback?code=abc' })
    )

    const result = await loginFlow('realm1', baseCredentials())

    expect(result.redirectTo).toBeUndefined()
    expect(result.response.redirectTo).toBe('https://app.example.com/callback?code=abc')
    expect(result.redirectPath).toBe('/user/profile')
    expect(storeMock.login).not.toHaveBeenCalled()
    expect(fetchAuthData).not.toHaveBeenCalled()
  })

  it('when redirectTo present and TOTP required, preserves redirectTo in response', async () => {
    vi.mocked(performLogin).mockResolvedValue(
      makeLoginResponse({
        requiresTotp: true,
        tempToken: 'temp-tok',
        redirectTo: 'https://app.example.com/callback?code=abc',
      })
    )

    const result = await loginFlow('realm1', baseCredentials())

    expect(result.response.redirectTo).toBe('https://app.example.com/callback?code=abc')
    expect(result.response.requiresTotp).toBe(true)
  })

  it('when redirectTo absent, runs existing logic with store.login and fetchAuthData', async () => {
    const storeMock = makeStoreMock()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )

    vi.mocked(performLogin).mockResolvedValue(makeLoginResponse({ redirectTo: undefined }))
    vi.mocked(fetchAuthData).mockResolvedValue(makeAuthDataResponse())

    const result = await loginFlow('realm1', baseCredentials())

    expect(result.redirectTo).toBeUndefined()
    expect(result.response.redirectTo).toBeUndefined()
    expect(storeMock.login).toHaveBeenCalledWith('realm1')
    expect(fetchAuthData).toHaveBeenCalledWith()
    expect(result.redirectPath).toBe('/user/profile')
  })

  it('when redirectTo is null, same as absent — runs existing logic', async () => {
    const storeMock = makeStoreMock()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )

    vi.mocked(performLogin).mockResolvedValue(makeLoginResponse({ redirectTo: null }))
    vi.mocked(fetchAuthData).mockResolvedValue(makeAuthDataResponse())

    const result = await loginFlow('realm1', baseCredentials())

    expect(result.redirectTo).toBeUndefined()
    expect(result.response.redirectTo).toBeNull()
    expect(storeMock.login).toHaveBeenCalledWith('realm1')
    expect(fetchAuthData).toHaveBeenCalledWith()
  })

  it('calls store.logout when performLogin throws', async () => {
    const storeMock = makeStoreMock()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )

    vi.mocked(performLogin).mockRejectedValue(new Error('Network error'))

    await expect(loginFlow('realm1', baseCredentials())).rejects.toThrow('Network error')
    expect(storeMock.logout).toHaveBeenCalled()
  })
})

describe('completeLoginAfterTotp redirectTo logic', () => {
  it('when redirectTo present, returns it without calling fetchAuthData', async () => {
    const storeMock = makeStoreMock()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )

    const verifyResponse = makeVerifyTotpResponse({
      redirectTo: 'https://app.example.com/callback?code=abc',
    })

    const result = await completeLoginAfterTotp('realm1', verifyResponse)

    expect(result).toEqual({
      redirectPath: undefined,
      redirectTo: 'https://app.example.com/callback?code=abc',
    })
    expect(fetchAuthData).not.toHaveBeenCalled()
  })

  it('when redirectTo absent, fetches auth data and returns redirectPath', async () => {
    const storeMock = makeStoreMock()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )

    vi.mocked(fetchAuthData).mockResolvedValue(makeAuthDataResponse())

    const verifyResponse = makeVerifyTotpResponse({ redirectTo: undefined })

    const result = await completeLoginAfterTotp('realm1', verifyResponse)

    expect(result.redirectPath).toBe('/user/profile')
    expect(result.redirectTo).toBeUndefined()
    expect(fetchAuthData).toHaveBeenCalledWith()
    expect(storeMock.setAuthStatus).toHaveBeenCalled()
    expect(storeMock.setUserPermissions).toHaveBeenCalled()
    expect(storeMock.setUserProfile).toHaveBeenCalled()
  })

  it('returns the admin dashboard when the authenticated user has an admin permission', async () => {
    const storeMock = makeStoreMock()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )
    vi.mocked(fetchAuthData).mockResolvedValue(
      makeAuthDataResponse({ permissions: ['dashboard.view'] })
    )

    const result = await completeLoginAfterTotp(
      'realm1',
      makeVerifyTotpResponse({ redirectTo: undefined })
    )

    expect(result.redirectPath).toBe('/manage')
  })

  it('when redirectTo is null, same as absent — runs existing logic', async () => {
    const storeMock = makeStoreMock()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )

    vi.mocked(fetchAuthData).mockResolvedValue(makeAuthDataResponse())

    const verifyResponse = makeVerifyTotpResponse({ redirectTo: null })

    const result = await completeLoginAfterTotp('realm1', verifyResponse)

    expect(result.redirectTo).toBeUndefined()
    expect(fetchAuthData).toHaveBeenCalledWith()
  })

  it('calls store.logout when fetchAuthData throws', async () => {
    const storeMock = makeStoreMock()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )

    vi.mocked(fetchAuthData).mockRejectedValue(new Error('Session expired'))

    const verifyResponse = makeVerifyTotpResponse({ redirectTo: undefined })

    await expect(completeLoginAfterTotp('realm1', verifyResponse)).rejects.toThrow(
      'Session expired'
    )
    expect(storeMock.logout).toHaveBeenCalled()
  })
})

describe('validateOAuthParams completeness', () => {
  it.each([
    {
      name: 'all 3 present — complete',
      params: { oauthClientId: 'c1', redirectUri: 'https://app.example.com/cb', state: 's1' },
      expectedOAuth: {
        oauthClientId: 'c1',
        redirectUri: 'https://app.example.com/cb',
        state: 's1',
      },
      expectedPartial: false,
    },
    {
      name: 'only oauthClientId — partial',
      params: { oauthClientId: 'c1' },
      expectedOAuth: null,
      expectedPartial: true,
    },
    {
      name: 'only redirectUri — partial',
      params: { redirectUri: 'https://app.example.com/cb' },
      expectedOAuth: null,
      expectedPartial: true,
    },
    {
      name: 'only state — partial',
      params: { state: 's1' },
      expectedOAuth: null,
      expectedPartial: true,
    },
    {
      name: 'oauthClientId + redirectUri — partial',
      params: { oauthClientId: 'c1', redirectUri: 'https://app.example.com/cb' },
      expectedOAuth: null,
      expectedPartial: true,
    },
    {
      name: 'oauthClientId + state — partial',
      params: { oauthClientId: 'c1', state: 's1' },
      expectedOAuth: null,
      expectedPartial: true,
    },
    {
      name: 'redirectUri + state — partial',
      params: { redirectUri: 'https://app.example.com/cb', state: 's1' },
      expectedOAuth: null,
      expectedPartial: true,
    },
    {
      name: 'none present — not partial',
      params: {},
      expectedOAuth: null,
      expectedPartial: false,
    },
  ] as const)('$name', ({ params, expectedOAuth, expectedPartial }) => {
    const { oauthParams, hasPartialOAuth } = validateOAuthParams(params)
    expect(oauthParams).toEqual(expectedOAuth)
    expect(hasPartialOAuth).toBe(expectedPartial)
  })
})

describe('loginFlow passes OAuth fields to performLogin', () => {
  it('forwards oauthClientId, redirectUri, state in the payload', async () => {
    vi.mocked(performLogin).mockResolvedValue(
      makeLoginResponse({ redirectTo: 'https://app.example.com/callback?code=abc' })
    )

    const creds: LoginRequestPayload = {
      ...baseCredentials(),
      oauthClientId: 'oauth-client-42',
      redirectUri: 'https://app.example.com/callback',
      state: 'csrf-state-token',
    }

    await loginFlow('realm1', creds)

    expect(performLogin).toHaveBeenCalledWith(
      'realm1',
      expect.objectContaining({
        clientId: 'client-1',
        oauthClientId: 'oauth-client-42',
        redirectUri: 'https://app.example.com/callback',
        state: 'csrf-state-token',
      })
    )
  })
})

describe('loginFlow Herald FirstParty PKCE exchange', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      makeStoreMock() as ReturnType<typeof useAuthStore.getState>
    )
  })

  it('completes the PKCE token exchange when redirectTo carries a code and PKCE state is active', async () => {
    const storeMock = makeStoreMock()
    // Active PKCE state in the store → `tryCompletePkceExchange` will run.
    storeMock.getPkceState = vi.fn(() => ({
      codeVerifier: 'verifier-xyz',
      clientId: 'admin-web-console',
      redirectUri: 'http://localhost/callback',
      state: 'state-abc',
    }))
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )

    vi.mocked(performLogin).mockResolvedValue(
      makeLoginResponse({
        // FirstParty callback URL with an authorization code.
        redirectTo: 'http://localhost/callback?code=ac_123&state=state-abc',
      })
    )
    vi.mocked(performPkceTokenExchange).mockResolvedValue({
      accessToken: 'at-new',
      refreshToken: 'rt-new',
      tokenType: 'Bearer',
      expiresIn: 900,
      refreshExpiresIn: 2592000,
    })
    vi.mocked(fetchAuthData).mockResolvedValue(
      makeAuthDataResponse({ permissions: ['dashboard.view'] })
    )

    // No explicit oauthClientId → loginFlow bootstraps Herald PKCE, but since
    // PKCE state is already "active" (getPkceState), it reuses it instead of
    // re-calling oauthAuthorize.
    const result = await loginFlow('realm1', {
      clientId: 'admin-web-console',
      username: 'admin@test.com',
      password: 'password123',
    })

    // The exchange was performed with the code from redirectTo + stored verifier.
    expect(performPkceTokenExchange).toHaveBeenCalledWith('realm1', {
      code: 'ac_123',
      codeVerifier: 'verifier-xyz',
      redirectUri: 'http://localhost/callback',
      clientId: 'admin-web-console',
    })
    // Token family injected into the Herald SDK client (AT in its holder, RT in
    // its storage).
    expect(applyTokenSet).toHaveBeenCalledWith({
      accessToken: 'at-new',
      refreshToken: 'rt-new',
      clientId: 'admin-web-console',
    })
    expect(heraldMock.state.accessToken).toBe('at-new')
    expect(heraldMock.state.refreshToken).toBe('rt-new')
    // PKCE state cleared after success.
    expect(storeMock.setPkceState).toHaveBeenCalledWith(null)
    // redirectTo is nulled in the returned response so the caller proceeds to
    // its post-login redirect logic instead of navigating to /callback.
    expect(result.response.redirectTo).toBeNull()
    expect(result.redirectPath).toBe('/manage')
  })

  it('preserves redirectTo when no PKCE state is active (external OAuth client)', async () => {
    const storeMock = makeStoreMock()
    // No PKCE state → exchange does not apply; redirectTo preserved.
    storeMock.getPkceState = vi.fn(() => null)
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )

    vi.mocked(performLogin).mockResolvedValue(
      makeLoginResponse({
        redirectTo: 'https://third-party.example.com/cb?code=abc',
      })
    )

    const result = await loginFlow('realm1', baseCredentials())

    expect(performPkceTokenExchange).not.toHaveBeenCalled()
    expect(result.response.redirectTo).toBe('https://third-party.example.com/cb?code=abc')
  })

  it('refuses the PKCE exchange when the returned state does not match (CSRF guard)', async () => {
    const storeMock = makeStoreMock()
    // Active PKCE state was seeded with `state-abc` by our authorize call.
    storeMock.getPkceState = vi.fn(() => ({
      codeVerifier: 'verifier-xyz',
      clientId: 'admin-web-console',
      redirectUri: 'http://localhost/callback',
      state: 'state-abc',
    }))
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )

    // The redirect carries a code but a DIFFERENT state — an attacker-injected
    // code that did not originate from our authorize call.
    vi.mocked(performLogin).mockResolvedValue(
      makeLoginResponse({
        redirectTo: 'http://localhost/callback?code=ac_attacker&state=state-evil',
      })
    )

    const result = await loginFlow('realm1', baseCredentials())

    // The exchange must NOT run with the mismatched code.
    expect(performPkceTokenExchange).not.toHaveBeenCalled()
    // No token material written.
    expect(applyTokenSet).not.toHaveBeenCalled()
    // PKCE state is dropped so it cannot be replayed.
    expect(storeMock.setPkceState).toHaveBeenCalledWith(null)
    // redirectTo preserved so the caller treats it as a non-PKCE redirect.
    expect(result.response.redirectTo).toBe(
      'http://localhost/callback?code=ac_attacker&state=state-evil'
    )
  })

  // After a successful PKCE token exchange, a transient failure in the subsequent
  // auth-data fetch (e.g. the freshly-issued access token 401-ing on its first
  // use before backend propagation completes) must NOT wipe the just-persisted
  // refresh token. If it does, a full-page navigation to a protected route
  // (e.g. /manage/...) cannot restore the session via `initializeAuth`'s
  // refresh-first restore and the user is wrongly bounced to login. The token
  // family was already established by the exchange — tearing it down belongs
  // only to the pre-exchange failure path (bad credentials, exchange failure).
  it('does NOT call store.logout when fetchAuthData throws AFTER the PKCE exchange succeeds', async () => {
    const storeMock = makeStatefulStoreMock({
      pkceState: {
        codeVerifier: 'verifier-xyz',
        clientId: 'admin-web-console',
        redirectUri: 'http://localhost/callback',
        state: 'state-abc',
      },
    })
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )

    vi.mocked(performLogin).mockResolvedValue(
      makeLoginResponse({
        redirectTo: 'http://localhost/callback?code=ac_123&state=state-abc',
      })
    )
    vi.mocked(performPkceTokenExchange).mockResolvedValue({
      accessToken: 'at-new',
      refreshToken: 'rt-new',
      tokenType: 'Bearer',
      expiresIn: 900,
      refreshExpiresIn: 2592000,
    })
    // The post-exchange auth-data fetch fails (the transient 401 surface).
    vi.mocked(fetchAuthData).mockRejectedValue(new Error('Request failed with status 401'))

    // loginFlow re-throws the error so the caller surfaces it...
    await expect(
      loginFlow('realm1', {
        clientId: 'admin-web-console',
        username: 'admin@test.com',
        password: 'password123',
      })
    ).rejects.toThrow('401')

    // ...but the Bearer token family established by the exchange MUST survive.
    // logout() would tear down the session; assert it was never called.
    expect(storeMock.logout).not.toHaveBeenCalled()
    // The token family injected by the successful exchange is still in the
    // Herald SDK client.
    expect(heraldMock.state.accessToken).toBe('at-new')
    expect(heraldMock.state.refreshToken).toBe('rt-new')
  })
})

describe('initializeAuth token-family preservation', () => {
  it('preserves an established refresh token when status initialization fails transiently', async () => {
    const storeMock = makeStatefulStoreMock()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )
    heraldMock.resetState({ accessToken: 'at-established', refreshToken: 'rt-established' })
    vi.mocked(fetchAuthData).mockRejectedValue(new Error('transient status failure'))

    const result = await initializeAuth('admin', 'admin-web-console', true)

    expect(result.authenticated).toBe(false)
    expect(storeMock.reset).not.toHaveBeenCalled()
    expect(storeMock.logout).not.toHaveBeenCalled()
    expect(storeMock.setAuthStatus).toHaveBeenCalledWith(false)
    // The established token family in the Herald SDK client survives the
    // transient failure.
    expect(heraldMock.state.refreshToken).toBe('rt-established')
    expect(heraldMock.state.accessToken).toBe('at-established')
  })

  it('preserves the replacement token family when post-switch hydration fails transiently', async () => {
    const storeMock = makeStatefulStoreMock()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )
    heraldMock.resetState({ accessToken: 'at-user', refreshToken: 'rt-user' })
    vi.mocked(fetchAuthData)
      .mockResolvedValueOnce({
        ...makeAuthDataResponse(),
        authStatus: {
          ...makeAuthDataResponse().authStatus,
          clientId: 'user-account-center',
        },
      })
      .mockRejectedValueOnce(new Error('transient post-switch status failure'))
    vi.mocked(switchFirstPartyClient).mockResolvedValue({
      accessToken: 'at-admin',
      refreshToken: 'rt-admin',
      tokenType: 'Bearer',
      expiresIn: 900,
      refreshExpiresIn: 2592000,
      clientId: 'admin-web-console',
    })

    const result = await initializeAuth('admin', 'admin-web-console', true)

    expect(result.authenticated).toBe(false)
    expect(storeMock.reset).not.toHaveBeenCalled()
    // The switch-client result replaced the token family via the SDK bridge.
    expect(heraldMock.state.refreshToken).toBe('rt-admin')
    expect(heraldMock.state.accessToken).toBe('at-admin')
    expect(applyTokenSet).toHaveBeenCalledWith(
      expect.objectContaining({
        accessToken: 'at-admin',
        refreshToken: 'rt-admin',
        clientId: 'admin-web-console',
      })
    )
  })

  it('clears the session when startup refresh is explicitly rejected', async () => {
    const storeMock = makeStatefulStoreMock()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )
    heraldMock.resetState({
      accessToken: null,
      refreshToken: 'rt-revoked',
      refreshError: new Error('refresh token revoked'),
    })

    const result = await initializeAuth('admin', 'admin-web-console', true)

    expect(result.authenticated).toBe(false)
    expect(storeMock.logout).toHaveBeenCalledOnce()
    expect(storeMock.reset).not.toHaveBeenCalled()
    // The stale token material is not restored: initializeAuth's failure path
    // does not re-issue tokens. (The token purge itself is the herald-client
    // session-expired bridge contract, asserted in the SDK bridge suites.)
  })
})

// ---------------------------------------------------------------------------
// PKCE S256 generation — pure-function correctness (RFC 7636).
// `pkce-utils` has no store / network deps, so these exercise the real Web
// Crypto helpers directly (jsdom polyfills `crypto.subtle`).
// ---------------------------------------------------------------------------

describe('PKCE S256 code_verifier / code_challenge generation', () => {
  it('generates a 64-char verifier from the RFC 7636 unreserved set', async () => {
    const { generateCodeVerifier } = await import('@/lib/pkce-utils')
    const verifier = generateCodeVerifier()
    expect(verifier).toHaveLength(64)
    // Unreserved chars only: ALPHA / DIGIT / "-" / "." / "_" / "~"
    expect(verifier).toMatch(/^[A-Za-z0-9\-._~]+$/)
  })

  it('produces high-entropy, unique verifiers', async () => {
    const { generateCodeVerifier } = await import('@/lib/pkce-utils')
    const samples = new Set<string>()
    for (let i = 0; i < 50; i++) samples.add(generateCodeVerifier())
    // 50 draws must all be distinct (randomness sanity check).
    expect(samples.size).toBe(50)
  })

  it('derives code_challenge = base64url(sha256(verifier)) — recomputable', async () => {
    const { generatePkcePair, computeCodeChallenge } = await import('@/lib/pkce-utils')
    const { codeVerifier, codeChallenge } = await generatePkcePair()

    // S256 challenge is a deterministic function of the verifier.
    const recomputed = await computeCodeChallenge(codeVerifier)
    expect(recomputed).toBe(codeChallenge)

    // base64url: no padding, no URL-unsafe chars.
    expect(codeChallenge).toMatch(/^[A-Za-z0-9\-_]+$/)
    expect(codeChallenge).not.toContain('=')
  })

  it('two different verifiers yield two different S256 challenges', async () => {
    const { generatePkcePair } = await import('@/lib/pkce-utils')
    const a = await generatePkcePair()
    const b = await generatePkcePair()
    expect(a.codeVerifier).not.toBe(b.codeVerifier)
    expect(a.codeChallenge).not.toBe(b.codeChallenge)
  })

  it('extracts the authorization code (and state) from a FirstParty redirectTo', async () => {
    const { extractAuthorizationCode } = await import('@/lib/pkce-utils')
    const parsed = extractAuthorizationCode('http://localhost/callback?code=ac_xyz789&state=st-1')
    expect(parsed).toEqual({ code: 'ac_xyz789', state: 'st-1' })
  })

  it('returns null from extractAuthorizationCode when there is no code', async () => {
    const { extractAuthorizationCode } = await import('@/lib/pkce-utils')
    expect(extractAuthorizationCode('http://localhost/callback?error=denied')).toBeNull()
  })
})

// ---------------------------------------------------------------------------
// loginFlow Herald PKCE: seeding + persistence of PKCE state via the store.
// ---------------------------------------------------------------------------

describe('loginFlow Herald FirstParty PKCE seeding', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      makeStoreMock() as ReturnType<typeof useAuthStore.getState>
    )
  })

  it('seeds OAuth state (verifier + S256 challenge + state token) when no PKCE state is active and no explicit oauthClientId is supplied', async () => {
    const { oauthAuthorize } = await import('@/lib/api-generated')
    // Stateful store: the seed writes PKCE state, the exchange reads it back.
    const storeMock = makeStatefulStoreMock({ pkceState: null })
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as unknown as ReturnType<typeof useAuthStore.getState>
    )

    // Login returns a redirectTo with a code → PKCE exchange path runs.
    vi.mocked(performLogin).mockResolvedValue(
      makeLoginResponse({ redirectTo: 'http://localhost/callback?code=ac_seed&state=st-seed' })
    )
    vi.mocked(performPkceTokenExchange).mockResolvedValue({
      accessToken: 'at-seed',
      refreshToken: 'rt-seed',
      tokenType: 'Bearer',
      expiresIn: 900,
      refreshExpiresIn: 2592000,
    })
    vi.mocked(fetchAuthData).mockResolvedValue(makeAuthDataResponse())

    await loginFlow('realm1', {
      clientId: 'admin-web-console',
      username: 'admin@test.com',
      password: 'password123',
    })

    // The authorize seed call was made with code_challenge_method=S256.
    expect(oauthAuthorize).toHaveBeenCalledWith(
      expect.objectContaining({
        path: { realmId: 'realm1' },
        query: expect.objectContaining({
          client_id: 'admin-web-console',
          response_type: 'code',
          code_challenge_method: 'S256',
        }),
      })
    )

    // The PKCE state (verifier + clientId + redirectUri + state) was persisted
    // to the store so the post-login exchange can retrieve it.
    expect(storeMock.setPkceState).toHaveBeenCalledWith(
      expect.objectContaining({
        codeVerifier: expect.any(String),
        clientId: 'admin-web-console',
        redirectUri: expect.stringContaining('/callback'),
        state: expect.any(String),
      })
    )
    // And the login payload carried the PKCE OAuth params through.
    expect(performLogin).toHaveBeenCalledWith(
      'realm1',
      expect.objectContaining({
        oauthClientId: 'admin-web-console',
        redirectUri: expect.stringContaining('/callback'),
        state: expect.any(String),
      })
    )
  })

  it('reuses the active PKCE state (does not re-call oauthAuthorize) when a verifier is already in flight', async () => {
    const { oauthAuthorize } = await import('@/lib/api-generated')
    const storeMock = makeStoreMock()
    storeMock.getPkceState = vi.fn(() => ({
      codeVerifier: 'existing-verifier',
      clientId: 'admin-web-console',
      redirectUri: 'http://localhost/callback',
      state: 'existing-state',
    }))
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )

    vi.mocked(performLogin).mockResolvedValue(
      makeLoginResponse({
        redirectTo: 'http://localhost/callback?code=ac_reuse&state=existing-state',
      })
    )
    vi.mocked(performPkceTokenExchange).mockResolvedValue({
      accessToken: 'at-reuse',
      refreshToken: 'rt-reuse',
      tokenType: 'Bearer',
      expiresIn: 900,
      refreshExpiresIn: 2592000,
    })
    vi.mocked(fetchAuthData).mockResolvedValue(makeAuthDataResponse())

    await loginFlow('realm1', {
      clientId: 'admin-web-console',
      username: 'admin@test.com',
      password: 'password123',
    })

    // Re-using the in-flight state means NO new authorize seed call.
    expect(oauthAuthorize).not.toHaveBeenCalled()
    // The EXCHANGE uses the existing verifier (not a freshly generated one).
    expect(performPkceTokenExchange).toHaveBeenCalledWith('realm1', {
      code: 'ac_reuse',
      codeVerifier: 'existing-verifier',
      redirectUri: 'http://localhost/callback',
      clientId: 'admin-web-console',
    })
  })
})

// ---------------------------------------------------------------------------
// 2FA detour carries pending PKCE state through to completion.
// ---------------------------------------------------------------------------

describe('2FA detour carries pending PKCE state', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      makeStoreMock() as ReturnType<typeof useAuthStore.getState>
    )
  })

  it('loginFlow: requiresTotp=true does NOT exchange early — pending PKCE state stays in the store', async () => {
    const storeMock = makeStoreMock()
    storeMock.getPkceState = vi.fn(() => ({
      codeVerifier: 'pending-verifier',
      clientId: 'admin-web-console',
      redirectUri: 'http://localhost/callback',
      state: 'pending-state',
    }))
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )

    vi.mocked(performLogin).mockResolvedValue(
      makeLoginResponse({ requiresTotp: true, tempToken: 'temp-tok' })
    )

    const result = await loginFlow('realm1', {
      clientId: 'admin-web-console',
      username: 'admin@test.com',
      password: 'password123',
    })

    expect(result.response.requiresTotp).toBe(true)
    // No token exchange, no token material written, no auth data fetched during the detour.
    expect(performPkceTokenExchange).not.toHaveBeenCalled()
    expect(applyTokenSet).not.toHaveBeenCalled()
    expect(fetchAuthData).not.toHaveBeenCalled()
    // PKCE state is NOT cleared — it must survive the 2FA step.
    expect(storeMock.setPkceState).not.toHaveBeenCalledWith(null)
  })

  it('completeLoginAfterTotp: after TOTP, the pending PKCE verifier completes the exchange', async () => {
    const storeMock = makeStoreMock()
    storeMock.getPkceState = vi.fn(() => ({
      codeVerifier: 'pending-verifier',
      clientId: 'admin-web-console',
      redirectUri: 'http://localhost/callback',
      state: 'pending-state',
    }))
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )

    vi.mocked(performPkceTokenExchange).mockResolvedValue({
      accessToken: 'at-after-totp',
      refreshToken: 'rt-after-totp',
      tokenType: 'Bearer',
      expiresIn: 900,
      refreshExpiresIn: 2592000,
    })
    vi.mocked(fetchAuthData).mockResolvedValue(makeAuthDataResponse())

    const verifyResponse = makeVerifyTotpResponse({
      token: 'post-totp-token',
      redirectTo: 'http://localhost/callback?code=ac_after_totp&state=pending-state',
    })

    const result = await completeLoginAfterTotp('realm1', verifyResponse)

    // Exchange completed with the CARRIED pending verifier.
    expect(performPkceTokenExchange).toHaveBeenCalledWith('realm1', {
      code: 'ac_after_totp',
      codeVerifier: 'pending-verifier',
      redirectUri: 'http://localhost/callback',
      clientId: 'admin-web-console',
    })
    expect(applyTokenSet).toHaveBeenCalledWith({
      accessToken: 'at-after-totp',
      refreshToken: 'rt-after-totp',
      clientId: 'admin-web-console',
    })
    // PKCE state cleared once the exchange succeeds.
    expect(storeMock.setPkceState).toHaveBeenCalledWith(null)
    expect(result.redirectPath).toBe('/user/profile')
  })

  it('completeLoginAfterPasskey: pending PKCE state completes the exchange after Passkey verify', async () => {
    const { completeLoginAfterPasskey } = await import('@/lib/auth-utils')
    const storeMock = makeStoreMock()
    storeMock.getPkceState = vi.fn(() => ({
      codeVerifier: 'pending-verifier',
      clientId: 'admin-web-console',
      redirectUri: 'http://localhost/callback',
      state: 'pending-state',
    }))
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )

    vi.mocked(performPkceTokenExchange).mockResolvedValue({
      accessToken: 'at-after-passkey',
      refreshToken: 'rt-after-passkey',
      tokenType: 'Bearer',
      expiresIn: 900,
      refreshExpiresIn: 2592000,
    })
    vi.mocked(fetchAuthData).mockResolvedValue(makeAuthDataResponse())

    const verifyResponse: PasskeyVerifyResponse = {
      userId: 'user-1',
      token: 'post-passkey-token',
      message: 'OK',
      expiresInSeconds: 3600,
      redirectTo: 'http://localhost/callback?code=ac_after_passkey&state=pending-state',
    }

    const result = await completeLoginAfterPasskey('realm1', verifyResponse)

    expect(performPkceTokenExchange).toHaveBeenCalledWith('realm1', {
      code: 'ac_after_passkey',
      codeVerifier: 'pending-verifier',
      redirectUri: 'http://localhost/callback',
      clientId: 'admin-web-console',
    })
    expect(applyTokenSet).toHaveBeenCalledWith(
      expect.objectContaining({ clientId: 'admin-web-console' })
    )
    expect(storeMock.setPkceState).toHaveBeenCalledWith(null)
    expect(result.redirectPath).toBe('/user/profile')
  })
})

// ---------------------------------------------------------------------------
// Error-code distinguishability across the login / PKCE paths.
// The caller routes to re-login on every failure, but the surfaced error must
// be distinguishable (not collapsed to a single generic message) so the UI can
// tell "reuse detected → full re-login" from "transient / network".
// ---------------------------------------------------------------------------

describe('loginFlow / PKCE error-code distinguishability', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      makeStoreMock() as ReturnType<typeof useAuthStore.getState>
    )
  })

  it('loginFlow: a login API error surfaces verbatim (not a generic "login failed")', async () => {
    const storeMock = makeStoreMock()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )
    const apiError = Object.assign(new Error('origin_not_configured'), {
      status: 400,
    })
    vi.mocked(performLogin).mockRejectedValue(apiError)

    await expect(loginFlow('realm1', baseCredentials())).rejects.toMatchObject({
      message: 'origin_not_configured',
    })
    // A clean store reset is part of the re-login routing.
    expect(storeMock.logout).toHaveBeenCalled()
  })

  it.each([
    {
      label: 'invalid authorization code (PKCE exchange rejected)',
      exchangeError: 'invalid_grant',
      expectedMessagePart: 'PKCE token exchange failed',
    },
    {
      label: 'bad PKCE verifier',
      exchangeError: 'invalid_grant_bad_verifier',
      expectedMessagePart: 'PKCE token exchange failed',
    },
  ])(
    'loginFlow PKCE path: $label forces a full re-login (logout + cleared PKCE state + distinguishable error)',
    async ({ exchangeError, expectedMessagePart }) => {
      const storeMock = makeStoreMock()
      storeMock.getPkceState = vi.fn(() => ({
        codeVerifier: 'verifier-xyz',
        clientId: 'admin-web-console',
        redirectUri: 'http://localhost/callback',
        state: 'state-abc',
      }))
      vi.mocked(useAuthStore.getState).mockReturnValue(
        storeMock as ReturnType<typeof useAuthStore.getState>
      )

      vi.mocked(performLogin).mockResolvedValue(
        makeLoginResponse({ redirectTo: 'http://localhost/callback?code=ac_bad&state=state-abc' })
      )
      // Exchange rejected — `tryCompletePkceExchange` throws + forces re-login.
      vi.mocked(performPkceTokenExchange).mockRejectedValue(
        Object.assign(new Error(exchangeError), { status: 400 })
      )

      await expect(
        loginFlow('realm1', {
          clientId: 'admin-web-console',
          username: 'admin@test.com',
          password: 'password123',
        })
      ).rejects.toThrow(expectedMessagePart)

      // logout() revokes the family + clears RT/PKCE; state explicitly nulled.
      expect(storeMock.logout).toHaveBeenCalled()
      expect(storeMock.setPkceState).toHaveBeenCalledWith(null)
      // No token material was written on failure.
      expect(applyTokenSet).not.toHaveBeenCalled()
    }
  )

  it('loginFlow: distinguishes permission-insufficient from token-invalid via the propagated error', async () => {
    const storeMock = makeStoreMock()
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )

    vi.mocked(performLogin).mockRejectedValue(
      Object.assign(new Error('permission_insufficient'), { status: 403 })
    )

    await expect(loginFlow('realm1', baseCredentials())).rejects.toMatchObject({
      message: 'permission_insufficient',
      status: 403,
    })
  })

  it('completeLoginAfterTotp PKCE path: exchange failure forces re-login with a distinguishable error', async () => {
    const storeMock = makeStoreMock()
    storeMock.getPkceState = vi.fn(() => ({
      codeVerifier: 'pending-verifier',
      clientId: 'admin-web-console',
      redirectUri: 'http://localhost/callback',
      state: 'pending-state',
    }))
    vi.mocked(useAuthStore.getState).mockReturnValue(
      storeMock as ReturnType<typeof useAuthStore.getState>
    )
    vi.mocked(performPkceTokenExchange).mockRejectedValue(new Error('invalid_grant'))

    const verifyResponse = makeVerifyTotpResponse({
      token: 'tok',
      redirectTo: 'http://localhost/callback?code=ac_bad_totp&state=pending-state',
    })

    await expect(completeLoginAfterTotp('realm1', verifyResponse)).rejects.toThrow(
      'PKCE token exchange failed'
    )
    expect(storeMock.logout).toHaveBeenCalled()
    expect(storeMock.setPkceState).toHaveBeenCalledWith(null)
  })
})
