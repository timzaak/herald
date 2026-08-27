import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { http, HttpResponse } from 'msw'
import { server } from '@/test/mocks/server'
import type { LegalAgreementSummary, LoginResponse } from '@/lib/api-generated'

/**
 * FE-D03 — white-label integration for the login route.
 *
 * Every auth sub-state (main form, consent re-consent, TOTP second factor,
 * passkey second factor) renders its OWN `AuthPageWrapper`. The fix threads the
 * derived `whiteLabel` into each call; this suite asserts the brand surfaces
 * (logo image via `auth-brand-logo`, the `loginTitle` override) on every branch.
 *
 * To make a missing prop a *real* failure rather than a silent pass, the mock
 * config ships a `logoUrl` and a `loginTitle`. If a branch forgets to forward
 * `whiteLabel`, the wrapper falls back to `auth-brand-text` (Herald) and there
 * is no logo node — the branch-specific assertion then fails.
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
  isConsentRequired: (response: { consentRequired?: boolean | null }) => !!response.consentRequired,
  getSafeRedirect: (path: string | undefined) => path ?? '/user/profile',
  checkAdminPermission: () => false,
  validateOAuthParams: () => ({ oauthParams: null, hasPartialOAuth: false }),
  FIRST_PARTY_CLIENT_ID: 'admin-web-console',
}))

vi.mock('@/hooks/use-oauth-login', () => ({
  useOAuthLogin: () => ({ initiateOAuthLogin: vi.fn() }),
}))

/**
 * White-label mock. The `logoUrl` is what makes the assertions discriminating:
 * a branch that forgets to forward `whiteLabel` will show `auth-brand-text`
 * (Herald) instead of `auth-brand-logo`, failing the logo presence check.
 */
