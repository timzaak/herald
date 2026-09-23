import { useEffect, useMemo } from 'react'
import { useNavigate } from '@tanstack/react-router'
import { toast } from 'sonner'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import {
  googlePlayConfigSchema,
  type GooglePlayConfigForm as GooglePlayConfigFormValues,
  getGooglePlayConfigDefaults,
} from '@/lib/schemas/google-config'
import { PageHeader } from '@/components/shared/page-header'
import { FormActionBar } from '@/components/shared/form-action-bar'
import { TextField, TextareaField } from '@/components/shared/form-fields'
import { batchUpsertRealmConfigs } from '@/lib/api-generated/sdk.gen'
import { buildGoogleConfigRequest } from '@/lib/google-config-utils'
import { requireFieldOnCreate } from '@/lib/form-utils'
import { useSaveConfigMutation } from '@/hooks/use-save-config-mutation'
import { m } from '@/paraglide/messages'
import { realmPath, useResolvedRealmContext } from '@/lib/realm-routing'

interface GooglePlayConfigFormPageProps {
  realmId: string
  mode: 'create' | 'edit'
  initialValues?: Partial<GooglePlayConfigFormValues>
}

export function GooglePlayConfigFormPage({
  realmId,
  mode,
  initialValues,
}: GooglePlayConfigFormPageProps) {
  const navigate = useNavigate()
  const realmContext = useResolvedRealmContext()
  const isEditing = mode === 'edit'

  const defaultValues = useMemo(() => getGooglePlayConfigDefaults(initialValues), [initialValues])

  const saveMutation = useSaveConfigMutation<GooglePlayConfigFormValues>({
    realmId,
    providerName: 'Google Play',
    isEditing,
    mutationFn: async (data) => {
      const response = await batchUpsertRealmConfigs({
        body: { configs: buildGoogleConfigRequest(data) },
      })
      if (response.error) throw response.error
    },
  })

  const form = useAppForm({
    schema: googlePlayConfigSchema,
    defaultValues,
    onSubmit: async ({ value }) => {
      if (
        !requireFieldOnCreate(
          form,
          isEditing,
          'packageName',
          value.packageName,
          m['billing.google_package_name_required']()
        )
      )
        return
      // The mutation's `onError` surfaces the failure toast. Swallow the rejected
      // promise so it doesn't propagate as an unhandled rejection (sibling forms
      // share this pattern). See CreemConfigForm handoff note.
      await saveMutation.mutateAsync(value).catch(() => {})
      toast.info(m['billing.google_service_account_hint']())
    },
  })

  useEffect(() => {
    form.reset(defaultValues)
  }, [defaultValues, form])

  const handleCancel = () => {
    navigate({
      to: realmPath({ ...realmContext, realmId }, '/manage/billing/payment-providers'),
    })
  }

  const isSubmitting = saveMutation.isPending
  const serviceAccountHelpText = isEditing
    ? `${m['billing.google_service_account_json_help']()}. ${m['billing.leave_empty_keep']()}`
    : m['billing.google_service_account_json_help']()

  return (
    <div className="container max-w-3xl mx-auto py-6 px-6" data-testid="google-config-form-page">
      <PageHeader
        title={isEditing ? m['billing.google_edit']() : m['billing.google_configure']()}
        headingTestId="google-config-form-page-heading"
      />

      <form
        onSubmit={async (e) => {
          e.preventDefault()
          e.stopPropagation()
          await form.validateAllFields('submit')
          if (!form.state.isFieldsValid) {
            return
          }
          await form.handleSubmit()
        }}
        data-testid="google-config-page-form"
        className="space-y-6 pt-6"
      >
        <AppForm>
          <div className="space-y-6">
            <TextField
              form={form}
              name="packageName"
              label={m['billing.google_package_name']()}
              dataTestId="google-package-name-input"
              placeholder="com.example.app"
              helpText={m['billing.google_package_name_help']()}
              required={!isEditing}
            />

            <TextareaField
              form={form}
              name="serviceAccountJson"
              label={m['billing.google_service_account_json']()}
              dataTestId="google-service-account-json-input"
              placeholder='{ "type": "service_account", ... }'
              helpText={serviceAccountHelpText}
              rows={10}
              required={!isEditing}
            />
          </div>
        </AppForm>

        <FormActionBar
          onCancel={handleCancel}
          isSubmitting={isSubmitting}
          isEditing={isEditing}
          cancelTestId="google-config-page-cancel-button"
          submitTestId="google-config-page-submit-button"
        />
      </form>
    </div>
  )
}
