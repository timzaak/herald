/**
 * EmailOtpLoginForm component test (design §6.1, FE-D01 step 6).
 *
 * Covers the state machine branches the route/Demo cannot cheaply reach:
 *   - email step renders + send success advances to the code step
 *   - 409 `consent_required` → agreement gate + re-send with agreements
 *   - 409 `email_not_registered` → guidance + register link
 *   - verify 401 (wrong code) → error region, retry enabled
 *   - verify success → `onSuccess()` (the Herald SDK applied the token set)
 *
 * The Herald SDK client is mocked with `vi.mock('@/lib/herald-client')`
 * exposing controllable `loginWithEmailOtp.send` / `.verify` mocks
 * (DEC-js-sdk-014 result shapes). The component does not call `status2` or
 * `getTurnstileStatus` directly — the route owns those queries and passes
 * `turnstileStatus` down as a prop — so they are not needed in this isolated
 * component test.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, act } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { EmailOtpLoginForm } from '../email-otp-login-form'
import type { LegalAgreementSummary } from '@/lib/api-generated'

// --- Mocks ---------------------------------------------------------------

// `loginWithEmailOtp.send` / `.verify` are the only SDK surface the form
// invokes (via the email-otp-mutations hooks). Each returns the SDK result
// shape; the controller below flips responses per test.
const sendMock = vi.fn()
const verifyMock = vi.fn()
const bindClientIdMock = vi.fn()

const heraldFake = {
  tokens: { bindClientId: bindClientIdMock },
  loginWithEmailOtp: { send: sendMock, verify: verifyMock },
}

vi.mock('@/lib/herald-client', () => ({
  ensureHeraldClient: () => heraldFake,
  getActiveHeraldClient: () => heraldFake,
  applyTokenSet: vi.fn(),
  bindHeraldClientId: vi.fn(),
}))

// Mock the Turnstile widget so it never tries to load the real script.
vi.mock('../turnstile-widget', () => ({
  TurnstileWidget: ({ onTokenChange }: { onTokenChange: (t: string | null) => void }) => (
    <div data-testid="turnstile-widget-mock">
      <button onClick={() => onTokenChange('turnstile-token')}>arm</button>
    </div>
  ),
}))

// Mock react-otp-input with a single controlled input so the test exercises the
// form's onChange → verify path without coupling to the library's per-digit
// rendering/ref-focusing behaviour (testing boundary: don't test the library).
// The component assigns `email-otp-code-input` to the wrapper div; the mocked
// input here is the only textbox inside it, so the test types via role.
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

// TanStack Router's `Link` reads router context (`router`/`isServer`) which is
// not present in an isolated component render. Mock it with a plain anchor so
// the not-registered branch renders without a router provider (mirrors the
// totp-consent test pattern).
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

// --- Helpers -------------------------------------------------------------

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  })
}

function makeAgreement(
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

function renderForm(overrides?: {
  turnstileStatus?: { enabled: boolean; siteKey?: string | null } | null
  onSuccess?: ReturnType<typeof vi.fn>
  onBack?: ReturnType<typeof vi.fn>
}) {
  const onSuccess = overrides?.onSuccess ?? vi.fn()
  const onBack = overrides?.onBack ?? vi.fn()
  const queryClient = createTestQueryClient()
  const view = render(
    <QueryClientProvider client={queryClient}>
      <EmailOtpLoginForm
        realmId="test-realm"
        clientId="admin-web-console"
        turnstileStatus={overrides?.turnstileStatus ?? null}
        onSuccess={onSuccess}
        onBack={onBack}
        registerPath="/test-realm/auth/register"
      />
    </QueryClientProvider>
  )
  return { ...view, onSuccess, onBack }
}

const EMAIL = 'user@example.com'

async function typeEmailAndSend(user: ReturnType<typeof userEvent.setup>) {
  await user.type(screen.getByTestId('email-otp-email-input'), EMAIL)
  await user.click(screen.getByTestId('email-otp-send-btn'))
}

describe('EmailOtpLoginForm', () => {
  const user = userEvent.setup({ delay: null })

  beforeEach(() => {
    sendMock.mockReset()
    verifyMock.mockReset()
  })

  it('renders the email input and send button on mount', () => {
    renderForm()
    expect(screen.getByTestId('email-otp-email-input')).toBeInTheDocument()
    expect(screen.getByTestId('email-otp-send-btn')).toBeInTheDocument()
    expect(screen.queryByTestId('email-otp-code-input')).not.toBeInTheDocument()
  })

  it('advances to the code step and shows the resend countdown after a successful send', async () => {
    sendMock.mockResolvedValue({ kind: 'sent', message: 'sent', expiresInSeconds: 300 })

    renderForm()
    await typeEmailAndSend(user)

    expect(await screen.findByTestId('email-otp-code-input')).toBeInTheDocument()
    expect(screen.getByTestId('email-otp-verify-btn')).toBeInTheDocument()
    // Countdown is active right after send → resend button hidden, countdown shown.
    expect(screen.getByTestId('email-otp-resend-countdown')).toBeInTheDocument()
    expect(screen.queryByTestId('email-otp-resend-btn')).not.toBeInTheDocument()

    // The resolved product clientId was bound onto the SDK client for the
    // request, and send was called with the expected payload shape.
    await waitFor(() => {
      expect(sendMock).toHaveBeenCalledTimes(1)
    })
    expect(bindClientIdMock).toHaveBeenCalledWith('admin-web-console')
    expect(sendMock).toHaveBeenCalledWith({ email: EMAIL })
  })

  it('renders the agreement gate on 409 consent_required and re-sends with agreements', async () => {
    const agreements = [
      makeAgreement('terms_of_service', 'tos-v2', 2),
      makeAgreement('privacy_policy', 'privacy-v3', 3),
    ]
    sendMock.mockResolvedValueOnce({
      kind: 'conflict',
      code: 'consent_required',
      consentRequired: true,
      message: 'consent required',
      // The SDK's conflict branch carries normalized agreements with the raw
      // backend summaries (DEC-js-sdk-013/014).
      agreements: agreements.map((raw) => ({
        agreementType: raw.agreement_type,
        versionId: raw.version_id,
        raw,
      })),
    })
    // Second send (after agreeing) succeeds.
    sendMock.mockResolvedValueOnce({ kind: 'sent', message: 'sent', expiresInSeconds: 300 })

    renderForm()
    await typeEmailAndSend(user)

    // Agreement gate appears.
    expect(await screen.findByTestId('email-otp-agreement-terms_of_service')).toBeInTheDocument()
    expect(screen.getByTestId('email-otp-agreement-privacy_policy')).toBeInTheDocument()

    // Agree and continue → re-send with agreements built from the summaries.
    await user.click(screen.getByTestId('email-otp-agree-and-continue-button'))

    await waitFor(() => {
      expect(sendMock).toHaveBeenCalledTimes(2)
    })
    expect(sendMock).toHaveBeenCalledWith({
      email: EMAIL,
      agreements: [
        { agreementType: 'terms_of_service', versionId: 'tos-v2' },
        { agreementType: 'privacy_policy', versionId: 'privacy-v3' },
      ],
    })
    // After the successful re-send the code step is shown.
    await screen.findByTestId('email-otp-code-input')
  })

  it('shows guidance and the register link on 409 email_not_registered', async () => {
    sendMock.mockResolvedValue({
      kind: 'conflict',
      code: 'email_not_registered',
      consentRequired: false,
      agreements: [],
      message: 'Please register first.',
    })

    renderForm()
    await typeEmailAndSend(user)

    const guidance = await screen.findByTestId('email-otp-not-registered-message')
    expect(guidance).toHaveTextContent(/register first/i)
    expect(screen.getByTestId('email-otp-register-link')).toBeInTheDocument()
    // Code step must NOT be shown.
    expect(screen.queryByTestId('email-otp-code-input')).not.toBeInTheDocument()
  })

  it('surfaces a verify 401 in the error region and keeps retry enabled', async () => {
    sendMock.mockResolvedValue({ kind: 'sent', message: 'sent', expiresInSeconds: 300 })
    verifyMock.mockRejectedValue(
      Object.assign(new Error('Invalid or expired code.'), { status: 401 })
    )

    renderForm()
    await typeEmailAndSend(user)
    const codeRegion = await screen.findByTestId('email-otp-code-input')

    // Type the code into the (mocked single-input) OTP field and verify.
    const codeInput = codeRegion.querySelector('input') as HTMLInputElement
    await user.type(codeInput, '123456')
    await user.click(screen.getByTestId('email-otp-verify-btn'))

    expect(await screen.findByTestId('email-otp-error-message')).toHaveTextContent(
      /invalid or expired code/i
    )
    // Verify is still usable (not locked / hidden).
    expect(screen.getByTestId('email-otp-verify-btn')).toBeInTheDocument()
  })

  it('notifies onSuccess (no payload — the SDK applied the token set) on verify success', async () => {
    sendMock.mockResolvedValue({ kind: 'sent', message: 'sent', expiresInSeconds: 300 })
    verifyMock.mockResolvedValue({ kind: 'success' })

    const { onSuccess } = renderForm()
    await typeEmailAndSend(user)
    const codeRegion = await screen.findByTestId('email-otp-code-input')

    const codeInput = codeRegion.querySelector('input') as HTMLInputElement
    await user.type(codeInput, '654321')
    await user.click(screen.getByTestId('email-otp-verify-btn'))

    await waitFor(() => {
      expect(onSuccess).toHaveBeenCalledTimes(1)
    })
    expect(onSuccess).toHaveBeenCalledWith()
  })
})

// Silence React act() warnings from the resend-countdown interval so the
// test output stays focused on assertion failures.
afterEach(() => {
  act(() => {
    vi.useRealTimers()
  })
})
