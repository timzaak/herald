import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

/**
 * Passkey entry gating on the login route by the public Passkey feature flag.
 *
 * Contract this protects (Rule 8 — encode WHY, not WHAT):
 *  The passkey entry's PRIMARY gate is `passkeyStatus.enabled` from
 *  `GET /api/auth/{realmId}/passkey/status`. When the realm has passkey
 *  disabled, the route MUST NOT mount <PasskeyLoginForm> — so the
 *  begin-options probe request is never fired (the whole point of the status
 *  endpoint vs the old 404-from-options fallback). Only when the flag is
 *  explicitly `true` should the entry mount.
 *
 *  This is a gating test: we mock <PasskeyLoginForm> to a stable marker and
 *  assert only the route's mounting decision, not WebAuthn internals (those
 *  are covered in components/auth/__tests__/passkey-login-form.test.tsx).
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
  validateOAuthParams: () => ({ oauthParams: null, hasPartialOAuth: false }),
  FIRST_PARTY_CLIENT_ID: 'admin-web-console',
}))

vi.mock('@/hooks/use-oauth-login', () => ({
  useOAuthLogin: () => ({ initiateOAuthLogin: vi.fn() }),
}))

// Stable marker so we assert the ROUTE's mounting decision, not WebAuthn.
vi.mock('@/components/auth/passkey-login-form', () => ({
  PasskeyLoginForm: () => <div data-testid="passkey-entry-mounted" />,
}))

/**
 * The Passkey feature flag under test. Each `it` flips this to drive the
 * route's gating decision.
 */
let mockPasskeyEnabled = false

vi.mock('@/data/query-options', () => ({
  publicConfigQueryOptions: () => ({
    queryKey: ['public-config', 'test-realm'],
    queryFn: () =>
      Promise.resolve({
        realmName: 'Test Realm',
        oauthProviders: [],
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
  // THE flag under test. Default false (realm has not enabled passkey).
  passkeyStatusQueryOptions: () => ({
    queryKey: ['passkey-status', 'test-realm'],
    queryFn: () => Promise.resolve({ enabled: mockPasskeyEnabled }),
  }),
  toAuthConsentAgreements: () => [],
}))

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

describe('LoginPage passkey entry gating by passkey status flag', () => {
  beforeEach(() => {
    mockPasskeyEnabled = false
  })

  afterEach(() => {
    vi.clearAllMocks()
  })

  it('does NOT mount the passkey entry when the realm has passkey disabled', async () => {
    mockPasskeyEnabled = false
    renderLoginPage()

    // Wait for queries to settle so the gate decision is final, not pending.
    await waitFor(() => {
      expect(screen.queryByTestId('passkey-entry-mounted')).not.toBeInTheDocument()
    })
  })

  it('mounts the passkey entry when the realm has passkey enabled', async () => {
    mockPasskeyEnabled = true
    renderLoginPage()

    expect(await screen.findByTestId('passkey-entry-mounted')).toBeInTheDocument()
  })
})
