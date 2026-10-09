import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { http, HttpResponse } from 'msw'
import { server } from '@/test/mocks/server'
import { RegisterForm } from '../register-form'

vi.mock('@marsidev/react-turnstile', () => ({
  Turnstile: vi.fn(() => <div data-testid="turnstile-mock" />),
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

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  })
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  }
}

describe('RegisterForm', () => {
  const mockOnSuccess = vi.fn()
  const realmId = 'test-realm'

  beforeEach(() => {
    mockOnSuccess.mockClear()
    server.use(
      http.post('/api/auth/:realmId/turnstile/status', () =>
        HttpResponse.json({ enabled: false, site_key: null })
      ),
      http.post('/api/auth/:realmId/register', () =>
        HttpResponse.json({ verificationRequired: false, message: 'Registration successful' })
      )
    )
  })

  async function renderForm() {
    const user = userEvent.setup({ delay: null })
    const view = render(<RegisterForm realmId={realmId} onSuccess={mockOnSuccess} />, {
      wrapper: createWrapper(),
    })
    return { user, ...view }
  }

  async function fillRequiredFields(user: ReturnType<typeof userEvent.setup>) {
    await user.type(screen.getByTestId('register-email-input'), 'user@example.com')
    await user.type(screen.getByTestId('register-nickname-input'), 'Test User')
    await user.type(screen.getByTestId('register-password-input'), 'Password123!')
    await user.type(screen.getByTestId('register-confirm-password-input'), 'Password123!')
  }

  it('GIVEN consent is unchecked WHEN submitting THEN blocks submit and shows validation error', async () => {
    const { user } = await renderForm()
    await fillRequiredFields(user)

    await user.click(screen.getByTestId('register-submit-button'))

    await waitFor(() => {
      expect(screen.getByTestId('register-consent-error')).toHaveTextContent(
        /Please read and agree/i
      )
    })
    expect(mockOnSuccess).not.toHaveBeenCalled()
  })

  // 昵称是注册的必填项：为空时提交必须被前端校验拦下，不能发出注册请求。
  it('GIVEN nickname is empty WHEN submitting THEN blocks submit with a required error', async () => {
    const { user } = await renderForm()
    await user.type(screen.getByTestId('register-email-input'), 'user@example.com')
    await user.type(screen.getByTestId('register-password-input'), 'Password123!')
    await user.type(screen.getByTestId('register-confirm-password-input'), 'Password123!')
    await user.click(screen.getByTestId('register-consent-checkbox'))

    await user.click(screen.getByTestId('register-submit-button'))

    await waitFor(() => {
      expect(screen.getByText(/Please enter a nickname/i)).toBeInTheDocument()
    })
    expect(mockOnSuccess).not.toHaveBeenCalled()
  })

  it('GIVEN consent is checked WHEN submitting THEN calls register and invokes onSuccess', async () => {
    const { user } = await renderForm()
    await fillRequiredFields(user)
    await user.click(screen.getByTestId('register-consent-checkbox'))

    await user.click(screen.getByTestId('register-submit-button'))

    await waitFor(() => {
      expect(mockOnSuccess).toHaveBeenCalledWith(false)
    })
  })

  it('GIVEN the form renders THEN agreement links point to public legal pages', async () => {
    await renderForm()

    const termsLink = screen.getByTestId('terms-of-service-link')
    const privacyLink = screen.getByTestId('privacy-policy-link')

    expect(termsLink).toHaveAttribute('href', `/${realmId}/legal/terms_of_service`)
    expect(privacyLink).toHaveAttribute('href', `/${realmId}/legal/privacy_policy`)
  })
})
