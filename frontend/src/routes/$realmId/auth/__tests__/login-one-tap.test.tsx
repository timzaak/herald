import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

/**
 * Google One Tap gating on the login route (design §4.4.3).
 *
 * The route decides whether to mount `<OneTapLogin>` based on two conditions
 * encoded here as design invariants, not incidental behavior:
 *
 *  1. The realm must expose Google as an enabled provider with a client_id in
 *     publicConfig (otherwise the backend One Tap endpoint returns 404 and GIS
 *     has no client_id to initialize).
 *  2. The page must NOT be serving a third-party OAuth downstream login
 *     (`oauthParams` present). In that mode One Tap's direct-session flow would
 *     mint a first-party token, conflicting with the Code+PKCE grant the third
 *     party is waiting on; the redirect OAuth buttons already cover that case.
 *
 * `<OneTapLogin>` itself is mocked to a stable marker so we assert only the
 * route's mounting decision, not GIS internals (covered in
 * `components/auth/__tests__/one-tap-login.test.tsx`).
 */

const mockNavigate = vi.fn()

vi.mock('@tanstack/react-router', async () => {
  const actual =
    await vi.importActual<typeof import('@tanstack/react-router')>('@tanstack/react-router')
  return {
    ...actual,
    createFileRoute: () => (config: Record<string, unknown>) => ({
      useParams: () => ({ realmId: 'test-realm' }),
      useSearch: () => ({}),
      ...config,
    }),
    useRouter: () => ({ navigate: mockNavigate }),
    Link: ({
      to,
      params,
      children,
      ...props
    }: {
      to: string
      params?: Record<string, string>
      children?: React.ReactNode
    }) => {
      let href = to as string
      if (params) {
        Object.entries(params).forEach(([key, value]) => {
          href = href.replace(new RegExp(`\\$\\${key}|\\$\\{${key}\\}`, 'g'), value)
        })
      }
      return (
        <a href={href} {...props}>
          {children}
        </a>
      )
    },
  }
})

vi.mock('@/lib/auth-utils', () => ({
  loginFlow: vi.fn(),
  completeLoginAfterTotp: vi.fn(),
  completeLoginAfterPasskey: vi.fn(),
  completeLoginAfterEmailOtp: vi.fn(),
  completeLoginAfterOneTap: vi.fn(),
  isConsentRequired: (response: { consentRequired?: boolean | null }) => !!response.consentRequired,
  getSafeRedirect: (path: string | undefined) => path ?? '/user/profile',
  checkAdminPermission: () => false,
  validateOAuthParams: vi.fn(),
  FIRST_PARTY_CLIENT_ID: 'admin-web-console',
}))

vi.mock('@/hooks/use-oauth-login', () => ({
  useOAuthLogin: () => ({ initiateOAuthLogin: vi.fn() }),
}))

vi.mock('@/components/auth/one-tap-login', () => ({
  OneTapLogin: (props: { googleClientId: string; realmId: string }) => (
    <div data-testid="one-tap-container" data-google-client-id={props.googleClientId} />
  ),
}))

/**
 * Configurable publicConfig. Tests override `oauthProviders` to flip the Google
 * provider on/off, and `validateOAuthParams` return to flip the downstream mode.
 */
let mockOauthProviders: Array<{
  name: string
  displayName: string
  enabled: boolean
  clientId?: string
}> = []
let mockValidateOAuthParamsReturn: {
  oauthParams: { oauthClientId: string; redirectUri: string; state: string } | null
  hasPartialOAuth: boolean
} = { oauthParams: null, hasPartialOAuth: false }

vi.mock('@/data/query-options', () => ({
  publicConfigQueryOptions: () => ({
    queryKey: ['public-config', 'test-realm'],
    queryFn: () =>
      Promise.resolve({
        realmName: 'Test Realm',
        oauthProviders: mockOauthProviders,
        registration: { enabled: false },
      }),
  }),
  turnstileStatusQueryOptions: () => ({
    queryKey: ['turnstile-status', 'test-realm'],
    queryFn: () => Promise.resolve({ enabled: false, site_key: null }),
  }),
  emailOtpStatusQueryOptions: () => ({
    queryKey: ['email-otp-status', 'test-realm'],
    queryFn: () => Promise.resolve({ enabled: false }),
  }),
  // LDAP entry gate; default disabled keeps the corporate-account entry
  // hidden in these password-flow tests (fail-closed).
  ldapStatusQueryOptions: () => ({
    queryKey: ['ldap-status', 'test-realm'],
    queryFn: () => Promise.resolve({ enabled: false }),
  }),
  // Passkey status gate for the passkey entry. Default true to preserve the
  // pre-flag mount behaviour (the form's own options probe + onUnavailable
  // remains the per-browser fallback).
  passkeyStatusQueryOptions: () => ({
    queryKey: ['passkey-status', 'test-realm'],
    queryFn: () => Promise.resolve({ enabled: true }),
  }),
  toAuthConsentAgreements: () => [],
}))

// Re-import validateOAuthParams after the auth-utils mock so we can override
// its return per test via vi.mocked.
import { validateOAuthParams } from '@/lib/auth-utils'
import { LoginPage } from '../login'

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
}

function renderLoginPage() {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <LoginPage />
    </QueryClientProvider>
  )
}

describe('LoginPage Google One Tap gating', () => {
  beforeEach(() => {
    mockOauthProviders = []
    mockValidateOAuthParamsReturn = { oauthParams: null, hasPartialOAuth: false }
    vi.mocked(validateOAuthParams).mockImplementation(() => mockValidateOAuthParamsReturn)
  })

  afterEach(() => {
    vi.clearAllMocks()
  })

  it('mounts One Tap when Google provider is enabled with a client_id', async () => {
    mockOauthProviders = [
      { name: 'google', displayName: 'Google', enabled: true, clientId: 'google-client-123' },
    ]
    renderLoginPage()

    const container = await screen.findByTestId('one-tap-container')
    expect(container).toHaveAttribute('data-google-client-id', 'google-client-123')
  })

  it('does not mount One Tap when Google provider is not configured', async () => {
    // Without a Google provider, the backend endpoint would 404 and GIS would
    // have no client_id — the entry must not render.
    mockOauthProviders = [
      { name: 'github', displayName: 'GitHub', enabled: true, clientId: 'gh-123' },
    ]
    renderLoginPage()

    // Give the query a chance to settle before asserting absence.
    await waitFor(() => {
      expect(screen.queryByTestId('one-tap-container')).not.toBeInTheDocument()
    })
  })

  it('does not mount One Tap when serving a third-party OAuth downstream login', async () => {
    // oauthParams present → the page is brokering a Code+PKCE grant for a third
    // party. One Tap direct-session mode would mint a first-party token,
    // breaking that contract. This is the key boundary decision.
    mockOauthProviders = [
      { name: 'google', displayName: 'Google', enabled: true, clientId: 'google-client-123' },
    ]
    mockValidateOAuthParamsReturn = {
      oauthParams: {
        oauthClientId: 'third-party-app',
        redirectUri: 'https://app.example.com/callback',
        state: 'xyz',
      },
      hasPartialOAuth: false,
    }
    renderLoginPage()

    await waitFor(() => {
      expect(screen.queryByTestId('one-tap-container')).not.toBeInTheDocument()
    })
  })
})
