import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { http, HttpResponse } from 'msw'
import { server } from '@/test/mocks/server'
import { SignupForm } from '../signup-form'

// The signup success path delegates to `completeSignup`, which performs real
// auth-data fetches. Mock the module so the test asserts the form's contract
// (body shape + onSuccess handoff) without those side effects.
vi.mock('@/lib/auth-utils', async () => {
  const actual = await vi.importActual<typeof import('@/lib/auth-utils')>('@/lib/auth-utils')
  return {
    ...actual,
    completeSignup: vi.fn(async (_realmId: string, _tokens: unknown, _clientId: string) => ({
      redirectPath: '/manage',
    })),
  }
})

// Turnstile widget is a third-party script; stub it so its render is observable.
vi.mock('@marsidev/react-turnstile', () => ({
  Turnstile: vi.fn(() => <div data-testid="turnstile-mock" />),
}))

import { completeSignup } from '@/lib/auth-utils'

// The signup POST's success body, shared by the default handler and the
// request-capturing overrides below (one copy so a response-shape change
// cannot leave the fixtures diverging).
const SIGNUP_OK_BODY = {
  accessToken: 'at-new',
  refreshToken: 'rt-new',
  expiresIn: 900,
  refreshExpiresIn: 2592000,
  tokenType: 'Bearer',
  realmId: 'new-realm-123',
  realmName: 'New Realm',
}

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  }
}

