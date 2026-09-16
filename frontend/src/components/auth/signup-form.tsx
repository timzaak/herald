import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import { signup } from '@/lib/api-generated'
import type { SignupRequest } from '@/lib/api-generated'
import { completeSignup } from '@/lib/auth-utils'
import { ADMIN_REALM_ID, ADMIN_WEB_CONSOLE_CLIENT_ID } from '@/lib/constants/auth-constants'
import { DEFAULT_PASSWORD_CONFIG } from '@/lib/password-strength'
import { useFormMutation } from '@/hooks/use-form-mutation'
import { PasswordStrengthMeter } from './password-strength-meter'
import { TurnstileWidget } from './turnstile-widget'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { getFieldErrorMessage } from '@/lib/form-utils'
import { TextField } from '@/components/shared/form-fields/text-field'
import { useQuery } from '@tanstack/react-query'
import { queryKeys, turnstileStatusQueryOptions } from '@/data/query-options'
import { toast } from 'sonner'
import { m } from '@/paraglide/messages'
import { signupSchema, type SignupFormValues } from '@/lib/schemas/realm-signup'

// Self-service signup is an admin-realm-only public entry (DEC-001). All API
// calls are fixed to the admin realm regardless of the URL the page was opened
// under, and the Turnstile probe targets the admin-web-console Client App
// (DEC-008) — not the user-account-center default of `turnstileStatusQueryOptions`.

interface SignupFormProps {
  /** Called after the new realm's session is hydrated, with the redirect path. */
  onSuccess: (redirectPath: string, realmId: string) => void
}

export function SignupForm({ onSuccess }: SignupFormProps) {
  // Turnstile is bound to the admin realm's admin-web-console Client App
  // (DEC-008). Pass the client id explicitly — the default would probe
  // user-account-center, which is not the signup entry's Client App.
  const { data: turnstileStatus, isLoading: loadingTurnstile } = useQuery(
    turnstileStatusQueryOptions(ADMIN_REALM_ID, ADMIN_WEB_CONSOLE_CLIENT_ID)
  )

  const { isSubmitting, mutate } = useFormMutation({
    mutationFn: async (data: SignupFormValues) => {
      const apiData: SignupRequest = {
        realmName: data.realmName,
        realmSlug: data.realmSlug || null,
        email: data.email,
        password: data.password,
        turnstileToken: data.turnstileToken || null,
      }
      const { data: result, error } = await signup({
        path: { realmId: ADMIN_REALM_ID },
        body: apiData,
        throwOnError: false,
      })
      if (error) {
        throw error
      }
      if (!result) {
        throw new Error('No data in response')
      }
      return result
    },
    getSuccessMessage: () => m['auth.signup.success_title'](),
    invalidateQueries: [queryKeys.realmsList()],
    onSuccess: async (data) => {
      // The signup body issues a first-party token set for the NEW realm
      // (DEC-012). Persist it, hydrate permissions/profile, then navigate the
      // user into the new realm's management console.
      try {
        const { redirectPath } = await completeSignup(
          data.realmId,
          { accessToken: data.accessToken, refreshToken: data.refreshToken },
          ADMIN_WEB_CONSOLE_CLIENT_ID
        )
        onSuccess(redirectPath, data.realmId)
      } catch (error) {
        // Hydration failed after the realm was created — surface the error;
        // completeSignup already tore down the partial session.
        toast.error((error as Error)?.message ?? m['auth.signup.error_loading']())
      }
    },
  })

  const form = useAppForm({
    schema: signupSchema,
    defaultValues: {
      realmName: '',
      realmSlug: '',
      email: '',
      password: '',
      turnstileToken: undefined,
    },
    onSubmit: async ({ value }) => {
      // `useFormMutation` surfaces errors via its `onError` (toast). React
      // Query's `mutateAsync` also re-throws after `onError`; catch it here so
      // the form's onSubmit promise doesn't reject unhandled, while still
      // keeping the user-visible error display from `onError`.
      await mutate(value).catch(() => {})
    },
  })

  return (
    <AppForm>
      <form
        onSubmit={(e) => {
          e.preventDefault()
          e.stopPropagation()
          form.handleSubmit()
        }}
        className="space-y-4"
      >
        <TextField
          form={form}
          name="realmName"
          label={m['auth.signup.realm_name_label']()}
          dataTestId="signup-realm-name-input"
          disabled={isSubmitting}
        />

        <TextField
          form={form}
          name="realmSlug"
          label={m['auth.signup.realm_slug_label']()}
          placeholder={m['auth.signup.realm_slug_optional']()}
          dataTestId="signup-realm-slug-input"
          disabled={isSubmitting}
        />

        <TextField
          form={form}
          name="email"
          label={m['auth.signup.email_label']()}
          type="email"
          dataTestId="signup-email-input"
          disabled={isSubmitting}
        />

        <form.Field name="password">
          {(field) => (
            <div className="space-y-2">
              <Label htmlFor="signup-password">{m['auth.signup.password_label']()}</Label>
              <Input
                id="signup-password"
                type="password"
                value={field.state.value ?? ''}
                onChange={(e) => field.handleChange(e.target.value)}
                disabled={isSubmitting}
                data-testid="signup-password-input"
              />
              {(field.state.meta.isTouched || form.state.isSubmitted) &&
                field.state.meta.errors.length > 0 && (
                  <p className="text-sm text-destructive">
                    {getFieldErrorMessage(field.state.meta)}
                  </p>
                )}
              <PasswordStrengthMeter
                password={field.state.value ?? ''}
                config={DEFAULT_PASSWORD_CONFIG}
              />
            </div>
          )}
        </form.Field>

        {!loadingTurnstile && turnstileStatus?.enabled && (
          <form.Field name="turnstileToken">
            {(field) => (
              <div className="space-y-2">
                <Label>{m['auth.signup.security_verification']()}</Label>
                <TurnstileWidget
                  siteKey={turnstileStatus.siteKey || ''}
                  onTokenChange={(token) => field.handleChange(token || '')}
                  onError={(error) => {
                    console.error('Turnstile error:', error)
                  }}
                />
                {(field.state.meta.isTouched || form.state.isSubmitted) &&
                  field.state.meta.errors.length > 0 && (
                    <p className="text-sm text-destructive">
                      {getFieldErrorMessage(field.state.meta)}
                    </p>
                  )}
              </div>
            )}
          </form.Field>
        )}

        <Button
          type="submit"
          data-testid="signup-submit-button"
          disabled={isSubmitting}
          className="h-11 w-full md:h-10"
        >
          {isSubmitting ? m['auth.signup.submitting']() : m['auth.signup.submit']()}
        </Button>
      </form>
    </AppForm>
  )
}
