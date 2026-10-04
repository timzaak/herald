import { useMemo, useState } from 'react'
import { Link } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { Button } from '@/components/ui/button'
import { PointsBalanceCard } from './PointsBalanceCard'
import { PointsUsageDashboard } from './PointsUsageDashboard'
import { TransactionHistoryTable } from './TransactionHistoryTable'
import { TransactionFilters } from './TransactionFilters'
import { deriveUserPointsView } from './user-points-view'
import {
  userPointsWalletsQueryOptions,
  userPointsTransactionsQueryOptions,
  userFeatureAvailabilityQueryOptions,
} from '@/data/query-options'
import { DEFAULT_PAGE_SIZE } from '@/lib/constants'
import type { TransactionFilters as TransactionFiltersType } from '@/lib/schemas/points-forms'
import { m } from '@/paraglide/messages'

interface UserPointsPageProps {
  realmId: string
  userId: string
  /**
   * URL-synced `bucketId` transaction filter (`?bucketId=`).
   * Source of truth is the `/$realmId/user/points` route `validateSearch`
   * (`validateSearch`); this component mirrors it into `transactionFilters` and reports
   * user-driven changes back via {@link onBucketIdChange} so the URL stays
   * shareable. `undefined` means "all buckets".
   */
  bucketId?: string
  onBucketIdChange?: (bucketId: string | undefined) => void
}

const MAX_VISIBLE_TRANSACTIONS = 1000