// Self-service signup is an admin-realm-only entry: every API call is fixed to
// `realmId='admin'` regardless of where the page is mounted. These tests assert
// that contract (DEC-001) plus the Turnstile binding to admin-web-console.
describe('SignupForm', () => {
  const mockOnSuccess = vi.fn()

  beforeEach(() => {
    mockOnSuccess.mockClear()
    vi.mocked(completeSignup).mockClear()
    server.use(
      // Turnstile status is a GET; default disabled so the widget stays hidden.
      http.get('/api/auth/:realmId/turnstile/status', () =>
        HttpResponse.json({ enabled: false, siteKey: null })
      ),
      // Signup status: default enabled with email verification OFF, so the
      // verification-code step stays hidden unless a test overrides it.
      http.get('/api/auth/:realmId/signup/status', () =>
        HttpResponse.json({ enabled: true, emailVerificationRequired: false })
      ),
      http.post('/api/auth/:realmId/signup', () => HttpResponse.json(SIGNUP_OK_BODY))
    )
  })

  async function renderForm() {
    const user = userEvent.setup({ delay: null })
    const view = render(<SignupForm onSuccess={mockOnSuccess} />, {
      wrapper: createWrapper(),
    })
    return { user, ...view }
  }

  async function fillRequiredFields(user: ReturnType<typeof userEvent.setup>) {
    await user.type(screen.getByTestId('signup-realm-name-input'), 'New Realm')
    await user.type(screen.getByTestId('signup-email-input'), 'admin@example.com')
    await user.type(screen.getByTestId('signup-password-input'), 'Password123!')
  }

  it('GIVEN valid input WHEN submitting THEN calls signup on the admin realm and hands the new realm to onSuccess', async () => {
    let observedBody: Record<string, unknown> | undefined
    server.use(
      http.post('/api/auth/:realmId/signup', async ({ request }) => {
        observedBody = (await request.json()) as Record<string, unknown>
        return HttpResponse.json(SIGNUP_OK_BODY)
      })
    )

    const { user } = await renderForm()
    await fillRequiredFields(user)
    await user.click(screen.getByTestId('signup-submit-button'))

    // DEC-001: the call targets the admin realm only.
    await waitFor(() => {
      expect(completeSignup).toHaveBeenCalledWith(
        'new-realm-123',
        { accessToken: 'at-new', refreshToken: 'rt-new' },
        'admin-web-console'
      )
    })
    // The new realm's id + safe redirect path are handed to the page.
    await waitFor(() => {
      expect(mockOnSuccess).toHaveBeenCalledWith('/manage', 'new-realm-123')
    })
    // Empty optional fields are normalized to null on the wire.
    expect(observedBody).toMatchObject({
      realmName: 'New Realm',
      realmSlug: null,
      email: 'admin@example.com',
      password: 'Password123!',
      turnstileToken: null,
    })
  })

  it('GIVEN a custom realm slug WHEN submitting THEN sends the slug on the wire', async () => {
    let observedBody: Record<string, unknown> | undefined
    server.use(
      http.post('/api/auth/:realmId/signup', async ({ request }) => {
        observedBody = (await request.json()) as Record<string, unknown>
        return HttpResponse.json({
          accessToken: 'at',
          refreshToken: 'rt',
          expiresIn: 900,
          refreshExpiresIn: 2592000,
          tokenType: 'Bearer',
          realmId: 'acme-corp',
          realmName: 'Acme',
        })
      })
    )

    const { user } = await renderForm()
    await fillRequiredFields(user)
    await user.type(screen.getByTestId('signup-realm-slug-input'), 'acme-corp')
    await user.click(screen.getByTestId('signup-submit-button'))

    await waitFor(() => {
      expect(observedBody).toMatchObject({ realmSlug: 'acme-corp' })
    })
  })

  it('GIVEN signup returns a quota error WHEN submitting THEN does not hydrate or navigate and surfaces the error', async () => {
    server.use(
      http.post('/api/auth/:realmId/signup', () =>
        HttpResponse.json({ message: 'Signup limit reached for your IP' }, { status: 429 })
      )
    )

    const { user } = await renderForm()
    await fillRequiredFields(user)
    await user.click(screen.getByTestId('signup-submit-button'))

    await waitFor(() => {
      expect(completeSignup).not.toHaveBeenCalled()
    })
    expect(mockOnSuccess).not.toHaveBeenCalled()
  })

  it('GIVEN Turnstile is enabled for admin-web-console WHEN rendering THEN shows the security verification widget', async () => {
    server.use(
      http.get('/api/auth/:realmId/turnstile/status', () =>
        HttpResponse.json({ enabled: true, siteKey: '1x00000000000000000000AA' })
      )
    )

    await renderForm()

    await waitFor(() => {
      expect(screen.getByTestId('turnstile-mock')).toBeInTheDocument()
    })
  })

  it('GIVEN email verification is not required WHEN submitting THEN renders no code UI and the body carries no code', async () => {
    let observedBody: Record<string, unknown> | undefined
    server.use(
      http.post('/api/auth/:realmId/signup', async ({ request }) => {
        observedBody = (await request.json()) as Record<string, unknown>
        return HttpResponse.json(SIGNUP_OK_BODY)
      })
    )

    const { user } = await renderForm()
    await fillRequiredFields(user)
    await user.click(screen.getByTestId('signup-submit-button'))

    // Once the flow completed, the status query had long resolved false —
    // so the absence of the code UI here is the flag's doing, not loading.
    await waitFor(() => {
      expect(mockOnSuccess).toHaveBeenCalled()
    })
    expect(screen.queryByTestId('signup-email-code-input')).not.toBeInTheDocument()
    expect(screen.queryByTestId('signup-send-email-code-button')).not.toBeInTheDocument()
    // The optional code field is normalized to null, matching realmSlug and
    // turnstileToken (backend Option treats null as absent).
    expect(observedBody?.emailVerificationCode).toBeNull()
  })

  it('GIVEN email verification is required WHEN the code is missing THEN blocks submit, and sends it once filled', async () => {
    let signupCalls = 0
    let observedBody: Record<string, unknown> | undefined
    server.use(
      http.get('/api/auth/:realmId/signup/status', () =>
        HttpResponse.json({ enabled: true, emailVerificationRequired: true })
      ),
      http.post('/api/auth/:realmId/signup', async ({ request }) => {
        signupCalls += 1
        observedBody = (await request.json()) as Record<string, unknown>
        return HttpResponse.json(SIGNUP_OK_BODY)
      })
    )

    const { user } = await renderForm()
    // The verification step appears once the status resolves.
    expect(await screen.findByTestId('signup-email-code-input')).toBeInTheDocument()
    expect(screen.getByTestId('signup-send-email-code-button')).toBeInTheDocument()

    await fillRequiredFields(user)
    await user.click(screen.getByTestId('signup-submit-button'))

    // Schema-level required-when-flag: the validation error surfaces and the
    // signup POST never fires without the code.
    expect(await screen.findByText(/6-digit code/i)).toBeInTheDocument()
    expect(signupCalls).toBe(0)

    await user.type(screen.getByTestId('signup-email-code-input'), '123456')
    await user.click(screen.getByTestId('signup-submit-button'))

    await waitFor(() => {
      expect(observedBody).toMatchObject({
        email: 'admin@example.com',
        emailVerificationCode: '123456',
      })
    })
  })

  it('GIVEN email verification is required WHEN the code send succeeds THEN enters the resend countdown', async () => {
    let observedCodeBody: Record<string, unknown> | undefined
    server.use(
      http.get('/api/auth/:realmId/signup/status', () =>
        HttpResponse.json({ enabled: true, emailVerificationRequired: true })
      ),
      http.post('/api/auth/:realmId/signup/email_code', async ({ request }) => {
        observedCodeBody = (await request.json()) as Record<string, unknown>
        return HttpResponse.json({ message: 'ok' })
      })
    )

    const { user } = await renderForm()
    const sendButton = await screen.findByTestId('signup-send-email-code-button')

    // The send button targets the typed mailbox: disabled until the email
    // passes basic validation, then a click posts the address.
    expect(sendButton).toBeDisabled()
    await fillRequiredFields(user)
    expect(sendButton).toBeEnabled()
    await user.click(sendButton)

    await waitFor(() => {
      expect(observedCodeBody).toEqual({ email: 'admin@example.com' })
    })
    // Countdown is active after the successful send: the button is disabled
    // and shows the remaining seconds instead of the send label.
    await waitFor(() => {
      expect(sendButton).toBeDisabled()
    })
    expect(sendButton).toHaveTextContent(/Resend in \d+s/)
  })
})
