/**
 * @vitest-environment jsdom
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { http, HttpResponse } from 'msw'
import { Suspense } from 'react'
import { server } from '@/test/mocks/server'
import { queryKeys } from '@/data/query-options'

/**
 * FE-T03 — Settings white-label Tab lifecycle integration.
 *
 * Renders the real `SettingsPage` against MSW so the page's own mutations run
 * end-to-end (no internal mutation functions are mocked). The suite verifies:
 *   - the request payload each action sends (PUT /draft, POST /publish,
 *     DELETE /draft, POST /restore),
 *   - the query cache invalidation boundary from design §4.4.2:
 *       save-draft / discard-draft → only `whiteLabelRealmConfig(realmId)`,
 *       publish / restore           → `whiteLabelRealmConfig` AND `publicConfig`.
 *
 * Invalidation is asserted two ways for robustness:
 *   1. MSW request observation — `whiteLabelRealmConfig` IS mounted by the
 *      page, so invalidating it triggers a refetch the spy handler observes.
 *   2. A `vi.spyOn(queryClient, 'invalidateQueries')` call record — this is the
 *      only reliable signal for `publicConfig`, which the Settings page does
 *      NOT mount, so its GET handler would never be re-invoked even when
 *      correctly invalidated. Asserting the call args directly tests the
 *      boundary contract and fails if the invalidate is missing.
 */

const API_BASE_URL = 'http://localhost:3000'

// --- Route + auth mocks ------------------------------------------------------
// The page reads `realmId` via `Route.useParams()` (the `Route` object is built
// by `createFileRoute` at module load). Stub the router so params resolve, and
// stub `useAuth` so the user holds both `settings.view` and `settings.manage`.
//
// `importActual` is required because the TanStack Router Vite plugin rewrites
// `createFileRoute(...)({ component })` to wrap the component in
// `lazyRouteComponent` (auto code-splitting) — the mock must preserve the real
// module's exports. The test imports `SettingsPage` directly so it renders
// synchronously, sidestepping the `lazyRouteComponent` Suspense dance that
// under parallel suite load left the first render stuck in JSDOM.
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
    // `settings.view` lets the page render; `settings.manage` enables the
    // action buttons (the page gates every mutation behind `canUpdateConfig`).
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

// The white-label path is the subject under test, so it runs end-to-end against
// MSW. The sibling tabs (Providers, Legal) fire their own queries that are not
// under test; stubbing them keeps the suite focused and avoids a long tail of
// incidental MSW handlers. This mirrors the api-keys permission-gating test,
// which stubs `ApiKeyTable`/`DeleteApiKeyDialog` for the same reason.
vi.mock('@/components/oauth-config/provider-config-page', () => ({
  ProviderConfigPage: () => <div data-testid="providers-stub" />,
}))
vi.mock('@/components/settings/LegalAgreementTab', () => ({
  LegalAgreementTab: () => <div data-testid="legal-stub" />,
}))

// Imported after the `vi.mock` calls so the hoisted router/auth mocks apply
// before the page module evaluates. `SettingsPage` is the direct component
// (not the Vite-plugin-wrapped `Route.component`) so the render is synchronous.
import { SettingsPage } from '../settings'

// --- Fixtures ----------------------------------------------------------------

const PUBLISHED_LOGO = 'https://cdn.example.com/published-logo.svg'
const PUBLISHED_FOOTER = '© Published Inc.'

/** A realm with a published config and no draft. */
function publishedState(hasPrevious = false) {
  return {
    published: {
      brandName: 'Published Brand',
      logoUrl: PUBLISHED_LOGO,
      faviconUrl: 'https://cdn.example.com/published.ico',
      accentColor: '#2563eb',
      background: null,
      footerText: PUBLISHED_FOOTER,
      loginTitle: 'Sign in to Published',
      loginSubtitle: 'Use your Published account',
      registerTitle: 'Create your Published account',
      registerSubtitle: 'Start with Published',
    },
    draft: null,
    hasPrevious,
    publishedUpdatedAt: '2026-06-01T00:00:00Z',
    draftUpdatedAt: null,
  }
}

