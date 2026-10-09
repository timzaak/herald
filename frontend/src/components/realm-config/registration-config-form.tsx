import React from 'react'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import {
  registrationConfigSchema,
  type RegistrationConfigForm as RegistrationConfigFormValues,
} from '@/lib/schemas/realm-config'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { ConfigSwitchField } from './config-switch-field'
import { m } from '@/paraglide/messages'

interface RegistrationConfigFormProps {
  initialConfig?: RegistrationConfigFormValues
  onSave: (config: RegistrationConfigFormValues) => Promise<void>
  isLoading?: boolean
  disabled?: boolean
  emailConfigured?: boolean
}

export function RegistrationConfigForm({
  initialConfig,
  onSave,
  isLoading,
  disabled,
  emailConfigured = true,
}: RegistrationConfigFormProps) {
  const [isSubmitting, setIsSubmitting] = React.useState(false)

  const form = useAppForm({
    schema: registrationConfigSchema,
    defaultValues: initialConfig || {
      enabled: true,
      requireEmailVerification: true,
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
        <CardTitle>{m['realm_config.registration_title']()}</CardTitle>
        <CardDescription>{m['realm_config.registration_description']()}</CardDescription>
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
            {/* Allow Registration */}
            <form.Field
              name="enabled"
              children={(field) => (
                <ConfigSwitchField
                  field={field}
                  form={form}
                  id="reg-enabled"
                  label={m['realm_config.registration_enable_label']()}
                  description={m['realm_config.registration_enable_description']()}
                  disabled={disabled}
                  errorTestId="reg-enabled-error"
                />
              )}
            />

            {/* Require Email Verification */}
            <form.Field
              name="requireEmailVerification"
              children={(field) => (
                <ConfigSwitchField
                  field={field}
                  form={form}
                  id="reg-require-email"
                  label={m['realm_config.registration_email_verify_label']()}
                  description={m['realm_config.registration_email_verify_description']()}
                  checked={emailConfigured ? field.state.value : false}
                  disabled={disabled || !emailConfigured}
                  switchTooltip={
                    !emailConfigured
                      ? m['realm_config.registration_email_not_configured']()
                      : undefined
                  }
                  errorTestId="reg-require-email-error"
                />
              )}
            />

            <div className="flex justify-end">
              <Button
                type="submit"
                disabled={isLoading || isSubmitting || disabled}
                data-testid="reg-save-button"
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
