import { type ColumnDef, flexRender, getCoreRowModel, useReactTable } from '@tanstack/react-table'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import type { ClientAppItem } from '@/lib/api-generated'
import { Badge } from '@/components/ui/badge'
import { Switch } from '@/components/ui/switch'
import { Button } from '@/components/ui/button'
import { BuiltinBadge } from '@/components/shared/builtin-badge'
import { Alert } from '@/components/ui/alert'
import { MCP_CLIENT_ID } from '@/lib/constants'
import { m } from '@/paraglide/messages'
import { getErrorMessage } from '@/lib/error-utils'

interface ClientAppTableProps {
  data?: ClientAppItem[]
  isLoading?: boolean
  error?: unknown
  onEdit?: (clientApp: ClientAppItem) => void
  onDelete?: (clientApp: ClientAppItem) => void
  onToggleEnabled?: (clientApp: ClientAppItem) => void
  onRetryStatus?: (clientApp: ClientAppItem) => void
  canUpdate?: boolean
  canDelete?: boolean
  togglingAppId?: string | null
  statusErrorAppIds?: string[]
}

interface ColumnFactoryOptions {
  onEdit?: (clientApp: ClientAppItem) => void
  onDelete?: (clientApp: ClientAppItem) => void
  onToggleEnabled?: (clientApp: ClientAppItem) => void
  onRetryStatus?: (clientApp: ClientAppItem) => void
  canUpdate?: boolean
  canDelete?: boolean
  togglingAppId?: string | null
  statusErrorAppIds?: string[]
}

function createClientAppColumns({
  onEdit,
  onDelete,
  onToggleEnabled,
  onRetryStatus,
  canUpdate = true,
  canDelete = true,
  togglingAppId = null,
  statusErrorAppIds = [],
}: ColumnFactoryOptions): ColumnDef<ClientAppItem>[] {
  return [
    {
      id: 'icon',
      header: m['client_apps.table_icon'](),
      cell: ({ row }) =>
        row.original.iconUrl ? (
          <img
            src={row.original.iconUrl}
            alt={row.original.name}
            className="w-8 h-8 rounded"
            data-testid="client-app-icon"
          />
        ) : (
          <div className="w-8 h-8 bg-muted rounded flex items-center justify-center text-muted-foreground text-xs">
            N/A
          </div>
        ),
    },
    {
      id: 'clientId',
      accessorKey: 'clientId',
      header: m['client_apps.table_client_id'](),
      cell: ({ row }) => (
        <span className="font-mono text-sm" data-testid="client-app-client-id">
          {row.getValue('clientId')}
        </span>
      ),
    },
    {
      id: 'name',
      accessorKey: 'name',
      header: m['client_apps.table_name'](),
      cell: ({ row }) => (
        <div className="flex items-center gap-2">
          {row.getValue('name')}
          <BuiltinBadge isBuiltin={row.original.isSystemBuiltin} />
        </div>
      ),
    },
    {
      id: 'redirectUris',
      header: m['client_apps.table_redirect_uris'](),
      cell: ({ row }) => (
        <div
          className="max-w-xs truncate"
          data-testid="client-app-redirect-uris"
          title={row.original.redirectUris.join(', ')}
        >
          {row.original.redirectUris.join(', ')}
        </div>
      ),
    },
    {
      id: 'browserRefreshAbsoluteTtlSeconds',
      header: m['client_apps.table_refresh_ttl'](),
      cell: ({ row }) => (
        <span data-testid="client-app-refresh-ttl">
          {m['client_apps.refresh_ttl_days']({
            days: Math.round(row.original.browserRefreshAbsoluteTtlSeconds / 86400),
          })}
        </span>
      ),
    },
    {
      id: 'enabled',
      header: m['client_apps.table_status'](),
      cell: ({ row }) => {
        const statusUnconfirmed = statusErrorAppIds.includes(row.original.id)
        // Serialize the switch being submitted, not the whole table: an
        // in-flight toggle on one app must not block an urgent disable of
        // an unrelated app.
        const thisRowToggling = togglingAppId === row.original.id
        return (
          <div className="flex flex-col gap-1">
            <div className="flex items-center gap-2">
              <Switch
                checked={row.original.enabled}
                onCheckedChange={() => onToggleEnabled?.(row.original)}
                disabled={!canUpdate || thisRowToggling || statusUnconfirmed}
                aria-label={row.original.name}
                title={
                  !canUpdate
                    ? m['client_apps.edit_disabled_title']()
                    : statusUnconfirmed
                      ? m['client_apps.status_unconfirmed']()
                      : undefined
                }
                data-testid="client-app-enabled-switch"
              />
              <Badge
                variant={row.original.enabled ? 'default' : 'secondary'}
                data-testid="client-app-status-badge"
              >
                {row.original.enabled
                  ? m['client_apps.status_enabled_label']()
                  : m['client_apps.status_disabled_label']()}
              </Badge>
            </div>
            {row.original.clientId === MCP_CLIENT_ID && (
              <p className="text-xs text-muted-foreground" data-testid="client-app-mcp-hint">
                {m['client_apps.mcp_disable_hint']()}
              </p>
            )}
            {statusUnconfirmed && (
              <div className="flex items-center gap-2">
                <span className="text-xs text-destructive" data-testid="client-app-status-error">
                  {m['client_apps.status_unconfirmed']()}
                </span>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => onRetryStatus?.(row.original)}
                  data-testid="client-app-status-retry-button"
                >
                  {m['common.retry']()}
                </Button>
              </div>
            )}
          </div>
        )
      },
    },
    {
      id: 'actions',
      header: m['client_apps.table_actions'](),
      cell: ({ row }) => (
        <div className="flex gap-2" data-testid="client-app-actions">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => onEdit?.(row.original)}
            data-testid="edit-client-app-button"
            disabled={!canUpdate}
            title={!canUpdate ? m['client_apps.edit_disabled_title']() : undefined}
          >
            {m['client_apps.edit_button']()}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => onDelete?.(row.original)}
            data-testid="delete-client-app-button"
            disabled={!canDelete || row.original.isSystemBuiltin}
            title={
              row.original.isSystemBuiltin
                ? m['client_apps.delete_disabled_builtin_title']()
                : !canDelete
                  ? m['client_apps.delete_disabled_title']()
                  : undefined
            }
          >
            {m['client_apps.delete_button']()}
          </Button>
        </div>
      ),
    },
  ]
}

