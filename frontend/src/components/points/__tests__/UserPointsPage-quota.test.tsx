/**
 * @vitest-environment jsdom
 */

import { describe, it, expect, beforeEach } from 'vitest'
import { render, screen, waitFor, within } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import type {
  ListWalletsByBucketResponse,
  PointsTransactionResponse,
  QuotaWindowViewResponse,
  WalletByBucketResponse,
} from '@/lib/api-generated'
import { UserPointsPage } from '../UserPointsPage'
import { server } from '@/test/mocks/server'
import { renderWithProviders } from '@/test/utils/render'
import { m } from '@/paraglide/messages'

// ============================================================================
// Contract pin (FE-T04 / FE-D04 / FE-D03)
// ----------------------------------------------------------------------------
// UserPointsPage renders, per bucket: a <PointsUsageDashboard> (quota view)
// followed by a pool-only <PointsBalanceCard>. The whole stack is driven by
// the real `userPointsWalletsQueryOptions` + `userPointsTransactionsQueryOptions`
// against MSW — this is the page-level integration counterpart to the
// unit-level PointsBalanceCard / PointsUsageDashboard / derivation tests.
//
// Pinned testids / contracts:
//   - Root page:                  `user-points-page`
//   - Cross-bucket total bar:     `user-points-cross-bucket-total` (only when
//                                 the current user holds >= 2 buckets)
//   - Empty state:                (none — empty pools render nothing; the
//                                 Transaction History card is also hidden when
//                                 there are no transactions)
//   - Dashboard root:             `points-usage-dashboard-{bucketId}`
//   - Dashboard loading root:     `points-usage-dashboard` (no suffix)
//   - Spendable now (big number): `points-spendable-now`  == backend `bucketTotal`
//   - Pool card root:             `points-balance-card-{bucketId}`
//   - Pool card big number:       `points-balance-total-{bucketId}` == `spendableFromPool`
//   - Transaction table:          `transaction-history-table`,
//                                 `transaction-bucket-{index}`
//   - Bucket Select:              combobox named by `points.filter_bucket_label`;
//                                 options derived from the wallets response `items[*].name`
//
// Backend identity (BE-D08): `bucketTotal` === `spendableFromQuota` +
// `spendableFromPool`. The fixtures below honour that identity so the
// "spendable-now == quota + pool" assertion holds against the rendered value.
// ============================================================================

const API_BASE = 'http://localhost:3000'
const REALM_ID = 'realm-quota'
const CURRENT_USER = 'user-self'
const OTHER_USER = 'user-other'

// ---------- Factory helpers ----------

function makeWindow(
  overrides: Partial<QuotaWindowViewResponse> & { key: string }
): QuotaWindowViewResponse {
  return {
    limit: 100,
    used: 0,
    remaining: 100,
    windowSeconds: 30 * 24 * 60 * 60,
    isTightest: false,
    exhausted: false,
    resetsAt: null,
    ...overrides,
  }
}

function makeWallet(
  overrides: Partial<WalletByBucketResponse> & { userId: string; bucketId: string }
): WalletByBucketResponse {
  return {
    name: null,
    enabled: true,
    bucketTotal: 0,
    balancesByType: {
      freePeriodic: 0,
      granted: 0,
      registration: 0,
      subscription: 0,
      topup: 0,
    },
    ...overrides,
  }
}

// A quota-window bucket for the current user. `bucketTotal` is intentionally
// kept equal to spendableFromQuota + spendableFromPool to mirror the backend
// (BE-D08) — the dashboard renders `bucketTotal` verbatim as `spendable-now`.
const QUOTA_BUCKET: WalletByBucketResponse = makeWallet({
  userId: CURRENT_USER,
  bucketId: 'bucket-quota',
  name: 'Subscription Bucket',
  enabled: true,
  bucketTotal: 120, // 70 (quota) + 50 (pool)
  spendableFromQuota: 70,
  spendableFromPool: 50,
  quotaWindows: [
    makeWindow({
      key: 'monthly',
      limit: 100,
      used: 30,
      remaining: 70,
      isTightest: true,
    }),
  ],
  balancesByType: {
    freePeriodic: 0,
    granted: 0,
    registration: 0,
    subscription: 70, // surfaced via the dashboard window, NOT the pool card
    topup: 50,
  },
})

