import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { http, HttpResponse } from 'msw'
import { server } from '@/test/mocks/server'
import { getActiveHeraldClient } from '@/lib/herald-client'
import { Passkey2FaForm } from '../passkey-2fa-form'
import type {
  Passkey2FaOptionsResponse,
  PasskeyVerifyResponse,
  BrowserTokenResponse,
  LegalAgreementSummary,
} from '@/lib/api-generated'

/**
 * Passkey second-factor form (FE-D04). Holds a tempToken (password already
 * verified). On mount it fetches 2fa/options, then "Use Passkey" calls
 * navigator.credentials.get → serializeAssertion → 2fa/verify. The "Use TOTP
 * instead" link is shown only when secondFactors also includes 'totp'.
 */

// The re-consent view renders <Link> (from @tanstack/react-router) via
// AgreementLinks; without a RouterProvider the router's <Link> throws. Stub a
// minimal <Link> (matches the pattern used by totp-consent.test.tsx).
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

function makeMockAssertionCredential(): PublicKeyCredential {
  return {
    id: 'cred-1',
    rawId: new TextEncoder().encode('raw-id').buffer,
    type: 'public-key',
    response: {
      authenticatorData: new TextEncoder().encode('auth-data').buffer,
      clientDataJSON: new TextEncoder().encode('client-data').buffer,
      signature: new TextEncoder().encode('sig').buffer,
      userHandle: null,
    } as unknown as AuthenticatorAssertionResponse,
  } as PublicKeyCredential
}

const mock2FaOptionsResponse: Passkey2FaOptionsResponse = {
  authToken: 'auth-token-2fa',
  options: {
    publicKey: {
      challenge: 'Y2hhbGxlbmdl', // base64url("challenge")
      allowCredentials: [],
    },
  },
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

/**
 * Success body per the real backend contract (DEC-js-sdk-011): 2FA passkey
 * verify answers 200 with a `BrowserTokenResponse`; the SDK applies the token
 * set and the form surfaces completion via `onSuccess`.
 */
function makeSuccessResponse(): BrowserTokenResponse {
  return {
    accessToken: 'at-passkey-2fa',
    refreshToken: 'rt-passkey-2fa',
    tokenType: 'Bearer',
    expiresIn: 900,
    refreshExpiresIn: 2592000,
  }
}

function makeConsentRequiredResponse(agreements: LegalAgreementSummary[]): PasskeyVerifyResponse {
  return {
    userId: 'user-001',
    token: 'temp-token',
    message: 'Consent required',
    expiresInSeconds: 3600,
    consentRequired: true,
    agreements,
  }
}

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
}

function renderForm(props: {
  secondFactors?: string[] | null
  onSuccess?: (r: PasskeyVerifyResponse) => void
  onBack?: () => void
  onSwitchToTotp?: () => void
}) {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <Passkey2FaForm
        realmId="test-realm"
        tempToken="temp-token-2fa"
        secondFactors={props.secondFactors}
        onSuccess={props.onSuccess ?? vi.fn()}
        onBack={props.onBack}
        onSwitchToTotp={props.onSwitchToTotp}
      />
    </QueryClientProvider>
  )
}

function stubWebAuthnSupport(supported: boolean) {
  if (supported) {
    Object.defineProperty(window, 'PublicKeyCredential', {
      value: function PublicKeyCredential() {},
      configurable: true,
      writable: true,
    })
  } else {
    // @ts-expect-error — intentionally delete the browser global.
    delete (window as { PublicKeyCredential?: unknown }).PublicKeyCredential
  }
}