export function ClientAppTable({
  data,
  isLoading = false,
  error,
  onEdit,
  onDelete,
  onToggleEnabled,
  onRetryStatus,
  canUpdate = true,
  canDelete = true,
  togglingAppId = null,
  statusErrorAppIds = [],
}: ClientAppTableProps) {
  const columns = createClientAppColumns({
    onEdit,
    onDelete,
    onToggleEnabled,
    onRetryStatus,
    canUpdate,
    canDelete,
    togglingAppId,
    statusErrorAppIds,
  })

  const table = useReactTable({
    data: data ?? [],
    columns,
    getCoreRowModel: getCoreRowModel(),
  })

  if (isLoading) {
    return (
      <div className="rounded-md border p-8">
        <div className="flex items-center justify-center">
          <div className="text-muted-foreground">{m['client_apps.loading']()}</div>
        </div>
      </div>
    )
  }

  if (error && (!data || data.length === 0)) {
    return (
      <div className="rounded-md border p-8">
        <div className="flex items-center justify-center text-destructive">
          {m['client_apps.error_loading']({ message: getErrorMessage(error) })}
        </div>
      </div>
    )
  }

  // A failed background re-fetch keeps the last data; the rows stay live so
  // the per-row "status unconfirmed / retry" recovery stays reachable —
  // replacing the table with a bare banner would hide exactly the UI the
  // recovery contract needs. The no-data error case returned above.
  const refetchFailed = Boolean(error)

  if (!data || data.length === 0) {
    return (
      <div className="rounded-md border p-8">
        <div className="flex items-center justify-center text-muted-foreground">
          {m['client_apps.empty']()}
        </div>
      </div>
    )
  }

  return (
    <div className="rounded-md border">
      {refetchFailed && (
        <Alert variant="destructive" className="rounded-none border-x-0 border-t-0">
          <span data-testid="client-apps-refetch-error">
            {m['client_apps.error_loading']({ message: getErrorMessage(error) })}
          </span>
        </Alert>
      )}
      <Table data-testid="client-apps-table">
        <TableHeader>
          {table.getHeaderGroups().map((headerGroup) => (
            <TableRow key={headerGroup.id}>
              {headerGroup.headers.map((header) => (
                <TableHead key={header.id}>
                  {header.isPlaceholder
                    ? null
                    : flexRender(header.column.columnDef.header, header.getContext())}
                </TableHead>
              ))}
            </TableRow>
          ))}
        </TableHeader>
        <TableBody>
          {table.getRowModel().rows?.length ? (
            table.getRowModel().rows.map((row) => (
              <TableRow
                key={row.id}
                data-state={row.getIsSelected() && 'selected'}
                data-testid={`client-app-row-${row.index}`}
                data-client-id={row.getValue('clientId')}
                data-app-id={row.original.id}
              >
                {row.getVisibleCells().map((cell) => (
                  <TableCell key={cell.id}>
                    {flexRender(cell.column.columnDef.cell, cell.getContext())}
                  </TableCell>
                ))}
              </TableRow>
            ))
          ) : (
            <TableRow>
              <TableCell colSpan={columns.length} className="h-24 text-center">
                {m['client_apps.no_results']()}
              </TableCell>
            </TableRow>
          )}
        </TableBody>
      </Table>
    </div>
  )
}
