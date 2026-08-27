/**
 * Corporate-directory (LDAP) login form.
 *
 * Presentational: fields + Turnstile + submit/back. The route owns the login
 * mutation and all post-login routing (second factor / consent / OAuth
 * redirect / success); submitted values travel up through `onSubmit`. Errors
 * render in the route's shared `login-error-message` region — including the
 * 503/429 localized mappings from `resolveLdapLoginError` below.
 */

import { useForm } from '@tanstack/react-form'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { TurnstileWidget } from '@/components/auth/turnstile-widget'
import { AgreementLinks } from '@/components/legal/AgreementLinks'
import { m } from '@/paraglide/messages'
import { ldapLoginSchema } from '@/lib/schemas/common'
import { getFieldErrorMessage } from '@/lib/error-utils'
import type { TurnstileStatusResponse } from '@/lib/api-generated'

export interface LdapLoginFormValues {
  username: string
  password: string
  turnstileToken?: string
}

export interface LdapLoginFormProps {
  realmId: string
  /** Shared login-mutation pending state; disables inputs + submit. */
  isPending: boolean
  /** Blocks submission while OAuth params are incomplete (same guard as the password form). */
  hasPartialOAuth?: boolean
  /** Client App-level Turnstile status resolved by the route. */
  turnstileStatus?: TurnstileStatusResponse | null
  onSubmit: (values: LdapLoginFormValues) => void
  /** Return to the password form. */
  onBack: () => void
}

export function LdapLoginForm({
  realmId,
  isPending,
  hasPartialOAuth,
  turnstileStatus,
  onSubmit,
  onBack,
}: LdapLoginFormProps) {
  const form = useForm({
    defaultValues: { username: '', password: '', turnstileToken: '' },
    onSubmit: async ({ value }) => {
      onSubmit({
        username: value.username,
        password: value.password,
        turnstileToken: value.turnstileToken || undefined,
      })
    },
  })

  const submitDisabled = isPending || !!hasPartialOAuth

  return (
    <div className="space-y-4" data-testid="ldap-login-form">
      <div>
        <h3 className="font-semibold">{m['auth.ldap.title']()}</h3>
        <p className="text-sm text-muted-foreground">{m['auth.ldap.description']()}</p>
      </div>

      <form
        onSubmit={(e) => {
          e.preventDefault()
          form.handleSubmit()
        }}
        className="space-y-4"
      >
        <form.Field name="username" validators={{ onChange: ldapLoginSchema.shape.username }}>
          {(field) => (
            <div>
              <Label htmlFor="ldap-username">{m['auth.ldap.username_label']()}</Label>
              <Input
                id="ldap-username"
                type="text"
                autoComplete="username"
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(e) => field.handleChange(e.target.value)}
                disabled={isPending}
                data-testid="ldap-username-input"
              />
              {field.state.meta.errors.length > 0 && (
                <p className="text-sm text-destructive mt-1">
                  {getFieldErrorMessage(field.state.meta.errors[0])}
                </p>
              )}
            </div>
          )}
        </form.Field>

        <form.Field name="password" validators={{ onChange: ldapLoginSchema.shape.password }}>
          {(field) => (
            <div>
              <Label htmlFor="ldap-password">{m['auth.ldap.password_label']()}</Label>
              <Input
                id="ldap-password"
                type="password"
                autoComplete="current-password"
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(e) => field.handleChange(e.target.value)}
                disabled={isPending}
                data-testid="ldap-password-input"
              />
              {field.state.meta.errors.length > 0 && (
                <p className="text-sm text-destructive mt-1">
                  {getFieldErrorMessage(field.state.meta.errors[0])}
                </p>
              )}
            </div>
          )}
        </form.Field>

        {turnstileStatus?.enabled && turnstileStatus.siteKey && (
          <form.Field name="turnstileToken">
            {(field) => (
              <TurnstileWidget
                siteKey={turnstileStatus.siteKey || ''}
                onTokenChange={(token) => field.handleChange(token || '')}
                onError={(error) => console.error('Turnstile error:', error)}
              />
            )}
          </form.Field>
        )}

        <Button
          type="submit"
          disabled={submitDisabled}
          className="w-full"
          data-testid="ldap-submit-button"
        >
          {isPending ? m['auth.login.logging_in']() : m['auth.ldap.submit']()}
        </Button>

        <div
          className="pt-2 text-xs leading-relaxed text-muted-foreground"
          data-testid="ldap-consent-statement"
        >
          {m['auth.login.consent_statement']()}
          <AgreementLinks
            realmId={realmId}
            beforeText=" "
            linkClassName="text-primary hover:text-primary/80 underline underline-offset-2"
          />
        </div>
      </form>

      <Button
        type="button"
        variant="ghost"
        className="w-full"
        onClick={onBack}
        data-testid="ldap-back-button"
      >
        {m['auth.ldap.back']()}
      </Button>
    </div>
  )
}