/** A realm with both a published config and an unpublished draft. */
function publishedWithDraftState(hasPrevious = false) {
  return {
    published: publishedState(hasPrevious).published,
    draft: {
      brandName: 'Draft Brand',
      logoUrl: 'https://cdn.example.com/draft-logo.svg',
      faviconUrl: 'https://cdn.example.com/draft.ico',
      accentColor: '#2563eb',
      background: null,
      footerText: '© Draft Inc.',
      loginTitle: 'Sign in to Draft',
      loginSubtitle: 'Use your Draft account',
      registerTitle: 'Create your Draft account',
      registerSubtitle: 'Start with Draft',
    },
    hasPrevious,
    publishedUpdatedAt: '2026-06-01T00:00:00Z',
    draftUpdatedAt: '2026-06-15T00:00:00Z',
  }
}

/**
 * Default MSW handlers for the white-label lifecycle endpoints. Each handler
 * records the requests it saw so tests can assert payload + call count.
 *
 * The GET handler closes over a mutable `state`; the DELETE handler clears
 * `draft` so subsequent GETs (the post-discard refetch) report the
 * published-only state.
 */
function installWhiteLabelHandlers(initialState: ReturnType<typeof publishedState>) {
  let state = initialState
  const calls = {
    get: vi.fn(),
    putDraft: vi.fn(),
    deleteDraft: vi.fn(),
    publish: vi.fn(),
    restore: vi.fn(),
  }

  const baseUrl = `${API_BASE_URL}/api/realms/test-realm/config/white-label`

  server.use(
    http.get(baseUrl, () => {
      calls.get()
      return HttpResponse.json(state)
    }),
    http.put(`${baseUrl}/draft`, async ({ request }) => {
      const body = await request.json()
      calls.putDraft(body)
      return HttpResponse.json({ ...state, draft: body, message: 'draft saved' })
    }),
    http.delete(`${baseUrl}/draft`, () => {
      calls.deleteDraft()
      // After discard the draft is gone — reflect that in subsequent GETs.
      state = { ...state, draft: null, draftUpdatedAt: null }
      return HttpResponse.json({ ...state, message: 'draft discarded' })
    }),
    http.post(`${baseUrl}/publish`, async ({ request }) => {
      const body = await request.json().catch(() => null)
      calls.publish(body)
      return HttpResponse.json({ ...state, draft: null, hasPrevious: true, message: 'published' })
    }),
    http.post(`${baseUrl}/restore`, () => {
      calls.restore()
      return HttpResponse.json({ ...state, hasPrevious: true, message: 'restored' })
    })
  )

  return { calls }
}

/**
 * Minimal handlers for the *other* Settings-page queries so mounting every tab
 * (TabsContent renders all children) does not fire unhandled requests. These
 * are not under test; returning empty/valid shapes keeps the page stable.
 */
