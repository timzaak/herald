/**
 * Email-OTP login form.
 *
 * State machine:
 *   email → (consent gate for auto-register) → Turnstile (per Client App) →
 *   send → 6-digit code input with resend countdown → verify → Bearer session
 *   handoff (the Herald SDK applies the token set; the route is notified via
 *   `onSuccess()`).
 *
 * Boundary (FE-D01): the component does NOT call `completeLoginAfterEmailOtp`
 * and does NOT navigate — it notifies the route via `onSuccess()`, mirroring
 * `PasskeyLoginForm` → `handlePasskeySuccess`. The route owns token-family
 * binding + navigation.
 *
 * Error matrix:
 *   - 409 `consent_required` (auto-register, missing agreements) → consent
 *     gate (agreement list + "agree and continue"), then re-send with
 *     agreements built via `toAuthConsentAgreements`.
 *   - 409 `email_not_registered` (auto-register off) → localized guidance +
 *     explicit-register link.
 *   - 401 wrong code (verify, retry-able) → error region, code input stays.
 *   - 401 expired/exhausted/disabled (verify) → error region, must resend.
 *   - 429 rate-limited → "try again later".
 *   - 400 realm disabled / bad request → error region.
 */

import { useEffect, useRef, useState } from 'react'
import { useForm } from '@tanstack/react-form'
import { z } from 'zod'
import OTPInput from 'react-otp-input'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { TurnstileWidget } from '@/components/auth/turnstile-widget'
import { AgreementLinks } from '@/components/legal/AgreementLinks'
import { Link } from '@tanstack/react-router'
import { m } from '@/paraglide/messages'
import { getFieldErrorMessage } from '@/lib/form-utils'
import { getErrorMessage, resolveApiError } from '@/lib/error-utils'
import { toAuthConsentAgreements } from '@/data/query-options'
import {
  useEmailOtpSendMutation,
  useEmailOtpVerifyMutation,
  type EmailOtpSendConflict,
} from '@/components/auth/email-otp-mutations'
import type { LegalAgreementSummary } from '@/lib/api-generated'
import { formatDate } from '@/lib/date-utils'

const emailSchema = z.object({
  email: z.string().min(1, 'Email is required').email('Invalid email'),
})

const CODE_LENGTH = 6

/** Turnstile status shape (from `TurnstileStatusResponse`). */
interface TurnstileStatus {
  enabled: boolean
  siteKey?: string | null
}

export interface EmailOtpLoginFormProps {
  realmId: string
  /** Client App id resolved by the route (`search.clientId || DEFAULT_CLIENT_ID`). */
  clientId: string
  /**
   * Client App-level Turnstile status for the resolved `clientId`. The route
   * already queries `turnstileStatusQueryOptions(realmId, clientId)` for the
   * password form; passing it down avoids a duplicate query. When omitted the
   * form assumes Turnstile is not configured (skipped).
   */
  turnstileStatus?: TurnstileStatus | null
  /**
   * Called after a successful verify. The Herald SDK applied the issued
   * token set itself (`loginWithEmailOtp.verify`); the route owns
   * `completeLoginAfterEmailOtp` (clientId rebind + hydration) + navigation.
   * OTP verify has no `redirectTo`/PKCE branch, so no external-redirect
   * callback is needed.
   */
  onSuccess: () => void
  /**
   * Called when verify answers `requires-second-factor` (user has an enabled
   * TOTP/passkey — the OTP alone must not complete the login). The route
   * swaps in the shared second-factor step keyed by the returned temp token.
   */
  onSecondFactorRequired?: (tempToken: string, secondFactors: string[]) => void
  /** Return to the password form (back button). */
  onBack: () => void
  /** Realm-context-aware register path, rendered when `email_not_registered`. */
  registerPath: string
}

