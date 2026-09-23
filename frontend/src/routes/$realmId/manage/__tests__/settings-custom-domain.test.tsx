/**
 * @vitest-environment jsdom
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { http, HttpResponse } from 'msw'
import { Suspense } from 'react'
import { server } from '@/test/mocks/server'
import { queryKeys } from '@/data/query-options'

/**
 * FE-T02 — Settings custom-domain Tab save integration.
 *
 * Renders the real `SettingsPage` against MSW so the page's own mutation runs
 * end-to-end (no internal mutation functions are mocked). The suite verifies:
 *   - the request payload the save action sends (PUT /config/custom-domain with
 *     `{ hostname }`),
 *   - the query cache invalidation boundary: save changes the published config
 *     + host→realm mapping in one step, so BOTH `customDomainRealmConfig` AND
 *     `publicConfig` are invalidated — otherwise terminal-user auth pages keep
 *     serving the stale published domain.
 *   - the 409 conflict path: when the PUT handler returns 409 "Custom domain
 *     already in use", the mutation rejects, `onError` runs and `toast.error`
 *     fires with the localized message.
 *
 * Invalidation is asserted via `vi.spyOn(queryClient, 'invalidateQueries')` —
 * the only reliable signal for `publicConfig`, which the Settings page does NOT
 * mount.
 */

const API_BASE_URL = 'http://localhost:3000'

// Mock `sonner` so the 409 test can observe `toast.error`.
vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}))

// --- Route + auth mocks ------------------------------------------------------
vi.mock('@tanstack/react-router', async () => {
  const actual =
    await vi.importActual<typeof import('@tanstack/react-router')>('@tanstack/react-router')
  return {
    ...actual,
    createFileRoute: () => (config: Record<string, unknown>) => ({
      useParams: () => ({ realmId: 'test-realm' }),
      ...config,
    }),
  }
})

vi.mock('@/hooks/use-auth', () => ({
  useAuth: () => ({
    isAuthenticated: true,
    isLoading: false,
    realmId: 'test-realm',
    user: null,
    permissions: ['settings.view', 'settings.manage'],
    roles: [],
    hasAdminPermission: true,
    login: vi.fn(),
    logout: vi.fn(),
    setAuthStatus: vi.fn(),
    setIsLoading: vi.fn(),
    setUserPermissions: vi.fn(),
    setUserProfile: vi.fn(),
    reset: vi.fn(),
  }),
}))

vi.mock('@/components/oauth-config/provider-config-page', () => ({
  ProviderConfigPage: () => <div data-testid="providers-stub" />,
}))
vi.mock('@/components/settings/LegalAgreementTab', () => ({
  LegalAgreementTab: () => <div data-testid="legal-stub" />,
}))

import { SettingsPage } from '../settings'
import { toast } from 'sonner'

// --- Fixtures ----------------------------------------------------------------

const PUBLISHED_HOSTNAME = 'login.published.example.com'

/** A realm with a published config and no draft (single-state model). */
function publishedState() {
  return {
    published: { hostname: PUBLISHED_HOSTNAME },
    cnameTarget: 'custom.herald.com',
    status: null,
  }
}

/**
 * Default MSW handlers for the custom-domain endpoint. The PUT handler records
 * the JSON body so tests can assert the payload + call count.
 */
function installCustomDomainHandlers(initialState: ReturnType<typeof publishedState>) {
  let state = initialState
  const calls = {
    get: vi.fn(),
    put: vi.fn(),
  }

  const baseUrl = `${API_BASE_URL}/api/realms/test-realm/config/custom-domain`

  server.use(
    http.get(baseUrl, () => {
      calls.get()
      return HttpResponse.json(state)
    }),
    http.put(baseUrl, async ({ request }) => {
      const body = await request.json()
      calls.put(body)
      // Reflect the saved hostname in subsequent GETs (status pending).
      state = { ...state, published: { hostname: body.hostname }, status: null }
      return HttpResponse.json({ message: 'updated', status: null })
    })
  )

  return { calls }
}

/**
 * Minimal handlers for the *other* Settings-page queries so mounting every tab
 * does not fire unhandled requests. Not under test.
 */
