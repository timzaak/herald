import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { http, HttpResponse } from 'msw'
import { server } from '@/test/mocks/server'
import type { LegalAgreementSummary, LoginResponse } from '@/lib/api-generated'

/**
 * LoginPage passkey second-factor routing (FE-D04) — the highest regression-
 * value file. Renders the full LoginPage, mocks loginFlow to return each
 * LoginResponse.secondFactors shape, and asserts the design §5.3 read order:
 *
 *   secondFactors present + non-empty → route (passkey → 2FA form; otherwise
 *   TOTP). secondFactors ABSENT → legacy requiresTotp fallback (byte-identical).
 *
 * This must NOT break the existing password+TOTP link. Consent interlock is also
 * asserted: a passkey verify returning consentRequired enters the re-consent UI.
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
  // isConsentRequired checks camelCase consentRequired + snake_case consent_required.
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
  // Passkey enabled by default so the PasskeyLoginForm entry mounts and the
  // per-test MSW handlers on /login/passkey/options drive the real behaviour.
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

import { loginFlow, completeLoginAfterPasskey } from '@/lib/auth-utils'
import { LoginPage } from '../login'

const mockLoginFlow = vi.mocked(loginFlow)
const mockCompleteLoginAfterPasskey = vi.mocked(completeLoginAfterPasskey)

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

/** Factory: build a LoginResponse shaped for a given second-factor scenario. */
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

