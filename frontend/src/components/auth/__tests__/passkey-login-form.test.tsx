import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { http, HttpResponse } from 'msw'
import { server } from '@/test/mocks/server'
import { getActiveHeraldClient } from '@/lib/herald-client'
import { PasskeyLoginForm } from '../passkey-login-form'
import type {
  PasskeyOptionsResponse,
  PasskeyVerifyResponse,
  BrowserTokenResponse,
  LegalAgreementSummary,
} from '@/lib/api-generated'

/**
 * Passkey first-factor login form (FE-D04). MSW serves the begin-options and
 * verify endpoints; navigator.credentials + window.PublicKeyCredential are
 * stubbed. The form mounts and immediately arms the conditional (autofill) UI,
 * then exposes an explicit "Use Passkey" button that re-arms with mediation
 * 'optional'. Consent interlock re-checks after every verify.
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

/** Build a minimal assertion-shape PublicKeyCredential for navigator.credentials.get. */
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

const mockOptionsResponse: PasskeyOptionsResponse = {
  authToken: 'auth-token-123',
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
 * Success body per the real backend contract (DEC-js-sdk-011): passkey verify
 * answers 200 with a `BrowserTokenResponse`; the SDK applies the token set and
 * the form surfaces completion via `onSuccess`.
 */
function makeSuccessResponse(): BrowserTokenResponse {
  return {
    accessToken: 'at-passkey-1fa',
    refreshToken: 'rt-passkey-1fa',
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

function renderForm(
  props: { onSuccess?: (r: PasskeyVerifyResponse) => void; onUnavailable?: () => void } = {}
) {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <PasskeyLoginForm
        realmId="test-realm"
        clientId="admin-web-console"
        onSuccess={props.onSuccess ?? vi.fn()}
        onUnavailable={props.onUnavailable}
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

describe('PasskeyLoginForm (first factor)', () => {
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

    stubWebAuthnSupport(true)
    // Both the conditional UI and the explicit button call navigator.credentials.get.
    // The conditional call is best-effort; we let it return null (pending) so the
    // explicit button path is exercised by tests.
    getMock.mockResolvedValue(null)
    vi.stubGlobal('navigator', {
      credentials: { get: getMock, create: vi.fn() },
    })

    server.resetHandlers()
    server.use(
      http.post(`${API_BASE_URL}/api/auth/:realmId/login/passkey/options`, () => {
        if (optionsStatus !== 200) {
          return HttpResponse.json({ error: 'options failed' }, { status: optionsStatus })
        }
        return HttpResponse.json(mockOptionsResponse)
      }),
      http.post(`${API_BASE_URL}/api/auth/:realmId/login/passkey/verify`, async ({ request }) => {
        verifyBodies.push(await request.json())
        if (verifyStatus !== 200) {
          return HttpResponse.json({ error: 'verify failed' }, { status: verifyStatus })
        }
        return HttpResponse.json(verifyResponse)
      })
    )
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  describe('availability', () => {
    it('GIVEN browser supports WebAuthn and realm exposes passkey WHEN mounting THEN should render the login button', async () => {
      renderForm()

      // Options resolve → button becomes enabled.
      await screen.findByTestId('passkey-login-button')
      expect(screen.getByTestId('passkey-login-form')).toBeInTheDocument()
    })

    it('GIVEN browser does not support WebAuthn WHEN mounting THEN should show unsupported message and call onUnavailable', async () => {
      stubWebAuthnSupport(false)
      const onUnavailable = vi.fn()
      renderForm({ onUnavailable })

      expect(screen.getByTestId('passkey-unsupported-message')).toBeInTheDocument()
      expect(screen.queryByTestId('passkey-login-button')).not.toBeInTheDocument()
      expect(onUnavailable).toHaveBeenCalled()
    })

    it('GIVEN realm has passkey disabled (options 404) WHEN mounting THEN should call onUnavailable silently', async () => {
      optionsStatus = 404
      const onUnavailable = vi.fn()
      renderForm({ onUnavailable })

      await waitFor(() => {
        expect(onUnavailable).toHaveBeenCalledTimes(1)
      })
      // No error surfaces — the password form remains usable.
      expect(screen.queryByTestId('passkey-verification-error')).not.toBeInTheDocument()
    })
  })

  describe('explicit Use Passkey flow', () => {
    it('GIVEN user clicks Use Passkey WHEN credential selected THEN should GET credential + POST verify with authToken + assertion', async () => {
      // Keep the conditional (autofill) UI pending (null) so the ONLY verify
      // comes from the explicit button click — isolates the click path.
      getMock.mockResolvedValueOnce(null)
      getMock.mockResolvedValueOnce(makeMockAssertionCredential())
      const onSuccess = vi.fn()
      renderForm({ onSuccess })

      // Wait for the begin challenge to load (conditional UI arms → getMock is
      // called) so the explicit button is enabled before clicking.
      await waitFor(() => {
        expect(getMock).toHaveBeenCalledTimes(1)
      })
      await user.click(screen.getByTestId('passkey-login-button'))

      await waitFor(() => {
        expect(getMock).toHaveBeenCalledTimes(2)
      })
      await waitFor(() => {
        expect(onSuccess).toHaveBeenCalledTimes(1)
      })
      // The success branch applied the issued token set inside the Herald SDK.
      expect(getActiveHeraldClient()?.tokens.getAccessToken()).toBe('at-passkey-1fa')
      expect(getActiveHeraldClient()?.storage.getRefreshToken()).toBe('rt-passkey-1fa')

      // Verify body must carry authToken + serialised assertion (base64url fields).
      expect(verifyBodies).toHaveLength(1)
      const body = verifyBodies[0] as {
        authToken: string
        assertion: { rawId: string; response: { signature: string } }
      }
      expect(body.authToken).toBe('auth-token-123')
      expect(body.assertion.rawId).not.toMatch(/=/) // unpadded base64url
      expect(body.assertion.response.signature).not.toMatch(/=/)
    })

    it('GIVEN verify returns 401 WHEN verifying THEN should show the unified verification error (no backend detail)', async () => {
      verifyStatus = 401
      getMock.mockResolvedValue(makeMockAssertionCredential())
      renderForm()

      await screen.findByTestId('passkey-login-button')
      await user.click(screen.getByTestId('passkey-login-button'))

      const error = await screen.findByTestId('passkey-verification-error')
      expect(error).toBeInTheDocument()
    })

    it('GIVEN user dismisses the native prompt WHEN get rejects THEN should stay silent (no error)', async () => {
      getMock.mockRejectedValue(new DOMException('Abort', 'AbortError'))
      renderForm()

      await screen.findByTestId('passkey-login-button')
      await user.click(screen.getByTestId('passkey-login-button'))

      await waitFor(() => {
        expect(getMock).toHaveBeenCalled()
      })
      // No verification error surfaced for a user-initiated dismissal.
      await new Promise((resolve) => setTimeout(resolve, 0))
      expect(screen.queryByTestId('passkey-verification-error')).not.toBeInTheDocument()
    })

    it('GIVEN conditional UI is armed on mount WHEN it stays pending (null) THEN should not throw', async () => {
      // Default getMock resolves null (pending conditional UI). Just assert the
      // mount + begin does not error and the button remains available.
      renderForm()
      await screen.findByTestId('passkey-login-button')
      expect(screen.queryByTestId('passkey-verification-error')).not.toBeInTheDocument()
    })
  })

  describe('consent interlock (first factor)', () => {
    it('GIVEN verify returns consentRequired + agreements WHEN verifying THEN should show re-consent view', async () => {
      verifyResponse = makeConsentRequiredResponse([
        makeAgreementSummary('terms_of_service', 'tos-v2', 2),
      ])
      // Keep the conditional (autofill) UI pending (null) so only the explicit
      // button click resolves a credential — isolates the consent path.
      getMock.mockResolvedValueOnce(null)
      getMock.mockResolvedValueOnce(makeMockAssertionCredential())
      renderForm()

      // Wait for the begin challenge to load (conditional UI arms → getMock is
      // called) so the explicit button is enabled before clicking.
      await waitFor(() => {
        expect(getMock).toHaveBeenCalledTimes(1)
      })
      await user.click(screen.getByTestId('passkey-login-button'))

      await screen.findByTestId('passkey-reconsent-agreement-terms_of_service')
      expect(screen.getByTestId('passkey-reconsent-agreement-terms_of_service')).toBeInTheDocument()
      expect(
        screen.getByTestId('passkey-reconsent-agreement-terms_of_service-version')
      ).toHaveTextContent('2')
      expect(screen.getByTestId('passkey-agree-and-continue-button')).toBeInTheDocument()
      expect(screen.queryByTestId('passkey-login-button')).not.toBeInTheDocument()
    })

    it('GIVEN user agrees to re-consent WHEN retrying THEN should replay the assertion with agreements and call onSuccess', async () => {
      // First verify → consent required; second verify → success.
      let verifyCall = 0
      server.use(
        http.post(`${API_BASE_URL}/api/auth/:realmId/login/passkey/verify`, async ({ request }) => {
          verifyCall += 1
          verifyBodies.push(await request.json())
          const body =
            verifyCall === 1
              ? makeConsentRequiredResponse([makeAgreementSummary('terms_of_service', 'tos-v2', 2)])
              : makeSuccessResponse()
          return HttpResponse.json(body)
        })
      )
      // Keep the conditional (autofill) UI pending (null) so only the explicit
      // button click resolves a credential — isolates the consent path.
      getMock.mockResolvedValueOnce(null)
      getMock.mockResolvedValueOnce(makeMockAssertionCredential())
      const onSuccess = vi.fn()
      renderForm({ onSuccess })

      // Wait for the begin challenge to load (conditional UI arms → getMock is
      // called) so the explicit button is enabled before clicking.
      await waitFor(() => {
        expect(getMock).toHaveBeenCalledTimes(1)
      })
      await user.click(screen.getByTestId('passkey-login-button'))

      await screen.findByTestId('passkey-agree-and-continue-button')
      await user.click(screen.getByTestId('passkey-agree-and-continue-button'))

      await waitFor(() => {
        expect(onSuccess).toHaveBeenCalledTimes(1)
      })
      // The post-consent success applied the token set inside the Herald SDK.
      expect(getActiveHeraldClient()?.tokens.getAccessToken()).toBe('at-passkey-1fa')

      // Second verify must carry the agreements array.
      expect(verifyBodies).toHaveLength(2)
      const second = verifyBodies[1] as {
        authToken: string
        agreements?: Array<{ agreementType: string; versionId: string }>
      }
      expect(second.agreements).toEqual([
        { agreementType: 'terms_of_service', versionId: 'tos-v2' },
      ])
    })
  })
})