function installSiblingTabHandlers() {
  server.use(
    // GeneralTab reads the realm (name/description) — needed so the tab mounts.
    http.get(`${API_BASE_URL}/api/realms/test-realm`, () =>
      HttpResponse.json({ id: 'test-realm', name: 'Test Realm', description: '' })
    ),
    // listRealmConfigs (TOTP/Turnstile/Registration/Email parse from this)
    http.get(`${API_BASE_URL}/api/configs`, () => HttpResponse.json([])),
    // email status
    http.get(`${API_BASE_URL}/api/configs/email/status`, () =>
      HttpResponse.json({ configured: false })
    ),
    // passkey realm config
    http.get(`${API_BASE_URL}/api/realms/test-realm/config/passkey`, () =>
      HttpResponse.json({
        enabled: false,
        userVerification: 'preferred',
        crossPlatformAuthenticator: true,
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
  // `Route.component` is the router-plugin's `lazyRouteComponent`-wrapped page,
  // which suspends on first render until its dynamic import resolves. Wrap it
  // in Suspense so the tree resolves instead of staying in an `act` limbo.
  return render(
    <QueryClientProvider client={queryClient}>
      <Suspense fallback={<div data-testid="settings-suspense">Loading…</div>}>
        <SettingsPage />
      </Suspense>
    </QueryClientProvider>
  )
}

/** Switches the visible tab to the white-label editor. */
async function openWhiteLabelTab(user: ReturnType<typeof userEvent.setup>) {
  // The page mounts with loading state while queries resolve; wait for the
  // tab strip (always rendered once not-loading / not-access-denied) first.
  await screen.findByTestId('white-label-tab', undefined, { timeout: 5000 })
  await user.click(screen.getByTestId('white-label-tab'))
  // Wait for the editor (loaded state resolves to the form) to appear.
  await screen.findByTestId('white-label-save-draft')
}

describe('Settings white-label tab lifecycle (FE-T03)', () => {
  let queryClient: QueryClient
  let invalidateSpy: ReturnType<typeof vi.spyOn>

  beforeEach(() => {
    vi.clearAllMocks()
    queryClient = createTestQueryClient()
    // Spy on invalidateQueries to assert the exact invalidation boundary. The
    // page's mutations call this directly in their `onSuccess`; recording the
    // queryKey args is the authoritative signal for which caches were touched.
    invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries')
  })

  afterEach(() => {
    invalidateSpy.mockRestore()
  })

  describe('save draft', () => {
    it('sends a PUT /draft with the edited values and invalidates only whiteLabelRealmConfig', async () => {
      const user = userEvent.setup({ delay: null })
      const { calls } = installWhiteLabelHandlers(publishedState(false))
      installSiblingTabHandlers()

      renderSettingsPage(queryClient)
      await openWhiteLabelTab(user)

      // Edit the three new fields. Setting values directly (instead of typing
      // each character) keeps this round-trip payload test fast under parallel
      // CI load; the asserted behavior is the forwarded PUT body, not typing.
      fireEvent.change(screen.getByTestId('white-label-logo-url'), {
        target: { value: 'https://cdn.example.com/new-logo.svg' },
      })
      fireEvent.change(screen.getByTestId('white-label-brand-name'), {
        target: { value: 'New Brand' },
      })
      fireEvent.change(screen.getByTestId('white-label-favicon-url'), {
        target: { value: 'https://cdn.example.com/new.ico' },
      })

      await user.click(screen.getByTestId('white-label-save-draft'))

      // PUT /draft was called once with the edited logoUrl preserved.
      await waitFor(() => {
        expect(calls.putDraft).toHaveBeenCalledTimes(1)
      })
      expect(calls.putDraft).toHaveBeenCalledWith(
        expect.objectContaining({
          logoUrl: 'https://cdn.example.com/new-logo.svg',
          brandName: 'New Brand',
          faviconUrl: 'https://cdn.example.com/new.ico',
        })
      )
      // The published footer is untouched and still forwarded (trimmed/non-empty).
      expect(calls.putDraft).toHaveBeenCalledWith(
        expect.objectContaining({ footerText: PUBLISHED_FOOTER })
      )

      // Invalidation boundary: only whiteLabelRealmConfig, NOT publicConfig.
      await waitFor(() => {
        expect(invalidateSpy).toHaveBeenCalledWith({
          queryKey: queryKeys.whiteLabelRealmConfig('test-realm'),
        })
      })
      expect(invalidateSpy).not.toHaveBeenCalledWith({
        queryKey: queryKeys.publicConfig('test-realm'),
      })

      // Observability cross-check: whiteLabelRealmConfig IS mounted by the page,
      // so invalidating it triggers a refetch the GET spy observes.
      await waitFor(() => {
        expect(calls.get.mock.calls.length).toBeGreaterThan(1)
      })
    })
  })

  describe('publish', () => {
    it('posts to /publish and invalidates both whiteLabelRealmConfig and publicConfig', async () => {
      const user = userEvent.setup({ delay: null })
      const { calls } = installWhiteLabelHandlers(publishedState(false))
      installSiblingTabHandlers()

      renderSettingsPage(queryClient)
      await openWhiteLabelTab(user)

      await user.click(screen.getByTestId('white-label-publish'))

      // POST /publish fired once, carrying the current form values.
      await waitFor(() => {
        expect(calls.publish).toHaveBeenCalledTimes(1)
      })
      expect(calls.publish).toHaveBeenCalledWith(
        expect.objectContaining({ logoUrl: PUBLISHED_LOGO })
      )

      // Invalidation boundary: publish touches the published config, so BOTH
      // the admin state and the terminal-user publicConfig must be invalidated.
      await waitFor(() => {
        expect(invalidateSpy).toHaveBeenCalledWith({
          queryKey: queryKeys.whiteLabelRealmConfig('test-realm'),
        })
        expect(invalidateSpy).toHaveBeenCalledWith({
          queryKey: queryKeys.publicConfig('test-realm'),
        })
      })
    })
  })

  describe('restore', () => {
    it('gates restore behind hasPrevious, confirms via dialog, and invalidates both caches', async () => {
      const user = userEvent.setup({ delay: null })
      // hasPrevious=true enables the restore button.
      const { calls } = installWhiteLabelHandlers(publishedState(true))
      installSiblingTabHandlers()

      renderSettingsPage(queryClient)
      await openWhiteLabelTab(user)

      const restoreButton = screen.getByTestId('white-label-restore')
      // Gate: restore is only enabled when a previous version exists.
      expect(restoreButton).not.toBeDisabled()

      // Opening the confirm dialog must NOT fire restore yet.
      await user.click(restoreButton)
      const dialog = await screen.findByTestId('white-label-restore-dialog')
      expect(calls.restore).not.toHaveBeenCalled()

      // Confirm inside the dialog.
      const confirmButton = within(dialog).getByTestId('white-label-restore-confirm')
      await user.click(confirmButton)

      await waitFor(() => {
        expect(calls.restore).toHaveBeenCalledTimes(1)
      })

      // Restore changes the published config → both caches invalidated.
      await waitFor(() => {
        expect(invalidateSpy).toHaveBeenCalledWith({
          queryKey: queryKeys.whiteLabelRealmConfig('test-realm'),
        })
        expect(invalidateSpy).toHaveBeenCalledWith({
          queryKey: queryKeys.publicConfig('test-realm'),
        })
      })
    })

    it('disables restore when there is no previous version', async () => {
      const user = userEvent.setup({ delay: null })
      installWhiteLabelHandlers(publishedState(false))
      installSiblingTabHandlers()

      renderSettingsPage(queryClient)
      await openWhiteLabelTab(user)

      expect(screen.getByTestId('white-label-restore')).toBeDisabled()
    })
  })

  describe('discard draft', () => {
    it('deletes the draft, refetches admin state (draft cleared), and does not invalidate publicConfig', async () => {
      const user = userEvent.setup({ delay: null })
      // Load a state that already has an unpublished draft.
      const { calls } = installWhiteLabelHandlers(publishedWithDraftState(false))
      installSiblingTabHandlers()

      renderSettingsPage(queryClient)
      await openWhiteLabelTab(user)

      // The form edits `draft ?? published`, so the editor starts on the draft
      // logo — confirms the draft was loaded into the editor.
      expect(screen.getByTestId('white-label-logo-url')).toHaveValue(
        'https://cdn.example.com/draft-logo.svg'
      )

      // Edit a field to ensure the form is dirty before discarding.
      await user.type(screen.getByTestId('white-label-footer-text'), '-edited')

      await user.click(screen.getByTestId('white-label-discard-draft'))

      // DELETE /draft fired exactly once.
      await waitFor(() => {
        expect(calls.deleteDraft).toHaveBeenCalledTimes(1)
      })

      // Invalidation boundary: discard only touches admin draft state — only
      // whiteLabelRealmConfig is invalidated, publicConfig is NOT.
      await waitFor(() => {
        expect(invalidateSpy).toHaveBeenCalledWith({
          queryKey: queryKeys.whiteLabelRealmConfig('test-realm'),
        })
      })
      expect(invalidateSpy).not.toHaveBeenCalledWith({
        queryKey: queryKeys.publicConfig('test-realm'),
      })

      // The whiteLabelRealmConfig query IS mounted by the page, so invalidating
      // it triggers a refetch. The DELETE handler's response already cleared
      // `draft`, so the next GET observes the published-only state — i.e. the
      // admin cache no longer reports a draft (hasDraft is false afterwards).
      await waitFor(() => {
        // get spy called at least twice: initial mount + post-discard refetch.
        expect(calls.get.mock.calls.length).toBeGreaterThanOrEqual(2)
      })

      // The discard button is gated behind `hasDraft`; once the refetched state
      // reports no draft, the button disables — a stable, observable signal
      // that the admin state reflects "no draft" after discard.
      await waitFor(() => {
        expect(screen.getByTestId('white-label-discard-draft')).toBeDisabled()
      })

      // Design §5.5: after discard the form must revert to the `published`
      // config (the source flips from draft to published on refetch). The
      // editor started on the draft logo/footer and was edited mid-flight, so
      // both the draft value and the "-edited" tail must be gone.
      await waitFor(() => {
        expect(screen.getByTestId('white-label-logo-url')).toHaveValue(PUBLISHED_LOGO)
      })
      expect(screen.getByTestId('white-label-footer-text')).toHaveValue(PUBLISHED_FOOTER)
    })
  })
})
