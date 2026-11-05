import React from 'react'
import { useMutation } from '@tanstack/react-query'
import { useStore } from '@tanstack/react-form'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import {
  smsConfigSchema,
  phoneLoginConfigSchema,
  type SmsConfigForm as SmsConfigFormValues,
  type PhoneLoginConfigForm as PhoneLoginConfigFormValues,
} from '@/lib/schemas/realm-config'
import type { SmsStatusResponse } from '@/lib/api-generated/types.gen'
import { smsTest } from '@/lib/api-generated/sdk.gen'
import { handleApiResponse } from '@/lib/api-utils'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { ConfigSwitchField } from './config-switch-field'
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group'
import { Label } from '@/components/ui/label'
import { TextField } from '@/components/shared/form-fields/text-field'
import { PasswordField } from '@/components/shared/form-fields/password-field'
import { Input } from '@/components/ui/input'
import { getErrorMessage } from '@/lib/error-utils'
import { useFormSubmit } from './use-form-submit'
import { m } from '@/paraglide/messages'

interface SmsConfigFormProps {
  initialConfig?: SmsConfigFormValues
  onSave: (config: SmsConfigFormValues) => Promise<void>
  isLoading?: boolean
  disabled?: boolean
  smsStatus?: SmsStatusResponse | null
  smsStatusError?: string | null
  // The phone-login switch is gated on the SMS channel above being
  // configured: the switch stays disabled until `smsStatus.configured` is
  // true (fail-closed; the server re-checks on PUT). Passed through from the
  // settings page.
  phoneLoginInitialConfig?: PhoneLoginConfigFormValues
  /**
   * Settle state of the phone-login config query. `defaultValues` seed once
   * at mount, so the switch section renders only after the query has settled:
   * a late or failed query would otherwise leave the switch showing seed
   * defaults that a save would persist (silently disabling phone login
   * realm-wide).
   */
  phoneLoginLoading?: boolean
  phoneLoginError?: string | null
  onSavePhoneLogin?: (config: PhoneLoginConfigFormValues) => Promise<void>
}

