import { ConfirmDialog } from '@/components/shared'
import { deletePermission } from '@/lib/api-generated'
import { useFormMutation } from '@/hooks/use-form-mutation'
import { useRealmId } from '@/stores/auth-store'
import type { PermissionResponse } from '@/lib/api-generated'
import { queryKeys } from '@/data/query-options'
import { m } from '@/paraglide/messages'

interface DeletePermissionDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  permission: PermissionResponse
  realmId?: string // Optional for backward compatibility
}

export function DeletePermissionDialog({
  open,
  onOpenChange,
  permission,
  realmId: realmIdProp,
}: DeletePermissionDialogProps) {
  const storeRealmId = useRealmId()
  const realmId = realmIdProp ?? storeRealmId
  const { isSubmitting, mutate } = useFormMutation({
    mutationFn: (permissionId: string) =>
      deletePermission({
        path: { permissionDefinitionId: permissionId },
      }),
    getSuccessMessage: () => `Permission "${permission.name}" deleted successfully`,
    invalidateQueries: [queryKeys.permissions(realmId)],
    onSuccess: () => {
      onOpenChange(false)
    },
  })

  const handleDelete = () => {
    mutate(permission.id)
  }

  return (
    <ConfirmDialog
      open={open}
      onOpenChange={onOpenChange}
      title={m['permissions.delete_title']()}
      description={m['permissions.delete_description']({ name: permission.name })}
      onConfirm={handleDelete}
      isPending={isSubmitting}
      confirmTestId="permission-delete-confirm-button"
    />
  )
}
