import { useEffect, useMemo } from 'react'
import { useNavigate } from '@tanstack/react-router'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import {
  creemConfigSchema,
  type CreemConfigForm as CreemConfigFormValues,
  getCreemConfigDefaults,
} from '@/lib/schemas/creem-config'
import { PageHeader } from '@/components/shared/page-header'
import { FormActionBar } from '@/components/shared/form-action-bar'
import { PasswordField, NumberField } from '@/components/shared/form-fields'
import { batchUpsertRealmConfigs } from '@/lib/api-generated/sdk.gen'
import { buildCreemConfigRequest } from '@/lib/creem-config-utils'
import { requireFieldOnCreate } from '@/lib/form-utils'
import { useSaveConfigMutation } from '@/hooks/use-save-config-mutation'
import { m } from '@/paraglide/messages'
import { realmPath, useResolvedRealmContext } from '@/lib/realm-routing'

interface CreemConfigFormPageProps {
  realmId: string
  mode: 'create' | 'edit'
  initialValues?: Partial<CreemConfigFormValues>
}

export function CreemConfigFormPage({ realmId, mode, initialValues }: CreemConfigFormPageProps) {
  const navigate = useNavigate()
  const realmContext = useResolvedRealmContext()
  const isEditing = mode === 'edit'

  const defaultValues = useMemo(() => getCreemConfigDefaults(initialValues), [initialValues])

  const saveMutation = useSaveConfigMutation<CreemConfigFormValues>({
    realmId,
    providerName: 'Creem',
    isEditing,
    mutationFn: async (data) => {
      const response = await batchUpsertRealmConfigs({
        body: { configs: buildCreemConfigRequest(data) },
      })
      if (response.error) throw response.error
    },
  })

  const form = useAppForm({
    schema: creemConfigSchema,
    defaultValues,
    onSubmit: async ({ value }) => {
      if (
        !requireFieldOnCreate(
          form,
          isEditing,
          'apiKey',
          value.apiKey,
          m['billing.creem_api_key_required']()
        )
      )
        return
      // The mutation's `onError` surfaces the failure toast and logs the error.
      // Swallow the rejected promise so it doesn't propagate as an unhandled
      // rejection (per vitest config, rejections are expected to be handled in
      // components). Sibling forms share this latent leak; see FE-T06 handoff.
      await saveMutation.mutateAsync(value).catch(() => {})
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
  const apiKeyHelpText = isEditing
    ? `${m['billing.creem_api_key_help']()}. ${m['billing.creem_api_key_format']()}`
    : m['billing.creem_api_key_format']()

  return (
    <div className="container max-w-3xl mx-auto py-6 px-6" data-testid="creem-config-form-page">
      <PageHeader
        title={isEditing ? m['billing.edit_creem']() : m['billing.configure_creem']()}
        headingTestId="creem-config-form-page-heading"
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
        data-testid="creem-config-page-form"
        className="space-y-6 pt-6"
      >
        <AppForm>
          <div className="space-y-6">
            <PasswordField
              form={form}
              name="apiKey"
              label={m['billing.creem_api_key']()}
              dataTestId="page-creem-api-key-input"
              placeholder="ck_live_..."
              helpText={apiKeyHelpText}
              required={!isEditing}
            />

            <NumberField
              form={form}
              name="timeout"
              label={m['billing.creem_timeout']()}
              dataTestId="page-creem-timeout-input"
              min={1}
              max={120}
              helpText={m['billing.creem_timeout_help']()}
            />

            <PasswordField
              form={form}
              name="webhookSecret"
              label={m['billing.creem_webhook_secret']()}
              dataTestId="page-creem-webhook-secret-input"
              placeholder="whsec_..."
              helpText={m['billing.creem_webhook_secret_help']()}
            />
          </div>
        </AppForm>

        <FormActionBar
          onCancel={handleCancel}
          isSubmitting={isSubmitting}
          isEditing={isEditing}
          cancelTestId="creem-config-page-cancel-button"
          submitTestId="creem-config-page-submit-button"
        />
      </form>
    </div>
  )
}
