import { type ColumnDef } from '@tanstack/react-table'
import { DataTable } from '@/components/shared'
import type { ApiKeyListItem } from '@/lib/api-generated'
import { Badge } from '@/components/ui/badge'
import { Skeleton } from '@/components/ui/skeleton'
import { Switch } from '@/components/ui/switch'
import { Button } from '@/components/ui/button'
import { formatDate } from '@/lib/date-utils'
import { m } from '@/paraglide/messages'

interface ApiKeyColumnConfig {
  onEdit?: (apiKey: ApiKeyListItem) => void
  onDelete?: (apiKey: ApiKeyListItem) => void
  onToggleEnabled?: (apiKey: ApiKeyListItem) => void
  canUpdate?: boolean
  canDelete?: boolean
  onManageRoles?: (apiKey: ApiKeyListItem) => void
  canManageRoles?: boolean
  isLoading?: boolean
}

interface ApiKeyTableProps extends ApiKeyColumnConfig {
  data?: ApiKeyListItem[]
  isLoading?: boolean
  error?: Error | null
}

function createApiKeyColumns(config: ApiKeyColumnConfig): ColumnDef<ApiKeyListItem>[] {
  const {
    onEdit,
    onDelete,
    onToggleEnabled,
    canUpdate = true,
    canDelete = true,
    onManageRoles,
    canManageRoles,
    isLoading,
  } = config
  return [
    {
      id: 'name',
      accessorKey: 'name',
      header: m['api_keys.table_name'](),
      cell: ({ row }) => (
        <span className="font-medium" data-testid="api-key-name">
          {row.getValue('name')}
        </span>
      ),
    },
    {
      id: 'roles',
      header: m['api_keys.table_roles'](),
      cell: ({ row }) => {
        if (isLoading) {
          return <Skeleton className="h-5 w-20" />
        }

        const roles = row.original.roles
        if (!roles?.length) {
          return <span data-testid="api-key-roles-cell">&mdash;</span>
        }

        const visible = roles.slice(0, 2)
        const remaining = roles.length - visible.length

        return (
          <div className="flex flex-wrap gap-1" data-testid="api-key-roles-cell">
            {visible.map((role) => (
              <Badge key={role.id} variant="secondary">
                {role.name}
              </Badge>
            ))}
            {remaining > 0 && (
              <Badge variant="outline" data-testid="api-key-roles-overflow">
                {m['api_keys.more_roles']({ count: remaining })}
              </Badge>
            )}
          </div>
        )
      },
    },
    {
      id: 'clientApp',
      header: m['api_keys.table_client_app'](),
      cell: ({ row }) => (
        <div className="space-y-1" data-testid="api-key-client-app">
          <span>{row.original.clientAppName ?? row.original.clientAppId ?? '-'}</span>
          {row.original.clientAppEnabled === false && (
            <div>
              <Badge variant="destructive" data-testid="api-key-client-app-disabled-warning">
                {m['api_keys.client_app_disabled_warning']()}
              </Badge>
            </div>
          )}
        </div>
      ),
    },
    {
      id: 'enabled',
      header: m['api_keys.table_status'](),
      cell: ({ row }) => (
        <div className="flex items-center gap-2">
          <Switch
            checked={row.original.enabled}
            onCheckedChange={() => onToggleEnabled?.(row.original)}
            data-testid="api-key-enabled-switch"
          />
          <Badge
            variant={row.original.enabled ? 'default' : 'secondary'}
            data-testid="api-key-status-badge"
          >
            {row.original.enabled ? m['common.enabled']() : m['common.disabled']()}
          </Badge>
        </div>
      ),
    },
    {
      id: 'expiresAt',
      header: m['api_keys.table_expires'](),
      cell: ({ row }) => (
        <span data-testid="api-key-expires">
          {row.original.expiresAt ? formatDate(row.original.expiresAt) : m['api_keys.never']()}
        </span>
      ),
    },
    {
      id: 'lastUsedAt',
      header: m['api_keys.table_last_used'](),
      cell: ({ row }) => (
        <span data-testid="api-key-last-used">
          {row.original.lastUsedAt ? formatDate(row.original.lastUsedAt) : m['api_keys.never']()}
        </span>
      ),
    },
    {
      id: 'actions',
      header: m['api_keys.table_actions'](),
      cell: ({ row }) => (
        <div className="flex gap-2" data-testid="api-key-actions">
          {canManageRoles && (
            <Button
              variant="ghost"
              size="sm"
              onClick={() => onManageRoles?.(row.original)}
              data-testid="manage-api-key-roles-button"
            >
              {m['api_keys.roles_button']()}
            </Button>
          )}
          <Button
            variant="ghost"
            size="sm"
            onClick={() => onEdit?.(row.original)}
            data-testid="edit-api-key-button"
            disabled={!canUpdate}
            title={!canUpdate ? m['api_keys.edit_disabled_title']() : undefined}
          >
            {m['api_keys.edit_button']()}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => onDelete?.(row.original)}
            data-testid="delete-api-key-button"
            disabled={!canDelete}
            title={!canDelete ? m['api_keys.delete_disabled_title']() : undefined}
          >
            {m['api_keys.delete_button']()}
          </Button>
        </div>
      ),
    },
  ]
}

export function ApiKeyTable({
  data,
  isLoading = false,
  error,
  onEdit,
  onDelete,
  onToggleEnabled,
  canUpdate = true,
  canDelete = true,
  onManageRoles,
  canManageRoles,
}: ApiKeyTableProps) {
  const columns = createApiKeyColumns({
    onEdit,
    onDelete,
    onToggleEnabled,
    canUpdate,
    canDelete,
    onManageRoles,
    canManageRoles,
    isLoading,
  })

  return (
    <DataTable
      columns={columns}
      data={data ?? []}
      isLoading={isLoading}
      error={error ?? undefined}
      loadingMessage={m['api_keys.loading']()}
      emptyMessage={m['api_keys.empty']()}
      data-testid="api-keys-table"
    />
  )
}
