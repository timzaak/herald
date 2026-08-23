import { useCallback, useState } from 'react'
import { createFileRoute, Link, useNavigate } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { Plus } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { ListPagination, PageHeader } from '@/components/shared'
import { PurchaseHistoryList } from '@/components/purchase/purchase-history-list'
import { PurchaseDetailsDialog } from '@/components/purchase/purchase-details-dialog'
import {
  userFeatureAvailabilityQueryOptions,
  purchaseHistoryQueryOptions,
  requireUserFeature,
} from '@/data/query-options'
import { initializeAuth } from '@/lib/auth-utils'
import { USER_ACCOUNT_CENTER_CLIENT_ID } from '@/lib/constants/auth-constants'
import { DEFAULT_PAGE_SIZE } from '@/lib/constants'
import type { PurchaseHistoryItem } from '@/lib/api-generated'
import { m } from '@/paraglide/messages'
import { realmPath, useResolvedRealmContext } from '@/lib/realm-routing'

export const Route = createFileRoute('/$realmId/user/subscription-history')({
  beforeLoad: async ({ context, params }) => {
    // Route beforeLoads run ahead of the __root loader, so on a cold reload
    // the feature query below would fire before initializeAuth restores the
    // Bearer token (401 → route error boundary). Idempotent: short-circuits
    // once the realm/client is initialized.
    await initializeAuth(params.realmId, USER_ACCOUNT_CENTER_CLIENT_ID)
    await requireUserFeature(context.queryClient, (f) => f.user.pointsVisible, {
      to: '/$realmId/user/profile',
      params: { realmId: params.realmId },
    })
  },
  component: PurchaseRecordsRoute,
})

export function PurchaseRecordsRoute() {
  const realmContext = useResolvedRealmContext()
  const realmId = realmContext.realmId
  const navigate = useNavigate()
  const [purchaseHistoryPage, setPurchaseHistoryPage] = useState(1)
  const [selectedPurchase, setSelectedPurchase] = useState<PurchaseHistoryItem | null>(null)

  const { data: purchaseHistoryData, isLoading: purchaseHistoryLoading } = useQuery(
    purchaseHistoryQueryOptions(realmId, {
      page: purchaseHistoryPage,
      pageSize: DEFAULT_PAGE_SIZE,
    })
  )
  const { data: features } = useQuery(userFeatureAvailabilityQueryOptions)
  const invoicesVisible = features?.user.invoicesVisible === true
  const canPurchasePoints = features?.user.pointsVisible === true

  const handleDetailsClick = useCallback(
    (attemptId: string) => {
      const purchase = purchaseHistoryData?.items?.find((p) => p.attemptId === attemptId)
      if (purchase) setSelectedPurchase(purchase)
    },
    [purchaseHistoryData?.items]
  )

  const handleApplyInvoice = useCallback(
    (attemptId: string) => {
      navigate({
        to: realmPath(realmContext, '/user/invoices/new'),
        search: {
          paymentAttemptId: attemptId,
          returnTo: realmPath(realmContext, '/user/subscription-history'),
        },
      })
    },
    [realmContext, navigate]
  )

  return (
    <div className="space-y-6" data-testid="purchase-records-page">
      <div className="flex items-center justify-between gap-4">
        <PageHeader title={m['billing.purchase_records_page_title']()} />
        {canPurchasePoints && (
          <Button asChild data-testid="purchase-records-purchase-points-button">
            <Link to={realmPath(realmContext, '/user/purchase-points')}>
              <Plus className="mr-2 h-4 w-4" />
              {m['points.user_points_purchase_button']()}
            </Link>
          </Button>
        )}
      </div>

      <section>
        <h2 className="text-base font-semibold">{m['billing.purchase_records_history_title']()}</h2>
        <div className="mt-4 space-y-4 border-t border-border pt-4">
          <PurchaseHistoryList
            purchases={purchaseHistoryData?.items || []}
            isLoading={purchaseHistoryLoading}
            onDetailsClick={handleDetailsClick}
            realmId={invoicesVisible ? realmId : undefined}
            onApplyInvoice={invoicesVisible ? handleApplyInvoice : undefined}
          />
          {purchaseHistoryData && purchaseHistoryData.total > 0 && (
            <ListPagination
              page={purchaseHistoryPage - 1}
              pageSize={DEFAULT_PAGE_SIZE}
              total={purchaseHistoryData.total}
              onPageChange={(page) => setPurchaseHistoryPage(page + 1)}
              testIdPrefix="purchase-records-pagination"
            />
          )}
        </div>
      </section>

      <PurchaseDetailsDialog
        purchase={selectedPurchase}
        open={selectedPurchase !== null}
        onOpenChange={(open) => {
          if (!open) setSelectedPurchase(null)
        }}
      />
    </div>
  )
}
