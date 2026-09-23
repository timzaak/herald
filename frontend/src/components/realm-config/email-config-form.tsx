import React from 'react'
import { useMutation } from '@tanstack/react-query'
import { useStore } from '@tanstack/react-form'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import {
  emailConfigSchema,
  emailOtpConfigSchema,
  type EmailConfigForm as EmailConfigFormValues,
  type EmailOtpConfigForm as EmailOtpConfigFormValues,
} from '@/lib/schemas/realm-config'
import type { EmailStatusResponse } from '@/lib/api-generated/types.gen'
import { emailTest } from '@/lib/api-generated/sdk.gen'
import { handleApiResponse } from '@/lib/api-utils'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { ConfigSwitchField } from './config-switch-field'
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group'
import { Label } from '@/components/ui/label'
import { TextField } from '@/components/shared/form-fields/text-field'
import { PasswordField } from '@/components/shared/form-fields/password-field'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Input } from '@/components/ui/input'
import { getErrorMessage } from '@/lib/error-utils'
import { useFormSubmit } from './use-form-submit'
import { m } from '@/paraglide/messages'

interface EmailConfigFormProps {
  initialConfig?: EmailConfigFormValues
  onSave: (config: EmailConfigFormValues) => Promise<void>
  isLoading?: boolean
  disabled?: boolean
  emailStatus?: EmailStatusResponse | null
  emailStatusError?: string | null
  // Email-OTP login config is gated on the email channel above being
  // configured: the OTP switches stay disabled until `emailStatus.configured`
  // is true. Passed through from the settings page.
  emailOtpInitialConfig?: EmailOtpConfigFormValues
  onSaveEmailOtp?: (config: EmailOtpConfigFormValues) => Promise<void>
}

