/**
 * @vitest-environment jsdom
 *
 * Change-email dialog flow tests: the three-factor reauth step machine,
 * error-class routing (verify vs request), the sent panel, and the resend
 * restart. The mail-link confirm half of the flow is covered by
 * change-email-confirm.test.tsx; the gated entry visibility and the full
 * happy path are covered by the Playwright demo.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { ChangeEmailDialog } from '../change-email-dialog'

vi.mock('@/lib/api-generated', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/lib/api-generated')>()
  return {
    ...original,
    handleBeginReauth: vi.fn(),
    handleVerifyReauth: vi.fn(),
    changeEmailRequest: vi.fn(),
  }
})

vi.mock('@/lib/realm-routing', () => ({
  useResolvedRealmId: () => 'realm-001',
}))

import { handleBeginReauth, handleVerifyReauth, changeEmailRequest } from '@/lib/api-generated'

const mockBeginReauth = vi.mocked(handleBeginReauth)
const mockVerifyReauth = vi.mocked(handleVerifyReauth)
const mockChangeEmailRequest = vi.mocked(changeEmailRequest)

const PASSKEY_CHALLENGE = {
  challengeToken: 'challenge-token-1',
  options: { challenge: 'YWJj', rpId: 'example.com', allowCredentials: [] },
}

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  })
}

function renderDialog() {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ChangeEmailDialog currentEmail="current@example.com" onClose={vi.fn()} />
    </QueryClientProvider>
  )
}

/** Drive the dialog from mount through password verification to the email step. */
async function advanceToEmailStep(user: ReturnType<typeof userEvent.setup>) {
  await user.type(await screen.findByTestId('change-email-password-input'), 'correct-password')
  await user.click(screen.getByTestId('change-email-verify-button'))
  await screen.findByTestId('change-email-new-email-input')
}

/** Minimal WebAuthn credential for serializeAssertion round-trips. */
function fakeCredential() {
  const bytes = new Uint8Array([1, 2, 3])
  return {
    id: 'cred-1',
    rawId: bytes.buffer,
    type: 'public-key' as const,
    response: {
      authenticatorData: bytes.buffer,
      clientDataJSON: bytes.buffer,
      signature: bytes.buffer,
    },
  }
}

function mockCredentialsGet(impl: () => Promise<unknown>) {
  Object.defineProperty(window.navigator, 'credentials', {
    configurable: true,
    value: { get: vi.fn(impl) },
  })
}