export function SmsConfigForm({
  initialConfig,
  onSave,
  isLoading,
  disabled,
  smsStatus,
  smsStatusError,
  phoneLoginInitialConfig,
  phoneLoginLoading,
  phoneLoginError,
  onSavePhoneLogin,
}: SmsConfigFormProps) {
  const [saveError, setSaveError] = React.useState<string | null>(null)
  const [testPhone, setTestPhone] = React.useState('')

  const { handleSubmit, isSubmitting } = useFormSubmit(onSave)

  const form = useAppForm({
    schema: smsConfigSchema,
    defaultValues: initialConfig || {
      provider: 'aliyun' as const,
    },
    onSubmit: async ({ value }) => {
      setSaveError(null)
      try {
        await handleSubmit(value)
      } catch (error) {
        setSaveError(getErrorMessage(error))
      }
    },
  })

  const provider = useStore(form.store, (state) => state.values.provider)

  const phoneLoginForm = useAppForm({
    schema: phoneLoginConfigSchema,
    defaultValues: phoneLoginInitialConfig || { enabled: false, defaultCountryCode: 'CN' },
    onSubmit: async ({ value }) => {
      await onSavePhoneLogin?.(value)
    },
  })

  // Fail-closed switch gate (US-PL-004 scene 3 UI surface), direction-aware:
  // the server rejects ENABLING without a configured channel but accepts a
  // disable, so an enabled switch stays interactive when the channel breaks —
  // locking it would strand a dangling enabled config with no in-UI way out.
  // The dependency re-evaluates live because `smsStatus` is invalidated on
  // channel save.
  const smsConfigured = smsStatus?.configured ?? false
  const phoneLoginEnabled = useStore(phoneLoginForm.store, (state) => state.values.enabled)
  const phoneLoginDisabled = disabled || (!smsConfigured && !phoneLoginEnabled)
  const phoneLoginSaveDisabled = disabled || (!smsConfigured && phoneLoginEnabled)
  const phoneLoginSwitchTooltip =
    smsConfigured || phoneLoginEnabled ? undefined : m['realm_config.sms.not_configured']()

  const testSmsMutation = useMutation({
    mutationFn: async (phone: string) => {
      const response = await smsTest({
        body: { phone },
      })
      return handleApiResponse(response)
    },
  })

  const handleSendTestSms = () => {
    // Non-empty only: the server is the phone format authority
    // (DEC-phone-login-004); its uniform invalid-phone copy surfaces verbatim.
    if (!testPhone.trim()) return
    testSmsMutation.mutate(testPhone.trim())
  }

  return (
    <Card data-testid="sms-config-form">
      <CardHeader>
        <div className="flex items-center justify-between">
          <div className="space-y-1">
            <CardTitle>{m['realm_config.sms.title']()}</CardTitle>
            <CardDescription>{m['realm_config.sms.description']()}</CardDescription>
          </div>
          {smsStatus && (
            <Badge
              variant={smsStatus.configured ? 'default' : 'outline'}
              className={
                smsStatus.configured
                  ? 'bg-success/10 text-success border-success/20'
                  : 'bg-warning/10 text-warning border-warning/20'
              }
              data-testid="sms-config-status-badge"
            >
              {smsStatus.configured
                ? m['realm_config.sms.configured']()
                : m['realm_config.sms.not_configured']()}
            </Badge>
          )}
        </div>
      </CardHeader>
      {smsStatusError && (
        <div className="px-6 pb-2">
          <p className="text-sm text-destructive" data-testid="sms-status-error">
            {smsStatusError}
          </p>
        </div>
      )}
      {!smsStatus?.configured && smsStatus && smsStatus.missingFields.length > 0 && (
        <div className="px-6 pb-2">
          <p className="text-sm text-muted-foreground" data-testid="sms-missing-fields">
            {m['realm_config.sms.missing_fields']({
              fields: smsStatus.missingFields.join(', '),
            })}
          </p>
        </div>
      )}
      <CardContent>
        <AppForm>
          <form
            onSubmit={(e) => {
              e.preventDefault()
              setSaveError(null)
              form.handleSubmit()
            }}
            className="space-y-6"
          >
            <form.Field
              name="provider"
              children={(field) => (
                <div className="space-y-2">
                  <Label>{m['realm_config.sms.provider_label']()}</Label>
                  <RadioGroup
                    value={field.state.value}
                    onValueChange={(value) => field.handleChange(value as 'aliyun' | 'twilio')}
                    disabled={disabled}
                    className="flex gap-4"
                    data-testid="sms-provider-radio"
                  >
                    <div className="flex items-center space-x-2">
                      <RadioGroupItem value="aliyun" id="sms-provider-aliyun" />
                      <Label htmlFor="sms-provider-aliyun">
                        {m['realm_config.sms.provider_aliyun']()}
                      </Label>
                    </div>
                    <div className="flex items-center space-x-2">
                      <RadioGroupItem value="twilio" id="sms-provider-twilio" />
                      <Label htmlFor="sms-provider-twilio">
                        {m['realm_config.sms.provider_twilio']()}
                      </Label>
                    </div>
                  </RadioGroup>
                </div>
              )}
            />

            {provider === 'aliyun' && (
              <div className="space-y-4">
                <TextField
                  form={form}
                  name="aliyunAccessKeyId"
                  label={m['realm_config.sms.aliyun_access_key_id_label']()}
                  inputId="sms-aliyun-access-key-id"
                  dataTestId="sms-aliyun-access-key-id-input"
                  placeholder="LTAI5t..."
                  disabled={disabled}
                />

                <PasswordField
                  form={form}
                  name="aliyunAccessKeySecret"
                  label={m['realm_config.sms.aliyun_access_key_secret_label']()}
                  inputId="sms-aliyun-access-key-secret"
                  dataTestId="sms-aliyun-access-key-secret-input"
                  disabled={disabled}
                />

                <TextField
                  form={form}
                  name="aliyunSignName"
                  label={m['realm_config.sms.aliyun_sign_name_label']()}
                  inputId="sms-aliyun-sign-name"
                  dataTestId="sms-aliyun-sign-name-input"
                  placeholder="Herald"
                  disabled={disabled}
                />

                <TextField
                  form={form}
                  name="aliyunTemplateCode"
                  label={m['realm_config.sms.aliyun_template_code_label']()}
                  inputId="sms-aliyun-template-code"
                  dataTestId="sms-aliyun-template-code-input"
                  placeholder="SMS_123456789"
                  disabled={disabled}
                />
              </div>
            )}

            {provider === 'twilio' && (
              <div className="space-y-4">
                <TextField
                  form={form}
                  name="twilioAccountSid"
                  label={m['realm_config.sms.twilio_account_sid_label']()}
                  inputId="sms-twilio-account-sid"
                  dataTestId="sms-twilio-account-sid-input"
                  placeholder="ACxxxxxxxx"
                  disabled={disabled}
                />

                <PasswordField
                  form={form}
                  name="twilioAuthToken"
                  label={m['realm_config.sms.twilio_auth_token_label']()}
                  inputId="sms-twilio-auth-token"
                  dataTestId="sms-twilio-auth-token-input"
                  disabled={disabled}
                />

                <TextField
                  form={form}
                  name="twilioFrom"
                  label={m['realm_config.sms.twilio_from_label']()}
                  inputId="sms-twilio-from"
                  dataTestId="sms-twilio-from-input"
                  placeholder="+15017122661"
                  disabled={disabled}
                />

                <TextField
                  form={form}
                  name="twilioContentSid"
                  label={m['realm_config.sms.twilio_content_sid_label']()}
                  inputId="sms-twilio-content-sid"
                  dataTestId="sms-twilio-content-sid-input"
                  placeholder="HXxxxxxxxx"
                  disabled={disabled}
                />
              </div>
            )}

            {saveError && (
              <p className="text-sm text-destructive" data-testid="sms-save-error">
                {saveError}
              </p>
            )}

            <div className="flex items-center justify-between gap-4">
              <div className="flex items-center gap-2">
                <Input
                  placeholder={m['realm_config.sms.test_phone_placeholder']()}
                  value={testPhone}
                  onChange={(e) => setTestPhone(e.target.value)}
                  disabled={disabled}
                  type="tel"
                  className="max-w-[260px]"
                  data-testid="sms-test-phone-input"
                />
                <Button
                  type="button"
                  variant="outline"
                  onClick={handleSendTestSms}
                  disabled={disabled || !testPhone.trim() || testSmsMutation.isPending}
                  data-testid="sms-test-send-button"
                >
                  {testSmsMutation.isPending
                    ? m['realm_config.sms.test_sending']()
                    : m['realm_config.sms.test_button']()}
                </Button>
              </div>

              <Button
                type="submit"
                disabled={isLoading || isSubmitting || disabled}
                data-testid="sms-save-button"
              >
                {isSubmitting ? m['realm_config.saving']() : m['realm_config.save']()}
              </Button>
            </div>

            {testSmsMutation.isError && (
              <p className="text-sm text-destructive" data-testid="sms-test-error">
                {getErrorMessage(testSmsMutation.error)}
              </p>
            )}

            {testSmsMutation.isSuccess && (
              <p className="text-sm text-success" data-testid="sms-test-success">
                {testSmsMutation.data?.message || m['realm_config.sms.test_success']()}
              </p>
            )}

            {onSavePhoneLogin && phoneLoginLoading && !phoneLoginInitialConfig && (
              <div className="border-t pt-6" data-testid="phone-login-section">
                <p className="text-sm text-muted-foreground">{m['settings.config_loading']()}</p>
              </div>
            )}

            {onSavePhoneLogin && phoneLoginError && (
              <div className="border-t pt-6" data-testid="phone-login-section">
                <p className="text-sm text-destructive" data-testid="phone-login-config-error">
                  {phoneLoginError}
                </p>
              </div>
            )}

            {onSavePhoneLogin &&
              !phoneLoginError &&
              !(phoneLoginLoading && !phoneLoginInitialConfig) && (
                <div className="space-y-4 border-t pt-6" data-testid="phone-login-section">
                  <div className="space-y-1">
                    <h4 className="text-base font-medium">
                      {m['realm_config.sms.switch_title']()}
                    </h4>
                    <p className="text-sm text-muted-foreground">
                      {m['realm_config.sms.switch_description']()}
                    </p>
                  </div>

                  <phoneLoginForm.Field
                    name="enabled"
                    children={(field) => (
                      <ConfigSwitchField
                        field={field}
                        form={phoneLoginForm}
                        id="phone-login-enabled"
                        label={m['realm_config.sms.switch_label']()}
                        description={m['realm_config.sms.switch_enable_description']()}
                        disabled={phoneLoginDisabled}
                        switchTooltip={phoneLoginSwitchTooltip}
                        errorTestId="phone-login-enabled-error"
                      />
                    )}
                  />

                  <phoneLoginForm.Field
                    name="defaultCountryCode"
                    children={(field) => (
                      <div className="space-y-2">
                        <Label htmlFor="phone-login-country-code">
                          {m['realm_config.sms.default_country_code_label']()}
                        </Label>
                        <Input
                          id="phone-login-country-code"
                          value={field.state.value}
                          onBlur={field.handleBlur}
                          onChange={(e) => field.handleChange(e.target.value.toUpperCase())}
                          disabled={phoneLoginDisabled}
                          className="max-w-[80px] uppercase"
                          maxLength={2}
                          placeholder="CN"
                          data-testid="default-country-code-input"
                        />
                        {field.state.meta.errors.length > 0 && (
                          <p className="text-sm text-destructive">
                            {m['realm_config.sms.default_country_code_invalid']()}
                          </p>
                        )}
                      </div>
                    )}
                  />

                  <div className="flex justify-end">
                    <Button
                      type="button"
                      onClick={() => phoneLoginForm.handleSubmit()}
                      disabled={phoneLoginSaveDisabled}
                      data-testid="phone-login-save-button"
                    >
                      {m['realm_config.save']()}
                    </Button>
                  </div>
                </div>
              )}
          </form>
        </AppForm>
      </CardContent>
    </Card>
  )
}
