import { Badge } from '@/components/ui/badge'
import { getWebhookStatusLabel } from '@/lib/constants/webhook'

interface WebhookStatusBadgeProps {
  status: string
}

/** Three-state subscription badge: paused is system-only and warning-toned. */
export function WebhookStatusBadge({ status }: WebhookStatusBadgeProps) {
  if (status === 'paused') {
    return (
      <Badge
        variant="outline"
        className="border-warning/50 bg-warning/10 text-warning"
        data-testid="webhook-status-badge"
      >
        {getWebhookStatusLabel(status)}
      </Badge>
    )
  }
  return (
    <Badge
      variant={status === 'active' ? 'default' : 'secondary'}
      data-testid="webhook-status-badge"
    >
      {getWebhookStatusLabel(status)}
    </Badge>
  )
}
