/**
 * SmsOtpLoginForm component test.
 *
 * Covers the state machine branches the route/Demo cannot cheaply reach
 * (the Demo only walks the happy path):
 *   - phone step renders + send success advances to the code step with the
 *     backend-derived masked number and a countdown
 *   - 409 `phone_not_registered` → guidance + register link, and NO consent
 *     branch (SMS has no auto-register, DEC-phone-login-001)
 *   - send 502 → localized channel-failure fallback (explicit, never fake
 *     success)
 *   - verify 429 → localized rate-limit copy
 *   - verify 401 (wrong code) → backend message verbatim, retry enabled
 *   - verify success → `onSuccess()` (the Herald SDK applied the token set)
 *
 * The Herald SDK client is mocked with `vi.mock('@/lib/herald-client')`
 * exposing controllable `loginWithSmsOtp.send` / `.verify` mocks. The phone
 * input is deliberately loose (non-empty only): the server is the format
 * authority, so there is intentionally NO client-side format rejection to
 * assert — a "too strict here" regression would surface as divergent error
 * copy vs the server's uniform invalid_phone (DEC-phone-login-004).
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, waitFor, act, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { SmsOtpLoginForm } from '../sms-otp-login-form'

const sendMock = vi.fn()
const verifyMock = vi.fn()
const bindClientIdMock = vi.fn()

const heraldFake = {
  tokens: { bindClientId: bindClientIdMock },
  loginWithSmsOtp: { send: sendMock, verify: verifyMock },
}

vi.mock('@/lib/herald-client', () => ({
  ensureHeraldClient: () => heraldFake,
  getActiveHeraldClient: () => heraldFake,
  applyTokenSet: vi.fn(),
  bindHeraldClientId: vi.fn(),
}))

// Token counter for the widget mock. Auto-incrementing (instead of a constant)
// is deliberate: the verify path must carry the token issued by the CURRENT
// widget mount — a constant token would pass even if the form submitted a
// stale/consumed one, so the tests below could not lock the freshness rule.
let mockTokenCounter = 0

vi.mock('../turnstile-widget', () => ({
  TurnstileWidget: ({ onTokenChange }: { onTokenChange: (t: string | null) => void }) => (
    <div data-testid="turnstile-widget-mock">
      {/* type="button": the widget sits inside the login <form>; a default
      submit button here would fire the form submit as a side effect. */}
      <button type="button" onClick={() => onTokenChange(`turnstile-token-${++mockTokenCounter}`)}>
        arm
      </button>
    </div>
  ),
}))

vi.mock('react-otp-input', () => ({
  __esModule: true,
  default: ({ value, onChange }: { value: string; onChange: (v: string) => void }) => (
    <input
      type="text"
      inputMode="numeric"
      maxLength={6}
      value={value}
      onChange={(e) => onChange(e.target.value)}
    />
  ),
}))

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

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  })
}

function renderForm(overrides?: {
  turnstileStatus?: { enabled: boolean; siteKey?: string | null } | null
  onSuccess?: ReturnType<typeof vi.fn>
  onBack?: ReturnType<typeof vi.fn>
  onSecondFactorRequired?: ReturnType<typeof vi.fn>
}) {
  const onSuccess = overrides?.onSuccess ?? vi.fn()
  const onBack = overrides?.onBack ?? vi.fn()
  const onSecondFactorRequired = overrides?.onSecondFactorRequired ?? vi.fn()
  const queryClient = createTestQueryClient()
  const view = render(
    <QueryClientProvider client={queryClient}>
      <SmsOtpLoginForm
        realmId="test-realm"
        clientId="admin-web-console"
        turnstileStatus={overrides?.turnstileStatus ?? null}
        onSuccess={onSuccess}
        onSecondFactorRequired={onSecondFactorRequired}
        onBack={onBack}
        registerPath="/test-realm/auth/register"
      />
    </QueryClientProvider>
  )
  return { ...view, onSuccess, onBack, onSecondFactorRequired }
}

