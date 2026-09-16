import { m } from '@/paraglide/messages'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { format } from 'date-fns'
import { CreditCard, Package } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import type { PurchaseHistoryItem } from '@/lib/api-generated'
import { formatInvoiceAmount, getPaymentStatusBadgeVariant } from '@/lib/invoice-utils'

interface PurchaseDetailsDialogProps {
  purchase: PurchaseHistoryItem | null
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function PurchaseDetailsDialog({
  purchase,
  open,
  onOpenChange,
}: PurchaseDetailsDialogProps) {
  if (!open) return null

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent data-testid="purchase-details-dialog" className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>{m['points.purchase_details_title']()}</DialogTitle>
          <DialogDescription>{m['points.purchase_details_description']()}</DialogDescription>
        </DialogHeader>

        {purchase ? (
          <div className="space-y-6">
            {/* Purchase Information */}
            <div className="rounded-lg border p-4" data-testid="purchase-details-package-info">
              <div className="mb-3 flex items-center gap-2">
                <Package className="h-5 w-5 text-primary" />
                <h3 className="font-semibold">{m['points.purchase_details_package_info']()}</h3>
              </div>
              <div className="space-y-2 text-sm">
                <div className="flex flex-wrap justify-between gap-x-4 gap-y-0.5">
                  <span className="text-muted-foreground">
                    {m['points.purchase_details_product_name']()}
                  </span>
                  <span className="min-w-0 break-all text-right font-medium">
                    {purchase.productName ?? 'N/A'}
                  </span>
                </div>
                <div className="flex flex-wrap justify-between gap-x-4 gap-y-0.5">
                  <span className="text-muted-foreground">
                    {m['points.purchase_details_mapping_id']()}
                  </span>
                  <span className="min-w-0 break-all text-right font-mono text-xs">
                    {purchase.targetMappingId}
                  </span>
                </div>
                <div className="flex justify-between">
                  <span className="text-muted-foreground">
                    {m['points.purchase_details_points']()}
                  </span>
                  <span className="font-medium">
                    {purchase.points != null ? purchase.points.toLocaleString() : '--'}
                  </span>
                </div>
                <div className="flex justify-between">
                  <span className="text-muted-foreground">
                    {m['points.purchase_details_amount']()}
                  </span>
                  <span className="font-medium">
                    {formatInvoiceAmount(purchase.amount, purchase.currency)}
                  </span>
                </div>
              </div>
            </div>

            {/* Payment Information */}
            <div className="rounded-lg border p-4" data-testid="purchase-details-payment-info">
              <div className="mb-3 flex items-center gap-2">
                <CreditCard className="h-5 w-5 text-primary" />
                <h3 className="font-semibold">{m['points.purchase_details_payment_info']()}</h3>
              </div>
              <div className="space-y-2 text-sm">
                <div className="flex justify-between">
                  <span className="text-muted-foreground">
                    {m['points.purchase_details_provider']()}
                  </span>
                  <span className="font-medium">{purchase.paymentProvider}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-muted-foreground">
                    {m['points.purchase_details_status']()}
                  </span>
                  <Badge
                    variant={getPaymentStatusBadgeVariant(purchase.status)}
                    className="text-xs"
                  >
                    {purchase.status}
                  </Badge>
                </div>
                <div className="flex flex-wrap justify-between gap-x-4 gap-y-0.5">
                  <span className="text-muted-foreground">
                    {m['points.purchase_details_attempt_id']()}
                  </span>
                  <span className="min-w-0 break-all text-right font-mono text-xs">
                    {purchase.attemptId}
                  </span>
                </div>
                <div className="flex justify-between">
                  <span className="text-muted-foreground">
                    {m['points.purchase_details_purchased']()}
                  </span>
                  <span className="font-medium">{format(new Date(purchase.createdAt), 'PPp')}</span>
                </div>
                {purchase.completedAt && (
                  <div className="flex justify-between">
                    <span className="text-muted-foreground">
                      {m['points.purchase_details_completed_at']()}
                    </span>
                    <span className="font-medium">
                      {format(new Date(purchase.completedAt), 'PPp')}
                    </span>
                  </div>
                )}
              </div>
            </div>
          </div>
        ) : (
          <div className="py-8 text-center text-sm text-muted-foreground">
            {m['points.purchase_details_load_failed']()}
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}
