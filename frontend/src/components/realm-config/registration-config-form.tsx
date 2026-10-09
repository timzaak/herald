import React from 'react'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import {
  registrationConfigSchema,
  type RegistrationConfigForm as RegistrationConfigFormValues,
} from '@/lib/schemas/realm-config'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { ConfigSwitchField } from './config-switch-field'
import { useFormSubmit } from './use-form-submit'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { m } from '@/paraglide/messages'

interface RegistrationConfigFormProps {
  initialConfig?: RegistrationConfigFormValues
  onSave: (config: RegistrationConfigFormValues) => Promise<void>
  isLoading?: boolean
  disabled?: boolean
  emailConfigured?: boolean
  /**
   * True while any legal agreement still rides the platform default template.
   * Enabling registration then intercepts the save with a reminder dialog.
   * Unknown (undefined) fails open so the reminder always gets a chance.
   */
  agreementsUsingDefault?: boolean
  /** Jump to the agreements/legal settings tab from the reminder dialog. */
  onGoToLegal?: () => void
}

export function RegistrationConfigForm({
  initialConfig,
  onSave,
  isLoading,
  disabled,
  emailConfigured = true,
  agreementsUsingDefault,
  onGoToLegal,
}: RegistrationConfigFormProps) {
  const { handleSubmit: submitConfig, isSubmitting } = useFormSubmit(onSave)
  // Validated form values parked behind the agreements reminder dialog;
  // confirming drains them into onSave, cancel/go-configure drops them.
  const [pendingEnable, setPendingEnable] = React.useState<RegistrationConfigFormValues | null>(
    null
  )

  const form = useAppForm({
    schema: registrationConfigSchema,
    defaultValues: initialConfig || {
      enabled: true,
      requireEmailVerification: true,
    },
    onSubmit: async ({ value }) => {
      // Opening registration is the moment users start consenting to this
      // realm's agreements; if either one is still the platform default
      // placeholder, hold the save and remind the admin to publish their
      // own version first. Confirmable — advice, not a hard gate. Only the
      // disabled→enabled transition triggers it so routine saves on an
      // already-open realm don't nag.
      const wasEnabled = initialConfig?.enabled ?? false
      if (value.enabled && !wasEnabled && (agreementsUsingDefault ?? true)) {
        setPendingEnable(value)
        return
      }
      try {
        await submitConfig(value)
      } catch {
        // useFormSubmit already logged; the parent's mutation onError toast
        // is the user-facing error surface.
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

        {/* Agreements reminder: opening signup while the realm still uses the
            platform default agreement templates. Cancel drops the (unsaved)
            switch state — the tab remounts from the saved config on return —
            and optionally jumps to the agreements tab; the action proceeds
            with the parked save. */}
        <AlertDialog
          open={pendingEnable !== null}
          onOpenChange={(open) => {
            if (!open) setPendingEnable(null)
          }}
        >
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle data-testid="reg-agreement-dialog-title">
                {m['realm_config.registration_agreement_dialog_title']()}
              </AlertDialogTitle>
              <AlertDialogDescription data-testid="reg-agreement-dialog-description">
                {m['realm_config.registration_agreement_dialog_description']()}
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel
                onClick={() => onGoToLegal?.()}
                disabled={isSubmitting}
                data-testid="reg-agreement-dialog-go-legal"
              >
                {onGoToLegal
                  ? m['realm_config.registration_agreement_dialog_go_legal']()
                  : m['common.cancel']()}
              </AlertDialogCancel>
              <AlertDialogAction
                onClick={() => {
                  // Cancel/Action auto-close the dialog, which clears
                  // pendingEnable via onOpenChange; capture it first.
                  const value = pendingEnable
                  if (value) void submitConfig(value).catch(() => {})
                }}
                disabled={isSubmitting}
                data-testid="reg-agreement-dialog-confirm"
              >
                {m['realm_config.registration_agreement_dialog_enable_anyway']()}
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </CardContent>
    </Card>
  )
}
