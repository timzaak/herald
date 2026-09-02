import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { http, HttpResponse } from 'msw'
import { server } from '@/test/mocks/server'
import { PasskeyRegisterForm } from '../passkey-register-form'
import type { BeginRegistrationResponse } from '@/lib/api-generated'

/**
 * Passkey registration form (FE-D03) — a two-step state machine:
 *   1. confirm password → POST registration/begin → navigator.credentials.create
 *   2. name device → POST registration/finish → onSuccess + list refresh
 *
 * We let the generated SDK run against MSW handlers so the begin/finish request
 * bodies are observable (mirrors totp-consent.test.tsx). `navigator.credentials`
 * and `window.PublicKeyCredential` are stubbed via vi.stubGlobal so the WebAuthn
 * helpers in passkey-utils operate on hand-rolled credential objects.
 */

const API_BASE_URL = 'http://localhost:3000'

/** Build a minimal fake PublicKeyCredential (attestation shape). */
function makeMockPublicKeyCredential(): PublicKeyCredential {
  const rawId = new TextEncoder().encode('cred-raw-id').buffer
  return {
    id: 'cred-raw-id',
    rawId,
    type: 'public-key',
    response: {
      clientDataJSON: new TextEncoder().encode('client-data').buffer,
      attestationObject: new TextEncoder().encode('attestation').buffer,
      getTransports: () => ['internal', 'hybrid'],
    } as unknown as AuthenticatorAttestationResponse,
  } as PublicKeyCredential
}

const mockBeginResponse: BeginRegistrationResponse = {
  regToken: 'reg-token-123',
  // Server options carry base64url challenge / user.id strings (camelCase).
  options: {
    publicKey: {
      challenge: 'Y2hhbGxlbmdl', // base64url("challenge")
      rp: { name: 'Herald' },
      user: {
        id: 'dXNlci0x', // base64url("user-1")
        name: 'user@example.com',
        displayName: 'User',
      },
      pubKeyCredParams: [{ type: 'public-key', alg: -7 }],
      attestation: 'none',
    },
  },
}

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
}

