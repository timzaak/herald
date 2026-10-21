import { beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import { http, HttpResponse } from 'msw'
import { server } from '@/test/mocks/server'
import { useAuthStore } from '@/stores/auth-store'
import { realmPath, useResolvedRealmContext } from '@/lib/realm-routing'
import { manageFeatureGuard } from '@/lib/manage-feature-guard'
import { userFeatureGuard } from '@/lib/user-feature-guard'
import {
  ADMIN_WEB_CONSOLE_CLIENT_ID,
  USER_ACCOUNT_CENTER_CLIENT_ID,
} from '@/lib/constants/auth-constants'

const authCalls = vi.hoisted(() => vi.fn())
vi.mock('@/lib/auth-utils', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/lib/auth-utils')>()),
  initializeAuth: (realmId: string, clientId: string) => {
    authCalls(realmId, clientId)
    useAuthStore.setState({
      realmId,
      isAuthenticated: true,
      permissions: ['settings.view'],
      user: { id: 'oncewise-user', email: 'user@example.com', status: 1 },
    })
    return Promise.resolve({ authenticated: true, clientId })
  },
}))

import { Route as AppRoot } from '@/routes/__root'

function Probe() {
  const context = useResolvedRealmContext()
  return (
    <>
      <div data-testid="realm">{context.realmId}</div>
      <a data-testid="settings-link" href={realmPath(context, '/manage/settings')}>
        Settings
      </a>
    </>
  )
}

function renderAt(pathname: string) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const root = createRootRouteWithContext<{ queryClient: QueryClient }>()({
    beforeLoad: AppRoot.options.beforeLoad,
    component: Outlet,
    errorComponent: ({ error }) => <div data-testid="realm-error">{error.message}</div>,
  })
  const settings = createRoute({
    getParentRoute: () => root,
    path: '/manage/settings',
    beforeLoad: manageFeatureGuard((features) => features.admin.invoicesVisible),
    component: Probe,
  })
  const subscription = createRoute({
    getParentRoute: () => root,
    path: '/subscription',
    beforeLoad: userFeatureGuard((features) => features.user.subscriptionVisible),
    component: Probe,
  })
  const prefixed = createRoute({
    getParentRoute: () => root,
    path: '/$realmId/manage/settings',
    component: Probe,
  })
  const router = createRouter({
    routeTree: root.addChildren([settings, subscription, prefixed]),
    context: { queryClient },
    history: createMemoryHistory({ initialEntries: [pathname] }),
  })
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  )
  return router
}

beforeEach(() => {
  useAuthStore.getState().reset()
  useAuthStore.getState().setAuthStatus(true, 'admin')
  authCalls.mockClear()
  server.use(
    http.get('*/api/public-config/custom-domain/resolve', () =>
      HttpResponse.json({ realmId: 'oncewise' })
    ),
    http.get('*/api/bill/feature-availability', () =>
      HttpResponse.json({ admin: { invoicesVisible: true } })
    ),
    http.get('*/api/user/feature-availability', () =>
      HttpResponse.json({ user: { subscriptionVisible: true } })
    )
  )
})

describe('custom-domain routes inherit the Host realm before child guards run', () => {
  it.each([
    ['/manage/settings', ADMIN_WEB_CONSOLE_CLIENT_ID],
    ['/subscription', USER_ACCOUNT_CENTER_CLIENT_ID],
  ])(
    'keeps %s prefix-less and uses oncewise for both root and child auth',
    async (pathname, clientId) => {
      const router = renderAt(pathname)
      expect(await screen.findByTestId('realm')).toHaveTextContent('oncewise')
      expect(screen.getByTestId('settings-link')).toHaveAttribute('href', '/manage/settings')
      expect(authCalls.mock.calls).toEqual([
        ['oncewise', clientId],
        ['oncewise', clientId],
      ])
      expect(router.state.location.pathname).toBe(pathname)
    }
  )

  it('canonicalizes its own prefixed URL without losing return parameters', async () => {
    const router = renderAt('/oncewise/manage/settings?tab=security')
    expect(await screen.findByTestId('realm')).toHaveTextContent('oncewise')
    await waitFor(() => expect(router.state.location.pathname).toBe('/manage/settings'))
    expect(router.state.location.search).toMatchObject({ tab: 'security' })
  })

  it('rejects another realm in the path before initializing any session', async () => {
    renderAt('/admin/manage/settings')
    expect(await screen.findByTestId('realm-error')).toHaveTextContent(
      'URL realm does not match the custom domain'
    )
    expect(authCalls).not.toHaveBeenCalled()
  })
})