describe('LoginPage passkey second-factor routing', () => {
  const user = userEvent.setup({ delay: null })

  beforeEach(() => {
    vi.clearAllMocks()
    mockLoginFlow.mockReset()
    mockCompleteLoginAfterPasskey.mockReset()

    // Keep WebAuthn "supported" so the Passkey2FaForm renders its button path.
    Object.defineProperty(window, 'PublicKeyCredential', {
      value: function PublicKeyCredential() {},
      configurable: true,
      writable: true,
    })
    // Conditional / explicit get stays pending (null) by default — isolates
    // routing from the actual verify interaction.
    vi.stubGlobal('navigator', {
      credentials: { get: vi.fn().mockResolvedValue(null), create: vi.fn() },
    })

    // MSW: serve the first-factor options (so the entry point mounts) and the
    // second-factor options (so Passkey2FaForm can arm). verify is overridden
    // per-test where needed.
    server.resetHandlers()
    server.use(
      http.post(`${API_BASE_URL}/api/auth/:realmId/login/passkey/options`, () =>
        HttpResponse.json({
          authToken: 'auth-1fa',
          options: { publicKey: { challenge: 'Y2hhbGxlbmdl' } },
        })
      ),
      http.post(`${API_BASE_URL}/api/auth/:realmId/login/passkey/2fa/options`, () =>
        HttpResponse.json({
          authToken: 'auth-2fa',
          options: { publicKey: { challenge: 'Y2hhbGxlbmdl' } },
        })
      ),
      http.post(`${API_BASE_URL}/api/auth/:realmId/login/passkey/2fa/verify`, () =>
        HttpResponse.json({
          userId: 'user-001',
          token: 'session-token',
          message: 'ok',
          expiresInSeconds: 3600,
        })
      )
    )
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  describe('secondFactors read order (design §5.3)', () => {
    it('GIVEN secondFactors=[totp] (no passkey) WHEN logging in THEN should route to the TOTP step (existing link intact)', async () => {
      mockLoginFlow.mockResolvedValueOnce(
        makeLoginFlowResult(makeSecondFactorsLoginResponse({ secondFactors: ['totp'] }))
      )

      renderLoginPage()
      await submitPasswordLogin(user)

      await waitFor(() => {
        expect(screen.getByTestId('totp-verification-form')).toBeInTheDocument()
      })
      expect(screen.queryByTestId('passkey-2fa-form')).not.toBeInTheDocument()
    })

    it('GIVEN secondFactors=[totp, passkey] WHEN logging in THEN should default to passkey 2FA and offer the Use TOTP link', async () => {
      mockLoginFlow.mockResolvedValueOnce(
        makeLoginFlowResult(makeSecondFactorsLoginResponse({ secondFactors: ['totp', 'passkey'] }))
      )

      renderLoginPage()
      await submitPasswordLogin(user)

      await waitFor(() => {
        expect(screen.getByTestId('passkey-2fa-form')).toBeInTheDocument()
      })
      expect(screen.getByTestId('passkey-use-totp-link')).toBeInTheDocument()
    })

    it('GIVEN secondFactors=[passkey] (only passkey) WHEN logging in THEN should render passkey 2FA with NO TOTP link', async () => {
      mockLoginFlow.mockResolvedValueOnce(
        makeLoginFlowResult(makeSecondFactorsLoginResponse({ secondFactors: ['passkey'] }))
      )

      renderLoginPage()
      await submitPasswordLogin(user)

      await waitFor(() => {
        expect(screen.getByTestId('passkey-2fa-form')).toBeInTheDocument()
      })
      expect(screen.queryByTestId('passkey-use-totp-link')).not.toBeInTheDocument()
    })

    it('GIVEN secondFactors=[unknown] (unknown factor, no passkey) WHEN logging in THEN should degrade gracefully to TOTP', async () => {
      mockLoginFlow.mockResolvedValueOnce(
        makeLoginFlowResult(makeSecondFactorsLoginResponse({ secondFactors: ['sms'] }))
      )

      renderLoginPage()
      await submitPasswordLogin(user)

      await waitFor(() => {
        expect(screen.getByTestId('totp-verification-form')).toBeInTheDocument()
      })
      expect(screen.queryByTestId('passkey-2fa-form')).not.toBeInTheDocument()
    })

    it('GIVEN user clicks Use TOTP WHEN on passkey 2FA with both factors THEN should switch to the TOTP form', async () => {
      mockLoginFlow.mockResolvedValueOnce(
        makeLoginFlowResult(makeSecondFactorsLoginResponse({ secondFactors: ['totp', 'passkey'] }))
      )

      renderLoginPage()
      await submitPasswordLogin(user)
      await screen.findByTestId('passkey-2fa-form')

      await user.click(screen.getByTestId('passkey-use-totp-link'))

      await waitFor(() => {
        expect(screen.getByTestId('totp-verification-form')).toBeInTheDocument()
      })
      expect(screen.queryByTestId('passkey-2fa-form')).not.toBeInTheDocument()
    })
  })

  describe('backward-compatible fallback (no secondFactors)', () => {
    it('GIVEN no secondFactors + requiresTotp=true (legacy) WHEN logging in THEN should route to TOTP', async () => {
      mockLoginFlow.mockResolvedValueOnce(
        makeLoginFlowResult(
          makeSecondFactorsLoginResponse({ secondFactors: null, requiresTotp: true })
        )
      )

      renderLoginPage()
      await submitPasswordLogin(user)

      await waitFor(() => {
        expect(screen.getByTestId('totp-verification-form')).toBeInTheDocument()
      })
      expect(screen.queryByTestId('passkey-2fa-form')).not.toBeInTheDocument()
    })

    it('GIVEN no secondFactors + requiresTotp=false WHEN logging in THEN should complete login directly (no 2FA form)', async () => {
      mockLoginFlow.mockResolvedValueOnce({
        response: {
          userId: 'user-001',
          realmId: 'test-realm',
          message: 'ok',
          expiresInSeconds: 3600,
          secondFactors: null,
          requiresTotp: false,
        },
        redirectPath: '/user/profile',
      })

      renderLoginPage()
      await submitPasswordLogin(user)

      // Direct login → navigates away; no 2FA step rendered.
      await waitFor(() => {
        expect(mockNavigate).toHaveBeenCalled()
      })
      expect(screen.queryByTestId('totp-verification-form')).not.toBeInTheDocument()
      expect(screen.queryByTestId('passkey-2fa-form')).not.toBeInTheDocument()
    })
  })

  describe('consent interlock preserved across passkey branches', () => {
    it('GIVEN passkey 2FA verify returns consentRequired + agreements WHEN verifying THEN should show re-consent view', async () => {
      mockLoginFlow.mockResolvedValueOnce(
        makeLoginFlowResult(makeSecondFactorsLoginResponse({ secondFactors: ['passkey'] }))
      )

      const credential = {
        id: 'cred-1',
        rawId: new TextEncoder().encode('raw-id').buffer,
        type: 'public-key',
        response: {
          authenticatorData: new TextEncoder().encode('auth').buffer,
          clientDataJSON: new TextEncoder().encode('cd').buffer,
          signature: new TextEncoder().encode('sig').buffer,
          userHandle: null,
        },
      } as unknown as PublicKeyCredential
      vi.stubGlobal('navigator', {
        credentials: { get: vi.fn().mockResolvedValue(credential), create: vi.fn() },
      })

      server.resetHandlers()
      server.use(
        http.post(`${API_BASE_URL}/api/auth/:realmId/login/passkey/2fa/options`, () =>
          HttpResponse.json({
            authToken: 'auth-2fa',
            options: { publicKey: { challenge: 'Y2hhbGxlbmdl' } },
          })
        ),
        http.post(`${API_BASE_URL}/api/auth/:realmId/login/passkey/2fa/verify`, () =>
          HttpResponse.json({
            userId: 'user-001',
            token: 'temp-token',
            message: 'consent required',
            expiresInSeconds: 3600,
            consentRequired: true,
            agreements: [makeAgreementSummary('terms_of_service', 'tos-v2', 2)],
          })
        )
      )

      renderLoginPage()
      await submitPasswordLogin(user)
      const button = await screen.findByTestId('passkey-login-button')
      await user.click(button)

      const reconsent = await screen.findByTestId('passkey-reconsent-view')
      expect(reconsent).toBeInTheDocument()
      expect(screen.getByTestId('passkey-reconsent-agreement-terms_of_service')).toBeInTheDocument()
      expect(
        screen.getByTestId('passkey-reconsent-agreement-terms_of_service-version')
      ).toHaveTextContent('2')
      expect(screen.getByTestId('passkey-agree-and-continue-button')).toBeInTheDocument()
    })

    it('GIVEN first-factor passkey verify returns consentRequired WHEN verifying THEN should show inline re-consent (handled in PasskeyLoginForm)', async () => {
      const credential = {
        id: 'cred-1',
        rawId: new TextEncoder().encode('raw-id').buffer,
        type: 'public-key',
        response: {
          authenticatorData: new TextEncoder().encode('auth').buffer,
          clientDataJSON: new TextEncoder().encode('cd').buffer,
          signature: new TextEncoder().encode('sig').buffer,
          userHandle: null,
        },
      } as unknown as PublicKeyCredential
      vi.stubGlobal('navigator', {
        credentials: { get: vi.fn().mockResolvedValue(credential), create: vi.fn() },
      })

      server.resetHandlers()
      server.use(
        http.post(`${API_BASE_URL}/api/auth/:realmId/login/passkey/options`, () =>
          HttpResponse.json({
            authToken: 'auth-1fa',
            options: { publicKey: { challenge: 'Y2hhbGxlbmdl' } },
          })
        ),
        http.post(`${API_BASE_URL}/api/auth/:realmId/login/passkey/verify`, () =>
          HttpResponse.json({
            userId: 'user-001',
            token: 'temp-token',
            message: 'consent required',
            expiresInSeconds: 3600,
            consentRequired: true,
            agreements: [makeAgreementSummary('privacy_policy', 'privacy-v3', 3)],
          })
        )
      )

      renderLoginPage()

      // The first-factor entry mounts on the login card; click Use Passkey.
      const usePasskey = await screen.findByTestId('passkey-login-button')
      await user.click(usePasskey)

      const agreement = await screen.findByTestId('passkey-reconsent-agreement-privacy_policy')
      expect(agreement).toBeInTheDocument()
      expect(screen.getByTestId('passkey-agree-and-continue-button')).toBeInTheDocument()
    })
  })
})
