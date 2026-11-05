import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

/**
 * SMS-OTP entry gating on the login route by the public phone-login flag.
 *
 * Contract this protects:
 *  The SMS entry's only gate is `phoneLoginStatus.enabled` from
 *  `GET /api/auth/{realmId}/phone-login/status`. The entry must render ONLY on
 *  an explicit `enabled === true`: a mistimed or over-eager render hands the
 *  user an entry that 400s on submit (realm has phone login off), which is
 *  worse than no entry at all (fail-closed security posture). The
 *  same flag also drives the password-form identifier label: while the realm
 *  accepts a phone number in the username field the label must say so
 *  (zero-payload transport, DEC-phone-login-009), and must NOT promise phone
 *  support when the flag is off.
 *
 *  This is a gating test: we mock the heavy card bodies to stable markers and
 *  assert only the route's render decisions, not the SMS form internals
 *  (covered in components/auth/__tests__/sms-otp-login-form.test.tsx).
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

// Heavy card bodies become stable markers so the test asserts the route's
// mounting decision only.
vi.mock('@/components/auth/passkey-login-form', () => ({
  PasskeyLoginForm: () => <div data-testid="passkey-entry-mounted" />,
}))
vi.mock('@/components/auth/one-tap-login', () => ({
  OneTapLogin: () => <div data-testid="one-tap-entry-mounted" />,
}))
vi.mock('@/components/auth/email-otp-login-form', () => ({
  EmailOtpLoginForm: () => <div data-testid="email-otp-form-mounted" />,
}))
vi.mock('@/components/auth/ldap-login-form', () => ({
  LdapLoginForm: () => <div data-testid="ldap-form-mounted" />,
}))
vi.mock('@/components/auth/sms-otp-login-form', () => ({
  SmsOtpLoginForm: () => <div data-testid="sms-otp-form-mounted" />,
}))

/**
 * The phone-login flag under test: `undefined` models the query still loading
 * (the queryFn resolving late), so the route sees `data === undefined`.
 */
let mockPhoneLoginEnabled: boolean | 'loading' = false

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
  ldapStatusQueryOptions: () => ({
    queryKey: ['ldap-status', 'test-realm'],
    queryFn: () => Promise.resolve({ enabled: false }),
  }),
  passkeyStatusQueryOptions: () => ({
    queryKey: ['passkey-status', 'test-realm'],
    queryFn: () => Promise.resolve({ enabled: false }),
  }),
  // THE flag under test. `loading` never resolves: the gate must hold while
  // the status query is in flight (fail-closed beats optimistic).
  phoneLoginStatusQueryOptions: () => ({
    queryKey: ['phone-login-status', 'test-realm'],
    queryFn: () =>
      mockPhoneLoginEnabled === 'loading'
        ? new Promise<boolean>(() => {})
        : Promise.resolve({ enabled: mockPhoneLoginEnabled }),
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

describe('LoginPage SMS entry gating by phone-login status flag', () => {
  beforeEach(() => {
    mockPhoneLoginEnabled = false
  })

  afterEach(() => {
    vi.clearAllMocks()
  })

  it('does NOT render the SMS entry when the realm has phone login disabled', async () => {
    mockPhoneLoginEnabled = false
    renderLoginPage()

    // Wait for the settled state (password form visible) so the gate decision
    // is final, not merely pending.
    await screen.findByTestId('login-form')
    expect(screen.queryByTestId('phone-otp-toggle')).not.toBeInTheDocument()
  })

  it('does NOT render the SMS entry while the status query is in flight', async () => {
    mockPhoneLoginEnabled = 'loading'
    renderLoginPage()

    await screen.findByTestId('login-form')
    expect(screen.queryByTestId('phone-otp-toggle')).not.toBeInTheDocument()
  })

  it('renders the SMS entry only on an explicit enabled === true', async () => {
    mockPhoneLoginEnabled = true
    renderLoginPage()

    expect(await screen.findByTestId('phone-otp-toggle')).toBeInTheDocument()
  })

  it('switches the password identifier label to include phone only while the flag is on', async () => {
    mockPhoneLoginEnabled = true
    renderLoginPage()
    await screen.findByTestId('phone-otp-toggle')

    expect(screen.getByTestId('login-form')).toHaveTextContent('Email / phone / username')
  })

  it('keeps the default identifier label when the flag is off', async () => {
    mockPhoneLoginEnabled = false
    renderLoginPage()
    await screen.findByTestId('login-form')

    expect(screen.getByTestId('login-form')).toHaveTextContent(/username or email/i)
    expect(screen.getByTestId('login-form')).not.toHaveTextContent(/phone/i)
  })
})