describe('ChangeEmailDialog', () => {
  const user = userEvent.setup()

  beforeEach(() => {
    vi.clearAllMocks()
    mockBeginReauth.mockResolvedValue({
      data: { availableFactors: ['password'] },
      error: undefined,
    })
    mockVerifyReauth.mockResolvedValue({
      data: { reauthToken: 'reauth-token-123', expiresIn: 120 },
      error: undefined,
    })
    mockChangeEmailRequest.mockResolvedValue({ data: { message: 'ok' }, error: undefined })
  })

  describe('factor inventory (identity verification step)', () => {
    it('GIVEN several available factors WHEN the dialog opens THEN they render as radios with the first one selected', async () => {
      mockBeginReauth.mockResolvedValue({
        data: { availableFactors: ['password', 'totp', 'passkey'], challenge: PASSKEY_CHALLENGE },
        error: undefined,
      })

      renderDialog()

      expect(await screen.findByTestId('change-email-factor-password-radio')).toBeChecked()
      expect(screen.getByTestId('change-email-factor-totp-radio')).toBeInTheDocument()
      expect(screen.getByTestId('change-email-factor-passkey-radio')).toBeInTheDocument()
      // The default factor is the first one, so the password input shows.
      expect(screen.getByTestId('change-email-password-input')).toBeInTheDocument()
    })

    it('GIVEN only one available factor WHEN the dialog opens THEN the radio group is skipped and its input shows directly', async () => {
      renderDialog()

      await screen.findByTestId('change-email-password-input')
      expect(screen.queryByTestId('change-email-factor-password-radio')).not.toBeInTheDocument()
      expect(screen.queryByTestId('change-email-factor-totp-radio')).not.toBeInTheDocument()
    })

    it('GIVEN the totp radio is selected WHEN switching factors THEN the input follows the selection', async () => {
      mockBeginReauth.mockResolvedValue({
        data: { availableFactors: ['password', 'totp'], challenge: undefined },
        error: undefined,
      })

      renderDialog()
      await screen.findByTestId('change-email-factor-totp-radio')
      await user.click(screen.getByTestId('change-email-factor-totp-radio'))

      expect(screen.getByTestId('change-email-totp-input')).toBeInTheDocument()
      expect(screen.queryByTestId('change-email-password-input')).not.toBeInTheDocument()
    })

    it('GIVEN an account with no usable factor WHEN the dialog opens THEN it explains the dead end instead of offering verification', async () => {
      mockBeginReauth.mockResolvedValue({ data: { availableFactors: [] }, error: undefined })

      renderDialog()

      expect(await screen.findByTestId('change-email-no-factors')).toHaveTextContent(
        'no available verification method'
      )
      expect(screen.queryByTestId('change-email-verify-button')).not.toBeInTheDocument()
    })
  })

  describe('verification failures keep the user on the verify step', () => {
    it('GIVEN a wrong password WHEN verify returns 401 THEN the dialog stays on verification with the wrong-password message', async () => {
      renderDialog()
      await user.type(await screen.findByTestId('change-email-password-input'), 'wrong-password')
      mockVerifyReauth.mockResolvedValue({
        data: undefined,
        error: { message: 'Unauthorized', status: 401 },
      })

      await user.click(screen.getByTestId('change-email-verify-button'))

      expect(await screen.findByTestId('change-email-error-message')).toHaveTextContent(
        'The password you entered is incorrect.'
      )
      expect(screen.getByTestId('change-email-password-input')).toBeInTheDocument()
      expect(screen.queryByTestId('change-email-new-email-input')).not.toBeInTheDocument()
    })

    it('GIVEN an empty password WHEN submitting verification THEN the required message shows and no verify call fires', async () => {
      renderDialog()
      await screen.findByTestId('change-email-password-input')

      await user.click(screen.getByTestId('change-email-verify-button'))

      expect(await screen.findByTestId('change-email-error-message')).toHaveTextContent(
        'Please enter your current password to confirm.'
      )
      expect(mockVerifyReauth).not.toHaveBeenCalled()
    })
  })

  describe('request-step error routing', () => {
    // The request endpoint has no email-conflict precheck, so a 409 there is
    // always ticket-class: the flow must restart at verification, not claim
    // an email conflict.
    it('GIVEN the request fails with 409 WHEN submitting the new email THEN the dialog returns to verification and begins a fresh reauth', async () => {
      renderDialog()
      await advanceToEmailStep(user)
      mockChangeEmailRequest.mockResolvedValue({
        data: undefined,
        error: { message: 'Conflict', status: 409 },
      })

      await user.type(screen.getByTestId('change-email-new-email-input'), 'new@example.com')
      await user.click(screen.getByTestId('change-email-submit-button'))

      expect(await screen.findByTestId('change-email-error-message')).toHaveTextContent(
        'Your verification has expired. Please verify your identity again.'
      )
      expect(screen.getByTestId('change-email-password-input')).toBeInTheDocument()
      expect(screen.queryByTestId('change-email-new-email-input')).not.toBeInTheDocument()
      // The consumed ticket is useless: restart issues a new begin.
      await waitFor(() => expect(mockBeginReauth).toHaveBeenCalledTimes(2))
    })

    it('GIVEN the request is rate limited WHEN submitting the new email THEN the dialog stays on the email step with the wait hint', async () => {
      renderDialog()
      await advanceToEmailStep(user)
      mockChangeEmailRequest.mockResolvedValue({
        data: undefined,
        error: { message: 'Too Many Requests', status: 429 },
      })

      await user.type(screen.getByTestId('change-email-new-email-input'), 'new@example.com')
      await user.click(screen.getByTestId('change-email-submit-button'))

      expect(await screen.findByTestId('change-email-error-message')).toHaveTextContent(
        'Too many attempts. Please wait about 2 minutes and try again.'
      )
      expect(screen.getByTestId('change-email-new-email-input')).toBeInTheDocument()
    })

    it('GIVEN the new email equals the current one WHEN submitting THEN zod rejects it before any request fires', async () => {
      renderDialog()
      await advanceToEmailStep(user)

      await user.type(screen.getByTestId('change-email-new-email-input'), 'current@example.com')
      await user.click(screen.getByTestId('change-email-submit-button'))

      await waitFor(() => {
        expect(
          screen.getByText('The new email must be different from the current one.')
        ).toBeInTheDocument()
      })
      expect(mockChangeEmailRequest).not.toHaveBeenCalled()
    })
  })

  describe('sent panel and resend', () => {
    it('GIVEN the request succeeds WHEN submitting the new email THEN the sent panel explains the logged-in-browser step and the old-mailbox notice', async () => {
      renderDialog()
      await advanceToEmailStep(user)

      await user.type(screen.getByTestId('change-email-new-email-input'), 'new@example.com')
      await user.click(screen.getByTestId('change-email-submit-button'))

      expect(await screen.findByTestId('change-email-sent-panel')).toHaveTextContent(
        'A confirmation link has been sent to new@example.com.'
      )
      expect(screen.getByTestId('change-email-sent-panel')).toHaveTextContent(
        'The previous email address will receive a change notification.'
      )
      expect(mockChangeEmailRequest).toHaveBeenCalledWith({
        path: { realmId: 'realm-001' },
        body: { newEmail: 'new@example.com', reauthToken: 'reauth-token-123' },
      })
    })

    it('GIVEN the sent panel WHEN resending THEN verification restarts and the email step comes back prefilled', async () => {
      renderDialog()
      await advanceToEmailStep(user)
      await user.type(screen.getByTestId('change-email-new-email-input'), 'new@example.com')
      await user.click(screen.getByTestId('change-email-submit-button'))
      await screen.findByTestId('change-email-sent-panel')

      await user.click(screen.getByTestId('change-email-resend-button'))

      // The ticket is single-use, so resend goes back through verification.
      expect(await screen.findByTestId('change-email-password-input')).toBeInTheDocument()
      await user.type(screen.getByTestId('change-email-password-input'), 'correct-password')
      await user.click(screen.getByTestId('change-email-verify-button'))

      expect(await screen.findByTestId('change-email-new-email-input')).toHaveValue(
        'new@example.com'
      )
    })
  })

  describe('passkey factor', () => {
    it('GIVEN the passkey factor WHEN the authenticator returns a credential THEN verify carries the serialized assertion and the begin challenge token', async () => {
      mockBeginReauth.mockResolvedValue({
        data: { availableFactors: ['passkey'], challenge: PASSKEY_CHALLENGE },
        error: undefined,
      })
      mockCredentialsGet(async () => fakeCredential())

      renderDialog()
      await user.click(await screen.findByTestId('change-email-passkey-button'))

      await waitFor(() => expect(mockVerifyReauth).toHaveBeenCalledTimes(1))
      const body = mockVerifyReauth.mock.calls[0][0].body
      expect(body.factor).toBe('passkey')
      expect(body.passkeyAssertion?.challengeToken).toBe('challenge-token-1')
      expect(body.passkeyAssertion?.assertion).toMatchObject({ id: 'cred-1', type: 'public-key' })
    })

    it('GIVEN the user dismisses the native prompt WHEN passkey verification runs THEN nothing is submitted and no error appears', async () => {
      mockBeginReauth.mockResolvedValue({
        data: { availableFactors: ['passkey'], challenge: PASSKEY_CHALLENGE },
        error: undefined,
      })
      mockCredentialsGet(async () => null)

      renderDialog()
      await user.click(await screen.findByTestId('change-email-passkey-button'))

      await waitFor(() => {
        expect(screen.queryByTestId('change-email-error-message')).not.toBeInTheDocument()
      })
      expect(mockVerifyReauth).not.toHaveBeenCalled()
    })
  })
})