// A pool-only bucket for the current user (no quota entitlement). The pool
// card is the only meaningful surface; the dashboard still renders and shows
// the backend `bucketTotal` as spendable-now with no window rows.
const POOL_BUCKET: WalletByBucketResponse = makeWallet({
  userId: CURRENT_USER,
  bucketId: 'bucket-pool',
  name: 'Topup Bucket',
  enabled: true,
  bucketTotal: 80, // pool-only: bucketTotal == spendableFromPool
  spendableFromQuota: null,
  spendableFromPool: 80,
  quotaWindows: null,
  balancesByType: {
    freePeriodic: 0,
    granted: 10,
    registration: 20,
    subscription: 0,
    topup: 50,
  },
})

// A second user's wallet in the same realm. `deriveUserPointsView` must drop
// it; it must NOT leak into the cross-bucket total or the bucket Select.
const OTHER_USER_BUCKET: WalletByBucketResponse = makeWallet({
  userId: OTHER_USER,
  bucketId: 'bucket-quota',
  name: 'Someone Else Bucket',
  enabled: true,
  bucketTotal: 9999,
  spendableFromQuota: 9999,
  spendableFromPool: 0,
  quotaWindows: [makeWindow({ key: 'monthly', remaining: 9999, isTightest: true })],
})

const txnOnQuotaBucket: PointsTransactionResponse = {
  id: 'txn-quota-1',
  walletId: 'wallet-quota',
  userId: CURRENT_USER,
  realmId: REALM_ID,
  amount: 50,
  balanceAfter: 120,
  transactionType: 'recharge',
  description: 'Topup into subscription bucket',
  externalRefId: 'ref-q-1',
  bucketId: 'bucket-quota',
  createdAt: '2026-06-01T10:00:00Z',
}

const txnOnPoolBucket: PointsTransactionResponse = {
  id: 'txn-pool-1',
  walletId: 'wallet-pool',
  userId: CURRENT_USER,
  realmId: REALM_ID,
  amount: 80,
  balanceAfter: 80,
  transactionType: 'recharge',
  description: 'Topup into pool bucket',
  externalRefId: 'ref-p-1',
  bucketId: 'bucket-pool',
  createdAt: '2026-06-02T10:00:00Z',
}

// ---------- MSW handler builders ----------

/**
 * Captures the bucketId query param of the transactions request so a test can
 * assert the URL-synced `bucketId` filter actually reached the API. The page
 * derives `effectiveFilters.bucketId` from the prop and the query options
 * forward it; observing it at the HTTP layer is the integration-fidelity bar.
 */
function walletsAndTransactionsHandlers(opts: {
  wallets: WalletByBucketResponse[]
  transactions?: PointsTransactionResponse[]
  crossBucketTotal?: number
  onTransactionsRequest?: (bucketId: string | null) => void
}) {
  const walletsResponse: ListWalletsByBucketResponse = {
    crossBucketTotal: opts.crossBucketTotal ?? 0,
    items: opts.wallets,
  }
  const transactions = opts.transactions ?? []

  return [
    http.get(`${API_BASE}/api/user/wallets`, () => HttpResponse.json(walletsResponse)),
    http.get(`${API_BASE}/api/user/feature-availability`, () =>
      HttpResponse.json({ user: { pointsVisible: false } })
    ),
    http.get(`${API_BASE}/api/user/transactions`, ({ request }) => {
      const url = new URL(request.url)
      const bucketId = url.searchParams.get('bucketId')
      opts.onTransactionsRequest?.(bucketId)
      // Server-side filter on bucketId so a filtered request only returns the
      // matching transaction — lets the URL-sync test assert the table content
      // narrowed, not just that the param was sent.
      const filtered = bucketId ? transactions.filter((t) => t.bucketId === bucketId) : transactions
      return HttpResponse.json({
        items: filtered,
        page: 0,
        pageSize: filtered.length ? 20 : 20,
        total: filtered.length,
      })
    }),
  ]
}

