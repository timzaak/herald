import { useMemo } from 'react'
import { useStore } from '@tanstack/react-form'
import { useQuery, useMutation } from '@tanstack/react-query'
import { toast } from 'sonner'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import { signup, sendEmailCode } from '@/lib/api-generated'
import type { SignupRequest } from '@/lib/api-generated'
import { completeSignup } from '@/lib/auth-utils'
import { ADMIN_REALM_ID, ADMIN_WEB_CONSOLE_CLIENT_ID } from '@/lib/constants/auth-constants'
import { DEFAULT_PASSWORD_CONFIG } from '@/lib/password-strength'
import { useFormMutation } from '@/hooks/use-form-mutation'
import { useCountdown } from '@/hooks/use-countdown'
import { PasswordStrengthMeter } from './password-strength-meter'
import { TurnstileWidget } from './turnstile-widget'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { getFieldErrorMessage } from '@/lib/form-utils'
import { getErrorMessage } from '@/lib/error-utils'
import { TextField } from '@/components/shared/form-fields/text-field'
import {
  queryKeys,
  turnstileStatusQueryOptions,
  signupStatusQueryOptions,
} from '@/data/query-options'
import { m } from '@/paraglide/messages'
import {
  createSignupSchema,
  signupEmailSchema,
  type SignupFormValues,
} from '@/lib/schemas/realm-signup'

// Self-service signup is an admin-realm-only public entry (DEC-001). All API
// calls are fixed to the admin realm regardless of the URL the page was opened
// under, and the Turnstile probe targets the admin-web-console Client App
// (DEC-008) — not the user-account-center default of `turnstileStatusQueryOptions`.

// Resend cooldown for the signup email verification code. The backend's own
// limiter is 1 request / 120s per IP and per mailbox; the button's countdown is
// the gentler client-side guard (60s), matching the email-otp login form.
const EMAIL_CODE_COUNTDOWN_SECONDS = 60

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

  // Whether the signup flow must collect a mailbox verification code (admin
  // realm registration config; fail-open `false` when no mail channel).
  const { data: signupStatus } = useQuery(signupStatusQueryOptions(ADMIN_REALM_ID))
  const emailVerificationRequired = signupStatus?.emailVerificationRequired === true

  const { isSubmitting, mutate } = useFormMutation({
    mutationFn: async (data: SignupFormValues) => {
      const apiData: SignupRequest = {
        realmName: data.realmName,
        realmSlug: data.realmSlug || null,
        email: data.email,
        password: data.password,
        turnstileToken: data.turnstileToken || null,
        emailVerificationCode: data.emailVerificationCode || null,
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

  // The flag is stable once the status query resolves; rebuild the schema only
  // when it flips, so re-renders (typing, the per-second countdown tick) don't
  // hand the form a new validator identity.
  const signupSchema = useMemo(
    () => createSignupSchema(emailVerificationRequired),
    [emailVerificationRequired]
  )

  const form = useAppForm({
    schema: signupSchema,
    defaultValues: {
      realmName: '',
      realmSlug: '',
      email: '',
      password: '',
      turnstileToken: undefined,
      emailVerificationCode: '',
    },
    onSubmit: async ({ value }) => {
      // `useFormMutation` surfaces errors via its `onError` (toast). React
      // Query's `mutateAsync` also re-throws after `onError`; catch it here so
      // the form's onSubmit promise doesn't reject unhandled, while still
      // keeping the user-visible error display from `onError`.
      await mutate(value).catch(() => {})
    },
  })

  // Send-code countdown (seconds remaining), `null` while none is active.
  // Declared before the mutation hook below closes over `startCountdown`
  // (project `react-hooks/immutability` rule).
  const { countdown, startCountdown } = useCountdown()

  const sendCodeMutation = useMutation({
    mutationFn: async (email: string) => {
      const { data, error } = await sendEmailCode({
        path: { realmId: ADMIN_REALM_ID },
        body: { email },
        throwOnError: false,
      })
      if (error) {
        throw error
      }
      return data
    },
    onSuccess: () => {
      toast.success(m['auth.signup.email_code_sent']())
      startCountdown(EMAIL_CODE_COUNTDOWN_SECONDS)
    },
    onError: (error) => {
      // 429 rate-limit and every other failure share the unified error toast.
      toast.error(getErrorMessage(error))
    },
  })

  // The send button targets the mailbox the user typed — gate it on the same
  // basic email validation the schema applies to the field.
  const email = useStore(form.store, (state) => state.values.email)
  const emailValid = signupEmailSchema.safeParse(email).success
  const sendCodeDisabled =
    !emailValid || sendCodeMutation.isPending || countdown !== null || isSubmitting

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

        {emailVerificationRequired && (
          <form.Field name="emailVerificationCode">
            {(field) => (
              <div className="space-y-2">
                <Label htmlFor="signup-email-code">{m['auth.signup.email_code_label']()}</Label>
                <div className="flex gap-2">
                  <Input
                    id="signup-email-code"
                    type="text"
                    inputMode="numeric"
                    autoComplete="one-time-code"
                    maxLength={6}
                    placeholder={m['auth.signup.email_code_placeholder']()}
                    value={field.state.value ?? ''}
                    onChange={(e) => field.handleChange(e.target.value)}
                    disabled={isSubmitting}
                    data-testid="signup-email-code-input"
                    className="flex-1"
                  />
                  <Button
                    type="button"
                    variant="outline"
                    disabled={sendCodeDisabled}
                    onClick={() => sendCodeMutation.mutate(email)}
                    data-testid="signup-send-email-code-button"
                  >
                    {sendCodeMutation.isPending
                      ? m['auth.signup.email_code_sending']()
                      : countdown !== null
                        ? m['auth.signup.email_code_resend_in']({ countdown })
                        : m['auth.signup.email_code_send']()}
                  </Button>
                </div>
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
          className="h-11 w-full"
        >
          {isSubmitting ? m['auth.signup.submitting']() : m['auth.signup.submit']()}
        </Button>
      </form>
    </AppForm>
  )
}
