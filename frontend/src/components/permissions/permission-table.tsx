import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { BuiltinBadge } from '@/components/shared/builtin-badge'
import { Edit, Trash2 } from 'lucide-react'
import type { PermissionResponse } from '@/lib/api-generated'
import { EditPermissionDialog } from './edit-permission-dialog'
import { DeletePermissionDialog } from './delete-permission-dialog'
import { useDialogManager } from '@/hooks/use-dialog-state'
import { m } from '@/paraglide/messages'

function TableHeaders() {
  return (
    <TableRow>
      <TableHead>{m['permissions.table_name']()}</TableHead>
      <TableHead>{m['permissions.table_resource']()}</TableHead>
      <TableHead>{m['permissions.table_action']()}</TableHead>
      <TableHead>{m['permissions.table_description']()}</TableHead>
      <TableHead className="text-right">{m['permissions.table_actions']()}</TableHead>
    </TableRow>
  )
}

interface PermissionTableProps {
  permissions: PermissionResponse[]
  isLoading: boolean
  error: unknown
  /**
   * `permissions.manage` gate (permissions.md §6 按钮级控制): with view-only
   * access the list still renders but every write action stays disabled.
   */
  canManage: boolean
}

export function PermissionTable({
  permissions,
  isLoading,
  error,
  canManage,
}: PermissionTableProps) {
  const editDialog = useDialogManager<PermissionResponse>()
  const deleteDialog = useDialogManager<PermissionResponse>()

  const handleEdit = (permission: PermissionResponse) => {
    editDialog.open(permission)
  }

  const handleDelete = (permission: PermissionResponse) => {
    if (permission.isBuiltin) {
      return
    }
    deleteDialog.open(permission)
  }

  if (isLoading) {
    return (
      <div className="rounded-md border">
        <Table>
          <TableHeader>
            <TableHeaders />
          </TableHeader>
          <TableBody>
            <TableRow>
              <TableCell colSpan={5} className="text-center py-8 text-muted-foreground">
                {m['permissions.loading']()}
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
            <TableHeaders />
          </TableHeader>
          <TableBody>
            <TableRow>
              <TableCell colSpan={5} className="text-center py-8 text-destructive">
                {m['permissions.error']()}
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </div>
    )
  }

  if (permissions.length === 0) {
    return (
      <div className="rounded-md border">
        <Table>
          <TableHeader>
            <TableHeaders />
          </TableHeader>
          <TableBody>
            <TableRow>
              <TableCell colSpan={5} className="text-center py-8 text-muted-foreground">
                {m['permissions.empty']()}
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </div>
    )
  }

  return (
    <>
      <div className="rounded-md border" data-testid="permissions-table">
        <Table>
          <TableHeader>
            <TableHeaders />
          </TableHeader>
          <TableBody>
            {permissions.map((permission) => (
              <TableRow key={permission.id}>
                <TableCell className="font-medium">
                  <div className="flex items-center gap-2">
                    {permission.name}
                    <BuiltinBadge isBuiltin={permission.isBuiltin} />
                  </div>
                </TableCell>
                <TableCell>
                  <Badge variant="outline">{permission.resource}</Badge>
                </TableCell>
                <TableCell>
                  <Badge variant="secondary">{permission.action}</Badge>
                </TableCell>
                <TableCell className="text-muted-foreground max-w-md truncate">
                  {permission.description || '-'}
                </TableCell>
                <TableCell className="text-right">
                  <div className="flex justify-end gap-2">
                    {!permission.isBuiltin && (
                      <>
                        <Button
                          variant="ghost"
                          size="icon"
                          onClick={() => handleEdit(permission)}
                          disabled={!canManage}
                          data-testid={`permission-edit-button-${permission.id}`}
                        >
                          <Edit className="h-4 w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          onClick={() => handleDelete(permission)}
                          disabled={!canManage}
                          data-testid={`permission-delete-button-${permission.id}`}
                        >
                          <Trash2 className="h-4 w-4 text-destructive" />
                        </Button>
                      </>
                    )}
                  </div>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>

      {editDialog.selectedItem && (
        <EditPermissionDialog
          open={editDialog.isOpen}
          onOpenChange={editDialog.onOpenChange}
          permission={editDialog.selectedItem}
          realmId={editDialog.selectedItem.realmId}
        />
      )}

      {deleteDialog.selectedItem && (
        <DeletePermissionDialog
          open={deleteDialog.isOpen}
          onOpenChange={deleteDialog.onOpenChange}
          permission={deleteDialog.selectedItem}
          realmId={deleteDialog.selectedItem.realmId}
        />
      )}
    </>
  )
}
