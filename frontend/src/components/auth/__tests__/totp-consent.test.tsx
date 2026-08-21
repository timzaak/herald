import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { http, HttpResponse } from 'msw'
import { server } from '@/test/mocks/server'
import { getActiveHeraldClient } from '@/lib/herald-client'
import type {
  LegalAgreementSummary,
  VerifyTotpResponse,
  BrowserTokenResponse,
} from '@/lib/api-generated'
import { TotpVerificationForm } from '../totp-verification-form'

vi.mock('@tanstack/react-router', async () => {
  const actual =
    await vi.importActual<typeof import('@tanstack/react-router')>('@tanstack/react-router')
  return {
    ...actual,
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

const API_BASE_URL = 'http://localhost:3000'

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  })
}

function renderForm(props: {
  onSuccess?: (response: VerifyTotpResponse) => void
  onBack?: () => void
}) {
  const queryClient = createTestQueryClient()
  return render(
    <QueryClientProvider client={queryClient}>
      <TotpVerificationForm
        realmId="test-realm"
        tempToken="temp-token"
        onSuccess={props.onSuccess ?? vi.fn()}
        onBack={props.onBack}
      />
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

function makeConsentRequiredResponse(agreements: LegalAgreementSummary[]): VerifyTotpResponse {
  return {
    userId: 'user-001',
    token: 'temp-token',
    message: 'Consent required',
    expiresInSeconds: 3600,
    consentRequired: true,
    agreements,
  }
}

/**
 * Success body per the real backend contract (DEC-js-sdk-011): verify-totp
 * answers 200 with a `BrowserTokenResponse`; the SDK applies the token set and
 * the form surfaces completion via `onSuccess`.
 */
function makeSuccessResponse(): BrowserTokenResponse {
  return {
    accessToken: 'at-totp',
    refreshToken: 'rt-totp',
    tokenType: 'Bearer',
    expiresIn: 900,
    refreshExpiresIn: 2592000,
  }
}

describe('TotpVerificationForm consent-required branch', () => {
  const user = userEvent.setup({ delay: null })
  let currentResponse: VerifyTotpResponse | BrowserTokenResponse = makeConsentRequiredResponse([])
  let requestBodies: unknown[] = []

  beforeEach(() => {
    requestBodies = []
    currentResponse = makeSuccessResponse()
    // Token state lives in the Herald SDK client now — reset it between cases.
    getActiveHeraldClient()?.tokens.clear()
    server.resetHandlers()
    server.use(
      http.post(`${API_BASE_URL}/api/auth/:realmId/login/verify-totp`, async ({ request }) => {
        const body = await request.json()
        requestBodies.push(body)
        return HttpResponse.json(currentResponse)
      })
    )
  })

  it('shows re-consent view when verify-totp returns consent_required and retries with agreements on agree', async () => {
    currentResponse = makeConsentRequiredResponse([
      makeAgreementSummary('terms_of_service', 'tos-v2', 2),
      makeAgreementSummary('privacy_policy', 'privacy-v3', 3),
    ])

    const onSuccess = vi.fn()
    renderForm({ onSuccess })

    await user.type(screen.getByTestId('totp-verification-code-input'), '123456')

    const reconsentView = await screen.findByTestId('totp-reconsent-view')
    expect(reconsentView).toBeInTheDocument()
    expect(screen.getByTestId('totp-reconsent-agreement-terms_of_service')).toBeInTheDocument()
    expect(screen.getByTestId('totp-reconsent-agreement-privacy_policy-version')).toHaveTextContent(
      'Version: 3'
    )
    expect(screen.queryByTestId('totp-verification-code-input')).not.toBeInTheDocument()

    // Switch to the success response before agreeing
    currentResponse = makeSuccessResponse()
    await user.click(screen.getByTestId('totp-agree-and-continue-button'))

    await waitFor(() => {
      expect(onSuccess).toHaveBeenCalledTimes(1)
    })
    // The post-consent success applied the token set inside the Herald SDK.
    expect(getActiveHeraldClient()?.tokens.getAccessToken()).toBe('at-totp')

    expect(requestBodies).toHaveLength(2)
    const firstBody = requestBodies[0] as {
      code: string
      backupCode: null | string
      agreements?: unknown
    }
    expect(firstBody.code).toBe('123456')
    expect(firstBody.agreements).toBeUndefined()
    const secondBody = requestBodies[1] as {
      code: string
      backupCode: null | string
      agreements: Array<{ agreementType: string; versionId: string }>
    }
    expect(secondBody.code).toBe('123456')
    expect(secondBody.agreements).toEqual([
      { agreementType: 'terms_of_service', versionId: 'tos-v2' },
      { agreementType: 'privacy_policy', versionId: 'privacy-v3' },
    ])
  })

  it('calls onBack when user declines re-consent', async () => {
    currentResponse = makeConsentRequiredResponse([
      makeAgreementSummary('terms_of_service', 'tos-v2', 2),
    ])

    const onBack = vi.fn()
    renderForm({ onBack })

    await user.type(screen.getByTestId('totp-verification-code-input'), '123456')
    await screen.findByTestId('totp-reconsent-view')

    await user.click(screen.getByTestId('totp-decline-back-button'))

    expect(onBack).toHaveBeenCalled()
  })
})