function installSiblingTabHandlers() {
  server.use(
    http.get(`${API_BASE_URL}/api/realms/test-realm`, () =>
      HttpResponse.json({ id: 'test-realm', name: 'Test Realm', description: '' })
    ),
    http.get(`${API_BASE_URL}/api/configs`, () => HttpResponse.json([])),
    http.get(`${API_BASE_URL}/api/configs/email/status`, () =>
      HttpResponse.json({ configured: false })
    ),
    http.get(`${API_BASE_URL}/api/realms/test-realm/config/passkey`, () =>
      HttpResponse.json({
        enabled: false,
        userVerification: 'preferred',
        crossPlatformAuthenticator: true,
      })
    ),
    http.get(`${API_BASE_URL}/api/realms/test-realm/config/white-label`, () =>
      HttpResponse.json({
        published: null,
        draft: null,
        hasPrevious: false,
        publishedUpdatedAt: null,
        draftUpdatedAt: null,
      })
    )
  )
}

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0, staleTime: 0 },
      mutations: { retry: false },
    },
  })
}

function renderSettingsPage(queryClient: QueryClient) {
  return render(
    <QueryClientProvider client={queryClient}>
      <Suspense fallback={<div data-testid="settings-suspense">Loading…</div>}>
        <SettingsPage />
      </Suspense>
    </QueryClientProvider>
  )
}

/** Switches the visible tab to the custom-domain editor. */
async function openCustomDomainTab(user: ReturnType<typeof userEvent.setup>) {
  await screen.findByTestId('custom-domain-tab', undefined, { timeout: 5000 })
  await user.click(screen.getByTestId('custom-domain-tab'))
  await screen.findByTestId('custom-domain-save')
}

describe('Settings custom-domain tab save (FE-T02)', () => {
  let queryClient: QueryClient
  let invalidateSpy: ReturnType<typeof vi.spyOn>

  beforeEach(() => {
    vi.clearAllMocks()
    queryClient = createTestQueryClient()
    invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries')
  })

  afterEach(() => {
    invalidateSpy.mockRestore()
  })

  describe('save', () => {
    it('sends a PUT with the edited hostname and invalidates both customDomainRealmConfig and publicConfig', async () => {
      const user = userEvent.setup({ delay: null })
      const { calls } = installCustomDomainHandlers(publishedState())
      installSiblingTabHandlers()

      renderSettingsPage(queryClient)
      await openCustomDomainTab(user)

      // Edit the hostname. The form initial value is the published hostname;
      // clearing it and typing a new value makes the form dirty and changes
      // the payload.
      const hostnameInput = screen.getByTestId('custom-domain-hostname') as HTMLInputElement
      await user.clear(hostnameInput)
      await user.type(hostnameInput, 'login.new.example.com')

      await user.click(screen.getByTestId('custom-domain-save'))

      // PUT was called once with the edited hostname.
      await waitFor(() => {
        expect(calls.put).toHaveBeenCalledTimes(1)
      })
      expect(calls.put).toHaveBeenCalledWith(
        expect.objectContaining({ hostname: 'login.new.example.com' })
      )

      // Invalidation boundary: save changes the published config + mapping in
      // one step, so BOTH the admin state and the publicConfig must be
      // invalidated — terminal-user auth pages key off the published domain.
      await waitFor(() => {
        expect(invalidateSpy).toHaveBeenCalledWith({
          queryKey: queryKeys.customDomainRealmConfig('test-realm'),
        })
        expect(invalidateSpy).toHaveBeenCalledWith({
          queryKey: queryKeys.publicConfig('test-realm'),
        })
      })
    })
  })

  describe('409 conflict', () => {
    it('shows the localized conflict toast when PUT returns 409 "already in use"', async () => {
      const user = userEvent.setup({ delay: null })

      const baseUrl = `${API_BASE_URL}/api/realms/test-realm/config/custom-domain`
      server.use(
        http.get(baseUrl, () => HttpResponse.json(publishedState())),
        // Override ONLY the PUT handler to return a 409 conflict. The mandatory
        // rethrow in the save mutation turns this into a rejection. The onError
        // handler detects `status === 409` and shows the dedicated localized
        // `domain_in_use` message instead of the raw server string.
        http.put(baseUrl, () =>
          HttpResponse.json({ message: 'Custom domain already in use' }, { status: 409 })
        )
      )
      installSiblingTabHandlers()

      renderSettingsPage(queryClient)
      await openCustomDomainTab(user)

      // Edit the hostname to a value that collides with another realm.
      const hostnameInput = screen.getByTestId('custom-domain-hostname') as HTMLInputElement
      await user.clear(hostnameInput)
      await user.type(hostnameInput, 'login.taken.example.com')

      await user.click(screen.getByTestId('custom-domain-save'))

      // The mutation rejects, onError runs, and the 409 branch fires the
      // localized conflict message. Asserting the localized string — NOT the
      // raw server body — verifies the domain_in_use key is wired.
      await waitFor(() => {
        expect(toast.error).toHaveBeenCalledWith('This domain is already in use by another realm.')
      })
    })
  })
})
