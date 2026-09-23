import { ConfirmDialog } from '@/components/shared'
import { useFormMutation } from '@/hooks/use-form-mutation'
import { deleteRole } from '@/lib/api-generated'
import { useRealmId } from '@/stores/auth-store'
import type { RoleResponse } from '@/lib/api-generated'
import { queryKeys } from '@/data/query-options'
import { m } from '@/paraglide/messages'

interface DeleteRoleDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  role: RoleResponse
  realmId?: string // Optional for backward compatibility
}

export function DeleteRoleDialog({
  open,
  onOpenChange,
  role,
  realmId: realmIdProp,
}: DeleteRoleDialogProps) {
  const storeRealmId = useRealmId()
  const realmId = realmIdProp ?? storeRealmId
  const { isSubmitting, mutate } = useFormMutation({
    mutationFn: (_: void) =>
      deleteRole({
        path: { roleId: role.id },
      }),
    getSuccessMessage: () => m['roles.deleted_success']({ name: role.name }),
    invalidateQueries: [queryKeys.roles(realmId)],
    onSuccess: () => {
      onOpenChange(false)
    },
  })

  const handleDelete = () => {
    mutate()
  }

  return (
    <ConfirmDialog
      open={open}
      onOpenChange={onOpenChange}
      title={m['roles.delete_title']()}
      description={m['roles.delete_description']({ name: role.name })}
      onConfirm={handleDelete}
      isPending={isSubmitting}
      confirmTestId="role-delete-confirm-button"
    />
  )
}
