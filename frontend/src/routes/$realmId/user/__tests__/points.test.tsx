/**
 * @vitest-environment jsdom
 *
 * Route-stability regression for the user-points bucket-filter URL sync.
 *
 * `UserPointsWrapper` backs BOTH route matches: /$realmId/user/points
 * (deep links/bookmarks) and /user/points (sidebar). The bucket-dimension
 * URL sync must update only `?bucketId=` on the CURRENT match. A fixed
 * `/user/points` navigate target migrated realm-prefixed visitors across
 * route matches, remounting the page and dropping the local type/date filter
 * state, so clear-filters-button disappeared after Apply (demo US-PU-03,
 * docs/user-stories/billing/points-user.md).
 */
import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import { UserPointsWrapper } from '../points'
import { transactionBucketSearchSchema } from '@/lib/schemas/points-forms'
import type { ListWalletsByBucketResponse } from '@/lib/api-generated'

// Seed wallets/transactions without MSW (UserPointsPage test pattern). One
// wallet-backed bucket so the Bucket Select renders a real option.
vi.mock('@/data/query-options', () => ({
  userPointsWalletsQueryOptions: {
    queryKey: ['user-points-wallets'],
    queryFn: async () => walletsResponse,
  },
  // Key includes the filters so an applied filter is a fresh query, matching
  // the real key semantics closely enough for this suite.
  userPointsTransactionsQueryOptions: (filters: Record<string, unknown>) => ({
    queryKey: ['user-points-transactions', filters],
    queryFn: async () => transactionsResponse,
  }),
  userFeatureAvailabilityQueryOptions: {
    queryKey: ['user-feature-availability'],
    queryFn: async () => ({ user: { pointsVisible: false } }),
  },
  // Imported (unused here) by the route module's real Route beforeLoad.
  requireUserFeature: vi.fn(),
}))

const REALM_ID = 'realm-001'
// transactionBucketSearchSchema only accepts UUID bucketIds; a non-UUID
// fixture would be stripped from the URL and break the search assertions.
const PRIMARY_BUCKET_ID = '550e8400-e29b-41d4-a716-446655440000'

const walletsResponse: ListWalletsByBucketResponse = {
  crossBucketTotal: 100,
  items: [
    {
      bucketId: PRIMARY_BUCKET_ID,
      name: 'Primary Pool',
      enabled: true,
      bucketTotal: 100,
      userId: 'user-self',
      balancesByType: {
        freePeriodic: 0,
        granted: 0,
        registration: 0,
        subscription: 0,
        topup: 100,
      },
    },
  ],
}

const transactionsResponse = {
  transactions: [
    {
      id: 'txn-1',
      walletId: 'wallet-1',
      userId: 'user-self',
      realmId: REALM_ID,
      amount: 100,
      balanceAfter: 100,
      transactionType: 'recharge',
      description: 'Top up',
      externalRefId: 'ref-1',
      bucketId: PRIMARY_BUCKET_ID,
      createdAt: '2025-03-15T10:00:00Z',
    },
  ],
}

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0, staleTime: 0 },
      mutations: { retry: false },
    },
  })
}

/**
 * Mount the wrapper under a minimal REAL router exposing both user-points
 * matches. The regression is about which route match a filter Apply lands
 * on — observable only through actual navigation, not module mocks.
 */
function renderAt(initialPath: string) {
  const rootRoute = createRootRoute({ component: () => <Outlet /> })
  const realmPointsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/$realmId/user/points',
    validateSearch: transactionBucketSearchSchema,
    component: UserPointsWrapper,
  })
  const sessionPointsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/user/points',
    validateSearch: transactionBucketSearchSchema,
    component: UserPointsWrapper,
  })
  const queryClient = createTestQueryClient()
  const router = createRouter({
    routeTree: rootRoute.addChildren([realmPointsRoute, sessionPointsRoute]),
    context: { queryClient },
    history: createMemoryHistory({ initialEntries: [initialPath] }),
  })
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  )
  return router
}

type User = ReturnType<typeof userEvent.setup>

async function applyTypeFilter(user: User) {
  await user.click(screen.getByTestId('filter-transaction-type'))
  await user.click(await screen.findByRole('option', { name: /recharge/i }))
  await waitForApplyEnabled()
  await user.click(screen.getByTestId('apply-filters-button'))
}

async function applyBucketFilter(user: User) {
  await user.click(screen.getByTestId('filter-bucket'))
  await user.click(await screen.findByRole('option', { name: 'Primary Pool' }))
  await waitForApplyEnabled()
  await user.click(screen.getByTestId('apply-filters-button'))
}

async function waitForApplyEnabled() {
  await waitFor(() => expect(screen.getByTestId('apply-filters-button')).not.toBeDisabled())
}

describe('UserPointsWrapper bucket-filter URL sync (route stability)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('GIVEN a realm-prefixed deep link WHEN applying a type-only filter THEN the route match and filter state survive Apply', async () => {
    const user = userEvent.setup({ delay: null })
    const router = renderAt(`/${REALM_ID}/user/points`)
    await screen.findByTestId('transaction-history-table')

    await applyTypeFilter(user)

    // With the old fixed `/user/points` navigate target the page remounted,
    // the local type filter reset to "All types", and clear-filters-button
    // stopped rendering — the exact US-PU-03 demo failure.
    expect(await screen.findByTestId('clear-filters-button')).toBeInTheDocument()
    expect(router.state.location.pathname).toBe(`/${REALM_ID}/user/points`)
  })

  it('GIVEN a realm-prefixed deep link WHEN applying a bucket filter THEN only ?bucketId= updates in place', async () => {
    const user = userEvent.setup({ delay: null })
    const router = renderAt(`/${REALM_ID}/user/points`)
    await screen.findByTestId('transaction-history-table')

    await applyBucketFilter(user)

    expect(await screen.findByTestId('clear-filters-button')).toBeInTheDocument()
    expect(router.state.location.pathname).toBe(`/${REALM_ID}/user/points`)
    expect(router.state.location.search).toEqual({ bucketId: PRIMARY_BUCKET_ID })
  })

  it('GIVEN the session-scoped path WHEN applying a bucket filter THEN ?bucketId= still syncs (existing behavior)', async () => {
    const user = userEvent.setup({ delay: null })
    const router = renderAt('/user/points')
    await screen.findByTestId('transaction-history-table')

    await applyBucketFilter(user)

    expect(await screen.findByTestId('clear-filters-button')).toBeInTheDocument()
    expect(router.state.location.pathname).toBe('/user/points')
    expect(router.state.location.search).toEqual({ bucketId: PRIMARY_BUCKET_ID })
  })
})
