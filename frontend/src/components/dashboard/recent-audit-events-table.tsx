import { useQuery } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { auditListQueryOptions } from '@/data/query-options'
import { DataTable } from '@/components/shared'
import { Badge } from '@/components/ui/badge'
import { usePermission } from '@/hooks/use-permission'
import { PERMISSION } from '@/lib/constants/auth-constants'
import { formatAuditAction } from '@/lib/audit-utils'
import { formatDateTimeShort } from '@/lib/date-utils'
import type { AuditEventResponse } from '@/lib/api-generated'
import { m } from '@/paraglide/messages'
import { RecentTableCard } from './recent-table-card'

const columns: ColumnDef<AuditEventResponse>[] = [
  {
    accessorKey: 'createdAt',
    header: () => m['audit.table_time'](),
    cell: ({ getValue }) => (
      <span className="whitespace-nowrap font-mono text-sm">
        {formatDateTimeShort(getValue() as string)}
      </span>
    ),
  },
  {
    accessorKey: 'actorName',
    header: () => m['audit.table_actor'](),
    cell: ({ row }) => (
      <div className="max-w-[140px] truncate text-sm font-medium">
        {row.original.actorName || m['audit.unknown']()}
      </div>
    ),
  },
  {
    accessorKey: 'category',
    header: () => m['audit.table_category'](),
    meta: { hiddenOnMobile: true },
    cell: ({ getValue }) => (
      <span className="text-sm">{(getValue() as string).replace(/_/g, ' ')}</span>
    ),
  },
  {
    accessorKey: 'action',
    header: () => m['audit.table_action'](),
    cell: ({ getValue }) => (
      <span className="text-sm">{formatAuditAction(getValue() as string)}</span>
    ),
  },
  {
    accessorKey: 'result',
    header: () => m['audit.table_result'](),
    cell: ({ getValue }) => {
      const result = getValue() as string
      return (
        <Badge variant={result === 'success' ? 'default' : 'destructive'} className="text-xs">
          {result}
        </Badge>
      )
    },
  },
]

interface RecentAuditEventsTableProps {
  realmId: string
}

/** Latest platform activity on the manage dashboard — the first 5 audit
 * events (backend orders by created_at desc). Hidden when the admin
 * lacks audit.view. */
export function RecentAuditEventsTable({ realmId }: RecentAuditEventsTableProps) {
  const { hasPermission } = usePermission()
  const canView = hasPermission(PERMISSION.AUDIT_VIEW)

  const { data, isLoading, error } = useQuery({
    ...auditListQueryOptions(realmId, { page: 0, pageSize: 5 }),
    enabled: canView,
  })

  if (!canView) return null

  return (
    <RecentTableCard
      title={m['dashboard.recent_audit_title']()}
      linkTo="/$realmId/manage/audit"
      linkParams={{ realmId }}
      testId="dashboard-recent-audit-card"
      isLoading={isLoading}
    >
      <DataTable
        columns={columns}
        data={data?.items ?? []}
        error={error ?? undefined}
        emptyMessage={m['dashboard.recent_audit_empty']()}
        data-testid="dashboard-recent-audit-table"
      />
    </RecentTableCard>
  )
}
