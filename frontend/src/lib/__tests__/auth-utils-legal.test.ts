import { describe, it, expect, vi, beforeEach } from 'vitest'
import { loginFlow, completeLoginAfterTotp } from '@/lib/auth-utils'
import type { LoginResponse, VerifyTotpResponse } from '@/lib/api-generated'

const mockLogin = vi.fn()
const mockLogout = vi.fn()
const mockSetAuthStatus = vi.fn()
const mockSetUserPermissions = vi.fn()
const mockSetUserProfile = vi.fn()
const mockReset = vi.fn()
const mockSetIsLoading = vi.fn()
const mockClearAuthStorage = vi.fn()
const mockSetRefreshClientId = vi.fn()
const mockSetPkceState = vi.fn()
const mockGetPkceState = vi.fn(() => null)

// The Herald PKCE bootstrap calls `oauthAuthorize` to seed backend state; stub
// it so the consent / safe-redirect tests do not hit the network. The legal
// tests pass credentials WITHOUT an `oauthClientId`, so this path runs; we keep
// `getPkceState` returning null so no PKCE exchange is attempted on the
// (absent) redirectTo, preserving the original consent-focused assertions.
vi.mock('@/lib/api-generated', () => ({
  oauthAuthorize: vi.fn().mockResolvedValue({ data: {}, error: undefined }),
}))

vi.mock('@/stores/auth-store', () => ({
  useAuthStore: {
    getState: () => ({
      login: mockLogin,
      logout: mockLogout,
      setAuthStatus: mockSetAuthStatus,
      setUserPermissions: mockSetUserPermissions,
      setUserProfile: mockSetUserProfile,
      reset: mockReset,
      setIsLoading: mockSetIsLoading,
      setRefreshClientId: mockSetRefreshClientId,
      setPkceState: mockSetPkceState,
      getPkceState: mockGetPkceState,
    }),
  },
  clearAuthStorage: () => mockClearAuthStorage(),
}))

// The Herald SDK client bridge owns the token family; only `applyTokenSet`
// writes token material in these flows, so it carries the consent-interlock
// assertions previously held by the store's `setTokens`.
const mockApplyTokenSet = vi.fn()
vi.mock('@/lib/herald-client', () => ({
  ensureHeraldClient: () => ({
    storage: { getRefreshToken: () => null },
    tokens: { getAccessToken: () => null, clear: vi.fn() },
    refresh: vi.fn(),
  }),
  getActiveHeraldClient: () => null,
  applyTokenSet: (...args: unknown[]) => mockApplyTokenSet(...args),
}))

const mockPerformLogin = vi.fn()
const mockFetchAuthData = vi.fn()
const mockPerformLogout = vi.fn()
const mockPerformPkceTokenExchange = vi.fn()

vi.mock('@/lib/auth-service', () => ({
  performLogin: (...args: unknown[]) => mockPerformLogin(...args),
  fetchAuthData: (...args: unknown[]) => mockFetchAuthData(...args),
  performLogout: (...args: unknown[]) => mockPerformLogout(...args),
  performPkceTokenExchange: (...args: unknown[]) => mockPerformPkceTokenExchange(...args),
  ClientSwitchError: class ClientSwitchError extends Error {},
}))

// Only `hasAdminPermission` needs stubbing for the consent / redirect-path
// assertions. The real `getSafeRedirectPath` / `DEFAULT_*` MUST run — the
// `getSafeRedirect` open-redirect-guard tests below assert against the real
// whitelist, so stubbing them out (a prior version of this mock did) defeated
// the very assertion it was checking.
vi.mock('@/lib/constants/auth-constants', async (importActual) => {
  const actual = await importActual<typeof import('@/lib/constants/auth-constants')>()
  return {
    ...actual,
    hasAdminPermission: () => false,
  }
})

function makeLoginResponse(overrides?: Partial<LoginResponse>): LoginResponse {
  return {
    userId: 'user-001',
    realmId: 'realm-1',
    message: 'OK',
    expiresInSeconds: 3600,
    ...overrides,
  }
}

function makeVerifyTotpResponse(overrides?: Partial<VerifyTotpResponse>): VerifyTotpResponse {
  return {
    userId: 'user-001',
    token: 'token-001',
    message: 'OK',
    expiresInSeconds: 3600,
    ...overrides,
  }
}

describe('loginFlow consent required handling', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('returns early when consentRequired is true and does not authenticate', async () => {
    const response = makeLoginResponse({
      consentRequired: true,
      agreements: [
        {
          agreement_type: 'terms_of_service',
          version_id: 'tos-v2',
          version_no: 2,
          effective_at: '2026-06-30T00:00:00Z',
          title: 'Terms',
          summary: null,
        },
      ],
    })
    mockPerformLogin.mockResolvedValue(response)

    const result = await loginFlow('realm-1', {
      clientId: 'client-1',
      password: 'secret',
    })

    expect(result.response.consentRequired).toBe(true)
    expect(mockLogin).not.toHaveBeenCalled()
    expect(mockFetchAuthData).not.toHaveBeenCalled()
  })

  it('returns early when consent_required snake_case field is true', async () => {
    const response = makeLoginResponse({
      consent_required: true,
    } as LoginResponse)
    mockPerformLogin.mockResolvedValue(response)

    await loginFlow('realm-1', { clientId: 'client-1', password: 'secret' })

    expect(mockLogin).not.toHaveBeenCalled()
    expect(mockFetchAuthData).not.toHaveBeenCalled()
  })

  it('authenticates normally when consentRequired is false', async () => {
    const response = makeLoginResponse({ consentRequired: false })
    mockPerformLogin.mockResolvedValue(response)
    mockFetchAuthData.mockResolvedValue({
      authStatus: { authenticated: true, realmId: 'realm-1' },
      userPermissions: { permissions: [], roles: [] },
      userProfile: null,
    })

    await loginFlow('realm-1', { clientId: 'client-1', password: 'secret' })

    expect(mockLogin).toHaveBeenCalledWith('realm-1')
    expect(mockFetchAuthData).toHaveBeenCalledWith()
  })
})