export function EmailConfigForm({
  initialConfig,
  onSave,
  isLoading,
  disabled,
  emailStatus,
  emailStatusError,
  emailOtpInitialConfig,
  onSaveEmailOtp,
}: EmailConfigFormProps) {
  const [saveError, setSaveError] = React.useState<string | null>(null)
  const [testRecipient, setTestRecipient] = React.useState('')

  const { handleSubmit, isSubmitting } = useFormSubmit(onSave)

  const form = useAppForm({
    schema: emailConfigSchema,
    defaultValues: initialConfig || {
      provider: 'resend' as const,
      fromAddress: '',
      smtpPort: '587',
      smtpEncryption: 'starttls' as const,
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

  // Email-OTP login depends on the email channel being configured. When it
  // isn't, the switches are disabled and a hint is shown (mirrors the
  // `emailConfigured` pattern in registration-config-form). The dependency
  // re-evaluates live because `emailStatus` is invalidated on email save.
  const emailConfigured = emailStatus?.configured ?? false
  const emailOtpDisabled = disabled || !emailConfigured

  const otpForm = useAppForm({
    schema: emailOtpConfigSchema,
    defaultValues: emailOtpInitialConfig || { enabled: false, autoRegister: false },
    onSubmit: async ({ value }) => {
      await onSaveEmailOtp?.(value)
    },
  })

  const testEmailMutation = useMutation({
    mutationFn: async (recipient: string) => {
      const response = await emailTest({
        body: { recipient },
      })
      return handleApiResponse(response)
    },
  })

  const handleSendTestEmail = () => {
    if (!testRecipient) return
    testEmailMutation.mutate(testRecipient)
  }

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center justify-between">
          <div className="space-y-1">
            <CardTitle>{m['realm_config.email_title']()}</CardTitle>
            <CardDescription>{m['realm_config.email_description']()}</CardDescription>
          </div>
          {emailStatus && (
            <Badge
              variant={emailStatus.configured ? 'default' : 'outline'}
              className={
                emailStatus.configured
                  ? 'bg-success/10 text-success border-success/20'
                  : 'bg-warning/10 text-warning border-warning/20'
              }
              data-testid="email-config-status-badge"
            >
              {emailStatus.configured
                ? m['realm_config.email_configured']()
                : m['realm_config.email_not_configured']()}
            </Badge>
          )}
        </div>
      </CardHeader>
      {emailStatusError && (
        <div className="px-6 pb-2">
          <p className="text-sm text-destructive" data-testid="email-status-error">
            {emailStatusError}
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
                  <Label>{m['realm_config.email_provider_label']()}</Label>
                  <RadioGroup
                    value={field.state.value}
                    onValueChange={(value) => field.handleChange(value as 'resend' | 'smtp')}
                    disabled={disabled}
                    className="flex gap-4"
                  >
                    <div className="flex items-center space-x-2">
                      <RadioGroupItem
                        value="resend"
                        id="provider-resend"
                        data-testid="email-provider-resend"
                      />
                      <Label htmlFor="provider-resend">
                        {m['realm_config.email_provider_resend']()}
                      </Label>
                    </div>
                    <div className="flex items-center space-x-2">
                      <RadioGroupItem
                        value="smtp"
                        id="provider-smtp"
                        data-testid="email-provider-smtp"
                      />
                      <Label htmlFor="provider-smtp">
                        {m['realm_config.email_provider_smtp']()}
                      </Label>
                    </div>
                  </RadioGroup>
                </div>
              )}
            />

            <TextField
              form={form}
              name="fromAddress"
              label={m['realm_config.email_from_address_label']()}
              inputId="email-from-address"
              dataTestId="email-from-address-input"
              type="email"
              placeholder="noreply@example.com"
              disabled={disabled}
              required
            />

            {provider === 'resend' && (
              <PasswordField
                form={form}
                name="resendApiKey"
                label={m['realm_config.email_api_key_label']()}
                inputId="email-resend-api-key"
                dataTestId="email-resend-api-key-input"
                placeholder="re_xxxxxxxx"
                disabled={disabled}
                helpText={m['realm_config.email_api_key_help']()}
              />
            )}

            {provider === 'smtp' && (
              <div className="space-y-4">
                <TextField
                  form={form}
                  name="smtpHost"
                  label={m['realm_config.email_smtp_host_label']()}
                  inputId="email-smtp-host"
                  dataTestId="email-smtp-host-input"
                  placeholder="smtp.example.com"
                  disabled={disabled}
                />

                <TextField
                  form={form}
                  name="smtpPort"
                  label={m['realm_config.email_smtp_port_label']()}
                  inputId="email-smtp-port"
                  dataTestId="email-smtp-port-input"
                  placeholder="587"
                  disabled={disabled}
                />

                <form.Field
                  name="smtpEncryption"
                  children={(field) => (
                    <div className="space-y-2">
                      <Label htmlFor="email-smtp-encryption">
                        {m['realm_config.email_smtp_encryption_label']()}
                      </Label>
                      <Select
                        value={field.state.value}
                        onValueChange={(value) => field.handleChange(value as 'starttls' | 'ssl')}
                        disabled={disabled}
                      >
                        <SelectTrigger
                          id="email-smtp-encryption"
                          data-testid="email-smtp-encryption-select"
                        >
                          <SelectValue
                            placeholder={m['realm_config.email_smtp_encryption_placeholder']()}
                          />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="starttls">STARTTLS</SelectItem>
                          <SelectItem value="ssl">SSL</SelectItem>
                        </SelectContent>
                      </Select>
                    </div>
                  )}
                />

                <TextField
                  form={form}
                  name="smtpUsername"
                  label={m['realm_config.email_smtp_username_label']()}
                  inputId="email-smtp-username"
                  dataTestId="email-smtp-username-input"
                  placeholder="user@example.com"
                  disabled={disabled}
                />

                <PasswordField
                  form={form}
                  name="smtpPassword"
                  label={m['realm_config.email_smtp_password_label']()}
                  inputId="email-smtp-password"
                  dataTestId="email-smtp-password-input"
                  placeholder="Enter password"
                  disabled={disabled}
                />
              </div>
            )}

            {saveError && (
              <p className="text-sm text-destructive" data-testid="email-save-error">
                {saveError}
              </p>
            )}

            <div className="flex items-center justify-between gap-4">
              <div className="flex items-center gap-2">
                <Input
                  placeholder={m['realm_config.email_test_recipient_placeholder']()}
                  value={testRecipient}
                  onChange={(e) => setTestRecipient(e.target.value)}
                  disabled={disabled}
                  type="email"
                  className="max-w-[260px]"
                  data-testid="email-test-recipient-input"
                />
                <Button
                  type="button"
                  variant="outline"
                  onClick={handleSendTestEmail}
                  disabled={disabled || !testRecipient || testEmailMutation.isPending}
                  data-testid="email-test-button"
                >
                  {testEmailMutation.isPending
                    ? m['realm_config.email_test_sending']()
                    : m['realm_config.email_test_button']()}
                </Button>
              </div>

              <Button
                type="submit"
                disabled={isLoading || isSubmitting || disabled}
                data-testid="email-save-button"
              >
                {isSubmitting ? m['realm_config.saving']() : m['realm_config.save']()}
              </Button>
            </div>

            {testEmailMutation.isError && (
              <p className="text-sm text-destructive" data-testid="email-test-error">
                {getErrorMessage(testEmailMutation.error)}
              </p>
            )}

            {testEmailMutation.isSuccess && (
              <p className="text-sm text-success" data-testid="email-test-success">
                {testEmailMutation.data?.message || m['realm_config.email_test_success']()}
              </p>
            )}

            {onSaveEmailOtp && (
              <div className="space-y-4 border-t pt-6" data-testid="email-otp-section">
                <div className="space-y-1">
                  <h4 className="text-base font-medium">{m['realm_config.email_otp_title']()}</h4>
                  <p className="text-sm text-muted-foreground">
                    {m['realm_config.email_otp_description']()}
                  </p>
                </div>

                <otpForm.Field
                  name="enabled"
                  children={(field) => (
                    <ConfigSwitchField
                      field={field}
                      form={otpForm}
                      id="email-otp-enabled"
                      label={m['realm_config.email_otp_enable_label']()}
                      description={m['realm_config.email_otp_enable_description']()}
                      disabled={emailOtpDisabled}
                      errorTestId="email-otp-enabled-error"
                    />
                  )}
                />

                <otpForm.Field
                  name="autoRegister"
                  children={(field) => (
                    <ConfigSwitchField
                      field={field}
                      form={otpForm}
                      id="email-otp-auto-register"
                      label={m['realm_config.email_otp_auto_register_label']()}
                      description={m['realm_config.email_otp_auto_register_description']()}
                      disabled={emailOtpDisabled}
                      errorTestId="email-otp-auto-register-error"
                    />
                  )}
                />

                {!emailConfigured && (
                  <span
                    className="text-sm text-muted-foreground"
                    data-testid="email-otp-email-required-hint"
                  >
                    {m['realm_config.email_otp_email_not_configured']()}
                  </span>
                )}

                <div className="flex justify-end">
                  <Button
                    type="button"
                    onClick={() => otpForm.handleSubmit()}
                    disabled={emailOtpDisabled}
                    data-testid="email-otp-save-button"
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
