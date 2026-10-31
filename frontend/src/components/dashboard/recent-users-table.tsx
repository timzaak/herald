import { useQuery } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { usersQueryOptions } from '@/data/query-options'
import { DataTable } from '@/components/shared'
import { usePermission } from '@/hooks/use-permission'
import { PERMISSION } from '@/lib/constants/auth-constants'
import { getUserStatusLabel, USER_STATUS_COLORS } from '@/lib/constants/user'
import { formatDateTimeShort } from '@/lib/date-utils'
import type { UserResponse } from '@/lib/api-generated'
import { m } from '@/paraglide/messages'
import { RecentTableCard } from './recent-table-card'

const columns: ColumnDef<UserResponse>[] = [
  {
    accessorKey: 'email',
    header: () => m['users.table_email'](),
    cell: ({ row }) => {
      const email = row.original.email || '-'
      return (
        <div className="max-w-[220px]">
          <div className="truncate text-sm font-medium" title={email}>
            {email}
          </div>
          {row.original.nickname && (
            <div className="truncate text-xs text-muted-foreground">{row.original.nickname}</div>
          )}
        </div>
      )
    },
  },
  {
    accessorKey: 'status',
    header: () => m['users.table_status'](),
    cell: ({ row }) => {
      const status = row.original.status
      return (
        <span
          className={`inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium ${USER_STATUS_COLORS[status]}`}
          data-testid="dashboard-recent-users-status"
        >
          {getUserStatusLabel(status)}
        </span>
      )
    },
  },
  {
    accessorKey: 'createdAt',
    header: () => m['users.table_created_at'](),
    meta: { hiddenOnMobile: true },
    cell: ({ getValue }) => (
      <span className="whitespace-nowrap text-sm text-muted-foreground">
        {formatDateTimeShort(getValue() as string)}
      </span>
    ),
  },
]

interface RecentUsersTableProps {
  realmId: string
}

/** Newest signups on the manage dashboard — the first 5 users of the
 * admin list (backend orders by created_at desc). Hidden when the admin
 * lacks users.view. */
export function RecentUsersTable({ realmId }: RecentUsersTableProps) {
  const { hasPermission } = usePermission()
  const canView = hasPermission(PERMISSION.USERS_VIEW)

  const { data, isLoading, error } = useQuery({
    ...usersQueryOptions(realmId, { page: 0, pageSize: 5 }),
    enabled: canView,
  })

  if (!canView) return null

  return (
    <RecentTableCard
      title={m['dashboard.recent_users_title']()}
      linkTo="/$realmId/manage/users"
      linkParams={{ realmId }}
      testId="dashboard-recent-users-card"
      isLoading={isLoading}
    >
      <DataTable
        columns={columns}
        data={data?.items ?? []}
        error={error ?? undefined}
        emptyMessage={m['dashboard.recent_users_empty']()}
        data-testid="dashboard-recent-users-table"
      />
    </RecentTableCard>
  )
}
