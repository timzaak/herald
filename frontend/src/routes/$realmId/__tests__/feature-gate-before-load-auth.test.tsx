/**
 * @vitest-environment jsdom
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { QueryClient } from '@tanstack/react-query'
import { http, HttpResponse } from 'msw'
import { server } from '@/test/mocks/server'
import {
  ADMIN_WEB_CONSOLE_CLIENT_ID,
  USER_ACCOUNT_CENTER_CLIENT_ID,
} from '@/lib/constants/auth-constants'
import { Route as InvoicesRoute } from '../manage/billing/invoices'
import { Route as SubscriptionsRoute } from '../manage/billing/subscriptions'
import { Route as EntitlementMappingsRoute } from '../manage/billing/entitlement-mappings'
import { Route as CreditBucketsRoute } from '../manage/billing/credit-buckets'
import { Route as ManageSubscriptionHistoryRoute } from '../manage/subscription-history'
import { Route as MySubscriptionsRoute } from '../subscription/my-subscriptions'
import { Route as UserInvoicesRoute } from '../user/invoices'
import { Route as UserPointsRoute } from '../user/points'
import { Route as PurchasePointsRoute } from '../user/purchase-points'
import { Route as UserSubscriptionHistoryRoute } from '../user/subscription-history'

/**
 * Cold-reload auth ordering for feature-gated `$realmId` routes.
 *
 * TanStack Router runs every matched route's `beforeLoad` BEFORE the __root
 * loader, so on a full page reload of a realm-prefixed URL the routes below
 * used to fire their authenticated feature-availability query before
 * `initializeAuth` restored the in-memory Bearer token → 401 "missing bearer
 * token" → route CatchBoundary instead of the page. Each gated route now
 * awaits `initializeAuth` first. These tests lock that ordering: while the
 * auth restore is pending, no feature-availability request may be issued.
 */

const API_BASE_URL = 'http://localhost:3000'

/** Controllable `initializeAuth` double: stays pending until released. */
const authGate = vi.hoisted(() => {
  const events: string[] = []
  let resolveAuth: (() => void) | undefined
  const initializeAuth = vi.fn(() => {
    events.push('initializeAuth:start')
    return new Promise<void>((resolve) => {
      resolveAuth = resolve
    })
  })
  return {
    events,
    initializeAuth,
    releaseAuth: () => resolveAuth?.(),
    reset: () => {
      events.length = 0
      resolveAuth = undefined
      initializeAuth.mockClear()
    },
  }
})

vi.mock('@/lib/auth-utils', async () => {
  const actual = await vi.importActual<typeof import('@/lib/auth-utils')>('@/lib/auth-utils')
  return {
    ...actual,
    initializeAuth: authGate.initializeAuth,
  }
})

// Capture the route config each file passes to createFileRoute, so the test
// can invoke the real `beforeLoad` against MSW.
vi.mock('@tanstack/react-router', async () => {
  const actual =
    await vi.importActual<typeof import('@tanstack/react-router')>('@tanstack/react-router')
  return {
    ...actual,
    createFileRoute: () => (config: Record<string, unknown>) => config,
  }
})

type BeforeLoadFn = (ctx: {
  context: { queryClient: QueryClient }
  params: { realmId: string }
}) => Promise<void>

// The createFileRoute mock above returns the raw route config, so `beforeLoad`
// sits at the top level of the exported Route object (the real types nest it
// under `.options`).
function beforeLoadOf(route: unknown): BeforeLoadFn {
  return (route as { beforeLoad: BeforeLoadFn }).beforeLoad
}

