import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

/**
 * Signup CTA mount-site visibility on the admin (platform storefront) login.
 *
 * The CTA's feature gate is the operator's platform signup toggle (checked
 * inside SignupCtaBlock, fail-closed). The white-label brand override is
 * presentation only: suppressing the entry on `whiteLabel.brandName` would
 * orphan the signup page (still reachable and rendered under the same white
 * label) while the toggle says "show", and would make visibility depend on
 * the publicConfig race (the block flashes off once brandName arrives). This
 * test pins the contract: on the admin realm with the switch on, the branded
 * page still renders the entry.
 */

const mockNavigate = vi.fn()

vi.mock('@tanstack/react-router', async () => {
  const actual =
    (await vi.importActual) < typeof import('@tanstack/react-router')('@tanstack/react-router')
  return {
    ...actual,
    createFileRoute: () => (config: Record<string, unknown>) => ({
      useParams: () => ({ realmId: 'admin' }),
      useSearch: () => ({}),
      ...config,
    }),
    useRouter: () => ({ navigate: mockNavigate }),
    Link: ({
      to,
      children,
      ...props
    }: {
      to: string
      children?: React.ReactNode
    } & Record<string, unknown>) => (
      <a href={to} {...props}>
        {children}
      </a>
    ),
  }
})

vi.mock('@/lib/auth-utils', () => ({
  loginFlow: vi.fn(),
  completeLoginAfterTotp: vi.fn(),
  completeLoginAfterPasskey: vi.fn(),
  isConsentRequired: (response: { consentRequired?: boolean | null }) => !!response.consentRequired,
  getSafeRedirect: (path: string | undefined) => path ?? '/user/profile',
  checkAdminPermission: () => false,
  validateOAuthParams: () => ({ oauthParams: null, hasPartialOAuth: false }),
  FIRST_PARTY_CLIENT_ID: 'admin-web-console',
}))

vi.mock('@/hooks/use-oauth-login', () => ({
  useOAuthLogin: () => ({ initiateOAuthLogin: vi.fn() }),
}))

vi.mock('@/data/query-options', () => ({
  publicConfigQueryOptions: () => ({
    queryKey: ['public-config', 'admin'],
    queryFn: () =>
      Promise.resolve({
        realmName: 'Admin',
        realmDescription: null,
        oauthProviders: [],
        registration: { enabled: false },
        // The discriminating input: a published brand name (the demo seeds
        // exactly this — brandName "Herald") must NOT hide the signup entry.
        whiteLabel: { brandName: 'Herald' },
      }),
  }),
  turnstileStatusQueryOptions: () => ({
    queryKey: ['turnstile-status', 'admin'],
    queryFn: () => Promise.resolve({ enabled: false, site_key: null }),
  }),
  emailOtpStatusQueryOptions: () => ({
    queryKey: ['email-otp-status', 'admin'],
    queryFn: () => Promise.resolve({ enabled: false }),
  }),
  ldapStatusQueryOptions: () => ({
    queryKey: ['ldap-status', 'admin'],
    queryFn: () => Promise.resolve({ enabled: false }),
  }),
  passkeyStatusQueryOptions: () => ({
    queryKey: ['passkey-status', 'admin'],
    queryFn: () => Promise.resolve({ enabled: false }),
  }),
  signupStatusQueryOptions: () => ({
    queryKey: ['signup-status', 'admin'],
    queryFn: () => Promise.resolve({ enabled: true }),
  }),
}))

import { LoginPage } from '../login'

function renderLoginPage() {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({
          defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
        })
      }
    >
      <LoginPage />
    </QueryClientProvider>
  )
}

describe('LoginPage signup CTA mount gate (platform storefront)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('GIVEN the admin realm is white-labeled (brandName set) and the signup switch is on WHEN the login page settles THEN the CTA still renders and links to the signup page', async () => {
    renderLoginPage()

    const block = await screen.findByTestId('signup-cta-block')
    expect(block).toBeInTheDocument()

    const link = screen.getByTestId('signup-cta-link')
    expect(link).toHaveAttribute('href', '/admin/auth/signup')
  })
})
