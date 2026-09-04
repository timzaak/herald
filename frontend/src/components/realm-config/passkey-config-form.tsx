import React from 'react'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import {
  passkeyConfigSchema,
  type PasskeyConfigForm as PasskeyConfigFormValues,
} from '@/lib/schemas/realm-config'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Label } from '@/components/ui/label'
import { ConfigSwitchField } from './config-switch-field'
import { m } from '@/paraglide/messages'

interface PasskeyConfigFormProps {
  initialConfig?: PasskeyConfigFormValues
  onSave: (config: PasskeyConfigFormValues) => Promise<void>
  isLoading?: boolean
  disabled?: boolean
}

export function PasskeyConfigForm({
  initialConfig,
  onSave,
  isLoading,
  disabled,
}: PasskeyConfigFormProps) {
  const [isSubmitting, setIsSubmitting] = React.useState(false)

  const form = useAppForm({
    schema: passkeyConfigSchema,
    defaultValues: initialConfig || {
      enabled: false,
      forceEnabled: false,
      userVerification: 'preferred',
      crossPlatformAuthenticator: true,
    },
    onSubmit: async ({ value }) => {
      setIsSubmitting(true)
      try {
        await onSave(value)
      } catch (error) {
        // Log error for visibility but don't re-throw
        // The parent component should handle display of error messages
        console.error('Failed to save configuration:', error)
      } finally {
        setIsSubmitting(false)
      }
    },
  })

  return (
    <Card>
      <CardHeader>
        <CardTitle>{m['realm_config.passkey_title']()}</CardTitle>
        <CardDescription>{m['realm_config.passkey_description']()}</CardDescription>
      </CardHeader>
      <CardContent>
        <AppForm>
          <form
            onSubmit={(e) => {
              e.preventDefault()
              form.handleSubmit()
            }}
            className="space-y-4"
          >
            {/* Enable Passkey */}
            <form.Field
              name="enabled"
              children={(field) => (
                <ConfigSwitchField
                  field={field}
                  form={form}
                  id="passkey-enabled"
                  label={m['realm_config.passkey_enable_label']()}
                  description={m['realm_config.passkey_enable_description']()}
                  disabled={disabled}
                  errorTestId="passkey-enabled-error"
                />
              )}
            />

            {/* Force mode: guide users without a passkey to register one */}
            <form.Field
              name="forceEnabled"
              children={(field) => (
                <ConfigSwitchField
                  field={field}
                  form={form}
                  id="passkey-force-enabled"
                  label={m['realm_config.passkey_force_label']()}
                  description={m['realm_config.passkey_force_description']()}
                  disabled={disabled}
                  errorTestId="passkey-force-enabled-error"
                />
              )}
            />

            {/* P1: User verification requirement */}
            <form.Field
              name="userVerification"
              children={(field) => (
                <div className="space-y-2">
                  <Label htmlFor="passkey-user-verification">
                    {m['realm_config.passkey_user_verification_label']()}
                  </Label>
                  <Select
                    value={field.state.value}
                    onValueChange={(value) => field.handleChange(value as 'preferred' | 'required')}
                    disabled={disabled}
                  >
                    <SelectTrigger
                      id="passkey-user-verification"
                      data-testid="passkey-user-verification-select"
                    >
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="preferred">
                        {m['realm_config.passkey_user_verification_preferred']()}
                      </SelectItem>
                      <SelectItem value="required">
                        {m['realm_config.passkey_user_verification_required']()}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                  <p className="text-sm text-muted-foreground">
                    {m['realm_config.passkey_user_verification_description']()}
                  </p>
                </div>
              )}
            />

            {/* P1: Cross-platform authenticator requirement */}
            <form.Field
              name="crossPlatformAuthenticator"
              children={(field) => (
                <ConfigSwitchField
                  field={field}
                  form={form}
                  id="passkey-cross-platform"
                  label={m['realm_config.passkey_cross_platform_label']()}
                  description={m['realm_config.passkey_cross_platform_description']()}
                  disabled={disabled}
                  errorTestId="passkey-cross-platform-error"
                />
              )}
            />

            <div className="flex justify-end">
              <Button
                type="submit"
                disabled={isLoading || isSubmitting || disabled}
                data-testid="passkey-save-button"
              >
                {isSubmitting ? m['realm_config.saving']() : m['realm_config.save']()}
              </Button>
            </div>
          </form>
        </AppForm>
      </CardContent>
    </Card>
  )
}