export function UserPointsPage({
  realmId,
  userId,
  bucketId,
  onBucketIdChange,
}: UserPointsPageProps) {
  // TODO: Migrate pagination/filter state to URL search params via parent route
  // (/$realmId/user/points) for link sharing and refresh restoration.
  // `bucketId` is already URL-synced; the remaining ephemeral filters
  // (type/startTime/endTime) are intentionally local state.
  // loadedPages counts how many "Load More" windows have been expanded; the
  // server returns the latest N transactions in a single growing page, so we
  // never accumulate fetched pages in component state (which would need effects).
  const [loadedPages, setLoadedPages] = useState(1)
  // Ephemeral transaction filters (type/dates). `bucketId` is intentionally
  // NOT stored here: it is URL-synced and merged in below so external
  // URL changes (e.g. opening a shared `?bucketId=` link) reflect immediately.
  const [transactionFilters, setTransactionFilters] = useState<TransactionFiltersType>({})

  const { data: walletsData, isLoading: walletsLoading } = useQuery(userPointsWalletsQueryOptions)

  const { data: features } = useQuery(userFeatureAvailabilityQueryOptions)
  const pointsAreaVisible = features?.user?.pointsVisible === true

  // Bucket name lookup for the Bucket Select + Bucket column. The admin-only
  // credit-buckets directory (`/billing/credit-buckets`) 403s for regular users
  // (they only hold `points.view`), so we resolve bucket names from the user's
  // own wallets response — each held wallet already carries `{ bucketId, name }`.
  const bucketOptions = useMemo(() => {
    return (walletsData?.items ?? [])
      .filter((w): w is typeof w => Boolean(w.bucketId) && Boolean(w.name))
      .map((w) => ({
        id: w.bucketId as string,
        name: w.name as string,
        // Wallets carry `enabled?: boolean | null`; TransactionFilters requires a
        // concrete boolean for its option type. Treat null/undefined as enabled
        // (a wallet row only exists for a bucket the user holds balance in).
        enabled: w.enabled ?? true,
      }))
  }, [walletsData?.items])

  // Effective filters = ephemeral (type/dates) + URL-derived bucketId. The
  // URL wins for `bucketId` so the page always matches the shareable URL.
  const effectiveFilters: TransactionFiltersType = {
    ...transactionFilters,
    bucketId,
  }

  const requestedPageSize = Math.min(loadedPages * DEFAULT_PAGE_SIZE, MAX_VISIBLE_TRANSACTIONS)
  const { data: transactionsData, isLoading: transactionsLoading } = useQuery(
    userPointsTransactionsQueryOptions({
      page: 1,
      pageSize: requestedPageSize,
      ...effectiveFilters,
    })
  )

  const { showTotalBar, cards, crossBucketTotal } = deriveUserPointsView(
    walletsData?.items ?? [],
    userId
  )

  const transactions = transactionsData?.transactions ?? []
  // Whether the user has any active filter applied (ephemeral type/date/app
  // filters OR the URL-owned bucketId). Used to keep the transaction card
  // visible when a filter returns 0 results so the empty-state + Clear button
  // remain reachable instead of stranding the user.
  const hasActiveFilters = Boolean(
    effectiveFilters.transactionType ||
    effectiveFilters.startTime ||
    effectiveFilters.endTime ||
    effectiveFilters.clientAppId ||
    effectiveFilters.bucketId
  )
  const reachedLimit = transactions.length >= MAX_VISIBLE_TRANSACTIONS
  const canLoadMore =
    !transactionsLoading && transactions.length >= requestedPageSize && !reachedLimit

  function handleFiltersChange(filters: TransactionFiltersType) {
    // `bucketId` is URL-owned; strip it before storing ephemeral filters so
    // the URL remains the single source of truth for the bucket dimension.
    const ephemeral: TransactionFiltersType = {
      transactionType: filters.transactionType,
      startTime: filters.startTime,
      endTime: filters.endTime,
      clientAppId: filters.clientAppId,
    }
    setTransactionFilters(ephemeral)
    setLoadedPages(1)
    // Mirror the shareable bucket dimension back to the URL.
    onBucketIdChange?.(filters.bucketId)
  }

  function handleFiltersClear() {
    setTransactionFilters({})
    setLoadedPages(1)
    onBucketIdChange?.(undefined)
  }

  return (
    <div className="space-y-6" data-testid="user-points-page">
      {/* Header */}
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">{m['points.user_points_page_title']()}</h1>
      </div>

      {pointsAreaVisible && (
        <div
          className="flex flex-col items-start justify-between gap-4 sm:flex-row sm:items-center"
          data-testid="points-purchase-inline-block"
        >
          <div className="text-sm font-medium">{m['points.purchase_points_cta']()}</div>
          {/* TODO: add quick-pack chips (one_time packs) that deep-link to
              the purchase page with a preselected price once the preselect
              interaction is finalized. */}
          <Button asChild size="sm" data-testid="points-purchase-cta">
            <Link to="/$realmId/user/purchase-points" params={{ realmId }}>
              {m['points.purchase_points_cta']()}
            </Link>
          </Button>
        </div>
      )}

      {/* Cross-bucket total bar — only when the user holds >= 2 buckets */}
      {showTotalBar && (
        <div
          className="flex items-center justify-between border-t border-border py-4"
          data-testid="user-points-cross-bucket-total"
        >
          <span className="text-sm text-muted-foreground">{m['points.cross_bucket_total']()}</span>
          <span className="text-2xl font-bold">{crossBucketTotal.toLocaleString()}</span>
        </div>
      )}

      {/* Bucket card stack / empty state */}
      {walletsLoading ? (
        <div className="space-y-4">
          <PointsUsageDashboard
            card={{
              bucketId: '',
              name: null,
              enabled: null,
              bucketTotal: 0,
              balancesByType: {
                subscription: 0,
                topup: 0,
                registration: 0,
                freePeriodic: 0,
                granted: 0,
              },
              // Loading skeleton — pool-only defaults (no quota windows yet).
              quotaWindows: undefined,
              spendableFromQuota: undefined,
              spendableFromPool: undefined,
              expiresAt: null,
            }}
            loading
          />
          <PointsBalanceCard
            card={{
              bucketId: '',
              name: null,
              enabled: null,
              bucketTotal: 0,
              balancesByType: {
                subscription: 0,
                topup: 0,
                registration: 0,
                freePeriodic: 0,
                granted: 0,
              },
              // Loading skeleton — pool-only defaults (no quota windows yet).
              quotaWindows: undefined,
              spendableFromQuota: undefined,
              spendableFromPool: undefined,
              expiresAt: null,
            }}
            loading
          />
        </div>
      ) : cards.length === 0 ? null : (
        <div className="space-y-4">
          {cards.map((card) => (
            <div key={card.bucketId || 'unnamed'} className="space-y-4">
              <PointsUsageDashboard card={card} />
              <PointsBalanceCard card={card} />
            </div>
          ))}
        </div>
      )}

      {/* Hide the transaction history card only for a genuinely-empty user:
          no transactions, not loading, AND no active filters. When a filter
          returns 0 results, keep the card visible so the empty-state
          (no-transactions) shows and the Clear button stays reachable —
          otherwise the user would be unable to recover from a 0-result filter. */}
      {!transactionsLoading && transactions.length === 0 && !hasActiveFilters ? null : (
        <section data-testid="user-points-transaction-history">
          <h2 className="text-base font-semibold">
            {m['points.user_points_transaction_history']()}
          </h2>
          <div className="mt-4 space-y-4 border-t border-border pt-4">
            <TransactionFilters
              filters={effectiveFilters}
              onChange={handleFiltersChange}
              onClear={handleFiltersClear}
              buckets={bucketOptions}
              admin={false}
              loading={transactionsLoading}
            />
            <TransactionHistoryTable
              transactions={transactions}
              loading={transactionsLoading && loadedPages === 1}
              filters={effectiveFilters}
              buckets={bucketOptions}
              admin={false}
            />
            {reachedLimit && (
              <p className="text-center text-sm text-muted-foreground">
                {m['points.transaction_load_limit_reached']({
                  count: MAX_VISIBLE_TRANSACTIONS.toLocaleString(),
                })}
              </p>
            )}
            {canLoadMore && (
              <div className="flex justify-center">
                <Button
                  variant="outline"
                  onClick={() => setLoadedPages((pages) => pages + 1)}
                  disabled={transactionsLoading}
                  data-testid="transaction-load-more"
                >
                  {m['points.transaction_load_more']()}
                </Button>
              </div>
            )}
          </div>
        </section>
      )}
    </div>
  )
}