const FEATURE_GATED_ROUTES = [
  {
    name: '/$realmId/manage/billing/invoices',
    clientId: ADMIN_WEB_CONSOLE_CLIENT_ID,
    beforeLoad: beforeLoadOf(InvoicesRoute),
  },
  {
    name: '/$realmId/manage/billing/subscriptions',
    clientId: ADMIN_WEB_CONSOLE_CLIENT_ID,
    beforeLoad: beforeLoadOf(SubscriptionsRoute),
  },
  {
    name: '/$realmId/manage/billing/entitlement-mappings',
    clientId: ADMIN_WEB_CONSOLE_CLIENT_ID,
    beforeLoad: beforeLoadOf(EntitlementMappingsRoute),
  },
  {
    name: '/$realmId/manage/billing/credit-buckets',
    clientId: ADMIN_WEB_CONSOLE_CLIENT_ID,
    beforeLoad: beforeLoadOf(CreditBucketsRoute),
  },
  {
    name: '/$realmId/manage/subscription-history',
    clientId: ADMIN_WEB_CONSOLE_CLIENT_ID,
    beforeLoad: beforeLoadOf(ManageSubscriptionHistoryRoute),
  },
  {
    name: '/$realmId/subscription/my-subscriptions',
    clientId: USER_ACCOUNT_CENTER_CLIENT_ID,
    beforeLoad: beforeLoadOf(MySubscriptionsRoute),
  },
  {
    name: '/$realmId/user/invoices',
    clientId: USER_ACCOUNT_CENTER_CLIENT_ID,
    beforeLoad: beforeLoadOf(UserInvoicesRoute),
  },
  {
    name: '/$realmId/user/points',
    clientId: USER_ACCOUNT_CENTER_CLIENT_ID,
    beforeLoad: beforeLoadOf(UserPointsRoute),
  },
  {
    name: '/$realmId/user/purchase-points',
    clientId: USER_ACCOUNT_CENTER_CLIENT_ID,
    beforeLoad: beforeLoadOf(PurchasePointsRoute),
  },
  {
    name: '/$realmId/user/subscription-history',
    clientId: USER_ACCOUNT_CENTER_CLIENT_ID,
    beforeLoad: beforeLoadOf(UserSubscriptionHistoryRoute),
  },
] as const satisfies ReadonlyArray<{ name: string; clientId: string; beforeLoad: BeforeLoadFn }>

function installFeatureHandlers() {
  server.use(
    http.get(`${API_BASE_URL}/api/realms/admin/feature-availability`, () => {
      authGate.events.push('feature-request')
      return HttpResponse.json({
        admin: {
          invoicesVisible: true,
          entitlementMappingsVisible: true,
          pointsVisible: true,
          subscriptionHistoryVisible: true,
        },
      })
    }),
    http.get(`${API_BASE_URL}/api/user/feature-availability`, () => {
      authGate.events.push('feature-request')
      return HttpResponse.json({
        user: { pointsVisible: true, subscriptionVisible: true, invoicesVisible: true },
      })
    })
  )
}

describe('feature-gated $realmId routes await auth restore before querying', () => {
  beforeEach(() => {
    authGate.reset()
    installFeatureHandlers()
  })

  it.each(FEATURE_GATED_ROUTES)(
    '$name gates the feature query on initializeAuth',
    async (route) => {
      const queryClient = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      })

      const pending = route.beforeLoad({ context: { queryClient }, params: { realmId: 'admin' } })

      // beforeLoad must enter initializeAuth synchronously (nothing else first).
      expect(authGate.events).toEqual(['initializeAuth:start'])
      expect(authGate.initializeAuth).toHaveBeenCalledWith('admin', route.clientId)

      // While the auth restore is still pending, the authenticated feature
      // query must NOT have been issued — the cold-reload race this guards
      // against sends it without a Bearer token and 401s into the error
      // boundary. Give a wrongly-un-gated request time to register.
      await new Promise((resolve) => setTimeout(resolve, 100))
      expect(authGate.events).toEqual(['initializeAuth:start'])

      authGate.releaseAuth()
      await pending

      // Only after the auth restore resolves does the feature query fire.
      expect(authGate.events).toEqual(['initializeAuth:start', 'feature-request'])
    }
  )
})
