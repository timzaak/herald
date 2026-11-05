/**
 * @vitest-environment jsdom
 *
 * Change-phone dialog flow tests: the reauth → phone → code step machine and
 * the error classes the Demo cannot cheaply reach (it injects codes via Redis
 * and only walks the happy path):
 *   - reauth success advances to the phone step; the ticket rides the bind
 *     send payload
 *   - send success advances to the code step with the backend masked number
 *   - 409 `phone_already_in_use` → localized in-use message, flow does NOT
 *     advance, and a fresh reauth begin fires (the ticket was consumed
 *     before the occupancy precheck)
 *   - ticket-class 409/401 → restart reauth
 *   - confirm success → done panel + userPhoneStatus invalidation
 *
 * The Playwright demo covers the full bind/rebind/unbind story; these tests
 * pin the dialog's branch contracts.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { ChangePhoneDialog } from '../change-phone-dialog'

vi.mock('@/lib/api-generated', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/lib/api-generated')>()
  return {
    ...original,
    handleBeginReauth: vi.fn(),
    handleVerifyReauth: vi.fn(),
    userPhoneBindSend: vi.fn(),
    userPhoneBindConfirm: vi.fn(),
  }
})

vi.mock('@/lib/realm-routing', () => ({
  useResolvedRealmId: () => 'realm-001',
}))

import {
  handleBeginReauth,
  handleVerifyReauth,
  userPhoneBindSend,
  userPhoneBindConfirm,
} from '@/lib/api-generated'

const mockBeginReauth = vi.mocked(handleBeginReauth)
const mockVerifyReauth = vi.mocked(handleVerifyReauth)
const mockBindSend = vi.mocked(userPhoneBindSend)
const mockBindConfirm = vi.mocked(userPhoneBindConfirm)

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  })
}

function renderDialog(currentMaskedPhone: string | null = null) {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ChangePhoneDialog currentMaskedPhone={currentMaskedPhone} onClose={vi.fn()} />
    </QueryClientProvider>
  )
}

/** Drive the dialog from mount through password verification to the phone step. */
async function advanceToPhoneStep(user: ReturnType<typeof userEvent.setup>) {
  await user.type(await screen.findByTestId('phone-bind-password-input'), 'correct-password')
  await user.click(screen.getByTestId('phone-bind-verify-button'))
  await screen.findByTestId('phone-bind-phone-input')
}

/** Drive from the phone step through a successful send to the code step. */
async function advanceToCodeStep(user: ReturnType<typeof userEvent.setup>) {
  await user.type(screen.getByTestId('phone-bind-phone-input'), '+8613800138000')
  await user.click(screen.getByTestId('phone-bind-send-btn'))
  await screen.findByTestId('phone-bind-code-input')
}

describe('ChangePhoneDialog', () => {
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
    mockBindSend.mockResolvedValue({
      data: { message: 'ok', expiresInSeconds: 300, maskedPhone: '138****8000' },
      error: undefined,
    })
    mockBindConfirm.mockResolvedValue({ data: { maskedPhone: '138****8000' }, error: undefined })
  })

  it('advances reauth → phone step and carries the ticket on the bind send', async () => {
    renderDialog()
    await advanceToPhoneStep(user)
    await advanceToCodeStep(user)

    expect(mockVerifyReauth).toHaveBeenCalledWith({
      body: expect.objectContaining({ targetOperation: 'change_phone', factor: 'password' }),
    })
    await waitFor(() => {
      expect(mockBindSend).toHaveBeenCalledWith({
        body: { phone: '+8613800138000', reauthToken: 'reauth-token-123' },
      })
    })
  })

  it('shows the backend masked number on the code step and confirms with the sent-to phone', async () => {
    renderDialog()
    await advanceToPhoneStep(user)
    await advanceToCodeStep(user)

    expect(screen.getByText(/138\*\*\*\*8000/)).toBeInTheDocument()

    const codeRegion = screen.getByTestId('phone-bind-code-input')
    const codeInput = codeRegion.querySelector('input') as HTMLInputElement
    await user.type(codeInput, '123456')
    await user.click(screen.getByTestId('phone-bind-confirm-btn'))

    expect(await screen.findByTestId('phone-bind-done-panel')).toBeInTheDocument()
    // Confirm binds the phone the code was actually sent to — the snapshot at
    // send time, not whatever the input holds now.
    expect(mockBindConfirm).toHaveBeenCalledWith({
      body: { phone: '+8613800138000', code: '123456' },
    })
  })

  it('shows the in-use message and restarts reauth on 409 phone_already_in_use', async () => {
    renderDialog()
    await advanceToPhoneStep(user)
    mockBindSend.mockResolvedValue({
      data: undefined,
      error: {
        code: 'phone_already_in_use',
        message: 'Phone number is already in use',
        status: 409,
      },
    })

    await user.type(screen.getByTestId('phone-bind-phone-input'), '13900139000')
    await user.click(screen.getByTestId('phone-bind-send-btn'))

    expect(await screen.findByTestId('phone-bind-error-message')).toHaveTextContent(
      'This phone number is already in use.'
    )
    // The binding never advanced to a code step…
    expect(screen.queryByTestId('phone-bind-code-input')).not.toBeInTheDocument()
    // …and the consumed ticket forces a fresh reauth begin.
    await waitFor(() => expect(mockBeginReauth).toHaveBeenCalledTimes(2))
  })

  it('restarts reauth on a ticket-class 409 without claiming a phone conflict', async () => {
    renderDialog()
    await advanceToPhoneStep(user)
    mockBindSend.mockResolvedValue({
      data: undefined,
      error: { message: 'Reauth ticket already consumed', status: 409 },
    })

    await user.type(screen.getByTestId('phone-bind-phone-input'), '13900139000')
    await user.click(screen.getByTestId('phone-bind-send-btn'))

    expect(await screen.findByTestId('phone-bind-error-message')).toHaveTextContent(
      'Verification expired. Please verify your identity again.'
    )
    expect(screen.queryByTestId('phone-bind-code-input')).not.toBeInTheDocument()
    await waitFor(() => expect(mockBeginReauth).toHaveBeenCalledTimes(2))
  })

  it('keeps the phone step with the rate-limit hint on 429', async () => {
    renderDialog()
    await advanceToPhoneStep(user)
    mockBindSend.mockResolvedValue({
      data: undefined,
      error: { message: 'Too Many Requests', status: 429 },
    })

    await user.type(screen.getByTestId('phone-bind-phone-input'), '13900139000')
    await user.click(screen.getByTestId('phone-bind-send-btn'))

    expect(await screen.findByTestId('phone-bind-error-message')).toHaveTextContent(
      'Too many attempts. Please try again later.'
    )
    expect(screen.getByTestId('phone-bind-phone-input')).toBeInTheDocument()
  })

  it('blocks the send button while the phone input is empty (non-empty is the only client rule)', async () => {
    renderDialog()
    await advanceToPhoneStep(user)

    expect(screen.getByTestId('phone-bind-send-btn')).toBeDisabled()
    expect(mockBindSend).not.toHaveBeenCalled()
  })
})