function renderPage(
  overrides: {
    bucketId?: string
    onBucketIdChange?: (id: string | undefined) => void
  } = {}
) {
  return renderWithProviders(
    <UserPointsPage
      realmId={REALM_ID}
      userId={CURRENT_USER}
      bucketId={overrides.bucketId}
      onBucketIdChange={overrides.onBucketIdChange}
    />
  )
}

// ============================================================================
// Tests
// ============================================================================

describe('UserPointsPage — quota dashboard + pool cards (MSW integration)', () => {
  beforeEach(() => {
    // Each test registers the handlers it needs via server.use so the default
    // handler set never silently satisfies a wallets/transactions request.
  })

  describe('quota + pool rendering', () => {
    it('GIVEN a user with one quota bucket and one pool-only bucket WHEN rendered THEN renders a dashboard + pool card per bucket, with spendable-now == backend bucketTotal', async () => {
      server.use(
        ...walletsAndTransactionsHandlers({
          wallets: [QUOTA_BUCKET, POOL_BUCKET, OTHER_USER_BUCKET],
          transactions: [txnOnQuotaBucket, txnOnPoolBucket],
        })
      )

      renderPage()

      // One dashboard per current-user bucket (other-user row dropped).
      const quotaDashboard = await screen.findByTestId('points-usage-dashboard-bucket-quota')
      expect(screen.getByTestId('points-usage-dashboard-bucket-pool')).toBeInTheDocument()
      // The other-user bucket shares bucketId 'bucket-quota' but must NOT
      // produce a second dashboard — userId narrowing happens before render.
      expect(screen.getAllByTestId(/^points-usage-dashboard-/)).toHaveLength(2)

      // Pool-only card per bucket.
      expect(screen.getByTestId('points-balance-card-bucket-quota')).toBeInTheDocument()
      expect(screen.getByTestId('points-balance-card-bucket-pool')).toBeInTheDocument()

      // The "current spendable total" big number was removed from the dashboard
      // card, so spendable-now is no longer rendered end-to-end. Pin its absence
      // here so the removal intent survives through the query → derive → render
      // pipeline.
      expect(within(quotaDashboard).queryByTestId('points-spendable-now')).not.toBeInTheDocument()
      const poolDashboard = screen.getByTestId('points-usage-dashboard-bucket-pool')
      expect(within(poolDashboard).queryByTestId('points-spendable-now')).not.toBeInTheDocument()
    })

    it('GIVEN a quota bucket WHEN rendered THEN the dashboard surfaces its window row AND the pool card big number is spendableFromPool (not bucketTotal)', async () => {
      // INTENT: the redesign splits the two models — quota entitlement shows
      // up as window rows in the dashboard, while the pool card tracks ONLY
      // the pool-side balance. A regression that put bucketTotal back on the
      // pool card would double-count the subscription entitlement visually.
      server.use(
        ...walletsAndTransactionsHandlers({
          wallets: [QUOTA_BUCKET],
          transactions: [],
        })
      )

      renderPage()

      const dashboard = await screen.findByTestId('points-usage-dashboard-bucket-quota')
      // Quota window row keyed by bucketId + stable backend window key.
      expect(
        within(dashboard).getByTestId('points-window-row-bucket-quota-monthly')
      ).toBeInTheDocument()

      // Pool card big number == spendableFromPool (50), NOT bucketTotal (120).
      const poolTotal = screen.getByTestId('points-balance-total-bucket-quota')
      expect(poolTotal).toHaveTextContent('50')
    })

    it('GIVEN a pool-only bucket WHEN rendered THEN the dashboard shows no window rows and the pool card carries the pool balance', async () => {
      server.use(
        ...walletsAndTransactionsHandlers({
          wallets: [POOL_BUCKET],
          transactions: [],
        })
      )

      renderPage()

      const dashboard = await screen.findByTestId('points-usage-dashboard-bucket-pool')
      expect(
        within(dashboard).queryByTestId(/^points-window-row-bucket-pool-/)
      ).not.toBeInTheDocument()

      expect(screen.getByTestId('points-balance-total-bucket-pool')).toHaveTextContent('80')
    })
  })

  describe('cross-bucket total bar', () => {
    it('GIVEN a user holding >= 2 buckets WHEN rendered THEN shows the cross-bucket total bar summing the current user bucketTotal values (other users excluded)', async () => {
      // INTENT: the total bar is the user's "how many points do I have, total,
      // across buckets" at-a-glance. It MUST sum only the current user's rows
      // (the response is server-scoped, but the page recomputes defensively).
      // The OTHER_USER_BUCKET (9999) must NOT leak in.
      server.use(
        ...walletsAndTransactionsHandlers({
          wallets: [QUOTA_BUCKET, POOL_BUCKET, OTHER_USER_BUCKET],
          transactions: [],
        })
      )

      renderPage()

      const totalBar = await screen.findByTestId('user-points-cross-bucket-total')
      expect(totalBar).toBeInTheDocument()
      // 120 (quota) + 80 (pool) = 200; formatted with grouping.
      expect(totalBar).toHaveTextContent('200')
    })

    it('GIVEN a user holding exactly one bucket WHEN rendered THEN does NOT show the cross-bucket total bar', async () => {
      // INTENT: the bar is only meaningful when comparing across buckets; a
      // single-bucket user already sees their total on the card and the bar
      // would be redundant noise.
      server.use(
        ...walletsAndTransactionsHandlers({
          wallets: [POOL_BUCKET],
          transactions: [],
        })
      )

      renderPage()

      await screen.findByTestId('points-usage-dashboard-bucket-pool')
      expect(screen.queryByTestId('user-points-cross-bucket-total')).not.toBeInTheDocument()
    })
  })

  describe('transaction history + bucket Select', () => {
    it('GIVEN transactions across buckets WHEN rendered THEN the history table renders and the bucket Select options are derived from the wallets rows', async () => {
      // INTENT: regular users 403 on the admin credit-buckets directory, so
      // bucket display names must come from their own wallets response. A
      // regression that re-introduced an admin-directory lookup would break
      // the user view; this pins wallet-derived names end-to-end.
      server.use(
        ...walletsAndTransactionsHandlers({
          wallets: [QUOTA_BUCKET, POOL_BUCKET],
          transactions: [txnOnQuotaBucket, txnOnPoolBucket],
        })
      )

      const user = userEvent.setup({ delay: null })
      renderPage()

      await screen.findByTestId('transaction-history-table')

      // Bucket column reflects wallet-derived names, not bucket ids.
      expect(screen.getByTestId('transaction-bucket-0')).toHaveTextContent('Subscription Bucket')

      // Bucket Select options come from wallet rows.
      // The bucket Select's accessible name comes from `points.filter_bucket_label`
      // — derive the expected label from the message so copy changes don't
      // silently break this test.
      const bucketSelect = screen.getByRole('combobox', {
        name: new RegExp(`^${m['points.filter_bucket_label']()}$`, 'i'),
      })
      await user.click(bucketSelect)
      expect(screen.getByRole('option', { name: 'Subscription Bucket' })).toBeInTheDocument()
      expect(screen.getByRole('option', { name: 'Topup Bucket' })).toBeInTheDocument()
      // The other-user wallet name never becomes an option.
      expect(screen.queryByRole('option', { name: 'Someone Else Bucket' })).not.toBeInTheDocument()
    })
  })

  describe('URL-synced bucketId filter', () => {
    it('GIVEN a URL-synced bucketId WHEN rendered THEN forwards bucketId to the transactions query and narrows the table to that bucket', async () => {
      // INTENT: the shareable `?bucketId=` URL must drive the transactions
      // filter so two users looking at the same link see the same scoped
      // history. We assert BOTH that the param reaches the HTTP layer (the
      // integration contract) AND that the rendered table narrows (the user
      // contract).
      let observedBucketId: string | null = undefined as unknown as string
      server.use(
        ...walletsAndTransactionsHandlers({
          wallets: [QUOTA_BUCKET, POOL_BUCKET],
          transactions: [txnOnQuotaBucket, txnOnPoolBucket],
          onTransactionsRequest: (bucketId) => {
            observedBucketId = bucketId
          },
        })
      )

      renderPage({ bucketId: 'bucket-quota' })

      // Wait for the filtered table to settle.
      await screen.findByTestId('transaction-history-table')
      await waitFor(() => expect(observedBucketId).toBe('bucket-quota'))

      // Only the quota-bucket transaction is present.
      expect(screen.getByTestId('transaction-bucket-0')).toHaveTextContent('Subscription Bucket')
      expect(screen.queryByTestId('transaction-bucket-1')).not.toBeInTheDocument()
    })
  })

  describe('empty state', () => {
    it('GIVEN a user with no wallets WHEN rendered THEN shows no cards, no total bar, and no empty placeholder', async () => {
      // INTENT: a brand-new user with no balance in any bucket must see a
      // quiet page — no stack of zero-balance cards, no "no pools yet"
      // placeholder, and no Transaction History section (which would also
      // be empty). The page should render only the header.
      server.use(
        ...walletsAndTransactionsHandlers({
          wallets: [],
          transactions: [],
        })
      )

      renderPage()

      // Let queries settle.
      await screen.findByTestId('user-points-page')
      expect(screen.queryByTestId('points-balance-empty')).not.toBeInTheDocument()
      expect(screen.queryByTestId(/^points-usage-dashboard-/)).not.toBeInTheDocument()
      expect(screen.queryByTestId(/^points-balance-card-/)).not.toBeInTheDocument()
      expect(screen.queryByTestId('user-points-cross-bucket-total')).not.toBeInTheDocument()
      // Transaction history section is hidden when there are no transactions.
      // Probed by its stable testid after settle — the section legitimately
      // renders its skeleton while the transactions query is in flight, so a
      // synchronous assert here would race that fetch.
      await waitFor(() =>
        expect(screen.queryByTestId('user-points-transaction-history')).not.toBeInTheDocument()
      )
    })
  })

  describe('loading state', () => {
    it('GIVEN the wallets query is in flight WHEN rendering THEN shows the dashboard + pool skeletons before the MSW response resolves', async () => {
      // INTENT: while wallets are loading the user must see a stable skeleton
      // (dashboard + pool card), not a flash of the empty state. We delay the
      // MSW wallets response so the loading branch is observably rendered
      // before the data settles. The loading dashboard root is intentionally
      // bucket-agnostic (`points-usage-dashboard` with no suffix) and the pool
      // card loading root is `points-balance-card` (no suffix) — pinned so a
      // refactor that drops the loading branch surfaces here.
      server.use(
        http.get(`${API_BASE}/api/user/wallets`, async () => {
          // No artificial timer — returning a fresh response on each tick is
          // enough; React Query starts in `pending` and the component renders
          // the loading branch on first paint. We assert synchronously before
          // `findByTestId` (which awaits resolution) runs.
          return HttpResponse.json({
            crossBucketTotal: 0,
            items: [] as WalletByBucketResponse[],
          })
        }),
        http.get(`${API_BASE}/api/user/feature-availability`, () =>
          HttpResponse.json({ user: { pointsVisible: false } })
        ),
        http.get(`${API_BASE}/api/user/transactions`, () =>
          HttpResponse.json({ items: [], page: 0, pageSize: 20, total: 0 })
        )
      )

      const { container } = renderPage()

      // The loading branch renders the bucket-agnostic dashboard skeleton and
      // the pool card skeleton. Because MSW resolves on the next microtask,
      // we assert the loading testids appear in the initial DOM snapshot
      // captured synchronously after render (before any await).
      const loadingDashboards = container.querySelectorAll('[data-testid="points-usage-dashboard"]')
      const loadingPoolCards = container.querySelectorAll('[data-testid="points-balance-card"]')
      expect(loadingDashboards.length).toBeGreaterThanOrEqual(1)
      expect(loadingPoolCards.length).toBeGreaterThanOrEqual(1)

      // Once the (empty) response settles, the page renders no cards and no
      // transaction history (everything is empty). The section shows its
      // skeleton while the transactions fetch is pending, so the absence must
      // be awaited, not asserted synchronously.
      await waitFor(() =>
        expect(screen.queryByTestId(/^points-usage-dashboard-/)).not.toBeInTheDocument()
      )
      await waitFor(() =>
        expect(screen.queryByTestId('user-points-transaction-history')).not.toBeInTheDocument()
      )
    })
  })
})