describe('Passkey2FaForm (second factor)', () => {
  const user = userEvent.setup({ delay: null })
  let optionsStatus: number
  let verifyResponse: BrowserTokenResponse | PasskeyVerifyResponse
  let verifyStatus: number
  let verifyBodies: unknown[]
  let getMock: ReturnType<typeof vi.fn>

  beforeEach(() => {
    optionsStatus = 200
    verifyStatus = 200
    verifyResponse = makeSuccessResponse()
    verifyBodies = []
    getMock = vi.fn()
    // Token state lives in the Herald SDK client now — reset it between cases.
    getActiveHeraldClient()?.tokens.clear()
    getMock.mockResolvedValue(null)

    stubWebAuthnSupport(true)
    vi.stubGlobal('navigator', {
      credentials: { get: getMock, create: vi.fn() },
    })

    server.resetHandlers()
    server.use(
      http.post(`${API_BASE_URL}/api/auth/:realmId/login/passkey/2fa/options`, () => {
        if (optionsStatus !== 200) {
          return HttpResponse.json({ error: 'options failed' }, { status: optionsStatus })
        }
        return HttpResponse.json(mock2FaOptionsResponse)
      }),
      http.post(
        `${API_BASE_URL}/api/auth/:realmId/login/passkey/2fa/verify`,
        async ({ request }) => {
          verifyBodies.push(await request.json())
          if (verifyStatus !== 200) {
            return HttpResponse.json({ error: 'verify failed' }, { status: verifyStatus })
          }
          return HttpResponse.json(verifyResponse)
        }
      )
    )
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  describe('rendering + options fetch', () => {
    it('GIVEN form mounts WHEN WebAuthn supported THEN should fetch 2fa options and render the use-passkey button', async () => {
      renderForm({ secondFactors: ['passkey'] })

      // options must be fetched before the button enables.
      await waitFor(() => {
        expect(screen.getByTestId('passkey-login-button')).not.toBeDisabled()
      })
      expect(screen.getByTestId('passkey-2fa-form')).toBeInTheDocument()
    })

    it.each([
      { label: 'only passkey', factors: ['passkey'], shouldShow: false },
      { label: 'passkey + totp', factors: ['passkey', 'totp'], shouldShow: true },
      { label: 'passkey + unknown', factors: ['passkey', 'sms'], shouldShow: false },
    ])(
      'GIVEN secondFactors=$label WHEN rendering THEN should show Use TOTP link = $shouldShow',
      async ({ factors, shouldShow }) => {
        renderForm({ secondFactors: factors, onSwitchToTotp: vi.fn() })

        await screen.findByTestId('passkey-2fa-form')
        if (shouldShow) {
          expect(screen.getByTestId('passkey-use-totp-link')).toBeInTheDocument()
        } else {
          expect(screen.queryByTestId('passkey-use-totp-link')).not.toBeInTheDocument()
        }
      }
    )

    it('GIVEN onSwitchToTotp is undefined WHEN rendering THEN should never show the TOTP link even with totp factor', async () => {
      renderForm({ secondFactors: ['passkey', 'totp'] })
      await screen.findByTestId('passkey-2fa-form')
      expect(screen.queryByTestId('passkey-use-totp-link')).not.toBeInTheDocument()
    })
  })

  describe('verify flow', () => {
    it('GIVEN user clicks Use Passkey WHEN credential selected THEN should POST 2fa/verify with tempToken + authToken + assertion', async () => {
      getMock.mockResolvedValue(makeMockAssertionCredential())
      const onSuccess = vi.fn()
      renderForm({ secondFactors: ['passkey'], onSuccess })

      await screen.findByTestId('passkey-login-button')
      await user.click(screen.getByTestId('passkey-login-button'))

      await waitFor(() => {
        expect(onSuccess).toHaveBeenCalledTimes(1)
      })
      // The success branch applied the issued token set inside the Herald SDK.
      expect(getActiveHeraldClient()?.tokens.getAccessToken()).toBe('at-passkey-2fa')

      const body = verifyBodies[0] as {
        tempToken: string
        authToken: string
        assertion: { rawId: string; response: { signature: string } }
      }
      expect(body.tempToken).toBe('temp-token-2fa')
      expect(body.authToken).toBe('auth-token-2fa')
      expect(body.assertion.rawId).not.toMatch(/=/) // base64url unpadded
    })

    it('GIVEN verify returns 401 WHEN verifying THEN should show the unified verification error', async () => {
      verifyStatus = 401
      getMock.mockResolvedValue(makeMockAssertionCredential())
      renderForm({ secondFactors: ['passkey'] })

      await screen.findByTestId('passkey-login-button')
      await user.click(screen.getByTestId('passkey-login-button'))

      expect(await screen.findByTestId('passkey-verification-error')).toBeInTheDocument()
    })

    it('GIVEN user dismisses the native prompt WHEN get rejects THEN should stay silent (no error)', async () => {
      getMock.mockRejectedValue(new DOMException('Abort', 'AbortError'))
      renderForm({ secondFactors: ['passkey'] })

      await screen.findByTestId('passkey-login-button')
      await user.click(screen.getByTestId('passkey-login-button'))

      await waitFor(() => {
        expect(getMock).toHaveBeenCalled()
      })
      await new Promise((resolve) => setTimeout(resolve, 0))
      expect(screen.queryByTestId('passkey-verification-error')).not.toBeInTheDocument()
    })
  })

  describe('TOTP switch fallback', () => {
    it('GIVEN user clicks Use TOTP WHEN both factors present THEN should call onSwitchToTotp', async () => {
      const onSwitchToTotp = vi.fn()
      renderForm({ secondFactors: ['passkey', 'totp'], onSwitchToTotp })

      await screen.findByTestId('passkey-2fa-form')
      await user.click(screen.getByTestId('passkey-use-totp-link'))

      expect(onSwitchToTotp).toHaveBeenCalledTimes(1)
    })
  })

  describe('unsupported browser', () => {
    it('GIVEN browser does not support WebAuthn WHEN rendering THEN should show unsupported message and the TOTP fallback when available', async () => {
      stubWebAuthnSupport(false)
      renderForm({
        secondFactors: ['passkey', 'totp'],
        onSwitchToTotp: vi.fn(),
        onBack: vi.fn(),
      })

      expect(screen.getByTestId('passkey-unsupported-message')).toBeInTheDocument()
      expect(screen.getByTestId('passkey-use-totp-link')).toBeInTheDocument()
      expect(screen.getByTestId('passkey-use-password-link')).toBeInTheDocument()
      expect(screen.queryByTestId('passkey-login-button')).not.toBeInTheDocument()
    })
  })

  describe('consent interlock (second factor)', () => {
    it('GIVEN verify returns consentRequired + agreements WHEN verifying THEN should show re-consent view', async () => {
      verifyResponse = makeConsentRequiredResponse([
        makeAgreementSummary('privacy_policy', 'privacy-v3', 3),
      ])
      getMock.mockResolvedValue(makeMockAssertionCredential())
      renderForm({ secondFactors: ['passkey'] })

      await screen.findByTestId('passkey-login-button')
      await user.click(screen.getByTestId('passkey-login-button'))

      await screen.findByTestId('passkey-reconsent-view')
      expect(screen.getByTestId('passkey-reconsent-agreement-privacy_policy')).toBeInTheDocument()
      expect(
        screen.getByTestId('passkey-reconsent-agreement-privacy_policy-version')
      ).toHaveTextContent('3')
      expect(screen.getByTestId('passkey-agree-and-continue-button')).toBeInTheDocument()
    })

    it('GIVEN user agrees to re-consent WHEN retrying THEN should replay assertion with agreements and call onSuccess', async () => {
      let verifyCall = 0
      server.use(
        http.post(
          `${API_BASE_URL}/api/auth/:realmId/login/passkey/2fa/verify`,
          async ({ request }) => {
            verifyCall += 1
            verifyBodies.push(await request.json())
            const body =
              verifyCall === 1
                ? makeConsentRequiredResponse([
                    makeAgreementSummary('terms_of_service', 'tos-v2', 2),
                  ])
                : makeSuccessResponse()
            return HttpResponse.json(body)
          }
        )
      )
      getMock.mockResolvedValue(makeMockAssertionCredential())
      const onSuccess = vi.fn()
      renderForm({ secondFactors: ['passkey'], onSuccess })

      await screen.findByTestId('passkey-login-button')
      await user.click(screen.getByTestId('passkey-login-button'))
      await screen.findByTestId('passkey-agree-and-continue-button')
      await user.click(screen.getByTestId('passkey-agree-and-continue-button'))

      await waitFor(() => {
        expect(onSuccess).toHaveBeenCalledTimes(1)
      })
      // The post-consent success applied the token set inside the Herald SDK.
      expect(getActiveHeraldClient()?.tokens.getAccessToken()).toBe('at-passkey-2fa')

      const second = verifyBodies[1] as {
        agreements?: Array<{ agreementType: string; versionId: string }>
      }
      expect(second.agreements).toEqual([
        { agreementType: 'terms_of_service', versionId: 'tos-v2' },
      ])
    })

    it('GIVEN user declines re-consent WHEN clicking decline THEN should call onBack', async () => {
      verifyResponse = makeConsentRequiredResponse([
        makeAgreementSummary('terms_of_service', 'tos-v2', 2),
      ])
      getMock.mockResolvedValue(makeMockAssertionCredential())
      const onBack = vi.fn()
      renderForm({ secondFactors: ['passkey'], onBack })

      await screen.findByTestId('passkey-login-button')
      await user.click(screen.getByTestId('passkey-login-button'))
      await screen.findByTestId('passkey-decline-back-button')
      await user.click(screen.getByTestId('passkey-decline-back-button'))

      expect(onBack).toHaveBeenCalledTimes(1)
    })
  })
})
