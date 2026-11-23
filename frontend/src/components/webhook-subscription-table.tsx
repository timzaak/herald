import { type ColumnDef } from '@tanstack/react-table'
import { DataTable } from '@/components/shared'
import type { WebhookSubscriptionItem } from '@/lib/api-generated'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { formatDate } from '@/lib/date-utils'
import { WEBHOOK_CATEGORY_LABEL } from '@/lib/schemas/webhook-forms'
import { m } from '@/paraglide/messages'
import { WebhookStatusBadge } from '@/components/webhook-status-badge'
import { WebhookPausedReasonPopover } from '@/components/webhook-paused-reason-popover'

// Backend state machine → the single transition each status offers (paused is
// system-only, so its only action is resume).
const STATUS_ACTION: Record<
  string,
  { label: () => string; target: 'active' | 'disabled'; testId: string }
> = {
  active: {
    label: () => m['webhooks.disable_button'](),
    target: 'disabled',
    testId: 'webhook-disable-button',
  },
  paused: {
    label: () => m['webhooks.resume_button'](),
    target: 'active',
    testId: 'webhook-resume-button',
  },
  disabled: {
    label: () => m['webhooks.enable_button'](),
    target: 'active',
    testId: 'webhook-enable-button',
  },
}

interface WebhookColumnConfig {
  onEdit?: (subscription: WebhookSubscriptionItem) => void
  onSetStatus?: (subscription: WebhookSubscriptionItem, status: 'active' | 'disabled') => void
  onRotateSecret?: (subscription: WebhookSubscriptionItem) => void
  onDelete?: (subscription: WebhookSubscriptionItem) => void
  canManage?: boolean
  statusPendingIds?: ReadonlySet<string>
}

interface WebhookSubscriptionTableProps extends WebhookColumnConfig {
  data?: WebhookSubscriptionItem[]
  isLoading?: boolean
  error?: Error | null
}

function createWebhookColumns(config: WebhookColumnConfig): ColumnDef<WebhookSubscriptionItem>[] {
  const {
    onEdit,
    onSetStatus,
    onRotateSecret,
    onDelete,
    canManage = true,
    statusPendingIds,
  } = config
  return [
    {
      id: 'endpointUrl',
      accessorKey: 'endpointUrl',
      header: m['webhooks.table_endpoint_url'](),
      cell: ({ row }) => (
        <span
          className="block max-w-72 truncate font-mono text-sm"
          title={row.original.endpointUrl}
          data-testid="webhook-endpoint-url"
        >
          {row.original.endpointUrl}
        </span>
      ),
    },
    {
      id: 'eventCategories',
      header: m['webhooks.table_categories'](),
      cell: ({ row }) => (
        <div className="flex flex-wrap gap-1" data-testid="webhook-categories">
          {row.original.eventCategories.map((category) => (
            <Badge key={category} variant="secondary">
              {WEBHOOK_CATEGORY_LABEL[category]?.() ?? category}
            </Badge>
          ))}
        </div>
      ),
    },
    {
      id: 'status',
      header: m['webhooks.table_status'](),
      cell: ({ row }) => (
        <div className="flex items-center gap-1">
          <WebhookStatusBadge status={row.original.status} />
          {row.original.status === 'paused' && (
            <WebhookPausedReasonPopover subscription={row.original} />
          )}
        </div>
      ),
    },
    {
      id: 'secretHint',
      header: m['webhooks.table_secret_hint'](),
      cell: ({ row }) => (
        <span className="font-mono text-sm" data-testid="webhook-secret-hint">
          {row.original.secretHint}
        </span>
      ),
    },
    {
      id: 'createdAt',
      accessorKey: 'createdAt',
      header: m['webhooks.table_created_at'](),
      cell: ({ row }) => (
        <span data-testid="webhook-created-at">{formatDate(row.original.createdAt)}</span>
      ),
    },
    {
      id: 'actions',
      header: m['webhooks.table_actions'](),
      cell: ({ row }) => {
        const subscription = row.original
        const statusDisabled = !canManage || statusPendingIds?.has(subscription.id) === true
        const statusAction = STATUS_ACTION[subscription.status]
        return (
          <div className="flex gap-2" data-testid="webhook-actions">
            <Button
              variant="ghost"
              size="sm"
              onClick={() => onEdit?.(subscription)}
              data-testid="edit-webhook-button"
              disabled={!canManage}
              title={!canManage ? m['webhooks.action_disabled_title']() : undefined}
            >
              {m['webhooks.edit_button']()}
            </Button>
            {statusAction && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => onSetStatus?.(subscription, statusAction.target)}
                data-testid={statusAction.testId}
                disabled={statusDisabled}
                title={!canManage ? m['webhooks.action_disabled_title']() : undefined}
              >
                {statusAction.label()}
              </Button>
            )}
            <Button
              variant="ghost"
              size="sm"
              onClick={() => onRotateSecret?.(subscription)}
              data-testid="rotate-webhook-secret-button"
              disabled={!canManage}
              title={!canManage ? m['webhooks.action_disabled_title']() : undefined}
            >
              {m['webhooks.rotate_button']()}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => onDelete?.(subscription)}
              data-testid="delete-webhook-button"
              disabled={!canManage}
              title={!canManage ? m['webhooks.action_disabled_title']() : undefined}
            >
              {m['webhooks.delete_button']()}
            </Button>
          </div>
        )
      },
    },
  ]
}

export function WebhookSubscriptionTable({
  data,
  isLoading = false,
  error,
  onEdit,
  onSetStatus,
  onRotateSecret,
  onDelete,
  canManage = true,
  statusPendingIds,
}: WebhookSubscriptionTableProps) {
  const columns = createWebhookColumns({
    onEdit,
    onSetStatus,
    onRotateSecret,
    onDelete,
    canManage,
    statusPendingIds,
  })

  return (
    <DataTable
      columns={columns}
      data={data ?? []}
      isLoading={isLoading}
      error={error ?? undefined}
      loadingMessage={m['webhooks.loading']()}
      emptyMessage={m['webhooks.empty']()}
      data-testid="webhooks-table"
    />
  )
}
