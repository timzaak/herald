import { useQuery } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { ArrowDownRight, ArrowUpRight } from 'lucide-react'
import {
  featureAvailabilityQueryOptions,
  pointsTransactionsQueryOptions,
} from '@/data/query-options'
import { DataTable } from '@/components/shared'
import { Badge } from '@/components/ui/badge'
import { usePermission } from '@/hooks/use-permission'
import { PERMISSION } from '@/lib/constants/auth-constants'
import { formatDateTimeShort } from '@/lib/date-utils'
import type { PointsTransactionResponse } from '@/lib/api-generated'
import { m } from '@/paraglide/messages'
import { RecentTableCard } from './recent-table-card'

const columns: ColumnDef<PointsTransactionResponse>[] = [
  {
    accessorKey: 'createdAt',
    header: () => m['points.transaction_col_time'](),
    cell: ({ getValue }) => (
      <span className="whitespace-nowrap font-mono text-sm">
        {formatDateTimeShort(getValue() as string)}
      </span>
    ),
  },
  {
    id: 'type',
    accessorKey: 'amount',
    header: () => m['points.transaction_col_type'](),
    cell: ({ row }) => {
      // The API carries no explicit type field — recharge/consume is
      // derived from the amount sign, mirroring TransactionHistoryTable.
      const isRecharge = row.original.amount >= 0
      return (
        <div className="flex items-center gap-2">
          {isRecharge ? (
            <ArrowUpRight className="h-4 w-4 text-success" />
          ) : (
            <ArrowDownRight className="h-4 w-4 text-destructive" />
          )}
          <Badge variant={isRecharge ? 'default' : 'secondary'}>
            {isRecharge
              ? m['points.transaction_type_recharge']()
              : m['points.transaction_type_consume']()}
          </Badge>
        </div>
      )
    },
  },
  {
    accessorKey: 'amount',
    header: () => m['points.transaction_col_amount'](),
    cell: ({ row }) => (
      <div
        className={`font-semibold ${row.original.amount >= 0 ? 'text-success' : 'text-destructive'}`}
      >
        {row.original.amount >= 0 ? '+' : ''}
        {row.original.amount.toLocaleString()}
      </div>
    ),
  },
  {
    id: 'bucket',
    accessorKey: 'bucketId',
    header: () => m['points.transaction_bucket_column'](),
    meta: { hiddenOnMobile: true },
    cell: ({ row }) => {
      const bucketId = row.original.bucketId
      const label = bucketId ? String(bucketId).slice(0, 8) : '-'
      return <Badge variant="outline">{label}</Badge>
    },
  },
  {
    accessorKey: 'description',
    header: () => m['points.transaction_col_description'](),
    meta: { hiddenOnMobile: true },
    cell: ({ getValue }) => (
      <div className="max-w-xs truncate text-sm text-muted-foreground">
        {(getValue() as string | null) || '-'}
      </div>
    ),
  },
]

interface RecentPointsTransactionsTableProps {
  realmId: string
}

/** Latest points activity on the manage dashboard — the first 5
 * transactions (backend orders by created_at desc). Requires
 * points.manage, not points.view: the list endpoint silently scopes
 * plain points.view to the caller's own transactions, which would
 * mislead on an admin dashboard. Hidden when the realm has points
 * disabled. */
export function RecentPointsTransactionsTable({ realmId }: RecentPointsTransactionsTableProps) {
  const { hasPermission } = usePermission()
  const { data: features } = useQuery(featureAvailabilityQueryOptions(realmId))
  const visible =
    hasPermission(PERMISSION.POINTS_MANAGE) && (features?.admin?.pointsVisible ?? true)

  const { data, isLoading, error } = useQuery({
    ...pointsTransactionsQueryOptions(realmId, { page: 0, pageSize: 5 }),
    enabled: visible,
  })

  if (!visible) return null

  return (
    <RecentTableCard
      title={m['dashboard.recent_points_title']()}
      linkTo="/$realmId/manage/points/wallets"
      linkParams={{ realmId }}
      testId="dashboard-recent-points-card"
      isLoading={isLoading}
    >
      <DataTable
        columns={columns}
        data={data?.transactions ?? []}
        error={error ?? undefined}
        emptyMessage={m['dashboard.recent_points_empty']()}
        data-testid="dashboard-recent-points-table"
      />
    </RecentTableCard>
  )
}
