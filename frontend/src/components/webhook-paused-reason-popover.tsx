import type { WebhookSubscriptionItem } from '@/lib/api-generated'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Button } from '@/components/ui/button'
import { CircleAlert } from 'lucide-react'
import { formatDate } from '@/lib/date-utils'
import { m } from '@/paraglide/messages'

interface WebhookPausedReasonPopoverProps {
  subscription: WebhookSubscriptionItem
}

export function WebhookPausedReasonPopover({ subscription }: WebhookPausedReasonPopoverProps) {
  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button
          variant="ghost"
          size="sm"
          className="h-7 gap-1 px-2 text-warning"
          data-testid="webhook-paused-reason-trigger"
        >
          <CircleAlert className="h-3.5 w-3.5" />
          {m['webhooks.paused_reason_trigger']()}
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-80 space-y-3" data-testid="webhook-paused-reason-popover">
        <p className="text-sm font-medium">{m['webhooks.paused_reason_title']()}</p>
        <div className="space-y-1 text-xs text-muted-foreground">
          <p className="break-all">
            {m['webhooks.paused_last_error']({ error: subscription.lastError ?? '—' })}
          </p>
          <p>{m['webhooks.paused_failure_count']({ count: subscription.consecutiveFailures })}</p>
          {subscription.pausedAt && (
            <p>{m['webhooks.paused_at']({ time: formatDate(subscription.pausedAt) })}</p>
          )}
        </div>
        <p className="border-t pt-2 text-xs text-muted-foreground">
          {m['webhooks.no_backfill_hint']()}
        </p>
      </PopoverContent>
    </Popover>
  )
}