describe('completeLoginAfterTotp consent required handling', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('returns empty object when consentRequired is true and does not update auth state', async () => {
    const response = makeVerifyTotpResponse({
      consentRequired: true,
      agreements: [
        {
          agreement_type: 'privacy_policy',
          version_id: 'privacy-v3',
          version_no: 3,
          effective_at: '2026-06-30T00:00:00Z',
          title: 'Privacy',
          summary: null,
        },
      ],
    })

    const result = await completeLoginAfterTotp('realm-1', response)

    expect(result).toEqual({})
    expect(mockFetchAuthData).not.toHaveBeenCalled()
    expect(mockSetAuthStatus).not.toHaveBeenCalled()
  })

  it('returns early when consent_required snake_case field is true', async () => {
    const response = makeVerifyTotpResponse({
      consent_required: true,
    } as VerifyTotpResponse)

    await completeLoginAfterTotp('realm-1', response)

    expect(mockFetchAuthData).not.toHaveBeenCalled()
    expect(mockSetAuthStatus).not.toHaveBeenCalled()
  })

  it('completes login normally when consentRequired is absent', async () => {
    const response = makeVerifyTotpResponse()
    mockFetchAuthData.mockResolvedValue({
      authStatus: { authenticated: true, realmId: 'realm-1' },
      userPermissions: { permissions: [], roles: [] },
      userProfile: null,
    })

    await completeLoginAfterTotp('realm-1', response)

    expect(mockFetchAuthData).toHaveBeenCalledWith()
    expect(mockSetAuthStatus).toHaveBeenCalled()
  })
})

/**
 * Token-only flow regression (design §4.4): the consent interlock must still
 * hold under the Bearer token model — when consent is required, NO Bearer
 * token material (refresh token, in-memory access token) may be written.
 *
 * Note: the PKCE verifier is an OAuth *protocol nonce* that binds the
 * authorization code, not Bearer-family token material — it grants no access
 * on its own. loginFlow intentionally seeds it before performLogin() so the
 * verifier survives the consent detour (beginFirstPartyPkceFlow is idempotent
 * and reuses the in-flight state on the post-consent re-submit). The real
 * Bearer guard is therefore applyTokenSet() (the Herald SDK token bridge),
 * asserted below; PKCE seeding is expected and out of scope for this consent
 * interlock.
 */
describe('consent interlock: no token material written under token-only flow', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('loginFlow consent=true does not establish the Bearer token family', async () => {
    mockPerformLogin.mockResolvedValue(
      makeLoginResponse({
        consentRequired: true,
        agreements: [
          {
            agreement_type: 'terms_of_service',
            version_id: 'tos-v2',
            version_no: 2,
            effective_at: '2026-06-30T00:00:00Z',
            title: 'Terms',
            summary: null,
          },
        ],
      })
    )

    await loginFlow('realm-1', { clientId: 'admin-web-console', password: 'secret' })

    // The Bearer family must NOT be established before consent is granted.
    // (PKCE state MAY be seeded here — it is a protocol nonce, not a token;
    // see the describe docstring.)
    expect(mockApplyTokenSet).not.toHaveBeenCalled()
    expect(mockLogin).not.toHaveBeenCalled()
    expect(mockFetchAuthData).not.toHaveBeenCalled()
  })

  it('completeLoginAfterTotp consent=true writes neither refresh token nor PKCE state', async () => {
    const response = makeVerifyTotpResponse({
      consentRequired: true,
      agreements: [
        {
          agreement_type: 'privacy_policy',
          version_id: 'privacy-v3',
          version_no: 3,
          effective_at: '2026-06-30T00:00:00Z',
          title: 'Privacy',
          summary: null,
        },
      ],
    })

    const result = await completeLoginAfterTotp('realm-1', response)

    expect(result).toEqual({})
    expect(mockApplyTokenSet).not.toHaveBeenCalled()
    expect(mockSetPkceState).not.toHaveBeenCalledWith(expect.any(Object))
    expect(mockSetAuthStatus).not.toHaveBeenCalled()
    expect(mockFetchAuthData).not.toHaveBeenCalled()
  })
})

/**
 * getSafeRedirect under the token-only flow: behaviour is unchanged from the
 * cookie-session era — only the redirect whitelisting matters, which is
 * independent of how the session is established.
 */
describe('getSafeRedirect unchanged under token-only flow', () => {
  it('returns a whitelisted relative path verbatim', async () => {
    const { getSafeRedirect } = await import('@/lib/auth-utils')
    expect(getSafeRedirect('/user/profile')).toBe('/user/profile')
  })

  it('falls back when the requested path is not whitelisted (open-redirect guard)', async () => {
    const { getSafeRedirect } = await import('@/lib/auth-utils')
    expect(getSafeRedirect('//evil.example.com', '/user/profile')).toBe('/user/profile')
  })
})
