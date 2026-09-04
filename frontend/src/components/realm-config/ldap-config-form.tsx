import React from 'react'
import { useStore } from '@tanstack/react-form'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import {
  ldapConfigSchema,
  type LdapConfigForm as LdapConfigFormValues,
} from '@/lib/schemas/realm-config'
import type { LdapConfigState } from '@/lib/schemas/realm-config'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { ConfigSwitchField } from './config-switch-field'
import { TextField } from '@/components/shared/form-fields/text-field'
import { PasswordField } from '@/components/shared/form-fields/password-field'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { getFieldErrorMessage } from '@/lib/form-utils'
import { emptyLdapConfig } from '@/lib/realm-config-utils'
import { isLdapsUrl } from '@/lib/schemas/realm-config'
import { useFormSubmit } from './use-form-submit'
import { m } from '@/paraglide/messages'

interface LdapConfigFormProps {
  initialConfig?: LdapConfigState
  /**
   * Whether a `bind_password` row has ever been saved. The value is always
   * masked server-side, so row existence is the only usable signal; enabling
   * with a service-account DN but no stored password would be rejected by the
   * backend — the form blocks it first with an actionable message.
   */
  hasBindPassword?: boolean
  onSave: (config: LdapConfigFormValues) => Promise<void>
  isLoading?: boolean
  disabled?: boolean
}

export function LdapConfigForm({
  initialConfig,
  hasBindPassword,
  onSave,
  isLoading,
  disabled,
}: LdapConfigFormProps) {
  const { handleSubmit, isSubmitting } = useFormSubmit(onSave)
  // Blocked-save gate message (enable + bindDn + never-saved password). Kept
  // outside the zod schema because it depends on server state the schema
  // cannot see.
  const [saveBlocked, setSaveBlocked] = React.useState<string | null>(null)

  const form = useAppForm({
    schema: ldapConfigSchema,
    defaultValues: initialConfig ?? emptyLdapConfig(),
    onSubmit: async ({ value }) => {
      if (value.enabled && value.bindDn.trim() && !hasBindPassword && !value.bindPassword) {
        setSaveBlocked(m['settings.ldap.error_bind_password_required']())
        return
      }
      setSaveBlocked(null)
      try {
        await handleSubmit(value)
      } catch {
        // useFormSubmit already logged; the mutation's onError toast is the
        // user-facing error surface.
      }
    },
  })

  const urlValue = useStore(form.store, (state) => state.values.url as string)
  // ldaps:// carries TLS in the scheme; StartTLS would be redundant and is
  // rejected by the backend — the switch is locked off for ldaps:// URLs.
  const isLdaps = isLdapsUrl(urlValue)

  return (
    <Card>
      <CardHeader>
        <CardTitle>{m['settings.ldap.title']()}</CardTitle>
        <CardDescription>{m['settings.ldap.description']()}</CardDescription>
      </CardHeader>
      <CardContent>
        <AppForm>
          <form
            onSubmit={(e) => {
              e.preventDefault()
              form.handleSubmit()
            }}
            className="space-y-4 max-w-lg"
          >
            <form.Field
              name="enabled"
              children={(field) => (
                <ConfigSwitchField
                  field={field}
                  form={form}
                  id="ldap-enabled"
                  label={m['settings.ldap.enabled_label']()}
                  description={m['settings.ldap.enabled_description']()}
                  disabled={disabled}
                  errorTestId="ldap-enabled-error"
                />
              )}
            />

            <form.Field name="url">
              {(field) => (
                <div className="space-y-2">
                  <Label htmlFor="ldap-url">{m['settings.ldap.url_label']()}</Label>
                  <Input
                    id="ldap-url"
                    type="text"
                    placeholder="ldaps://directory.example.com:636"
                    value={field.state.value}
                    onBlur={field.handleBlur}
                    onChange={(e) => {
                      field.handleChange(e.target.value)
                      if (isLdapsUrl(e.target.value) && form.getFieldValue('starttls')) {
                        form.setFieldValue('starttls', false)
                      }
                    }}
                    disabled={disabled}
                    data-testid="ldap-url-input"
                  />
                  {(field.state.meta.isTouched || form.state.isSubmitted) &&
                    field.state.meta.errors.length > 0 && (
                      <p className="text-sm text-destructive" role="alert">
                        {getFieldErrorMessage(field.state.meta)}
                      </p>
                    )}
                </div>
              )}
            </form.Field>

            <form.Field
              name="starttls"
              children={(field) => (
                <ConfigSwitchField
                  field={field}
                  form={form}
                  id="ldap-starttls"
                  label={m['settings.ldap.starttls_label']()}
                  description={
                    isLdaps
                      ? m['settings.ldap.starttls_locked_description']()
                      : m['settings.ldap.starttls_description']()
                  }
                  disabled={disabled || isLdaps}
                  errorTestId="ldap-starttls-error"
                />
              )}
            />

            <TextField
              form={form}
              name="baseDn"
              label={m['settings.ldap.base_dn_label']()}
              inputId="ldap-basedn"
              dataTestId="ldap-basedn-input"
              placeholder="dc=example,dc=com"
              disabled={disabled}
            />

            <TextField
              form={form}
              name="bindDn"
              label={m['settings.ldap.bind_dn_label']()}
              inputId="ldap-binddn"
              dataTestId="ldap-binddn-input"
              placeholder="cn=herald,ou=services,dc=example,dc=com"
              helpText={m['settings.ldap.bind_dn_help']()}
              disabled={disabled}
            />

            <PasswordField
              form={form}
              name="bindPassword"
              label={m['settings.ldap.bind_password_label']()}
              inputId="ldap-bind-password"
              dataTestId="ldap-bind-password-input"
              placeholder={m['settings.ldap.bind_password_placeholder']()}
              helpText={m['settings.ldap.bind_password_help']()}
              disabled={disabled}
            />

            {saveBlocked && (
              <p
                className="text-sm text-destructive"
                data-testid="ldap-bind-password-error"
                role="alert"
              >
                {saveBlocked}
              </p>
            )}

            <TextField
              form={form}
              name="userFilter"
              label={m['settings.ldap.user_filter_label']()}
              inputId="ldap-user-filter"
              dataTestId="ldap-user-filter-input"
              placeholder="(&(objectClass=user)(sAMAccountName={login}))"
              helpText={m['settings.ldap.user_filter_help']({ token: '{login}' })}
              disabled={disabled}
            />

            <TextField
              form={form}
              name="mailAttribute"
              label={m['settings.ldap.mail_attribute_label']()}
              inputId="ldap-mail-attribute"
              dataTestId="ldap-mail-attribute-input"
              placeholder="mail"
              disabled={disabled}
            />

            <TextField
              form={form}
              name="displayNameAttribute"
              label={m['settings.ldap.display_name_attribute_label']()}
              inputId="ldap-display-name-attribute"
              dataTestId="ldap-display-name-attribute-input"
              placeholder="displayName"
              helpText={m['settings.ldap.display_name_attribute_help']()}
              disabled={disabled}
            />

            <div className="flex justify-end">
              <Button
                type="submit"
                disabled={isLoading || isSubmitting || disabled}
                data-testid="ldap-save-button"
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
