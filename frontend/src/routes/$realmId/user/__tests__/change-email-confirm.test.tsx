/**
 * @vitest-environment jsdom
 *
 * Change-email confirm page tests: the mailed link lands here with ?code= and
 * the page must commit the change exactly once with the current session,
 * map each failure status to actionable copy, render an invalid-link state
 * when the code is missing, and refresh the profile cache on success.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import type { ReactNode } from 'react'
import { ChangeEmailConfirmPage } from '../change-email/confirm'
import { queryKeys } from '@/data/query-options'
import { changeEmailConfirm } from '@/lib/api-generated'

vi.mock('@/lib/api-generated', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/lib/api-generated')>()
  return {
    ...original,
    changeEmailConfirm: vi.fn(),
  }
})

vi.mock('@tanstack/react-router', async (importOriginal) => {
  // The page module calls createFileRoute at import time, so the router mock
  // can only replace Link.
  const original = await importOriginal<typeof import('@tanstack/react-router')>()
  return {
    ...original,
    Link: ({ to, children, ...props }: { to: string; children: ReactNode }) => (
      <a href={to} {...props}>
        {children}
      </a>
    ),
  }
})

const mockChangeEmailConfirm = vi.mocked(changeEmailConfirm)

const CONFIRM_URL = '/realm-001/user/change-email/confirm?code=code-123'

function renderPage() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries')
  render(
    <QueryClientProvider client={queryClient}>
      <ChangeEmailConfirmPage />
    </QueryClientProvider>
  )
  return { invalidateSpy }
}

function visit(url: string) {
  window.history.pushState({}, '', url)
}

describe('ChangeEmailConfirmPage', () => {
  const user = userEvent.setup()

  beforeEach(() => {
    vi.clearAllMocks()
    visit(CONFIRM_URL)
    mockChangeEmailConfirm.mockResolvedValue({ data: { message: 'ok' }, error: undefined })
  })

  it('GIVEN a code in the link WHEN the page opens THEN it confirms automatically with the current session', async () => {
    renderPage()

    await screen.findByTestId('change-email-confirm-success')
    expect(mockChangeEmailConfirm).toHaveBeenCalledTimes(1)
    expect(mockChangeEmailConfirm).toHaveBeenCalledWith({
      path: { realmId: 'realm-001', changeCode: 'code-123' },
    })
  })

  it('GIVEN the change commits WHEN the page opens THEN the cached profile is invalidated so the profile page shows the new email', async () => {
    const { invalidateSpy } = renderPage()

    await screen.findByTestId('change-email-confirm-success')
    await waitFor(() => {
      expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: queryKeys.profile() })
    })
    expect(screen.getByTestId('change-email-confirm-success')).toHaveTextContent(
      'Your account email has been changed.'
    )
    // /user/** targets collapse to the session-scoped form (no realm prefix)
    // — same rule the profile sidebar links follow.
    expect(screen.getByTestId('change-email-confirm-back-link')).toHaveAttribute(
      'href',
      '/user/profile'
    )
  })

  it.each([
    [400, 'This confirmation link is invalid or has expired. Your account email was not changed.'],
    [
      401,
      'Your session has expired. Sign in again with the account that requested the change, then reopen the link from the email.',
    ],
    [403, 'This confirmation link belongs to a different account.'],
    [409, 'This email address is already in use. The change was not completed.'],
    [429, 'Too many attempts. Please try again later.'],
  ])(
    'GIVEN the confirm endpoint rejects with %s WHEN the page opens THEN the failure copy names that exact cause',
    async (status, expectedText) => {
      mockChangeEmailConfirm.mockResolvedValue({
        data: undefined,
        error: { message: 'Rejected', status },
      })

      renderPage()

      expect(await screen.findByTestId('change-email-confirm-error')).toHaveTextContent(
        expectedText
      )
      expect(screen.queryByTestId('change-email-confirm-success')).not.toBeInTheDocument()
    }
  )

  it('GIVEN a rate-limited attempt WHEN retry is clicked THEN the confirm fires again (the code was not consumed)', async () => {
    mockChangeEmailConfirm.mockResolvedValue({
      data: undefined,
      error: { message: 'Too Many Requests', status: 429 },
    })

    renderPage()
    expect(await screen.findByTestId('change-email-confirm-error')).toHaveTextContent(
      'Too many attempts. Please try again later.'
    )

    await user.click(screen.getByTestId('change-email-confirm-retry-button'))

    await waitFor(() => expect(mockChangeEmailConfirm).toHaveBeenCalledTimes(2))
  })

  it('GIVEN the link lost its code (e.g. the login redirect drops the query) WHEN the page opens THEN it renders the invalid-link state and never calls the endpoint', async () => {
    visit('/realm-001/user/change-email/confirm')

    renderPage()

    expect(screen.getByTestId('change-email-confirm-error')).toHaveTextContent(
      'This confirmation link is invalid or has expired.'
    )
    expect(mockChangeEmailConfirm).not.toHaveBeenCalled()
    expect(screen.getByTestId('change-email-confirm-back-link')).toBeInTheDocument()
  })
})