export function EmailOtpLoginForm({
  realmId,
  clientId,
  turnstileStatus,
  onSuccess,
  onSecondFactorRequired,
  onBack,
  registerPath,
}: EmailOtpLoginFormProps) {
  const [code, setCode] = useState('')
  const [codeSent, setCodeSent] = useState(false)
  const [error, setError] = useState<string | null>(null)
  // Conflict from a send 409. Drives the consent gate / not-registered view.
  const [conflict, setConflict] = useState<EmailOtpSendConflict | null>(null)
  // Tracks the last accepted-agreements snapshot so a resend re-uses them
  // without forcing the user to re-tick after consent was expressed.
  const [acceptedAgreements, setAcceptedAgreements] = useState<
    Array<{ agreementType: string; versionId: string }>
  >([])
  const [turnstileToken, setTurnstileToken] = useState<string>('')

  // Resend countdown (seconds remaining), seeded from `expiresInSeconds` of the
  // last successful send. `null` while no countdown is active.
  const [countdown, setCountdown] = useState<number | null>(null)
  const countdownRef = useRef<ReturnType<typeof setInterval> | null>(null)

  const turnstileEnabled = !!turnstileStatus?.enabled
  const turnstileSiteKey = turnstileStatus?.siteKey ?? ''

  const form = useForm({
    defaultValues: { email: '' },
    onSubmit: async ({ value }) => {
      void handleSend(value.email)
    },
  })

  // Countdown helpers declared before the mutation hooks that reference them
  // (project `react-hooks/immutability` rule: functions must be declared before
  // the hook callbacks that close over them).
  function clearCountdown() {
    if (countdownRef.current) {
      clearInterval(countdownRef.current)
      countdownRef.current = null
    }
    setCountdown(null)
  }

  function startCountdown(seconds: number) {
    clearCountdown()
    if (!seconds || seconds <= 0) return
    setCountdown(seconds)
    countdownRef.current = setInterval(() => {
      setCountdown((prev) => {
        if (prev === null) return null
        if (prev <= 1) {
          if (countdownRef.current) {
            clearInterval(countdownRef.current)
            countdownRef.current = null
          }
          return null
        }
        return prev - 1
      })
    }, 1000)
  }

  useEffect(() => {
    return () => clearCountdown()
  }, [])

  const sendMutation = useEmailOtpSendMutation({
    realmId,
    onSuccess: (result) => {
      if (result.conflict) {
        setConflict(result.conflict)
        // Drop any stale countdown when branching into the conflict UI.
        clearCountdown()
        return
      }
      // Real send succeeded: advance to the code step + seed the countdown.
      setConflict(null)
      setCodeSent(true)
      setCode('')
      setError(null)
      startCountdown(result.data?.expiresInSeconds ?? 0)
    },
    onError: (err) => {
      setError(getErrorMessage(err))
    },
  })

  const verifyMutation = useEmailOtpVerifyMutation({
    realmId,
    onSuccess: () => {
      clearCountdown()
      onSuccess()
    },
    onSecondFactorRequired: (tempToken, secondFactors) => {
      clearCountdown()
      onSecondFactorRequired?.(tempToken, secondFactors)
    },
    onError: (err) => {
      const status = resolveApiError(err).status
      // 429 is the only status with a bespoke message; every other failure
      // (401 wrong-code/expired/exhausted/disabled, 400, etc.) surfaces the
      // authoritative backend message verbatim so the user can tell retry-able
      // wrong-code from "must resend" exhaustion.
      setError(status === 429 ? m['auth.email_otp.rate_limited']() : getErrorMessage(err))
      // Clear the code so the user re-enters after an error (one-time-consumed
      // semantics on the backend mean the previous code is dead regardless).
      setCode('')
    },
  })

  function handleSend(email: string, agreements?: typeof acceptedAgreements) {
    setError(null)
    setConflict(null)
    const payload = {
      email,
      clientId,
      turnstileToken: turnstileToken || undefined,
      agreements,
    }
    sendMutation.mutate(payload)
  }

  function handleVerify() {
    if (code.length !== CODE_LENGTH) return
    setError(null)
    verifyMutation.mutate({
      email: form.getFieldValue('email'),
      code,
      clientId,
      // Re-send the accepted agreements on verify (idempotent register-consent
      // on the backend; required for the auto-register path, harmless for
      // existing users).
      agreements: acceptedAgreements.length > 0 ? acceptedAgreements : undefined,
    })
  }

  /** Consent gate: build agreements from the conflict list and re-send. */
  function handleConsentAgree() {
    if (!conflict || !conflict.agreements || conflict.agreements.length === 0) return
    const agreements = toAuthConsentAgreements(conflict.agreements)
    setAcceptedAgreements(agreements)
    handleSend(form.getFieldValue('email'), agreements)
  }

  function handleConsentDecline() {
    setConflict(null)
    setError(null)
  }

  function handleResend() {
    if (countdown !== null) return
    handleSend(
      form.getFieldValue('email'),
      acceptedAgreements.length > 0 ? acceptedAgreements : undefined
    )
  }

  const isConsentRequiredConflict =
    conflict?.code === 'consent_required' && conflict.consentRequired === true
  const isNotRegisteredConflict = conflict?.code === 'email_not_registered'

  // --- Render branches -----------------------------------------------------

  // 409 consent_required: agreement gate.
  if (isConsentRequiredConflict && conflict?.agreements) {
    return (
      <div className="space-y-4" data-testid="email-otp-login-form">
        <h3 className="font-semibold">{m['auth.email_otp.consent_title']()}</h3>
        <p className="text-sm text-muted-foreground">{m['auth.email_otp.consent_description']()}</p>
        {conflict.agreements.map((agreement: LegalAgreementSummary) => (
          <div
            key={agreement.version_id}
            className="rounded border p-3"
            data-testid={`email-otp-agreement-${agreement.agreement_type}`}
          >
            <div className="font-medium">
              <AgreementLinks
                realmId={realmId}
                agreements={[agreement]}
                agreementType={agreement.agreement_type as 'terms_of_service' | 'privacy_policy'}
              />
            </div>
            <div
              className="text-sm text-muted-foreground"
              data-testid={`email-otp-agreement-${agreement.agreement_type}-version`}
            >
              {m['legal.version_label']()}: {agreement.version_no} •{' '}
              {m['legal.effective_date_label']()}: {formatDate(agreement.effective_at)}
            </div>
          </div>
        ))}
        <Button
          type="button"
          className="w-full"
          disabled={sendMutation.isPending}
          data-testid="email-otp-agree-and-continue-button"
          onClick={handleConsentAgree}
        >
          {sendMutation.isPending
            ? m['common.loading']()
            : m['auth.email_otp.agree_and_continue']()}
        </Button>
        <Button
          type="button"
          variant="outline"
          className="w-full"
          data-testid="email-otp-agreement-back-button"
          onClick={handleConsentDecline}
        >
          {m['auth.email_otp.decline_back']()}
        </Button>
      </div>
    )
  }

  // 409 email_not_registered: guidance + explicit-register link.
  if (isNotRegisteredConflict) {
    return (
      <div className="space-y-4" data-testid="email-otp-login-form">
        <div
          className="p-3 bg-warning/10 border border-warning/20 rounded text-warning text-sm"
          data-testid="email-otp-not-registered-message"
        >
          {conflict?.message ?? m['auth.email_otp.not_registered_guidance']()}
        </div>
        <Button
          type="button"
          variant="outline"
          className="w-full"
          data-testid="email-otp-back-after-conflict-button"
          onClick={() => {
            setConflict(null)
            setError(null)
          }}
        >
          {m['auth.email_otp.try_different_email']()}
        </Button>
        <div className="text-sm">
          <Link
            to={registerPath}
            className="font-medium text-primary hover:text-primary/80"
            data-testid="email-otp-register-link"
          >
            {m['auth.email_otp.register_link']()}
          </Link>
        </div>
      </div>
    )
  }

  // Code step (after a successful send).
  if (codeSent) {
    const resendDisabled = countdown !== null || sendMutation.isPending || verifyMutation.isPending
    return (
      <div className="space-y-4" data-testid="email-otp-login-form">
        <div>
          <h3 className="font-semibold">{m['auth.email_otp.code_title']()}</h3>
          <p className="text-sm text-muted-foreground">
            {m['auth.email_otp.code_description']({
              email: form.getFieldValue('email'),
            })}
          </p>
        </div>

        {error && (
          <div
            className="p-3 bg-destructive/10 border border-destructive/20 rounded text-destructive text-sm"
            data-testid="email-otp-error-message"
          >
            {error}
          </div>
        )}

        <div>
          <Label htmlFor="email-otp-code-input">{m['auth.email_otp.code_label']()}</Label>
          <div className="mt-2 flex" data-testid="email-otp-code-input">
            <OTPInput
              value={code}
              onChange={setCode}
              numInputs={CODE_LENGTH}
              inputType="number"
              shouldAutoFocus
              renderInput={(props, index) => (
                <input
                  {...props}
                  key={index}
                  placeholder="•"
                  className="otp-digit-input mx-1 h-12 w-10 rounded-md border border-input bg-background text-center text-lg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  data-testid={`email-otp-code-digit-${index}`}
                  aria-label={`${m['auth.email_otp.code_label']()} ${index + 1}`}
                />
              )}
            />
          </div>
        </div>

        <Button
          type="button"
          className="w-full"
          disabled={code.length !== CODE_LENGTH || verifyMutation.isPending}
          data-testid="email-otp-verify-btn"
          onClick={handleVerify}
        >
          {verifyMutation.isPending
            ? m['auth.email_otp.verifying']()
            : m['auth.email_otp.verify']()}
        </Button>

        <div className="flex items-center gap-2 text-sm">
          {countdown !== null ? (
            <span className="text-muted-foreground" data-testid="email-otp-resend-countdown">
              {m['auth.email_otp.resend_in']({ countdown })}
            </span>
          ) : (
            <Button
              type="button"
              variant="link"
              className="h-auto p-0"
              disabled={resendDisabled}
              data-testid="email-otp-resend-btn"
              onClick={handleResend}
            >
              {m['auth.email_otp.resend']()}
            </Button>
          )}
        </div>

        <Button
          type="button"
          variant="ghost"
          className="w-full"
          data-testid="email-otp-back-to-email-button"
          onClick={() => {
            setCodeSent(false)
            setCode('')
            setError(null)
            clearCountdown()
          }}
        >
          {m['auth.email_otp.back_to_email']()}
        </Button>
      </div>
    )
  }

  // Email step (initial).
  return (
    <div className="space-y-4" data-testid="email-otp-login-form">
      <div>
        <h3 className="font-semibold">{m['auth.email_otp.title']()}</h3>
        <p className="text-sm text-muted-foreground">{m['auth.email_otp.description']()}</p>
      </div>

      {error && (
        <div
          className="p-3 bg-destructive/10 border border-destructive/20 rounded text-destructive text-sm"
          data-testid="email-otp-error-message"
        >
          {error}
        </div>
      )}

      <form
        onSubmit={(e) => {
          e.preventDefault()
          form.handleSubmit()
        }}
        className="space-y-4"
      >
        <form.Field name="email" validators={{ onChange: emailSchema.shape.email }}>
          {(field) => (
            <div>
              <Label htmlFor="email-otp-email">{m['auth.email_otp.email_label']()}</Label>
              <Input
                id="email-otp-email"
                type="email"
                autoComplete="email"
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(e) => field.handleChange(e.target.value)}
                disabled={sendMutation.isPending}
                data-testid="email-otp-email-input"
                placeholder="you@example.com"
              />
              {field.state.meta.errors.length > 0 && (
                <p className="text-sm text-destructive mt-1">
                  {getFieldErrorMessage(field.state.meta)}
                </p>
              )}
            </div>
          )}
        </form.Field>

        {turnstileEnabled && turnstileSiteKey && (
          <TurnstileWidget
            siteKey={turnstileSiteKey}
            onTokenChange={(token) => setTurnstileToken(token || '')}
            onError={(err) => console.error('Turnstile error:', err)}
          />
        )}

        <Button
          type="submit"
          className="w-full"
          disabled={sendMutation.isPending}
          data-testid="email-otp-send-btn"
        >
          {sendMutation.isPending ? m['auth.email_otp.sending']() : m['auth.email_otp.send']()}
        </Button>
      </form>

      <Button
        type="button"
        variant="ghost"
        className="w-full"
        data-testid="email-otp-back-button"
        onClick={onBack}
      >
        {m['auth.email_otp.use_password_instead']()}
      </Button>
    </div>
  )
}
