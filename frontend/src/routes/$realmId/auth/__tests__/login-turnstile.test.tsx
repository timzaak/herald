import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import type { LoginRequestPayload } from '@/lib/api-generated'

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
          href = href.replace(new RegExp(`\\$\\{${key}\\}|\\$${key}`, 'g'), value)
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
  isConsentRequired: (response: {
    consentRequired?: boolean | null
    consent_required?: boolean | null
  }) => !!response.consentRequired || !!response.consent_required,
  getSafeRedirect: (path: string | undefined) => path ?? '/user/profile',
  checkAdminPermission: () => false,
  validateOAuthParams: () => ({ oauthParams: null, hasPartialOAuth: false }),
  FIRST_PARTY_CLIENT_ID: 'admin-web-console',
}))

vi.mock('@/hooks/use-oauth-login', () => ({
  useOAuthLogin: () => ({ initiateOAuthLogin: vi.fn() }),
}))

// Turnstile enabled for this suite: the widget must render and the produced
// token must be forwarded on submit so the backend can verify it.
vi.mock('@/data/query-options', () => ({
  publicConfigQueryOptions: () => ({
    queryKey: ['public-config', 'test-realm'],
    queryFn: () =>
      Promise.resolve({
        realmName: 'Test Realm',
        realmDescription: '',
        oauthProviders: [],
        registration: { enabled: true },
      }),
  }),
  turnstileStatusQueryOptions: () => ({
    queryKey: ['turnstile-status', 'test-realm'],
    queryFn: () => Promise.resolve({ enabled: true, site_key: '0x4AAAA_TEST_SITE_KEY' }),
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

let turnstileCallbacks: { onSuccess?: (token: string) => void } = {}
vi.mock('@/components/auth/turnstile-widget', () => ({
  TurnstileWidget: ({ onTokenChange }: { onTokenChange: (token: string | null) => void }) => {
    // Capture so the test can simulate a successful challenge.
    turnstileCallbacks = { onSuccess: (token: string) => onTokenChange(token) }
    return <div data-testid="turnstile-mock" />
  },
}))

import { loginFlow } from '@/lib/auth-utils'
import { LoginPage } from '../login'

const mockLoginFlow = vi.mocked(loginFlow)

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

describe('LoginPage Turnstile', () => {
  const user = userEvent.setup({ delay: null })

  beforeEach(() => {
    vi.clearAllMocks()
    mockLoginFlow.mockReset()
    mockLoginFlow.mockResolvedValue({
      response: {
        userId: 'user-001',
        realmId: 'test-realm',
        message: 'Login successful',
        expiresInSeconds: 3600,
      },
      redirectPath: '/user/profile',
    })
  })

  it('renders the widget and forwards the produced token on submit', async () => {
    renderLoginPage()

    // Widget renders only when Turnstile is enabled for the realm.
    expect(await screen.findByTestId('turnstile-mock')).toBeInTheDocument()

    await user.type(screen.getByTestId('email-input'), 'user@example.com')
    await user.type(screen.getByTestId('password-input'), 'password123')

    // Simulate the user completing the challenge.
    turnstileCallbacks.onSuccess?.('dummy-turnstile-token')

    await user.click(screen.getByTestId('login-submit-button'))

    await waitFor(() => {
      expect(mockLoginFlow).toHaveBeenCalledTimes(1)
    })

    const payload = mockLoginFlow.mock.calls[0][1] as LoginRequestPayload
    expect(payload.turnstileToken).toBe('dummy-turnstile-token')
  })
})
