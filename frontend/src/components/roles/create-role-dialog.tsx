import { CreateResourceDialog, type ResourceFormConfig } from '@/components/shared'
import { createRoleSchema, type CreateRoleFormData } from '@/lib/schemas/common'
import { createRole } from '@/lib/api-generated'
import { useRealmId } from '@/stores/auth-store'
import { queryKeys } from '@/data/query-options'
import { m } from '@/paraglide/messages'

interface CreateRoleDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  realmId?: string // Optional for backward compatibility
}

export function CreateRoleDialog({
  open,
  onOpenChange,
  realmId: realmIdProp,
}: CreateRoleDialogProps) {
  const storeRealmId = useRealmId()
  const realmId = realmIdProp ?? storeRealmId
  const config: ResourceFormConfig<CreateRoleFormData> = {
    schema: createRoleSchema,
    defaultValues: {
      name: '',
      description: '',
    },
    mutationFn: (data: CreateRoleFormData) =>
      createRole({
        body: {
          ...data,
          clientId: 'admin-web-console',
        },
      }),
    getSuccessMessage: (response) => {
      const role = (response as { data?: { name?: string } }).data
      return role
        ? m['roles.added_success']({ name: role.name ?? '' })
        : m['roles.added_success_fallback']()
    },
    queryKeysToInvalidate: [queryKeys.roles(realmId)],
    nameFieldLabel: m['roles.name_label'](),
    nameFieldPlaceholder: m['roles.name_placeholder'](),
    nameFieldHelpText: m['roles.name_help'](),
    nameFieldTestId: 'role-create-name-input',
    nameInputId: 'role-name',
    descriptionFieldPlaceholder: m['roles.description_placeholder'](),
    descriptionFieldTestId: 'role-create-description-input',
    descriptionInputId: 'role-description',
    submitButtonTestId: 'role-create-submit-button',
    submitButtonText: m['roles.add_button_short'](),
    submittingButtonText: m['roles.adding'](),
  }

  return (
    <CreateResourceDialog
      open={open}
      onOpenChange={onOpenChange}
      config={config}
      title={m['roles.add_title']()}
      description={m['roles.add_description']()}
    />
  )
}
