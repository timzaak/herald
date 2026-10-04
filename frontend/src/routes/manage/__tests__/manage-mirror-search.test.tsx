/**
 * @vitest-environment jsdom
 */
import { describe, it, expect, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { QueryClientProvider } from '@tanstack/react-query'
import { createRouter, createMemoryHistory, RouterProvider } from '@tanstack/react-router'
import { http, HttpResponse } from 'msw'
import { server } from '@/test/mocks/server'
import { createTestQueryClient } from '@/test/utils/render'
import { routeTree } from '@/routeTree.gen'
import { LocaleProvider } from '@/components/shared/locale-provider'
import { useAuthStore } from '@/stores/auth-store'
import { ADMIN_PERMISSIONS, ADMIN_WEB_CONSOLE_CLIENT_ID } from '@/lib/constants/auth-constants'

/**
 * Full-route-tree regression for manage pages shared by BOTH route forms:
 * each page component backs /$realmId/manage/... and the prefix-less mirror
 * /manage/.... Production 0.6.0 showed `/manage/users?email=...` (mirror)
 * coming up unfiltered with a blank search box while the realm-prefixed
 * form filtered correctly — a page-bound Route.useNavigate() hopped mirror
 * visitors across route matches. The realms page carried the same navigate
 * bug (plus a search box that never followed URL changes). The suite boots
 * the REAL route tree (root loader included; only the network auth
 * bootstrap is stubbed) and asserts the user-visible contract on both
 * pages: the search box backfills from the URL, the API request carries
 * the filter, and filter edits stay on the current route match.
 */

// The root loader's real initializeAuth drives the herald SDK (refresh
// tokens, browser-token switching). Stub it at the module boundary as an
// authenticated admin console session; permission checks read the seeded
// Zustand store below.
vi.mock('@/lib/auth-utils', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/auth-utils')>()
  return {
    ...actual,
    initializeAuth: vi.fn(async () => ({
      authenticated: true,
      redirectPath: '/manage',
      clientId: ADMIN_WEB_CONSOLE_CLIENT_ID,
    })),
  }
})

const adminUser = {
  id: '01a0cd7f-9686-74e3-a656-2d5f98752b14',
  email: 'admin@fornetcode.com',
  nickname: 'Admin',
  status: 1,
  createdAt: '2026-09-23T09:01:26.021Z',
}

const usersRequests: string[] = []
const realmsRequests: string[] = []

beforeEach(() => {
  usersRequests.length = 0
  realmsRequests.length = 0
  useAuthStore.setState({
    isAuthenticated: true,
    isLoading: false,
    realmId: 'admin',
    permissions: [...ADMIN_PERMISSIONS],
    roles: [],
    refreshClientId: ADMIN_WEB_CONSOLE_CLIENT_ID,
  })
  server.use(
    http.get('*/api/users', ({ request }) => {
      usersRequests.push(request.url)
      return HttpResponse.json({
        items: [adminUser],
        page: 0,
        pageSize: 20,
        total: 1,
      })
    }),
    http.get('*/api/realms/paginated', ({ request }) => {
      realmsRequests.push(request.url)
      return HttpResponse.json({
        items: [],
        page: 0,
        pageSize: 20,
        total: 0,
      })
    }),
    // resolveRealmContext probes the custom-domain resolver for un-prefixed
    // hostnames; a 404 makes it fall back to the legacy realm model, matching
    // the main-domain production behavior.
    http.get(
      '*/api/public-config/custom-domain/resolve',
      () => new HttpResponse(null, { status: 404 })
    )
  )
})

// Wait budgets for booting the real route tree (137 route modules): the
// per-test budget must exceed the find + assertion waits combined, and all
// exceed vitest's default testTimeout of 15000.
const FIND_COMPONENT_MS = 15000
const WAIT_ASSERTION_MS = 10000
const TEST_BUDGET_MS = 30000