vi.mock('@/data/query-options', () => ({
  publicConfigQueryOptions: () => ({
    queryKey: ['public-config', 'test-realm'],
    queryFn: () =>
      Promise.resolve({
        realmName: 'Realm Name Fallback',
        realmDescription: 'Realm Description Fallback',
        oauthProviders: [],
        registration: { enabled: true },
        whiteLabel: {
          logoUrl: 'https://cdn.example.com/brand.svg',
          loginTitle: 'Sign in to Example',
          loginSubtitle: 'Example subtitle',
        },
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
  toAuthConsentAgreements: (agreements: LegalAgreementSummary[]) =>
    agreements.map((agreement) => ({
      agreementType: agreement.agreement_type,
      versionId: agreement.version_id,
    })),
}))

import { loginFlow } from '@/lib/auth-utils'
import { LoginPage } from '../login'

const mockLoginFlow = vi.mocked(loginFlow)

const API_BASE_URL = 'http://localhost:3000'

function makeAgreementSummary(
  agreementType: string,
  versionId: string,
  versionNo: number
): LegalAgreementSummary {
  return {
    agreement_type: agreementType,
    version_id: versionId,
    version_no: versionNo,
    effective_at: '2026-06-30T00:00:00Z',
    title: null,
    summary: null,
  }
}

function makeSecondFactorsLoginResponse(overrides: Partial<LoginResponse> = {}): LoginResponse {
  return {
    userId: 'user-001',
    realmId: 'test-realm',
    message: 'second factor required',
    expiresInSeconds: 3600,
    tempToken: 'temp-token-2fa',
    secondFactors: null,
    requiresTotp: false,
    ...overrides,
  }
}

function makeLoginFlowResult(response: LoginResponse) {
  return { response, redirectPath: '/user/profile' }
}

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

async function submitPasswordLogin(user: ReturnType<typeof userEvent.setup>) {
  await user.type(screen.getByTestId('email-input'), 'user@example.com')
  await user.type(screen.getByTestId('password-input'), 'password123')
  await user.click(screen.getByTestId('login-submit-button'))
}

describe('LoginPage white-label integration (FE-D03)', () => {
  const user = userEvent.setup({ delay: null })

  beforeEach(() => {
    vi.clearAllMocks()
    mockLoginFlow.mockReset()
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  describe('main form', () => {
    it('applies whiteLabel.loginTitle and the brand logo on the main login form', async () => {
      renderLoginPage()

      // logoUrl is set → brand logo renders, not the Herald text fallback.
      const logo = await screen.findByTestId('auth-brand-logo')
      expect(logo).toHaveAttribute('src', 'https://cdn.example.com/brand.svg')
      expect(screen.queryByTestId('auth-brand-text')).not.toBeInTheDocument()

      // loginTitle overrides realmName.
      expect(screen.getByTestId('login-title')).toHaveTextContent('Sign in to Example')
      // loginSubtitle overrides realmDescription.
      expect(screen.getByText('Example subtitle')).toBeInTheDocument()
    })
  })

  describe('consent re-consent sub-state', () => {
    it('renders the brand logo on the consent re-consent view', async () => {
      mockLoginFlow.mockResolvedValueOnce(
        makeLoginFlowResult(
          makeSecondFactorsLoginResponse({
            consentRequired: true,
            agreements: [makeAgreementSummary('terms_of_service', 'tos-v2', 2)],
          })
        )
      )

      renderLoginPage()
      await submitPasswordLogin(user)

      await screen.findByTestId('login-reconsent-view')
      // If consent branch forgot to forward whiteLabel, this logo would be absent.
      expect(screen.getByTestId('auth-brand-logo')).toHaveAttribute(
        'src',
        'https://cdn.example.com/brand.svg'
      )
    })
  })

  describe('TOTP second-factor sub-state', () => {
    it('renders the brand logo on the TOTP verification form', async () => {
      mockLoginFlow.mockResolvedValueOnce(
        makeLoginFlowResult(makeSecondFactorsLoginResponse({ secondFactors: ['totp'] }))
      )

      renderLoginPage()
      await submitPasswordLogin(user)

      await waitFor(() => {
        expect(screen.getByTestId('totp-verification-form')).toBeInTheDocument()
      })
      // If TOTP branch forgot to forward whiteLabel, this logo would be absent.
      expect(screen.getByTestId('auth-brand-logo')).toHaveAttribute(
        'src',
        'https://cdn.example.com/brand.svg'
      )
    })
  })

  describe('passkey second-factor sub-state', () => {
    beforeEach(() => {
      // Keep WebAuthn "supported" so the Passkey2FaForm renders its button path.
      Object.defineProperty(window, 'PublicKeyCredential', {
        value: function PublicKeyCredential() {},
        configurable: true,
        writable: true,
      })
      vi.stubGlobal('navigator', {
        credentials: { get: vi.fn().mockResolvedValue(null), create: vi.fn() },
      })

      // MSW: serve the 2fa options so Passkey2FaForm can arm; verify stays pending.
      server.resetHandlers()
      server.use(
        http.post(`${API_BASE_URL}/api/auth/:realmId/login/passkey/2fa/options`, () =>
          HttpResponse.json({
            authToken: 'auth-2fa',
            options: { publicKey: { challenge: 'Y2hhbGxlbmdl' } },
          })
        )
      )
    })

    it('renders the brand logo on the passkey 2FA form', async () => {
      mockLoginFlow.mockResolvedValueOnce(
        makeLoginFlowResult(makeSecondFactorsLoginResponse({ secondFactors: ['passkey'] }))
      )

      renderLoginPage()
      await submitPasswordLogin(user)

      await waitFor(() => {
        expect(screen.getByTestId('passkey-2fa-form')).toBeInTheDocument()
      })
      // If passkey branch forgot to forward whiteLabel, this logo would be absent.
      expect(screen.getByTestId('auth-brand-logo')).toHaveAttribute(
        'src',
        'https://cdn.example.com/brand.svg'
      )
    })
  })
})
