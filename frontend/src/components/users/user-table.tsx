import { type ColumnDef, flexRender, getCoreRowModel, useReactTable } from '@tanstack/react-table'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { formatDateTimeShort } from '@/lib/date-utils'
import { UserTableSkeleton } from './user-table-skeleton'
import { UserTableError } from './user-table-error'
import { UserTableEmpty } from './user-table-empty'
import { getUserStatusLabel, USER_STATUS_COLORS } from '@/lib/constants/user'
import type { UserResponse, RoleResponse } from '@/lib/api-generated'
import { Badge } from '@/components/ui/badge'
import { m } from '@/paraglide/messages'

// Type for user data with roles included (from API that extends UserResponse)
interface UserWithRoles extends UserResponse {
  roles?: RoleResponse[]
}

interface UserTableProps {
  data?: UserWithRoles[]
  isLoading?: boolean
  error?: Error
  onEdit?: (user: UserWithRoles) => void
  onDelete?: (user: UserWithRoles) => void
  onManageRoles?: (user: UserWithRoles) => void
  onResetPassword?: (user: UserWithRoles) => void
  onManageSessions?: (user: UserWithRoles) => void
}

function createUserColumns(
  onEdit?: (user: UserWithRoles) => void,
  onDelete?: (user: UserWithRoles) => void,
  onManageRoles?: (user: UserWithRoles) => void,
  onResetPassword?: (user: UserWithRoles) => void,
  onManageSessions?: (user: UserWithRoles) => void
): ColumnDef<UserWithRoles>[] {
  return [
    {
      id: 'id',
      accessorKey: 'id',
      header: m['users.table_id'](),
      cell: ({ row }) => (
        <div className="font-mono text-xs" data-testid={`user-table-${row.index}-id`}>
          {String(row.getValue('id')).slice(0, 8)}...
        </div>
      ),
    },
    {
      accessorKey: 'email',
      header: m['users.table_email'](),
      cell: ({ row }) => {
        const email = String(row.getValue('email') ?? '') || '-'
        return (
          <div className="max-w-xs truncate" title={email}>
            {email}
          </div>
        )
      },
    },
    {
      accessorKey: 'nickname',
      header: m['users.table_nickname'](),
      cell: ({ row }) => row.getValue('nickname') || '-',
    },
    {
      accessorKey: 'status',
      header: m['users.table_status'](),
      cell: ({ row }) => {
        const status = row.getValue('status') as number
        return (
          <span
            className={`inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium ${USER_STATUS_COLORS[status]}`}
            data-testid={`user-table-status-${status}`}
          >
            {getUserStatusLabel(status)}
          </span>
        )
      },
    },
    {
      accessorKey: 'createdAt',
      header: m['users.table_created_at'](),
      cell: ({ row }) => {
        const date = row.getValue('createdAt') as string
        return date ? formatDateTimeShort(date) : '-'
      },
    },
    {
      id: 'roles',
      header: m['users.table_roles'](),
      cell: ({ row }) => {
        const user = row.original
        const roles = user?.roles || []

        if (roles.length === 0) {
          return <span className="text-sm text-muted-foreground">{m['users.no_roles']()}</span>
        }

        return (
          <div className="flex flex-wrap gap-1" data-testid={`user-table-${row.index}-roles`}>
            {roles.slice(0, 2).map((role: RoleResponse) => (
              <Badge key={role.id} variant="secondary" className="text-xs">
                {role.name}
              </Badge>
            ))}
            {roles.length > 2 && (
              <Badge variant="outline" className="text-xs">
                +{roles.length - 2}
              </Badge>
            )}
          </div>
        )
      },
    },
    {
      id: 'actions',
      header: m['users.table_actions'](),
      cell: ({ row }) => (
        <div className="flex gap-2" data-testid={`user-table-${row.index}-actions`}>
          {onManageRoles && (
            <button
              onClick={() => onManageRoles(row.original)}
              className="text-sm text-primary hover:text-primary/80"
              data-testid={`user-table-${row.index}-manage-roles-button`}
            >
              {m['users.table_roles']()}
            </button>
          )}
          {onManageSessions && (
            <button
              onClick={() => onManageSessions(row.original)}
              className="text-sm text-primary hover:text-primary/80"
              data-testid={`user-table-${row.index}-sessions-button`}
            >
              {m['users.sessions.entry_button']()}
            </button>
          )}
          {onResetPassword && (
            <button
              onClick={() => onResetPassword(row.original)}
              className="text-sm text-warning hover:text-warning"
              data-testid={`user-table-${row.index}-reset-password-button`}
            >
              {m['users.reset_password_title']()}
            </button>
          )}
          {onEdit && (
            <button
              onClick={() => onEdit(row.original)}
              className="text-sm text-primary hover:text-primary/80"
              data-testid={`user-table-${row.index}-edit-button`}
            >
              {m['common.edit']()}
            </button>
          )}
          {onDelete && (
            <button
              onClick={() => onDelete(row.original)}
              className="text-sm text-destructive hover:text-destructive"
              data-testid={`user-table-${row.index}-delete-button`}
            >
              {m['common.delete']()}
            </button>
          )}
        </div>
      ),
    },
  ]
}

export function UserTable({
  data,
  isLoading = false,
  error,
  onEdit,
  onDelete,
  onManageRoles,
  onResetPassword,
  onManageSessions,
}: UserTableProps) {
  const columns = createUserColumns(
    onEdit,
    onDelete,
    onManageRoles,
    onResetPassword,
    onManageSessions
  )

  const table = useReactTable({
    data: data ?? [],
    columns,
    getCoreRowModel: getCoreRowModel(),
  })

  if (isLoading) {
    return <UserTableSkeleton />
  }

  if (error) {
    return <UserTableError error={error} />
  }

  if (!data || data.length === 0) {
    return <UserTableEmpty onCreateUser={() => {}} />
  }

  return (
    <div className="rounded-md border">
      <Table data-testid="users-table">
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
                data-testid={`user-row-${row.index}`}
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
                {m['common.no_results']()}
              </TableCell>
            </TableRow>
          )}
        </TableBody>
      </Table>
    </div>
  )
}
