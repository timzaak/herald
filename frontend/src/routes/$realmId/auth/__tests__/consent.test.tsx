import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

// WHY these tests exist: the consent page is the branch point that was
// missing when session-holding users were bounced off /auth/* — each test
// pins one branch (login forward / consent UI / silent continuation) so the
// original "authorization can never complete" failure cannot return silently.

const mockNavigate = vi.fn()

vi.mock('@tanstack/react-router', async () => {
  const actual =
    await vi.importActual<typeof import('@tanstack/react-router')>('@tanstack/react-router')
  return {
    ...actual,
    createFileRoute: () => (config: Record<string, unknown>) => ({
      useParams: () => ({ realmId: 'test-realm' }),
      useSearch: () => ({}),
      ...config,
    }),
    useRouter: () => ({ navigate: mockNavigate }),
  }
})

let currentSearch: Record<string, string | undefined> = {}

vi.mock('@/lib/realm-routing', () => ({
  useOptionalRouteParams: () => ({ realmId: 'test-realm' }),
  useResolvedRealmContext: () => ({ realmId: 'test-realm', isCustomDomain: false }),
  useCurrentSearch: () => currentSearch,
  realmPath: (_ctx: unknown, path: string) => `/test-realm${path}`,
}))

let authenticated = false

vi.mock('@/stores/auth-store', () => ({
  useIsAuthenticated: () => authenticated,
}))

const mockProbe = vi.fn()
const mockDecision = vi.fn()
const mockRecordConsent = vi.fn()

vi.mock('@/lib/api-generated', () => ({
  oauthConsentProbe: (...args: unknown[]) => mockProbe(...args),
  oauthConsentDecision: (...args: unknown[]) => mockDecision(...args),
  recordConsent: (...args: unknown[]) => mockRecordConsent(...args),
}))

vi.mock('@/data/query-options', () => ({
  publicConfigQueryOptions: () => ({
    queryKey: ['public-config', 'test-realm'],
    queryFn: () =>
      Promise.resolve({ realmName: 'Test Realm', whiteLabel: null, oauthProviders: [] }),
  }),
}))

import { OAuthConsentPage } from '../consent'
import { renderWithProviders } from '@/test/utils/render'

const OAUTH_SEARCH = {
  clientId: 'admin-web-console',
  oauthClientId: 'oncewise-flow',
  redirectUri: 'https://oncewise.example/callback',
  state: 'st-123',
}

describe('OAuthConsentPage', () => {
  const user = userEvent.setup({ delay: null })
  const originalHref = window.location.href

  beforeEach(() => {
    vi.clearAllMocks()
    authenticated = false
    currentSearch = { ...OAUTH_SEARCH }
    mockProbe.mockResolvedValue({ data: undefined, error: undefined })
    mockDecision.mockResolvedValue({ data: undefined, error: undefined })
    Object.defineProperty(window, 'location', { writable: true, value: { href: originalHref } })
  })

  afterEach(() => {
    Object.defineProperty(window, 'location', { writable: true, value: { href: originalHref } })
  })

  it('forwards an anonymous visitor to the login page with every continuation param', async () => {
    authenticated = false
    renderWithProviders(<OAuthConsentPage />)

    await waitFor(() =>
      expect(mockNavigate).toHaveBeenCalledWith({
        to: '/test-realm/auth/login',
        params: { realmId: 'test-realm' },
        search: {
          clientId: 'admin-web-console',
          oauthClientId: 'oncewise-flow',
          redirectUri: 'https://oncewise.example/callback',
          state: 'st-123',
        },
      })
    )
    // The session-only API is never touched for an anonymous visitor.
    expect(mockProbe).not.toHaveBeenCalled()
  })

  it('shows the authorize UI for a session user without a remembered grant', async () => {
    authenticated = true
    mockProbe.mockResolvedValue({
      data: {
        consent_required: true,
        client_app_name: 'OnceWise Flow',
        client_app_icon_url: null,
      },
      error: undefined,
    })

    renderWithProviders(<OAuthConsentPage />)

    expect(await screen.findByText('OnceWise Flow')).toBeInTheDocument()
    // No code request happens before the user decides.
    expect(mockDecision).not.toHaveBeenCalled()
  })

  it('issues the code on approval and follows the redirect', async () => {
    authenticated = true
    mockProbe.mockResolvedValue({
      data: {
        consent_required: true,
        client_app_name: 'OnceWise Flow',
        client_app_icon_url: null,
      },
      error: undefined,
    })
    mockDecision.mockResolvedValue({
      data: {
        status: 'authorized',
        redirect_to: 'https://oncewise.example/callback?code=ac_1&state=st-123',
      },
      error: undefined,
    })

    renderWithProviders(<OAuthConsentPage />)
    await user.click(await screen.findByTestId('device-authorize-button'))

    await waitFor(() =>
      expect(mockDecision).toHaveBeenCalledWith(
        expect.objectContaining({
          body: expect.objectContaining({ approved: true, state: 'st-123' }),
        })
      )
    )
    await waitFor(() =>
      expect(window.location.href).toBe('https://oncewise.example/callback?code=ac_1&state=st-123')
    )
  })

  it('silently continues when the probe reports a remembered grant', async () => {
    authenticated = true
    mockProbe.mockResolvedValue({
      data: {
        consent_required: false,
        client_app_name: 'OnceWise Flow',
        client_app_icon_url: null,
      },
      error: undefined,
    })
    mockDecision.mockResolvedValue({
      data: {
        status: 'authorized',
        redirect_to: 'https://oncewise.example/callback?code=ac_2&state=st-123',
      },
      error: undefined,
    })

    renderWithProviders(<OAuthConsentPage />)

    // No interaction: the approval fires by itself and the browser leaves.
    await waitFor(() =>
      expect(mockDecision).toHaveBeenCalledWith(
        expect.objectContaining({ body: expect.objectContaining({ approved: true }) })
      )
    )
    await waitFor(() =>
      expect(window.location.href).toBe('https://oncewise.example/callback?code=ac_2&state=st-123')
    )
  })

  it('follows the access_denied redirect on deny', async () => {
    authenticated = true
    mockProbe.mockResolvedValue({
      data: {
        consent_required: true,
        client_app_name: 'OnceWise Flow',
        client_app_icon_url: null,
      },
      error: undefined,
    })
    mockDecision.mockResolvedValue({
      data: {
        status: 'denied',
        redirect_to: 'https://oncewise.example/callback?error=access_denied&state=st-123',
      },
      error: undefined,
    })

    renderWithProviders(<OAuthConsentPage />)
    await user.click(await screen.findByTestId('device-deny-button'))

    await waitFor(() =>
      expect(mockDecision).toHaveBeenCalledWith(
        expect.objectContaining({ body: expect.objectContaining({ approved: false }) })
      )
    )
    await waitFor(() =>
      expect(window.location.href).toBe(
        'https://oncewise.example/callback?error=access_denied&state=st-123'
      )
    )
  })

  it('shows the restart hint when the pending state is gone', async () => {
    authenticated = true
    mockProbe.mockResolvedValue({
      data: undefined,
      error: { status: 400, body: { message: 'OAuth state not found' } },
    })

    renderWithProviders(<OAuthConsentPage />)

    expect(await screen.findByTestId('oauth-consent-probe-error')).toBeInTheDocument()
    expect(mockDecision).not.toHaveBeenCalled()
  })
})
