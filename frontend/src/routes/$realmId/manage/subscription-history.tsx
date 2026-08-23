import { useState } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { SubscriptionHistoryList } from '@/components/billing/subscription-history-list'
import { SubscriptionHistoryFilter } from '@/components/billing/subscription-history-filter'
import { globalSubscriptionHistoryQueryOptions, requireFeature } from '@/data/query-options'
import { initializeAuth } from '@/lib/auth-utils'
import { ADMIN_WEB_CONSOLE_CLIENT_ID } from '@/lib/constants/auth-constants'
import type { HistoryFilters } from '@/types/billing'
import { PageHeader, ListPagination } from '@/components/shared'
import { m } from '@/paraglide/messages'
import { useResolvedRealmId } from '@/lib/realm-routing'
import { getErrorMessage } from '@/lib/error-utils'

export const Route = createFileRoute('/$realmId/manage/subscription-history')({
  beforeLoad: async ({ context, params }) => {
    // Route beforeLoads run ahead of the __root loader, so on a cold reload
    // the feature query below would fire before initializeAuth restores the
    // Bearer token (401 → route error boundary). Idempotent: short-circuits
    // once the realm/client is initialized.
    await initializeAuth(params.realmId, ADMIN_WEB_CONSOLE_CLIENT_ID)
    await requireFeature(
      context.queryClient,
      params.realmId,
      (f) => f.admin.subscriptionHistoryVisible,
      {
        to: '/$realmId/manage',
        params: { realmId: params.realmId },
      }
    )
  },
  component: SubscriptionHistoryRoute,
})

export function SubscriptionHistoryRoute() {
  const realmId = useResolvedRealmId()

  // Filter state
  const [filters, setFilters] = useState<HistoryFilters>({
    sortBy: 'timestamp',
    sortOrder: 'desc',
  })
  const [page, setPage] = useState(1)
  const pageSize = 20

  // Query subscription history
  const {
    data: historyData,
    isLoading,
    error,
  } = useQuery(globalSubscriptionHistoryQueryOptions(realmId, filters, page, pageSize))

  // Handle filter changes
  function handleFiltersChange(newFilters: HistoryFilters) {
    setFilters(newFilters)
    setPage(1) // Reset to first page on filter change
  }

  // Handle filter reset
  function handleResetFilters() {
    setFilters({
      sortBy: 'timestamp',
      sortOrder: 'desc',
    })
    setPage(1)
  }

  // Handle page changes
  function handlePageChange(newPage: number) {
    setPage(newPage)
  }

  // Handle sort changes
  function handleSortChange(sortBy: string) {
    setFilters((prev) => ({
      ...prev,
      sortBy,
      sortOrder: prev.sortBy === sortBy && prev.sortOrder === 'desc' ? 'asc' : 'desc',
    }))
    setPage(1)
  }

  if (error) {
    return (
      <div className="space-y-6" data-testid="subscription-history-page">
        <Card className="border-destructive">
          <CardContent className="p-6">
            <p className="text-destructive">
              {m['billing.subscription_history_failed_load']({
                error: getErrorMessage(error),
              })}
            </p>
            <Button variant="outline" className="mt-4" onClick={() => window.location.reload()}>
              {m['common.retry']()}
            </Button>
          </CardContent>
        </Card>
      </div>
    )
  }

  return (
    <div className="space-y-6" data-testid="subscription-history-page">
      <PageHeader title={m['billing.subscription_history_page_title']()} />

      {/* Filters */}
      <SubscriptionHistoryFilter
        filters={filters}
        onFiltersChange={handleFiltersChange}
        onReset={handleResetFilters}
        loading={isLoading}
      />

      {/* History List */}
      <Card>
        <CardHeader>
          <CardTitle>{m['billing.subscription_history_events']()}</CardTitle>
        </CardHeader>
        <CardContent className="p-0">
          <SubscriptionHistoryList
            events={historyData?.events || []}
            loading={isLoading}
            onSortChange={handleSortChange}
          />
        </CardContent>
      </Card>

      {historyData?.pagination && historyData.pagination.totalCount > 0 && (
        <ListPagination
          page={historyData.pagination.page - 1}
          pageSize={pageSize}
          total={historyData.pagination.totalCount}
          onPageChange={(newPage) => handlePageChange(newPage + 1)}
          testIdPrefix="subscription-history-pagination"
        />
      )}
    </div>
  )
}
