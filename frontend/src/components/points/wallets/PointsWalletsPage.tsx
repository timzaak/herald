import { useMemo, useState } from 'react'
import { useQueries, useQuery } from '@tanstack/react-query'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { History, Search, Wallet } from 'lucide-react'
import { TransactionHistoryTable } from '../TransactionHistoryTable'
import { GrantPointsDialog } from '../grant-points-dialog'
import {
  adminUserQueryOptions,
  walletsByBucketQueryOptions,
  pointsTransactionsQueryOptions,
} from '@/data/query-options'
import { useBuckets } from '@/data/use-buckets'
import type { TransactionFilters } from '@/lib/schemas/points-forms'
import type { UserDetailResponse, WalletByBucketResponse } from '@/lib/api-generated'
import { formatPoolExpiry } from '../user-points-view'
import { DEFAULT_PAGE_SIZE, FILTER_ALL_VALUE } from '@/lib/constants'
import { PERMISSION } from '@/lib/constants/auth-constants'
import { PageHeader, ListPagination } from '@/components/shared'
import { usePermission } from '@/hooks/use-permission'
import { m } from '@/paraglide/messages'

interface PointsWalletsPageProps {
  realmId: string
}

const BALANCES_BY_TYPE_KEYS = [
  'subscription',
  'topup',
  'registration',
  'freePeriodic',
  'granted',
] as const

/**
 * Admin wallets view: realm-wide balances grouped by `(user, bucket)`.
 *
 * Source: `walletsByBucketQueryOptions` (`listWallets` SDK,
 * `points.view`). Unlike the user-facing page we consume the FULL
 * `items` set (every realm user × bucket) and the realm `crossBucketTotal`.
 *
 * LOUD (tech-debt resolution): this page previously consumed the
 * temporary `pointsWalletsQueryOptions` legacy adapter. The adapter and its
 * legacy view-model were removed in this item; rows are now driven directly by
 * `WalletByBucketResponse`. `WalletByBucketResponse.name`/`enabled` are
 * server-side "currently unset" (types.gen.ts), so we resolve name/enabled
 * via `useBuckets` by `bucketId`, falling back to a label when unresolved.
 */
