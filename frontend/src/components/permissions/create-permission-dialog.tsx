import { CreateResourceDialog, type ResourceFormConfig } from '@/components/shared'
import { createPermissionSchema, type CreatePermissionFormData } from '@/lib/schemas/common'
import { createPermissionDefinition } from '@/lib/api-generated'
import { useRealmId } from '@/stores/auth-store'
import { queryKeys } from '@/data/query-options'
import { m } from '@/paraglide/messages'

interface CreatePermissionDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  realmId?: string // Optional for backward compatibility
}

export function CreatePermissionDialog({
  open,
  onOpenChange,
  realmId: realmIdProp,
}: CreatePermissionDialogProps) {
  const storeRealmId = useRealmId()
  const realmId = realmIdProp ?? storeRealmId
  const config: ResourceFormConfig<CreatePermissionFormData> = {
    schema: createPermissionSchema,
    defaultValues: {
      name: '',
      description: '',
    },
    mutationFn: (data: CreatePermissionFormData) =>
      createPermissionDefinition({
        body: data,
      }),
    getSuccessMessage: (response) => {
      const permission = (response as { data?: { name?: string } }).data
      return `Permission "${permission?.name}" added successfully`
    },
    queryKeysToInvalidate: [queryKeys.permissions(realmId)],
    nameFieldLabel: m['permissions.name_label'](),
    nameFieldPlaceholder: m['permissions.name_placeholder'](),
    nameFieldHelpText: m['permissions.name_help'](),
    nameFieldTestId: 'permission-create-name-input',
    nameInputId: 'permission-name',
    descriptionFieldPlaceholder: m['permissions.description_placeholder'](),
    descriptionFieldTestId: 'permission-create-description-input',
    descriptionInputId: 'permission-description',
    submitButtonTestId: 'permission-create-submit-button',
    submitButtonText: m['permissions.add_button_short'](),
    submittingButtonText: m['permissions.adding'](),
  }

  return (
    <CreateResourceDialog
      open={open}
      onOpenChange={onOpenChange}
      config={config}
      title={m['permissions.add_title']()}
      description={m['permissions.add_description']()}
    />
  )
}
