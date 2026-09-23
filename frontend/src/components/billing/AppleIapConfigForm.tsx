import { useEffect, useMemo } from 'react'
import { useNavigate } from '@tanstack/react-router'
import { toast } from 'sonner'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import {
  appleConfigSchema,
  type AppleIapConfigForm as AppleIapConfigFormValues,
  getAppleIapConfigDefaults,
} from '@/lib/schemas/apple-config'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Label } from '@/components/ui/label'
import { PageHeader } from '@/components/shared/page-header'
import { FormActionBar } from '@/components/shared/form-action-bar'
import { TextField, TextareaField } from '@/components/shared/form-fields'
import { batchUpsertRealmConfigs } from '@/lib/api-generated/sdk.gen'
import { buildAppleConfigRequest } from '@/lib/apple-config-utils'
import { requireFieldOnCreate } from '@/lib/form-utils'
import { useSaveConfigMutation } from '@/hooks/use-save-config-mutation'
import { m } from '@/paraglide/messages'
import { realmPath, useResolvedRealmContext } from '@/lib/realm-routing'

interface AppleIapConfigFormPageProps {
  realmId: string
  mode: 'create' | 'edit'
  initialValues?: Partial<AppleIapConfigFormValues>
}

export function AppleIapConfigFormPage({
  realmId,
  mode,
  initialValues,
}: AppleIapConfigFormPageProps) {
  const navigate = useNavigate()
  const realmContext = useResolvedRealmContext()
  const isEditing = mode === 'edit'

  const defaultValues = useMemo(() => getAppleIapConfigDefaults(initialValues), [initialValues])

  const saveMutation = useSaveConfigMutation<AppleIapConfigFormValues>({
    realmId,
    providerName: 'App Store',
    isEditing,
    mutationFn: async (data) => {
      const response = await batchUpsertRealmConfigs({
        body: { configs: buildAppleConfigRequest(data) },
      })
      if (response.error) throw response.error
    },
  })

  const form = useAppForm({
    schema: appleConfigSchema,
    defaultValues,
    onSubmit: async ({ value }) => {
      if (
        !requireFieldOnCreate(
          form,
          isEditing,
          'bundleId',
          value.bundleId,
          m['billing.apple_bundle_id_required']()
        )
      )
        return
      if (
        !requireFieldOnCreate(
          form,
          isEditing,
          'issuerId',
          value.issuerId,
          m['billing.apple_issuer_id_required']()
        )
      )
        return
      if (
        !requireFieldOnCreate(
          form,
          isEditing,
          'keyId',
          value.keyId,
          m['billing.apple_key_id_required']()
        )
      )
        return
      // The mutation's `onError` surfaces the failure toast. Swallow the rejected
      // promise so it doesn't propagate as an unhandled rejection (sibling forms
      // share this pattern). See CreemConfigForm handoff note.
      await saveMutation.mutateAsync(value).catch(() => {})
      toast.info(
        m['billing.apple_webhook_url_hint']({ url: `/api/third/pay/${realmId}/apple/webhooks` })
      )
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
  const privateKeyHelpText = isEditing
    ? `${m['billing.apple_private_key_help']()}. ${m['billing.leave_empty_keep']()}`
    : m['billing.apple_private_key_help']()

  return (
    <div className="container max-w-3xl mx-auto py-6 px-6" data-testid="apple-config-form-page">
      <PageHeader
        title={isEditing ? m['billing.apple_edit']() : m['billing.apple_configure']()}
        headingTestId="apple-config-form-page-heading"
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
        data-testid="apple-config-page-form"
        className="space-y-6 pt-6"
      >
        <AppForm>
          <div className="space-y-6">
            <TextField
              form={form}
              name="bundleId"
              label={m['billing.apple_bundle_id']()}
              dataTestId="apple-bundle-id-input"
              placeholder="com.example.app"
              helpText={m['billing.apple_bundle_id_help']()}
              required={!isEditing}
            />

            <TextField
              form={form}
              name="issuerId"
              label={m['billing.apple_issuer_id']()}
              dataTestId="apple-issuer-id-input"
              placeholder="xxxxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
              helpText={m['billing.apple_issuer_id_help']()}
              required={!isEditing}
            />

            <TextField
              form={form}
              name="keyId"
              label={m['billing.apple_key_id']()}
              dataTestId="apple-key-id-input"
              placeholder="ABCDE12345"
              helpText={m['billing.apple_key_id_help']()}
              required={!isEditing}
            />

            <TextareaField
              form={form}
              name="privateKeyP8"
              label={m['billing.apple_private_key_p8']()}
              dataTestId="apple-private-key-p8-input"
              placeholder="-----BEGIN PRIVATE KEY-----"
              helpText={privateKeyHelpText}
              rows={8}
              required={!isEditing}
            />

            <form.Field
              name="environment"
              children={(field) => (
                <div className="space-y-2">
                  <Label htmlFor="apple-environment">{m['billing.apple_environment']()}</Label>
                  <Select
                    data-testid="apple-environment-select"
                    value={field.state.value ?? 'production'}
                    onValueChange={(value) => field.handleChange(value as 'sandbox' | 'production')}
                  >
                    <SelectTrigger
                      id="apple-environment"
                      className="w-full"
                      data-testid="apple-environment-select-trigger"
                    >
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="production">
                        {m['billing.apple_environment_production']()}
                      </SelectItem>
                      <SelectItem value="sandbox">
                        {m['billing.apple_environment_sandbox']()}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                  <p className="text-xs text-muted-foreground">
                    {m['billing.apple_environment_help']()}
                  </p>
                </div>
              )}
            />
          </div>
        </AppForm>

        <FormActionBar
          onCancel={handleCancel}
          isSubmitting={isSubmitting}
          isEditing={isEditing}
          cancelTestId="apple-config-page-cancel-button"
          submitTestId="apple-config-page-submit-button"
        />
      </form>
    </div>
  )
}
