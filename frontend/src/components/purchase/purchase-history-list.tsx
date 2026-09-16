import { m } from '@/paraglide/messages'
import { memo, useMemo } from 'react'
import { format } from 'date-fns'
import { ChevronRight, Calendar, Coins, CreditCard, AlertCircle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Badge } from '@/components/ui/badge'
import { Skeleton } from '@/components/ui/skeleton'
import type { PurchaseHistoryItem } from '@/lib/api-generated'
import { formatInvoiceAmount, getPaymentStatusBadgeVariant } from '@/lib/invoice-utils'
import { InvoiceApplyRowButton } from '@/components/billing/invoices/invoice-apply-row-button'
import { getErrorMessage } from '@/lib/error-utils'

interface PurchaseHistoryListProps {
  purchases: PurchaseHistoryItem[]
  isLoading: boolean
  error?: unknown
  onDetailsClick: (attemptId: string) => void
  /**
   * Render a per-row Invoice button gated by the apply-eligibility API.
   * Only rendered when the outer realm-level `invoicesVisible` gate is open
   * AND the purchase is not a Stripe transaction (Stripe invoices are pushed
   * via webhook — users apply through "My Invoices", never manually). The
   * button itself reflects per-resource eligibility (P1-3/P1-4).
   */
  realmId?: string
  onApplyInvoice?: (attemptId: string) => void
}

interface HistoryTableRowProps {
  realmId?: string
  purchase: PurchaseHistoryItem
  onDetailsClick: (attemptId: string) => void
  onApplyInvoice?: (attemptId: string) => void
}

const HistoryTableRow = memo(
  ({ realmId, purchase, onDetailsClick, onApplyInvoice }: HistoryTableRowProps) => {
    const timestamp = useMemo(() => {
      try {
        return format(new Date(purchase.createdAt), 'PPp')
      } catch {
        return purchase.createdAt
      }
    }, [purchase.createdAt])

    return (
      <TableRow data-testid={`purchase-history-item-${purchase.attemptId}`}>
        <TableCell className="whitespace-nowrap font-medium">{timestamp}</TableCell>
        <TableCell>
          <div className="max-w-[140px] truncate md:max-w-none md:whitespace-normal">
            {purchase.productName ?? (
              <span className="text-sm text-muted-foreground">
                {m['points.purchase_history_unknown_product']()}
              </span>
            )}
          </div>
        </TableCell>
        <TableCell>
          {purchase.points != null ? (
            <div className="flex items-center gap-2">
              <Coins className="h-4 w-4 text-primary" />
              <span className="font-medium">{purchase.points.toLocaleString()}</span>
            </div>
          ) : (
            <span className="text-sm text-muted-foreground">--</span>
          )}
        </TableCell>
        <TableCell className="hidden md:table-cell">
          <div className="text-sm">{formatInvoiceAmount(purchase.amount, purchase.currency)}</div>
        </TableCell>
        <TableCell className="hidden md:table-cell">
          <div className="flex items-center gap-2">
            <CreditCard className="h-4 w-4 text-muted-foreground" />
            <span className="text-sm">{purchase.paymentProvider}</span>
          </div>
        </TableCell>
        <TableCell className="hidden md:table-cell">
          <Badge variant={getPaymentStatusBadgeVariant(purchase.status)} className="text-xs">
            {purchase.status}
          </Badge>
        </TableCell>
        <TableCell className="whitespace-nowrap text-right">
          <div className="flex justify-end gap-1">
            {onApplyInvoice && realmId && purchase.paymentProvider !== 'stripe' && (
              <InvoiceApplyRowButton
                realmId={realmId}
                referenceType="payment_attempt"
                referenceId={purchase.attemptId}
                onApply={() => onApplyInvoice(purchase.attemptId)}
                srLabel={m['points.purchase_history_invoice_sr']()}
                variant="ghost"
                testIdPrefix={`purchase-history-invoice-button-${purchase.attemptId}`}
              />
            )}
            <Button
              variant="ghost"
              size="sm"
              onClick={() => onDetailsClick(purchase.attemptId)}
              data-testid={`purchase-history-details-button-${purchase.attemptId}`}
            >
              <ChevronRight className="h-4 w-4" />
              <span className="sr-only">{m['points.purchase_history_details_sr']()}</span>
            </Button>
          </div>
        </TableCell>
      </TableRow>
    )
  }
)

HistoryTableRow.displayName = 'HistoryTableRow'

export function PurchaseHistoryList({
  purchases,
  isLoading,
  error,
  onDetailsClick,
  realmId,
  onApplyInvoice,
}: PurchaseHistoryListProps) {
  if (isLoading) {
    return (
      <div data-testid="purchase-history-loading" className="space-y-4">
        {[...Array(5)].map((_, i) => (
          <div key={i} className="flex items-center space-x-4">
            <Skeleton className="h-12 w-full" />
          </div>
        ))}
      </div>
    )
  }

  if (error) {
    return (
      <div
        data-testid="purchase-history-error"
        className="flex flex-col items-center justify-center py-12 text-center"
      >
        <AlertCircle className="mb-4 h-12 w-12 text-destructive" />
        <h3 className="text-lg font-semibold">{m['points.purchase_history_error_title']()}</h3>
        <p className="text-sm text-muted-foreground">{getErrorMessage(error)}</p>
      </div>
    )
  }

  if (!purchases || purchases.length === 0) {
    return (
      <div
        data-testid="purchase-history-empty"
        className="flex flex-col items-center justify-center py-12 text-center"
      >
        <Calendar className="mb-4 h-12 w-12 text-muted-foreground" />
        <h3 className="text-lg font-semibold">{m['points.purchase_history_empty_title']()}</h3>
        <p className="text-sm text-muted-foreground">
          {m['points.purchase_history_empty_description']()}
        </p>
      </div>
    )
  }

  return (
    <div data-testid="purchase-history-list" className="rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>{m['points.purchase_history_date']()}</TableHead>
            <TableHead>{m['points.purchase_history_product']()}</TableHead>
            <TableHead>{m['points.purchase_history_points']()}</TableHead>
            <TableHead className="hidden md:table-cell">
              {m['points.purchase_history_amount']()}
            </TableHead>
            <TableHead className="hidden md:table-cell">
              {m['points.purchase_history_provider']()}
            </TableHead>
            <TableHead className="hidden md:table-cell">
              {m['points.purchase_history_status']()}
            </TableHead>
            <TableHead className="text-right">{m['points.purchase_history_actions']()}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {purchases.map((purchase) => (
            <HistoryTableRow
              key={purchase.attemptId}
              realmId={realmId}
              purchase={purchase}
              onDetailsClick={onDetailsClick}
              onApplyInvoice={onApplyInvoice}
            />
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