const PHONE = ' 13800138000 '

async function typePhoneAndSend(user: ReturnType<typeof userEvent.setup>) {
  await user.type(screen.getByTestId('phone-otp-phone-input'), PHONE)
  await user.click(screen.getByTestId('phone-otp-send-btn'))
}

describe('SmsOtpLoginForm', () => {
  const user = userEvent.setup({ delay: null })

  beforeEach(() => {
    sendMock.mockReset()
    verifyMock.mockReset()
    mockTokenCounter = 0
  })

  afterEach(() => {
    act(() => {
      vi.useRealTimers()
    })
  })

  it('renders the phone input and send button on mount', () => {
    renderForm()
    expect(screen.getByTestId('phone-otp-phone-input')).toBeInTheDocument()
    expect(screen.getByTestId('phone-otp-send-btn')).toBeInTheDocument()
    expect(screen.queryByTestId('phone-otp-code-input')).not.toBeInTheDocument()
  })

  it('advances to the code step after a successful send and submits the trimmed phone', async () => {
    sendMock.mockResolvedValue({
      kind: 'sent',
      message: 'sent',
      expiresInSeconds: 300,
      maskedPhone: '138****8000',
    })

    renderForm()
    await typePhoneAndSend(user)

    expect(await screen.findByTestId('phone-otp-code-input')).toBeInTheDocument()
    // The masked number comes from the backend; the form displays it verbatim
    // and never derives or echoes the raw input.
    expect(screen.getByText(/138\*\*\*\*8000/)).toBeInTheDocument()
    expect(screen.getByTestId('phone-otp-verify-btn')).toBeInTheDocument()
    expect(screen.getByTestId('phone-otp-resend-countdown')).toBeInTheDocument()
    expect(screen.queryByTestId('phone-otp-resend-btn')).not.toBeInTheDocument()

    await waitFor(() => {
      expect(sendMock).toHaveBeenCalledTimes(1)
    })
    expect(bindClientIdMock).toHaveBeenCalledWith('admin-web-console')
    expect(sendMock).toHaveBeenCalledWith({ phone: '13800138000' })
  })

  it('shows guidance and the register link on 409 phone_not_registered with no consent branch', async () => {
    sendMock.mockResolvedValue({
      kind: 'conflict',
      code: 'phone_not_registered',
      message: 'Phone number is not registered.',
    })

    renderForm()
    await typePhoneAndSend(user)

    const guidance = await screen.findByTestId('phone-otp-not-registered-message')
    expect(guidance).toHaveTextContent(/not registered/i)
    expect(screen.getByTestId('phone-otp-register-link')).toBeInTheDocument()
    // No code step, and explicitly no agreement/consent UI (DEC-001: no
    // auto-register branch exists for SMS).
    expect(screen.queryByTestId('phone-otp-code-input')).not.toBeInTheDocument()
    expect(screen.queryByTestId('phone-otp-agreement-terms_of_service')).not.toBeInTheDocument()
  })

  it('surfaces the localized channel-failure fallback on send 502', async () => {
    sendMock.mockRejectedValue(Object.assign(new Error('upstream send failed'), { status: 502 }))

    renderForm()
    await typePhoneAndSend(user)

    // A dead channel must read as an explicit failure — never a silent
    // success or a raw English upstream message.
    expect(await screen.findByTestId('phone-otp-error-message')).toHaveTextContent(
      /SMS sending failed/i
    )
    expect(screen.queryByTestId('phone-otp-code-input')).not.toBeInTheDocument()
  })

  it('surfaces the localized rate-limit copy on verify 429', async () => {
    sendMock.mockResolvedValue({
      kind: 'sent',
      message: 'sent',
      expiresInSeconds: 300,
      maskedPhone: '138****8000',
    })
    verifyMock.mockRejectedValue(Object.assign(new Error('Too Many Requests'), { status: 429 }))

    renderForm()
    await typePhoneAndSend(user)
    const codeRegion = await screen.findByTestId('phone-otp-code-input')
    const codeInput = codeRegion.querySelector('input') as HTMLInputElement
    await user.type(codeInput, '123456')
    await user.click(screen.getByTestId('phone-otp-verify-btn'))

    expect(await screen.findByTestId('phone-otp-error-message')).toHaveTextContent(
      /Too many attempts/i
    )
  })

  it('surfaces a verify 401 in the error region and keeps retry enabled', async () => {
    sendMock.mockResolvedValue({
      kind: 'sent',
      message: 'sent',
      expiresInSeconds: 300,
      maskedPhone: '138****8000',
    })
    verifyMock.mockRejectedValue(
      Object.assign(new Error('Invalid or expired verification code'), { status: 401 })
    )

    renderForm()
    await typePhoneAndSend(user)
    const codeRegion = await screen.findByTestId('phone-otp-code-input')
    const codeInput = codeRegion.querySelector('input') as HTMLInputElement
    await user.type(codeInput, '123456')
    await user.click(screen.getByTestId('phone-otp-verify-btn'))

    expect(await screen.findByTestId('phone-otp-error-message')).toHaveTextContent(
      /Invalid or expired verification code/i
    )
    // The dead code was cleared so a retry types a fresh one; verify stays usable.
    expect(screen.getByTestId('phone-otp-verify-btn')).toBeInTheDocument()
    expect((codeRegion.querySelector('input') as HTMLInputElement).value).toBe('')
  })

  it('notifies onSuccess (no payload — the SDK applied the token set) on verify success', async () => {
    sendMock.mockResolvedValue({
      kind: 'sent',
      message: 'sent',
      expiresInSeconds: 300,
      maskedPhone: '138****8000',
    })
    verifyMock.mockResolvedValue({ kind: 'success' })

    const { onSuccess } = renderForm()
    await typePhoneAndSend(user)
    const codeRegion = await screen.findByTestId('phone-otp-code-input')
    const codeInput = codeRegion.querySelector('input') as HTMLInputElement
    await user.type(codeInput, '654321')
    await user.click(screen.getByTestId('phone-otp-verify-btn'))

    await waitFor(() => {
      expect(onSuccess).toHaveBeenCalledTimes(1)
    })
    expect(onSuccess).toHaveBeenCalledWith()
  })

  it('hands the second-factor step to the route when verify answers requires-second-factor', async () => {
    // A user with TOTP/passkey enabled must complete the second factor before
    // a session is issued; the OTP alone must never complete the login
    // (US-PL-002). The form surfaces it via the callback
    // instead of treating it as success or an error.
    sendMock.mockResolvedValue({
      kind: 'sent',
      message: 'sent',
      expiresInSeconds: 300,
      maskedPhone: '138****8000',
    })
    verifyMock.mockResolvedValue({
      kind: 'requires-second-factor',
      tempToken: 'temp-token-1',
      secondFactors: ['totp'],
    })

    const { onSecondFactorRequired, onSuccess } = renderForm()
    await typePhoneAndSend(user)
    const codeRegion = await screen.findByTestId('phone-otp-code-input')
    const codeInput = codeRegion.querySelector('input') as HTMLInputElement
    await user.type(codeInput, '654321')
    await user.click(screen.getByTestId('phone-otp-verify-btn'))

    await waitFor(() => {
      expect(onSecondFactorRequired).toHaveBeenCalledTimes(1)
    })
    expect(onSecondFactorRequired).toHaveBeenCalledWith('temp-token-1', ['totp'])
    // The mutation must resolve the branch INSTEAD of the success completion:
    // a TanStack hook-level onSuccess firing here would run the route's
    // login completion (toast + hydration + navigation) on top of the 2FA
    // step and drop the temp token, breaking 2FA logins entirely.
    expect(onSuccess).not.toHaveBeenCalled()
    // No success, no error region: the route swaps in the second-factor step.
    expect(screen.queryByTestId('phone-otp-error-message')).not.toBeInTheDocument()
  })

  it('sends a fresh Turnstile token with verify when the widget is enabled', async () => {
    // Turnstile tokens are single-use and the backend enforces the check on
    // verify as well: the code step renders its own widget
    // and the verify call must carry the token from THAT mount — the
    // auto-incrementing mock makes a stale token observable.
    sendMock.mockResolvedValue({
      kind: 'sent',
      message: 'sent',
      expiresInSeconds: 300,
      maskedPhone: '138****8000',
    })
    verifyMock.mockResolvedValue({ kind: 'success' })

    renderForm({ turnstileStatus: { enabled: true, siteKey: 'site-key' } })
    // Phone step: arm → token-1; send consumes it.
    await user.click(
      within(screen.getByTestId('turnstile-widget-mock')).getByRole('button', { name: 'arm' })
    )
    await typePhoneAndSend(user)
    await screen.findByTestId('phone-otp-code-input')

    // Code step: fresh widget → arm issues token-2; verify must send token-2,
    // not the consumed token-1 and not a constant.
    await user.click(
      within(screen.getByTestId('turnstile-widget-mock')).getByRole('button', { name: 'arm' })
    )
    const codeRegion = screen.getByTestId('phone-otp-code-input')
    const codeInput = codeRegion.querySelector('input') as HTMLInputElement
    await user.type(codeInput, '654321')
    await user.click(screen.getByTestId('phone-otp-verify-btn'))

    await waitFor(() => {
      expect(verifyMock).toHaveBeenCalledTimes(1)
    })
    expect(verifyMock).toHaveBeenCalledWith({
      phone: '13800138000',
      code: '654321',
      turnstileToken: 'turnstile-token-2',
    })
  })

  it('issues a fresh Turnstile token after a resend so the retry verify carries the new mount token', async () => {
    // Resend consumes the code-step token too;
    // the widget is keyed by send attempt and must remount, otherwise the
    // solved widget never issues another token and verify 400s forever.
    sendMock.mockResolvedValue({
      kind: 'sent',
      message: 'sent',
      expiresInSeconds: 0, // no countdown → resend button immediately available
      maskedPhone: '138****8000',
    })
    verifyMock.mockResolvedValue({ kind: 'success' })

    renderForm({ turnstileStatus: { enabled: true, siteKey: 'site-key' } })
    // Phone step: arm → token-1; send consumes it.
    await user.click(
      within(screen.getByTestId('turnstile-widget-mock')).getByRole('button', { name: 'arm' })
    )
    await typePhoneAndSend(user)
    await screen.findByTestId('phone-otp-code-input')

    // Code step arm → token-2; resend consumes it and remounts the widget.
    await user.click(
      within(screen.getByTestId('turnstile-widget-mock')).getByRole('button', { name: 'arm' })
    )
    await user.click(screen.getByTestId('phone-otp-resend-btn'))
    await waitFor(() => {
      expect(sendMock).toHaveBeenCalledTimes(2)
    })
    // Resend sent the still-valid code-step token, not a cleared one.
    expect(sendMock).toHaveBeenLastCalledWith({
      phone: '13800138000',
      turnstileToken: 'turnstile-token-2',
    })

    // Fresh widget after resend: arm → token-3; verify must carry it.
    await user.click(
      within(screen.getByTestId('turnstile-widget-mock')).getByRole('button', { name: 'arm' })
    )
    const codeRegion = screen.getByTestId('phone-otp-code-input')
    const codeInput = codeRegion.querySelector('input') as HTMLInputElement
    await user.type(codeInput, '654321')
    await user.click(screen.getByTestId('phone-otp-verify-btn'))

    await waitFor(() => {
      expect(verifyMock).toHaveBeenCalledTimes(1)
    })
    expect(verifyMock).toHaveBeenCalledWith({
      phone: '13800138000',
      code: '654321',
      turnstileToken: 'turnstile-token-3',
    })
  })
})
