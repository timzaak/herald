import { memo, useMemo } from 'react'
import { format } from 'date-fns'
import { Clock, User, ArrowUpDown } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { HistoryEventBadge } from './history-event-badge'
import type { SubscriptionHistoryEventWithUser } from '@/types/billing'
import { m } from '@/paraglide/messages'

interface SubscriptionHistoryListProps {
  events: SubscriptionHistoryEventWithUser[]
  loading?: boolean
  onSortChange?: (sortBy: string) => void
  className?: string
}

interface HistoryTableRowProps {
  event: SubscriptionHistoryEventWithUser
  index: number
}

const HistoryTableRow = memo(({ event, index }: HistoryTableRowProps) => {
  const timestamp = useMemo(() => format(new Date(event.timestamp), 'PPp'), [event.timestamp])

  return (
    <TableRow data-testid={`history-row-${index}`}>
      <TableCell className="font-medium">{timestamp}</TableCell>
      <TableCell>
        <HistoryEventBadge eventType={event.eventType} />
      </TableCell>
      <TableCell>
        <div className="space-y-1">
          <div className="text-sm font-medium">
            {event.subscription.entitlementKey || m['billing.subscription_history_unknown_plan']()}
          </div>
          <div className="text-xs text-muted-foreground">{event.subscription.status}</div>
        </div>
      </TableCell>
      <TableCell>
        {event.user ? (
          <div className="space-y-1">
            <div className="max-w-xs truncate text-sm" title={event.user.email}>
              {event.user.email}
            </div>
            <div className="text-xs text-muted-foreground">{event.user.id}</div>
          </div>
        ) : (
          <span className="text-sm text-muted-foreground">-</span>
        )}
      </TableCell>
      <TableCell>
        <span className="text-sm">{event.actor || '-'}</span>
      </TableCell>
    </TableRow>
  )
})

HistoryTableRow.displayName = 'HistoryTableRow'

export function SubscriptionHistoryList({
  events,
  loading,
  onSortChange,
  className,
}: SubscriptionHistoryListProps) {
  function handleSort(sortBy: string) {
    onSortChange?.(sortBy)
  }

  return (
    <div className={`rounded-md border ${className ?? ''}`} data-testid="subscription-history-list">
      <div className="overflow-x-auto">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-auto p-0 font-medium"
                  onClick={() => handleSort('timestamp')}
                >
                  <Clock className="mr-2 h-3.5 w-3.5" />
                  {m['billing.subscription_history_list_timestamp']()}
                  <ArrowUpDown className="ml-2 h-3.5 w-3.5" />
                </Button>
              </TableHead>
              <TableHead>{m['billing.subscription_history_list_event_type']()}</TableHead>
              <TableHead>{m['billing.subscription_history_list_subscription']()}</TableHead>
              <TableHead>
                <User className="mr-2 h-3.5 w-3.5 inline" />
                {m['billing.subscription_history_list_user']()}
              </TableHead>
              <TableHead>{m['billing.subscription_history_list_actor']()}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {loading ? (
              <TableRow>
                <TableCell colSpan={5} className="h-24 text-center">
                  {m['billing.subscription_history_loading']()}
                </TableCell>
              </TableRow>
            ) : events.length === 0 ? (
              <TableRow>
                <TableCell colSpan={5} className="h-24 text-center text-muted-foreground">
                  {m['billing.subscription_history_no_events']()}
                </TableCell>
              </TableRow>
            ) : (
              events.map((event, index) => (
                <HistoryTableRow key={event.id} event={event} index={index} />
              ))
            )}
          </TableBody>
        </Table>
      </div>
    </div>
  )
}
