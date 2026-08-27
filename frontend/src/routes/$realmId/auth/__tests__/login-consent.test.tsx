import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import type { LegalAgreementSummary, LoginRequestPayload } from '@/lib/api-generated'

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

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  })
}

function renderLoginPage() {
  const queryClient = createTestQueryClient()
  return render(
    <QueryClientProvider client={queryClient}>
      <LoginPage />
    </QueryClientProvider>
  )
}

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

function makeConsentRequiredResponse(agreements: LegalAgreementSummary[]) {
  return {
    response: {
      userId: 'user-001',
      realmId: 'test-realm',
      message: 'Consent required',
      expiresInSeconds: 3600,
      consentRequired: true,
      agreements,
    },
    redirectPath: '/user/profile',
  }
}

function makeSuccessResponse() {
  return {
    response: {
      userId: 'user-001',
      realmId: 'test-realm',
      message: 'Login successful',
      expiresInSeconds: 3600,
    },
    redirectPath: '/user/profile',
  }
}

describe('LoginPage consent-required branch', () => {
  const user = userEvent.setup({ delay: null })

  beforeEach(() => {
    vi.clearAllMocks()
    mockLoginFlow.mockReset()
  })

  it('shows consent statement with agreement links on the login form', async () => {
    renderLoginPage()

    const statement = await screen.findByTestId('login-consent-statement')
    expect(statement).toBeInTheDocument()
    expect(screen.getByTestId('terms-of-service-link')).toHaveAttribute(
      'href',
      '/test-realm/legal/terms_of_service'
    )
    expect(screen.getByTestId('privacy-policy-link')).toHaveAttribute(
      'href',
      '/test-realm/legal/privacy_policy'
    )
  })

  it('switches to re-consent view when login returns consent_required', async () => {
    mockLoginFlow.mockResolvedValueOnce(
      makeConsentRequiredResponse([
        makeAgreementSummary('terms_of_service', 'tos-v2', 2),
        makeAgreementSummary('privacy_policy', 'privacy-v3', 3),
      ])
    )

    renderLoginPage()
    await user.type(screen.getByTestId('email-input'), 'user@example.com')
    await user.type(screen.getByTestId('password-input'), 'password123')
    await user.click(screen.getByTestId('login-submit-button'))

    const reconsentView = await screen.findByTestId('login-reconsent-view')
    expect(reconsentView).toBeInTheDocument()
    expect(screen.getByTestId('login-reconsent-agreement-terms_of_service')).toBeInTheDocument()
    expect(screen.getByTestId('login-reconsent-agreement-privacy_policy')).toBeInTheDocument()
    expect(
      screen.getByTestId('login-reconsent-agreement-privacy_policy-version')
    ).toHaveTextContent('Version: 3')
    expect(screen.queryByTestId('login-form')).not.toBeInTheDocument()
  })

  it('retries login with current version ids on agree and continues through existing path', async () => {
    mockLoginFlow
      .mockResolvedValueOnce(
        makeConsentRequiredResponse([
          makeAgreementSummary('terms_of_service', 'tos-v2', 2),
          makeAgreementSummary('privacy_policy', 'privacy-v3', 3),
        ])
      )
      .mockResolvedValueOnce(makeSuccessResponse())

    renderLoginPage()
    await user.type(screen.getByTestId('email-input'), 'user@example.com')
    await user.type(screen.getByTestId('password-input'), 'password123')
    await user.click(screen.getByTestId('login-submit-button'))

    await screen.findByTestId('login-reconsent-view')
    await user.click(screen.getByTestId('login-agree-and-continue-button'))

    await waitFor(() => {
      expect(mockLoginFlow).toHaveBeenCalledTimes(2)
    })

    const secondPayload = mockLoginFlow.mock.calls[1][1] as LoginRequestPayload
    expect(secondPayload.agreements).toEqual([
      { agreementType: 'terms_of_service', versionId: 'tos-v2' },
      { agreementType: 'privacy_policy', versionId: 'privacy-v3' },
    ])
    expect(screen.queryByTestId('login-reconsent-view')).not.toBeInTheDocument()
  })

  it('returns to the login form and keeps user unauthenticated on decline', async () => {
    mockLoginFlow.mockResolvedValueOnce(
      makeConsentRequiredResponse([makeAgreementSummary('terms_of_service', 'tos-v2', 2)])
    )

    renderLoginPage()
    await user.type(screen.getByTestId('email-input'), 'user@example.com')
    await user.type(screen.getByTestId('password-input'), 'password123')
    await user.click(screen.getByTestId('login-submit-button'))

    await screen.findByTestId('login-reconsent-view')
    await user.click(screen.getByTestId('login-decline-back-button'))

    expect(screen.queryByTestId('login-reconsent-view')).not.toBeInTheDocument()
    expect(screen.getByTestId('login-form')).toBeInTheDocument()
    expect(mockLoginFlow).toHaveBeenCalledTimes(1)
  })
})