function renderAt(initialUrl: string) {
  const queryClient = createTestQueryClient()
  const router = createRouter({
    routeTree,
    context: { queryClient },
    history: createMemoryHistory({ initialEntries: [initialUrl] }),
  })
  render(
    <QueryClientProvider client={queryClient}>
      <LocaleProvider>
        <RouterProvider router={router} />
      </LocaleProvider>
    </QueryClientProvider>
  )
  return router
}

function requestCarriesEmailFilter(): boolean {
  return usersRequests.some((url) => decodeURIComponent(url).includes('email=admin@fornetcode.com'))
}

function requestCarriesRealmsSearchFilter(): boolean {
  return realmsRequests.some((url) => decodeURIComponent(url).includes('search=admin'))
}

describe('users page URL search on mirror vs realm route forms', () => {
  it.each([
    ['realm form /admin/manage/users', '/admin/manage/users?email=admin%40fornetcode.com'],
    ['mirror form /manage/users', '/manage/users?email=admin%40fornetcode.com'],
  ])(
    '%s backfills the search box and filters the API call',
    async (_label, initialUrl) => {
      renderAt(initialUrl)
      const input = await screen.findByTestId(
        'users-search-input',
        {},
        { timeout: FIND_COMPONENT_MS }
      )
      await waitFor(() => expect(input).toHaveValue('admin@fornetcode.com'), {
        timeout: WAIT_ASSERTION_MS,
      })
      await waitFor(() => expect(requestCarriesEmailFilter()).toBe(true), {
        timeout: WAIT_ASSERTION_MS,
      })
    },
    TEST_BUDGET_MS
  )

  it(
    'editing the filter on the mirror form stays on the mirror route match',
    async () => {
      const router = renderAt('/manage/users')
      const input = await screen.findByTestId(
        'users-search-input',
        {},
        { timeout: FIND_COMPONENT_MS }
      )
      await userEvent.type(input, 'admin@fornetcode.com')
      // Debounced search change navigates via the page's handler; the
      // navigation must update the CURRENT match's search, not hop the
      // mirror visitor onto /$realmId/manage/users (which remounts the page
      // and drops in-flight filter state).
      await waitFor(
        () =>
          expect(decodeURIComponent(router.state.location.href)).toContain(
            'email=admin@fornetcode.com'
          ),
        { timeout: WAIT_ASSERTION_MS }
      )
      expect(router.state.location.pathname).toBe('/manage/users')
    },
    TEST_BUDGET_MS
  )
})

describe('realms page URL search on the mirror route form', () => {
  it(
    'mirror form /manage/realms backfills the search box and filters the API call',
    async () => {
      renderAt('/manage/realms?search=admin')
      const input = await screen.findByTestId(
        'realms-search-input',
        {},
        { timeout: FIND_COMPONENT_MS }
      )
      await waitFor(() => expect(input).toHaveValue('admin'), {
        timeout: WAIT_ASSERTION_MS,
      })
      await waitFor(() => expect(requestCarriesRealmsSearchFilter()).toBe(true), {
        timeout: WAIT_ASSERTION_MS,
      })
    },
    TEST_BUDGET_MS
  )

  it(
    'editing the filter on the mirror form stays on the mirror route match',
    async () => {
      const router = renderAt('/manage/realms')
      const input = await screen.findByTestId(
        'realms-search-input',
        {},
        { timeout: FIND_COMPONENT_MS }
      )
      await userEvent.type(input, 'admin')
      // Same contract as the users page: the navigation must update the
      // CURRENT match's search, not hop the mirror visitor onto
      // /$realmId/manage/realms.
      await waitFor(
        () => expect(decodeURIComponent(router.state.location.href)).toContain('search=admin'),
        { timeout: WAIT_ASSERTION_MS }
      )
      expect(router.state.location.pathname).toBe('/manage/realms')
    },
    TEST_BUDGET_MS
  )
})
