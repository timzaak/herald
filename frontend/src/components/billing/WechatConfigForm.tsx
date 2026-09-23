import { useEffect, useMemo } from 'react'
import { useNavigate } from '@tanstack/react-router'
import { toast } from 'sonner'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import {
  wechatConfigSchema,
  type WechatConfigForm as WechatConfigFormValues,
  getWechatConfigDefaults,
} from '@/lib/schemas/wechat-config'
import { PageHeader } from '@/components/shared/page-header'
import { FormActionBar } from '@/components/shared/form-action-bar'
import { TextField, TextareaField, PasswordField } from '@/components/shared/form-fields'
import { batchUpsertRealmConfigs } from '@/lib/api-generated/sdk.gen'
import { buildWechatConfigRequest } from '@/lib/wechat-config-utils'
import { requireFieldOnCreate } from '@/lib/form-utils'
import { useSaveConfigMutation } from '@/hooks/use-save-config-mutation'
import { m } from '@/paraglide/messages'
import { realmPath, useResolvedRealmContext } from '@/lib/realm-routing'

interface WechatConfigFormPageProps {
  realmId: string
  mode: 'create' | 'edit'
  initialValues?: Partial<WechatConfigFormValues>
}

export function WechatConfigFormPage({ realmId, mode, initialValues }: WechatConfigFormPageProps) {
  const navigate = useNavigate()
  const realmContext = useResolvedRealmContext()
  const isEditing = mode === 'edit'

  const defaultValues = useMemo(() => getWechatConfigDefaults(initialValues), [initialValues])

  const saveMutation = useSaveConfigMutation<WechatConfigFormValues>({
    realmId,
    providerName: 'WeChat Pay',
    isEditing,
    // The route's config query (['wechat-config', realmId]) is not covered by
    // the default invalidation keys; without invalidating it the reopened edit
    // form serves 5-min-stale cache and echoes pre-edit non-secret values.
    invalidateKeys: [
      ['payment-providers', realmId],
      ['realmConfig', realmId],
      ['wechat-config', realmId],
    ],
    mutationFn: async (data) => {
      const response = await batchUpsertRealmConfigs({
        body: { configs: buildWechatConfigRequest(data) },
      })
      if (response.error) throw response.error
    },
  })

  const form = useAppForm({
    schema: wechatConfigSchema,
    defaultValues,
    onSubmit: async ({ value }) => {
      if (
        !requireFieldOnCreate(
          form,
          isEditing,
          'appId',
          value.appId,
          m['billing.wechat_app_id_required']()
        )
      )
        return
      if (
        !requireFieldOnCreate(
          form,
          isEditing,
          'mchId',
          value.mchId,
          m['billing.wechat_mch_id_required']()
        )
      )
        return
      if (
        !requireFieldOnCreate(
          form,
          isEditing,
          'serialNo',
          value.serialNo,
          m['billing.wechat_serial_no_required']()
        )
      )
        return
      if (
        !requireFieldOnCreate(
          form,
          isEditing,
          'privateKey',
          value.privateKey,
          m['billing.wechat_private_key_required']()
        )
      )
        return
      if (
        !requireFieldOnCreate(
          form,
          isEditing,
          'v3Key',
          value.v3Key,
          m['billing.wechat_v3_key_required']()
        )
      )
        return
      if (
        !requireFieldOnCreate(
          form,
          isEditing,
          'notifyUrl',
          value.notifyUrl,
          m['billing.wechat_notify_url_required']()
        )
      )
        return
      // The mutation's `onError` surfaces the failure toast. Swallow the rejected
      // promise so it doesn't propagate as an unhandled rejection (sibling forms
      // share this pattern). See CreemConfigForm handoff note.
      await saveMutation.mutateAsync(value).catch(() => {})
      toast.info(
        m['billing.wechat_webhook_url_hint']({ url: `/api/third/pay/${realmId}/wechat/webhooks` })
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
    ? `${m['billing.wechat_private_key_help']()}. ${m['billing.leave_empty_keep']()}`
    : m['billing.wechat_private_key_help']()
  const v3KeyHelpText = isEditing
    ? `${m['billing.wechat_v3_key_help']()}. ${m['billing.leave_empty_keep']()}`
    : m['billing.wechat_v3_key_help']()

  return (
    <div className="container max-w-3xl mx-auto py-6 px-6" data-testid="wechat-config-form-page">
      <PageHeader
        title={isEditing ? m['billing.wechat_edit']() : m['billing.wechat_configure']()}
        headingTestId="wechat-config-form-page-heading"
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
        data-testid="wechat-config-page-form"
        className="space-y-6 pt-6"
      >
        <AppForm>
          <div className="space-y-6">
            <TextField
              form={form}
              name="appId"
              label={m['billing.wechat_app_id']()}
              dataTestId="wechat-app-id-input"
              placeholder="wx1234567890abcdef"
              helpText={m['billing.wechat_app_id_help']()}
              required={!isEditing}
            />

            <TextField
              form={form}
              name="mchId"
              label={m['billing.wechat_mch_id']()}
              dataTestId="wechat-mch-id-input"
              placeholder="1900000109"
              helpText={m['billing.wechat_mch_id_help']()}
              required={!isEditing}
            />

            <TextField
              form={form}
              name="serialNo"
              label={m['billing.wechat_serial_no']()}
              dataTestId="wechat-serial-no-input"
              placeholder="5157F09EFDC096DE15EBE81A47057A72..."
              helpText={m['billing.wechat_serial_no_help']()}
              required={!isEditing}
            />

            <TextareaField
              form={form}
              name="privateKey"
              label={m['billing.wechat_private_key']()}
              dataTestId="wechat-private-key-input"
              placeholder="-----BEGIN PRIVATE KEY-----"
              helpText={privateKeyHelpText}
              rows={8}
              required={!isEditing}
            />

            <PasswordField
              form={form}
              name="v3Key"
              label={m['billing.wechat_v3_key']()}
              dataTestId="wechat-v3-key-input"
              placeholder="32-character APIv3 key"
              helpText={v3KeyHelpText}
              required={!isEditing}
            />

            <TextField
              form={form}
              name="notifyUrl"
              label={m['billing.wechat_notify_url']()}
              dataTestId="wechat-notify-url-input"
              placeholder="https://example.com/api/third/pay/..."
              helpText={m['billing.wechat_notify_url_help']()}
              required={!isEditing}
            />

            <TextareaField
              form={form}
              name="platformPublicKey"
              label={m['billing.wechat_platform_public_key']()}
              dataTestId="wechat-platform-public-key-input"
              placeholder="-----BEGIN PUBLIC KEY-----"
              helpText={m['billing.wechat_platform_public_key_help']()}
              rows={6}
            />
          </div>
        </AppForm>

        <FormActionBar
          onCancel={handleCancel}
          isSubmitting={isSubmitting}
          isEditing={isEditing}
          cancelTestId="wechat-config-page-cancel-button"
          submitTestId="wechat-config-page-submit-button"
        />
      </form>
    </div>
  )
}