function renderForm(props: { onSuccess: () => void; onCancel: () => void }) {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <PasskeyRegisterForm onSuccess={props.onSuccess} onCancel={props.onCancel} />
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

describe('PasskeyRegisterForm', () => {
  const user = userEvent.setup({ delay: null })
  let beginStatus: number
  let beginBody: unknown
  let finishStatus: number
  let finishBodies: unknown[]
  let verifyCallCount: number
  let createMock: ReturnType<typeof vi.fn>

  beforeEach(() => {
    beginStatus = 200
    beginBody = undefined
    finishStatus = 200
    finishBodies = []
    verifyCallCount = 0
    createMock = vi.fn()

    stubWebAuthnSupport(true)
    vi.stubGlobal('navigator', {
      credentials: {
        create: createMock,
        get: vi.fn(),
      },
    })

    server.resetHandlers()
    server.use(
      // Reauth flow (bind_authenticator): begin → verify → single-use ticket.
      http.post(`${API_BASE_URL}/api/user/reauth`, () =>
        HttpResponse.json({ availableFactors: ['password'] })
      ),
      http.post(`${API_BASE_URL}/api/user/reauth/verify`, () => {
        verifyCallCount += 1
        // Each call returns a fresh single-use ticket. The backend consumes the
        // ticket on BOTH begin and finish, so the form MUST obtain a second,
        // distinct ticket for finish (regression guard).
        return HttpResponse.json({ reauthToken: `reauth-token-${verifyCallCount}`, expiresIn: 120 })
      }),
      http.post(`${API_BASE_URL}/api/user/passkey/registration/begin`, async ({ request }) => {
        beginBody = await request.json()
        if (beginStatus !== 200) {
          return HttpResponse.json({ error: 'begin failed' }, { status: beginStatus })
        }
        return HttpResponse.json(mockBeginResponse)
      }),
      http.post(`${API_BASE_URL}/api/user/passkey/registration/finish`, async ({ request }) => {
        finishBodies.push(await request.json())
        if (finishStatus !== 200) {
          return HttpResponse.json({ error: 'finish failed' }, { status: finishStatus })
        }
        return HttpResponse.json({})
      })
    )
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  describe('password confirmation step', () => {
    it('GIVEN user submits empty password WHEN submitting THEN should show a validation error', async () => {
      renderForm({ onSuccess: vi.fn(), onCancel: vi.fn() })

      await user.click(screen.getByTestId('passkey-register-submit-button'))

      await waitFor(() => {
        expect(screen.getByText(/required/i)).toBeInTheDocument()
      })
    })

    it('GIVEN user submits valid password WHEN submitting THEN should call begin with reauthToken in body', async () => {
      createMock.mockResolvedValue(makeMockPublicKeyCredential())
      renderForm({ onSuccess: vi.fn(), onCancel: vi.fn() })

      await user.type(screen.getByTestId('passkey-register-password-input'), 'password123')
      await user.click(screen.getByTestId('passkey-register-submit-button'))

      await waitFor(() => {
        // The first verify call returns `reauth-token-1`; begin carries it.
        expect(beginBody).toEqual({ reauthToken: 'reauth-token-1' })
      })
    })

    it('GIVEN begin succeeds WHEN create resolves THEN should call navigator.credentials.create with ArrayBuffer challenge', async () => {
      createMock.mockResolvedValue(makeMockPublicKeyCredential())
      renderForm({ onSuccess: vi.fn(), onCancel: vi.fn() })

      await user.type(screen.getByTestId('passkey-register-password-input'), 'password123')
      await user.click(screen.getByTestId('passkey-register-submit-button'))

      await waitFor(() => {
        expect(createMock).toHaveBeenCalledTimes(1)
      })
      const createArg = createMock.mock.calls[0][0] as CredentialCreationOptions
      const publicKey = createArg.publicKey as PublicKeyCredentialCreationOptions
      // challenge must be decoded from base64url into an ArrayBuffer
      expect(publicKey.challenge).toBeInstanceOf(ArrayBuffer)
      expect(new TextDecoder().decode(publicKey.challenge)).toBe('challenge')
      // user.id also decoded
      expect(publicKey.user?.id).toBeInstanceOf(ArrayBuffer)
    })
  })

  describe('naming step', () => {
    async function advanceToNameStep() {
      createMock.mockResolvedValue(makeMockPublicKeyCredential())
      renderForm({ onSuccess: vi.fn(), onCancel: vi.fn() })

      await user.type(screen.getByTestId('passkey-register-password-input'), 'password123')
      await user.click(screen.getByTestId('passkey-register-submit-button'))

      await waitFor(() => {
        expect(screen.getByTestId('passkey-rename-input')).toBeInTheDocument()
      })
    }

    it('GIVEN begin + create succeed WHEN advancing THEN should show the nickname input', async () => {
      await advanceToNameStep()
      expect(screen.getByTestId('passkey-rename-input')).toBeInTheDocument()
      expect(screen.getByTestId('passkey-register-submit-button')).toBeInTheDocument()
    })

    it('GIVEN user names the device WHEN finishing THEN should POST finish with regToken + base64url attestation fields', async () => {
      await advanceToNameStep()

      await user.type(screen.getByTestId('passkey-rename-input'), 'My YubiKey')
      await user.click(screen.getByTestId('passkey-register-submit-button'))

      await waitFor(() => {
        expect(finishBodies).toHaveLength(1)
      })
      const body = finishBodies[0] as {
        regToken: string
        attestation: {
          rawId: string
          type: string
          response: { clientDataJSON: string; attestationObject: string; transports?: string[] }
        }
        reauthToken: string
      }
      expect(body.regToken).toBe('reg-token-123')
      // base64url fields are present and unpadded (no '=' padding)
      expect(body.attestation.rawId).not.toMatch(/=/)
      expect(body.attestation.type).toBe('public-key')
      expect(body.attestation.response.clientDataJSON).not.toMatch(/=/)
      expect(body.attestation.response.attestationObject).not.toMatch(/=/)
      expect(body.attestation.response.transports).toEqual(['internal', 'hybrid'])
      // Regression guard: the backend consumes the reauth ticket on begin AND
      // finish, so finish must obtain its OWN fresh ticket. Assert the verify
      // endpoint was hit twice (once for begin, once for finish) and the finish
      // body carries the second ticket — not the begin ticket.
      expect(verifyCallCount).toBe(2)
      expect(body.reauthToken).toBe('reauth-token-2')
    })

    it('GIVEN finish succeeds WHEN completing THEN should call onSuccess', async () => {
      const onSuccess = vi.fn()
      createMock.mockResolvedValue(makeMockPublicKeyCredential())
      renderForm({ onSuccess, onCancel: vi.fn() })

      await user.type(screen.getByTestId('passkey-register-password-input'), 'password123')
      await user.click(screen.getByTestId('passkey-register-submit-button'))
      await waitFor(() => {
        expect(screen.getByTestId('passkey-rename-input')).toBeInTheDocument()
      })

      await user.type(screen.getByTestId('passkey-rename-input'), 'My YubiKey')
      await user.click(screen.getByTestId('passkey-register-submit-button'))

      await waitFor(() => {
        expect(onSuccess).toHaveBeenCalledTimes(1)
      })
    })

    it('GIVEN empty nickname WHEN finishing THEN should reject validation (zod min(1))', async () => {
      await advanceToNameStep()

      await user.click(screen.getByTestId('passkey-register-submit-button'))

      // finish must NOT have been called for an invalid nickname
      expect(finishBodies).toHaveLength(0)
    })

    it('GIVEN nickname exceeds 128 chars WHEN finishing THEN should reject validation (zod max(128))', async () => {
      await advanceToNameStep()

      await user.type(screen.getByTestId('passkey-rename-input'), 'x'.repeat(129))
      await user.click(screen.getByTestId('passkey-register-submit-button'))

      expect(finishBodies).toHaveLength(0)
    })
  })

  // NOTE on the begin/finish failure tests below: the form fires its mutations
  // fire-and-forget via `void mutation.mutate()` (where `mutate === mutateAsync`),
  // relying on the toast/UI for error feedback rather than awaiting the promise.
  // On a backend failure the promise rejects with the generic "Passkey
  // registration failed" message and has no `.catch()`. These tests assert the
  // *UI* outcome (no navigator.credentials.create / no onSuccess), not the
  // rejection itself, so the expected rejection is filtered out at the runner
  // level via `onUnhandledError` in vitest.config.ts — see that file for why
  // the legacy `errorOnUnhandledRejections` flag was insufficient.
  describe('error handling', () => {
    it('GIVEN browser is unsupported WHEN mounting THEN should show unsupported message and not render the form', async () => {
      stubWebAuthnSupport(false)
      renderForm({ onSuccess: vi.fn(), onCancel: vi.fn() })

      expect(screen.getByTestId('passkey-unsupported-message')).toBeInTheDocument()
      expect(screen.queryByTestId('passkey-register-password-input')).not.toBeInTheDocument()
    })

    it.each([
      ['401 bad password', 401],
      ['422 validation', 422],
      ['409 conflict', 409],
    ])(
      'GIVEN begin returns %s WHEN submitting THEN should show generic failure (no backend detail)',
      async (_label, status) => {
        beginStatus = status
        renderForm({ onSuccess: vi.fn(), onCancel: vi.fn() })

        await user.type(screen.getByTestId('passkey-register-password-input'), 'password123')
        await user.click(screen.getByTestId('passkey-register-submit-button'))

        // navigator.credentials.create must never be invoked on begin failure
        await waitFor(() => {
          expect(createMock).not.toHaveBeenCalled()
        })
      }
    )

    it('GIVEN finish fails WHEN completing THEN should not call onSuccess', async () => {
      finishStatus = 500
      createMock.mockResolvedValue(makeMockPublicKeyCredential())
      const onSuccess = vi.fn()
      renderForm({ onSuccess, onCancel: vi.fn() })

      await user.type(screen.getByTestId('passkey-register-password-input'), 'password123')
      await user.click(screen.getByTestId('passkey-register-submit-button'))
      await waitFor(() => {
        expect(screen.getByTestId('passkey-rename-input')).toBeInTheDocument()
      })

      await user.type(screen.getByTestId('passkey-rename-input'), 'My YubiKey')
      await user.click(screen.getByTestId('passkey-register-submit-button'))

      await waitFor(() => {
        expect(finishBodies).toHaveLength(1)
      })
      // Give the async onError a tick; onSuccess must remain uncalled.
      await new Promise((resolve) => setTimeout(resolve, 0))
      expect(onSuccess).not.toHaveBeenCalled()
    })
  })

  describe('cancel / abort', () => {
    it('GIVEN user cancels the native prompt WHEN create rejects THEN should stay silent (no error, no step change)', async () => {
      // navigator.credentials.create rejects (user dismissed the browser modal).
      createMock.mockRejectedValue(new DOMException('Abort', 'AbortError'))
      renderForm({ onSuccess: vi.fn(), onCancel: vi.fn() })

      await user.type(screen.getByTestId('passkey-register-password-input'), 'password123')
      await user.click(screen.getByTestId('passkey-register-submit-button'))

      // Still on the password step — silently swallowed, no verification error shown.
      await waitFor(() => {
        expect(createMock).toHaveBeenCalled()
      })
      expect(screen.getByTestId('passkey-register-password-input')).toBeInTheDocument()
      expect(screen.queryByTestId('passkey-rename-input')).not.toBeInTheDocument()
      expect(screen.queryByText(/verification failed/i)).not.toBeInTheDocument()
    })

    it('GIVEN user clicks cancel WHEN on confirm step THEN should call onCancel', async () => {
      const onCancel = vi.fn()
      renderForm({ onSuccess: vi.fn(), onCancel })

      await user.click(screen.getByTestId('passkey-register-cancel-button'))

      expect(onCancel).toHaveBeenCalledTimes(1)
    })
  })
})
