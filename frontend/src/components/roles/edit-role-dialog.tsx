import { EditResourceDialog, type ResourceFormConfig } from '@/components/shared'
import { updateRoleSchema, type UpdateRoleFormData } from '@/lib/schemas/common'
import { updateRole } from '@/lib/api-generated'
import { useRealmId } from '@/stores/auth-store'
import type { RoleResponse } from '@/lib/api-generated'
import { queryKeys } from '@/data/query-options'
import { m } from '@/paraglide/messages'

interface EditRoleDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  role: RoleResponse
  realmId?: string // Optional for backward compatibility
}

export function EditRoleDialog({
  open,
  onOpenChange,
  role,
  realmId: realmIdProp,
}: EditRoleDialogProps) {
  const storeRealmId = useRealmId()
  const realmId = realmIdProp ?? storeRealmId
  const config: ResourceFormConfig<UpdateRoleFormData> = {
    schema: updateRoleSchema,
    defaultValues: {
      name: role.name,
      description: role.description ?? '',
    },
    mutationFn: (data: UpdateRoleFormData) =>
      updateRole({
        path: { roleId: role.id },
        body: data,
      }),
    getSuccessMessage: (response) => {
      const r = (response as { data?: { name?: string } }).data
      return m['roles.updated_success']({ name: r?.name ?? '' })
    },
    queryKeysToInvalidate: [queryKeys.roles(realmId)],
    nameFieldLabel: m['roles.name_label'](),
    nameFieldPlaceholder: role.name,
    descriptionFieldPlaceholder: m['roles.description_placeholder'](),
    nameFieldTestId: 'role-edit-name-input',
    nameInputId: 'edit-role-name',
    descriptionFieldTestId: 'role-edit-description-input',
    descriptionInputId: 'edit-role-description',
    submitButtonTestId: 'role-edit-submit-button',
    submitButtonText: m['roles.update_button'](),
    submittingButtonText: m['roles.updating'](),
  }

  const builtinProtection = {
    isBuiltin: role.isBuiltin,
    disabledFieldHelpText: m['roles.builtin_name_disabled'](),
  }

  return (
    <EditResourceDialog
      open={open}
      onOpenChange={onOpenChange}
      config={config}
      title={m['roles.edit_title']()}
      description={m['roles.edit_description']()}
      builtinProtection={builtinProtection}
      currentValues={{
        name: role.name,
        description: role.description ?? '',
      }}
    />
  )
}
