import { useState, useEffect } from 'react'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Shield, Loader2 } from 'lucide-react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { assignPermissionToRole, removePermissionFromRole } from '@/lib/api-generated'
import { PermissionTransfer } from './permission-transfer'
import { useRealmId } from '@/stores/auth-store'
import type { RoleResponse, PermissionResponse } from '@/lib/api-generated'
import { queryKeys } from '@/data/query-options'
import { m } from '@/paraglide/messages'
import { getErrorMessage } from '@/lib/error-utils'

interface RolePermissionsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  role: RoleResponse
  realmId?: string
  allPermissions: PermissionResponse[]
  assignedPermissionIds: string[]
}

export function RolePermissionsDialog({
  open,
  onOpenChange,
  role,
  realmId: realmIdProp,
  allPermissions,
  assignedPermissionIds,
}: RolePermissionsDialogProps) {
  const storeRealmId = useRealmId()
  const realmId = realmIdProp ?? storeRealmId
  const queryClient = useQueryClient()

  const [localAssignedIds, setLocalAssignedIds] = useState<string[]>(assignedPermissionIds)

  // Sync from props when dialog opens
  useEffect(() => {
    if (open) {
      setLocalAssignedIds(assignedPermissionIds) // eslint-disable-line react-hooks/set-state-in-effect -- syncing state when dialog opens
    }
  }, [open, assignedPermissionIds])

  const addedIds = localAssignedIds.filter((id) => !assignedPermissionIds.includes(id))
  const removedIds = assignedPermissionIds.filter((id) => !localAssignedIds.includes(id))
  const hasChanges = addedIds.length > 0 || removedIds.length > 0

  const saveMutation = useMutation({
    mutationFn: async () => {
      const assignRequests = addedIds.map((permissionId) =>
        assignPermissionToRole({
          path: { realmId, roleId: role.id },
          body: { permissionId },
        }).then((response) => {
          if (response.error) throw response.error
          return response.data
        })
      )

      const removeRequests = removedIds.map((permissionId) =>
        removePermissionFromRole({
          path: { realmId, roleId: role.id, permissionId },
        }).then((response) => {
          if (response.error) throw response.error
          return response.data
        })
      )

      await Promise.all([...assignRequests, ...removeRequests])
      return { assigned: addedIds.length, removed: removedIds.length }
    },
    onSuccess: async (result) => {
      const parts: string[] = []
      if (result.assigned > 0)
        parts.push(m['roles.permissions_assigned_count']({ count: result.assigned }))
      if (result.removed > 0)
        parts.push(m['roles.permissions_removed_count']({ count: result.removed }))
      toast.success(m['roles.permissions_saved']({ details: parts.join(', ') }))
      await queryClient.invalidateQueries({
        queryKey: queryKeys.rolePermissions(realmId, role.id),
      })
      onOpenChange(false)
    },
    onError: (error: unknown) => {
      toast.error(m['roles.permissions_save_failed']({ message: getErrorMessage(error) }))
    },
  })

  const handleTogglePermission = (permissionId: string, checked: boolean) => {
    setLocalAssignedIds((prev) =>
      checked ? [...prev, permissionId] : prev.filter((id) => id !== permissionId)
    )
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="sm:max-w-5xl max-h-[80vh] overflow-hidden flex flex-col"
        data-testid="role-permissions-dialog"
      >
        <DialogHeader className="flex-shrink-0">
          <div className="flex items-center gap-2">
            <Shield className="h-5 w-5" />
            <DialogTitle>{m['roles.permissions_title']({ name: role.name })}</DialogTitle>
          </div>
          <DialogDescription>{m['roles.permissions_description']()}</DialogDescription>
        </DialogHeader>

        <div className="flex-1 min-h-0 overflow-y-auto space-y-4 pr-2">
          {/* Summary stats */}
          <div className="flex items-center gap-4 text-sm">
            <div className="flex items-center gap-2">
              <Badge variant="outline">
                {localAssignedIds.length} / {allPermissions.length}
              </Badge>
              <span className="text-muted-foreground">{m['roles.permissions_assigned']()}</span>
            </div>
            {saveMutation.isPending && (
              <div className="flex items-center gap-2 text-muted-foreground">
                <Loader2 className="h-4 w-4 animate-spin" />
                <span>{m['roles.permissions_saving']()}</span>
              </div>
            )}
          </div>

          {/* Permission transfer */}
          <PermissionTransfer
            permissions={allPermissions}
            assignedPermissionIds={localAssignedIds}
            onTogglePermission={handleTogglePermission}
            isBuiltinRole={role.isBuiltin}
            disabled={saveMutation.isPending}
            dataTestId="role-permissions-transfer"
          />
        </div>

        {/* Footer actions */}
        <div className="flex-shrink-0 flex justify-end gap-2 pt-4 border-t">
          <Button
            variant="outline"
            onClick={() => {
              setLocalAssignedIds(assignedPermissionIds)
              onOpenChange(false)
            }}
            disabled={saveMutation.isPending}
            data-testid="role-permissions-cancel-button"
          >
            {m['common.cancel']()}
          </Button>
          <Button
            onClick={() => saveMutation.mutate()}
            disabled={!hasChanges || saveMutation.isPending}
            data-testid="role-permissions-save-button"
          >
            {saveMutation.isPending ? (
              <>
                <Loader2 className="h-4 w-4 animate-spin mr-2" />
                {m['shared.saving']()}
              </>
            ) : (
              m['shared.save_changes']()
            )}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