export function PointsWalletsPage({ realmId }: PointsWalletsPageProps) {
  const { hasPermission } = usePermission()
  const [grantDialogOpen, setGrantDialogOpen] = useState(false)

  // UI state
  const [bucketFilter, setBucketFilter] = useState<string>(FILTER_ALL_VALUE)
  const [searchQuery, setSearchQuery] = useState<string>('')
  const [selectedRow, setSelectedRow] = useState<{ userId: string; bucketId: string } | null>(null)

  // Ephemeral transaction filters (type/dates/client-app). `userId` and
  // `bucketId` are derived from the selected row and merged below.
  const [transactionFilters, setTransactionFilters] = useState<TransactionFilters>({})
  const [transactionsPage, setTransactionsPage] = useState(1)

  // Queries — full realm items + realm cross-bucket total.
  const { data: walletsData, isLoading: walletsLoading } = useQuery(
    walletsByBucketQueryOptions(realmId)
  )

  // All buckets (incl. disabled) feed the Bucket filter Select and provide
  // name/enabled resolution (server returns these unset on wallet rows).
  const { buckets } = useBuckets(realmId)

  const bucketsById = useMemo(() => {
    const map = new Map<string, { id: string; name: string; enabled: boolean }>()
    buckets.forEach((b) => map.set(b.id, { id: b.id, name: b.name, enabled: b.enabled }))
    return map
  }, [buckets])

  const items = useMemo(() => walletsData?.items ?? [], [walletsData?.items])
  const crossBucketTotal = walletsData?.crossBucketTotal ?? 0

  // Client-side bucket + search filtering (endpoint has no client-side userId
  // filter; bucket filter is also applied locally since the page renders the
  // full realm cross-section).
  const filteredItems = useMemo(() => {
    return items.filter((row) => {
      if (bucketFilter !== FILTER_ALL_VALUE && (row.bucketId ?? '') !== bucketFilter) {
        return false
      }
      if (searchQuery.trim()) {
        const q = searchQuery.trim().toLowerCase()
        if (!row.userId.toLowerCase().includes(q)) {
          return false
        }
      }
      return true
    })
  }, [items, bucketFilter, searchQuery])

  // Resolve display names for every distinct user in the filtered set.
  const walletUserIds = useMemo(
    () => [...new Set(filteredItems.map((row) => row.userId))],
    [filteredItems]
  )

  const walletUserQueries = useQueries({
    queries: walletUserIds.map((userId) => ({
      ...adminUserQueryOptions(realmId, userId),
      enabled: walletUserIds.length > 0,
      retry: false,
    })),
  })

  const usersById = useMemo(() => {
    const users = new Map<string, UserDetailResponse>()
    walletUserQueries.forEach((query) => {
      if (query.data) {
        users.set(query.data.id, query.data)
      }
    })
    return users
  }, [walletUserQueries])

  // Transactions for the drilled-down (user, bucket) row.
  const { data: transactionsData, isLoading: transactionsLoading } = useQuery(
    pointsTransactionsQueryOptions(realmId, {
      userId: selectedRow?.userId,
      bucketId: selectedRow?.bucketId || undefined,
      page: transactionsPage,
      pageSize: DEFAULT_PAGE_SIZE,
      ...transactionFilters,
    })
  )

  /**
   * Resolve a bucket's display name. `WalletByBucketResponse.name` is
   * server-side "currently unset", so we prefer `useBuckets` and fall back to
   * the row's own `name`, a truncated id, or an unassigned label.
   */
  function resolveBucketName(bucketId: string, rowName?: string | null): string {
    if (!bucketId) return m['points.admin_wallets_bucket_unassigned']()
    const resolved = bucketsById.get(bucketId)
    if (resolved) return resolved.name
    if (rowName) return rowName
    return bucketId.slice(0, 8)
  }

  function resolveBucketEnabled(bucketId: string, rowEnabled?: boolean | null): boolean {
    if (rowEnabled !== null && rowEnabled !== undefined) return rowEnabled
    const resolved = bucketId ? bucketsById.get(bucketId) : undefined
    return resolved ? resolved.enabled : true
  }

  function handleRowSelect(userId: string, bucketId: string) {
    setSelectedRow({ userId, bucketId })
    setTransactionsPage(1)
    setTransactionFilters({})
  }

  function handleBucketFilterChange(value: string) {
    setBucketFilter(value)
    setSelectedRow(null)
    setTransactionsPage(1)
    setTransactionFilters({})
  }

  function handleSearchChange(value: string) {
    setSearchQuery(value)
    setSelectedRow(null)
    setTransactionsPage(1)
    setTransactionFilters({})
  }

  function renderWalletRow(row: WalletByBucketResponse) {
    const user = usersById.get(row.userId)
    const displayName = user?.nickname || user?.email || row.userId
    const bucketId = row.bucketId ?? ''
    const enabled = resolveBucketEnabled(bucketId, row.enabled)
    const rowTestId = `admin-wallet-row-${row.userId}-${bucketId}`
    const isSelected = selectedRow?.userId === row.userId && selectedRow?.bucketId === bucketId

    return (
      <div
        key={`${row.userId}|${bucketId}`}
        className={`p-4 border rounded-lg cursor-pointer hover:bg-muted/50 transition-colors ${
          isSelected ? 'bg-muted border-primary' : ''
        }`}
        onClick={() => handleRowSelect(row.userId, bucketId)}
        data-testid={rowTestId}
      >
        <div className="flex items-center justify-between gap-4">
          <div className="min-w-0">
            <div className="truncate font-medium">{displayName}</div>
            {user?.nickname && user.email && (
              <div className="truncate text-sm text-muted-foreground">{user.email}</div>
            )}
            <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
              <span className="truncate">
                {m['points.wallets_user_id_label']()}: {row.userId}
              </span>
              <span>·</span>
              <span className="truncate">
                {m['points.transaction_bucket_column']()}: {resolveBucketName(bucketId, row.name)}
              </span>
              <span>·</span>
              <span
                className="truncate"
                data-testid={`admin-wallet-row-expiry-${row.userId}-${bucketId}`}
              >
                {formatPoolExpiry(row.expiresAt)}
              </span>
              {!enabled && <Badge variant="secondary">{m['points.bucket_card_disabled']()}</Badge>}
            </div>
          </div>
          <div className="shrink-0 text-right">
            <div className="text-2xl font-bold">{row.bucketTotal.toLocaleString()}</div>
            <div className="mt-1 flex flex-wrap justify-end gap-1">
              {BALANCES_BY_TYPE_KEYS.map((typeKey) => {
                const value = row.balancesByType[typeKey]
                if (!value) return null
                return (
                  <Badge key={typeKey} variant="outline">
                    {m[`points.balance_type_${typeKey}`]({ count: value.toLocaleString() })}
                  </Badge>
                )
              })}
            </div>
          </div>
        </div>
      </div>
    )
  }

  return (
    <>
      <div className="space-y-6" data-testid="points-wallets-page">
        <PageHeader
          title={m['points.admin_wallets_page_title']()}
          action={{
            label: m['points.wallets_grant_points_button'](),
            onClick: () => setGrantDialogOpen(true),
            testId: 'grant-points-button',
            show: hasPermission(PERMISSION.POINTS_MANAGE),
          }}
        />

        {/* Realm cross-bucket total (spans every user × bucket in the realm) */}
        <Card data-testid="admin-wallets-cross-bucket-total">
          <CardContent className="py-4">
            <div className="flex items-center justify-between">
              <span className="flex items-center gap-2 text-sm text-muted-foreground">
                <Wallet className="h-4 w-4" />
                {m['points.admin_wallets_realm_total']()}
              </span>
              <span className="text-2xl font-bold">{crossBucketTotal.toLocaleString()}</span>
            </div>
          </CardContent>
        </Card>

        {/* Wallet rows grouped by (user, bucket) */}
        <Card>
          <CardHeader>
            <div className="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
              <CardTitle>{m['points.admin_wallets_card_title']()}</CardTitle>
              <div className="flex flex-col gap-2 sm:flex-row sm:items-end">
                <div className="space-y-1">
                  <Label htmlFor="admin-wallets-bucket-filter" className="text-xs">
                    {m['points.filter_bucket_label']()}
                  </Label>
                  <Select value={bucketFilter} onValueChange={handleBucketFilterChange}>
                    <SelectTrigger
                      id="admin-wallets-bucket-filter"
                      className="w-full sm:w-56"
                      data-testid="admin-wallets-bucket-filter"
                    >
                      <SelectValue placeholder={m['points.filter_bucket_all']()} />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value={FILTER_ALL_VALUE}>
                        {m['points.filter_bucket_all']()}
                      </SelectItem>
                      {buckets.map((bucket) => (
                        <SelectItem key={bucket.id} value={bucket.id}>
                          {bucket.name}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <div className="w-full space-y-1 sm:w-72">
                  <Label htmlFor="wallets-search-input" className="text-xs">
                    {m['points.admin_wallets_search_label']()}
                  </Label>
                  <div className="relative">
                    <Search className="absolute left-3 top-2.5 h-4 w-4 text-muted-foreground" />
                    <Input
                      id="wallets-search-input"
                      placeholder={m['points.wallets_search_placeholder']()}
                      value={searchQuery}
                      onChange={(e) => handleSearchChange(e.target.value)}
                      className="pl-10"
                      data-testid="wallets-search-input"
                    />
                  </div>
                </div>
              </div>
            </div>
          </CardHeader>
          <CardContent>
            {walletsLoading ? (
              <div className="text-center py-8">{m['points.wallets_loading']()}</div>
            ) : filteredItems.length > 0 ? (
              <div className="space-y-2">{filteredItems.map((row) => renderWalletRow(row))}</div>
            ) : (
              <div
                className="text-center py-8 text-muted-foreground"
                data-testid="admin-wallets-empty"
              >
                {bucketFilter !== FILTER_ALL_VALUE || searchQuery
                  ? m['points.admin_wallets_empty_filtered']()
                  : m['points.admin_wallets_empty']()}
              </div>
            )}
          </CardContent>
        </Card>

        {/* Drilldown: single user × single bucket ledger */}
        {selectedRow &&
          (() => {
            const selectedUser = usersById.get(selectedRow.userId)
            const selectedUserName =
              selectedUser?.nickname || selectedUser?.email || selectedRow.userId
            const selectedBucketName = resolveBucketName(selectedRow.bucketId)
            return (
              <div className="space-y-4">
                <Card>
                  <CardHeader>
                    <CardTitle className="flex items-center gap-2">
                      <History className="h-4 w-4" />
                      {m['points.admin_wallets_drilldown_title']({
                        user: selectedUserName,
                        bucket: selectedBucketName,
                      })}
                    </CardTitle>
                  </CardHeader>
                  <CardContent>
                    <TransactionHistoryTable
                      transactions={transactionsData?.transactions || []}
                      loading={transactionsLoading}
                      filters={{
                        ...transactionFilters,
                        bucketId: selectedRow.bucketId || undefined,
                      }}
                      buckets={
                        selectedRow.bucketId
                          ? [{ id: selectedRow.bucketId, name: selectedBucketName }]
                          : []
                      }
                      admin={true}
                    />
                  </CardContent>
                </Card>
                {transactionsData && transactionsData.total > 0 && (
                  <ListPagination
                    page={transactionsPage - 1}
                    pageSize={DEFAULT_PAGE_SIZE}
                    total={transactionsData.total}
                    onPageChange={(page) => setTransactionsPage(page + 1)}
                    testIdPrefix="transaction-pagination"
                  />
                )}
              </div>
            )
          })()}
      </div>
      {grantDialogOpen && (
        <GrantPointsDialog
          open={grantDialogOpen}
          onOpenChange={setGrantDialogOpen}
          realmId={realmId}
        />
      )}
    </>
  )
}
