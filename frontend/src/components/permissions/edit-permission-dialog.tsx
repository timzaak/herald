import { EditResourceDialog, type ResourceFormConfig } from '@/components/shared'
import { updatePermissionSchema, type UpdatePermissionFormData } from '@/lib/schemas/common'
import { updatePermission } from '@/lib/api-generated'
import { useRealmId } from '@/stores/auth-store'
import type { PermissionResponse } from '@/lib/api-generated'
import { queryKeys } from '@/data/query-options'
import { m } from '@/paraglide/messages'

interface EditPermissionDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  permission: PermissionResponse
  realmId?: string // Optional for backward compatibility
}

export function EditPermissionDialog({
  open,
  onOpenChange,
  permission,
  realmId: realmIdProp,
}: EditPermissionDialogProps) {
  const storeRealmId = useRealmId()
  const realmId = realmIdProp ?? storeRealmId
  const config: ResourceFormConfig<UpdatePermissionFormData> = {
    schema: updatePermissionSchema,
    defaultValues: {
      name: permission.name,
      description: permission.description ?? '',
    },
    mutationFn: (data: UpdatePermissionFormData) =>
      updatePermission({
        path: { permissionDefinitionId: permission.id },
        body: data,
      }),
    getSuccessMessage: (response) => {
      const perm = (response as { data?: { name?: string } }).data
      return `Permission "${perm?.name}" updated successfully`
    },
    queryKeysToInvalidate: [queryKeys.permissions(realmId)],
    nameFieldLabel: m['permissions.name_label'](),
    nameFieldPlaceholder: permission.name,
    descriptionFieldPlaceholder: m['permissions.description_placeholder'](),
    nameFieldTestId: 'permission-edit-name-input',
    nameInputId: 'edit-permission-name',
    descriptionFieldTestId: 'permission-edit-description-input',
    descriptionInputId: 'edit-permission-description',
    submitButtonTestId: 'permission-edit-submit-button',
    submitButtonText: m['permissions.update_button'](),
    submittingButtonText: m['permissions.updating'](),
  }

  const builtinProtection = {
    isBuiltin: permission.isBuiltin,
    alertMessage: m['permissions.builtin_alert'](),
    disabledFieldHelpText: m['permissions.builtin_name_disabled'](),
  }

  return (
    <EditResourceDialog
      open={open}
      onOpenChange={onOpenChange}
      config={config}
      title={m['permissions.edit_title']()}
      description={m['permissions.edit_description']()}
      builtinProtection={builtinProtection}
      currentValues={{
        name: permission.name,
        description: permission.description ?? '',
      }}
    />
  )
}
