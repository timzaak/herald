import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Button } from '@/components/ui/button'
import { BuiltinBadge } from '@/components/shared/builtin-badge'
import { Edit, Trash2, Shield } from 'lucide-react'
import type { RoleResponse, PermissionResponse } from '@/lib/api-generated'
import { EditRoleDialog } from './edit-role-dialog'
import { DeleteRoleDialog } from './delete-role-dialog'
import { RolePermissionsDialog } from './role-permissions-dialog'
import { useDialogManager } from '@/hooks/use-dialog-state'
import { useRealmId } from '@/stores/auth-store'
import { useQueries } from '@tanstack/react-query'
import { adminPermissionsQueryOptions, rolePermissionsQueryOptions } from '@/data/query-options'
import { m } from '@/paraglide/messages'

interface RoleTableProps {
  roles: RoleResponse[]
  isLoading: boolean
  error: unknown
  /**
   * `roles.manage` gate (permissions.md §6 按钮级控制): with view-only access
   * the list still renders but the create/edit/delete actions stay disabled.
   */
  canManage: boolean
  /**
   * `policies.manage` gate for the role→permission association dialog
   * (permissions.md §4.2: adding a permission to a role requires
   * `policies.manage` plus grantor self-hold — view-only callers must not be
   * able to open the write surface).
   */
  canManagePermissions: boolean
}

export function RoleTable({
  roles,
  isLoading,
  error,
  canManage,
  canManagePermissions,
}: RoleTableProps) {
  const realmId = useRealmId()
  const editDialog = useDialogManager<RoleResponse>()
  const deleteDialog = useDialogManager<RoleResponse>()
  const permissionsDialog = useDialogManager<RoleResponse>()

  const handleEdit = (role: RoleResponse) => {
    editDialog.open(role)
  }

  const handleDelete = (role: RoleResponse) => {
    if (role.isBuiltin) {
      return
    }
    deleteDialog.open(role)
  }

  const handleManagePermissions = async (role: RoleResponse) => {
    permissionsDialog.open(role)
  }

  if (isLoading) {
    return (
      <div className="rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{m['roles.table_name']()}</TableHead>
              <TableHead>{m['roles.table_description']()}</TableHead>
              <TableHead>{m['roles.table_permissions']()}</TableHead>
              <TableHead className="text-right">{m['roles.table_actions']()}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableRow>
              <TableCell colSpan={4} className="text-center py-8 text-muted-foreground">
                {m['roles.loading']()}
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </div>
    )
  }

  if (error) {
    return (
      <div className="rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{m['roles.table_name']()}</TableHead>
              <TableHead>{m['roles.table_description']()}</TableHead>
              <TableHead>{m['roles.table_permissions']()}</TableHead>
              <TableHead className="text-right">{m['roles.table_actions']()}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableRow>
              <TableCell colSpan={4} className="text-center py-8 text-destructive">
                {m['roles.error']()}
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </div>
    )
  }

  if (roles.length === 0) {
    return (
      <div className="rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{m['roles.table_name']()}</TableHead>
              <TableHead>{m['roles.table_description']()}</TableHead>
              <TableHead>{m['roles.table_permissions']()}</TableHead>
              <TableHead className="text-right">{m['roles.table_actions']()}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableRow>
              <TableCell colSpan={4} className="text-center py-8 text-muted-foreground">
                {m['roles.empty']()}
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </div>
    )
  }

  return (
    <>
      <div className="rounded-md border" data-testid="role-table">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{m['roles.table_name']()}</TableHead>
              <TableHead>{m['roles.table_description']()}</TableHead>
              <TableHead>{m['roles.table_permissions']()}</TableHead>
              <TableHead className="text-right">{m['roles.table_actions']()}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {roles.map((role) => (
              <TableRow key={role.id}>
                <TableCell className="font-medium">
                  <div className="flex items-center gap-2">
                    {role.name}
                    <BuiltinBadge isBuiltin={role.isBuiltin} />
                  </div>
                </TableCell>
                <TableCell className="text-muted-foreground max-w-md truncate">
                  {role.description || '-'}
                </TableCell>
                <TableCell>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="gap-1"
                    onClick={() => handleManagePermissions(role)}
                    disabled={!canManagePermissions}
                    data-testid={`role-permissions-button-${role.id}`}
                  >
                    <Shield className="h-3 w-3" />
                    {m['roles.manage_permissions']()}
                  </Button>
                </TableCell>
                <TableCell className="text-right">
                  <div className="flex justify-end gap-2">
                    <Button
                      variant="ghost"
                      size="icon"
                      onClick={() => handleEdit(role)}
                      disabled={!canManage}
                      data-testid={`role-edit-button-${role.id}`}
                    >
                      <Edit className="h-4 w-4" />
                    </Button>
                    {!role.isBuiltin && (
                      <Button
                        variant="ghost"
                        size="icon"
                        onClick={() => handleDelete(role)}
                        disabled={!canManage}
                        data-testid={`role-delete-button-${role.id}`}
                      >
                        <Trash2 className="h-4 w-4 text-destructive" />
                      </Button>
                    )}
                  </div>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>

      {editDialog.selectedItem && (
        <EditRoleDialog
          open={editDialog.isOpen}
          onOpenChange={editDialog.onOpenChange}
          role={editDialog.selectedItem}
          realmId={editDialog.selectedItem.realmId}
        />
      )}

      {deleteDialog.selectedItem && (
        <DeleteRoleDialog
          open={deleteDialog.isOpen}
          onOpenChange={deleteDialog.onOpenChange}
          role={deleteDialog.selectedItem}
          realmId={deleteDialog.selectedItem.realmId}
        />
      )}

      {permissionsDialog.selectedItem && (
        <RolePermissionsDataProvider
          realmId={realmId}
          roleId={permissionsDialog.selectedItem.id}
          open={permissionsDialog.isOpen}
          onOpenChange={permissionsDialog.onOpenChange}
          role={permissionsDialog.selectedItem}
        />
      )}
    </>
  )
}

// Inner component to handle data fetching for permissions dialog
function RolePermissionsDataProvider({
  realmId,
  roleId,
  open,
  onOpenChange,
  role,
}: {
  realmId: string
  roleId: string
  open: boolean
  onOpenChange: (open: boolean) => void
  role: RoleResponse
}) {
  const permissionsData = useQueries({
    queries: [adminPermissionsQueryOptions(realmId), rolePermissionsQueryOptions(realmId, roleId)],
    combine: (queries) => {
      const [perms, rolePerms] = queries
      return {
        allPermissions: perms.data ?? [],
        assignedPermissionIds: (rolePerms.data ?? []).map((p: PermissionResponse) => p.id),
      }
    },
  })

  if (!open) return null

  return (
    <RolePermissionsDialog
      open={open}
      onOpenChange={onOpenChange}
      role={role}
      realmId={realmId}
      allPermissions={permissionsData.allPermissions}
      assignedPermissionIds={permissionsData.assignedPermissionIds}
    />
  )
}
