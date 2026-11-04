import type { InvoiceStatus } from '@/lib/api-generated'
import { Badge } from '@/components/ui/badge'
import {
  getInvoiceStatusLabel,
  INVOICE_STATUS_COLORS,
  isExternalInvoice,
  getProviderLabel,
} from '@/lib/invoice-utils'

interface InvoiceStatusBadgeProps {
  status: InvoiceStatus
  provider?: string
}

export function InvoiceStatusBadge({ status, provider }: InvoiceStatusBadgeProps) {
  const label = getInvoiceStatusLabel(status)

  const statusBadgeEl = getStatusBadge(status, label)

  if (provider && isExternalInvoice(provider)) {
    return (
      <div className="flex items-center gap-2">
        {statusBadgeEl}
        <Badge variant="secondary" className="bg-info/10 text-info border-info/20 text-xs">
          {getProviderLabel(provider)}
        </Badge>
      </div>
    )
  }

  return statusBadgeEl
}

function getStatusBadge(status: InvoiceStatus, label: string) {
  if (status === 'paid') {
    return (
      <Badge className="border-transparent bg-success text-white shadow hover:bg-success/80">
        {label}
      </Badge>
    )
  }
  if (status === 'overdue') {
    return (
      <Badge className="border-transparent bg-warning text-white shadow hover:bg-warning/80">
        {label}
      </Badge>
    )
  }

  const variant = INVOICE_STATUS_COLORS[status] as
    | 'default'
    | 'secondary'
    | 'destructive'
    | 'outline'
    | undefined

  return <Badge variant={variant ?? 'secondary'}>{label}</Badge>
}
