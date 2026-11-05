/**
 * SMS-OTP login form.
 *
 * State machine:
 *   phone → Turnstile (per Client App) → send → 6-digit code input with resend
 *   countdown → verify → Bearer session handoff (the Herald SDK applies the
 *   token set; the route is notified via `onSuccess()`).
 *
 * Boundary: mirrors `EmailOtpLoginForm` — the component does NOT call
 * `completeLoginAfterEmailOtp` and does NOT navigate; it notifies the route
 * via `onSuccess()` / `onSecondFactorRequired()`. The route owns token-family
 * binding + navigation.
 *
 * Error matrix:
 *   - 409 `phone_not_registered` (no auto-register, DEC-phone-login-001) →
 *     localized guidance + explicit-register link. No consent branch exists.
 *   - 401 wrong code (verify, retry-able) → error region, code input cleared.
 *   - 429 rate-limited → "try again later" (localized).
 *   - 400 realm disabled / invalid phone / 502 channel failure → error region.
 *
 * Phone input is deliberately loose (DEC-phone-login-004): non-empty only, any
 * writing form is submitted as-is; the server is the format authority and its
 * single uniform `invalid_phone` error surfaces verbatim (anti-probing parity).
 */

import { useState } from 'react'
import { useForm } from '@tanstack/react-form'
import { z } from 'zod'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { TurnstileWidget } from '@/components/auth/turnstile-widget'
import { OtpCodeInput } from '@/components/shared'
import { useCountdown } from '@/hooks/use-countdown'
import { Link } from '@tanstack/react-router'
import { m } from '@/paraglide/messages'
import { getFieldErrorMessage } from '@/lib/form-utils'
import { getErrorMessage, resolveApiError } from '@/lib/error-utils'
import {
  useSmsOtpSendMutation,
  useSmsOtpVerifyMutation,
  type SmsOtpSendConflict,
} from '@/components/auth/sms-otp-mutations'

// Non-empty only: the server is the phone format authority (DEC-phone-login-004).
const phoneSchema = z.string().min(1, 'Phone number is required')

const CODE_LENGTH = 6

/** Turnstile status shape (from `TurnstileStatusResponse`). */
interface TurnstileStatus {
  enabled: boolean
  siteKey?: string | null
}

export interface SmsOtpLoginFormProps {
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
   * token set itself (`loginWithSmsOtp.verify`); the route owns
   * `completeLoginAfterEmailOtp` (clientId rebind + hydration) + navigation.
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
  /** Realm-context-aware register path, rendered when `phone_not_registered`. */
  registerPath: string
}

export function SmsOtpLoginForm({
  realmId,
  clientId,
  turnstileStatus,
  onSuccess,
  onSecondFactorRequired,
  onBack,
  registerPath,
}: SmsOtpLoginFormProps) {
  const [code, setCode] = useState('')
  const [codeSent, setCodeSent] = useState(false)
  // Backend-derived masked number shown on the code step (`138****8000`); the
  // frontend never computes or stores the raw number.
  const [maskedPhone, setMaskedPhone] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  // Conflict from a send 409. Drives the not-registered view.
  const [conflict, setConflict] = useState<SmsOtpSendConflict | null>(null)
  const [turnstileToken, setTurnstileToken] = useState<string>('')
  // Send attempt counter. Turnstile tokens are single-use and the widget has
  // no reset channel, so the widgets key on this counter: every attempt that
  // consumes a token — successful send, failed send, failed verify (the
  // backend siteverifies before any business check) — remounts a fresh widget
  // that can issue a new token for the next request.
  const [sendAttempt, setSendAttempt] = useState(0)

  // Resend countdown (seconds remaining), seeded from `expiresInSeconds` of the
  // last successful send. `null` while no countdown is active.
  const { countdown, startCountdown, clearCountdown } = useCountdown()

  const turnstileEnabled = !!turnstileStatus?.enabled
  const turnstileSiteKey = turnstileStatus?.siteKey ?? ''

  const form = useForm({
    defaultValues: { phone: '' },
    onSubmit: async ({ value }) => {
      void handleSend(value.phone)
    },
  })

  const sendMutation = useSmsOtpSendMutation({
    realmId,
    onSuccess: (result) => {
      if (result.conflict) {
        setConflict(result.conflict)
        // Drop any stale countdown when branching into the conflict UI, and
        // the consumed token with it (the conflict view unmounts the widget;
        // returning to the phone step remounts a fresh one).
        clearCountdown()
        setTurnstileToken('')
        return
      }
      // Real send succeeded: advance to the code step + seed the countdown.
      setConflict(null)
      setCodeSent(true)
      setCode('')
      setError(null)
      setMaskedPhone(result.data?.maskedPhone ?? null)
      // The send consumed this step's Turnstile token (single-use); the code
      // step renders its own widget (keyed by the new attempt) for a fresh
      // verify token.
      setTurnstileToken('')
      setSendAttempt((n) => n + 1)
      startCountdown(result.data?.expiresInSeconds ?? 0)
    },
    onError: (err) => {
      // 502 is a channel/upstream failure with a bespoke localized fallback
      // (explicit failure, never a fake success); every other
      // error surfaces the authoritative backend message verbatim. The failed
      // request still consumed this step's Turnstile token (siteverify runs
      // before any business check): drop it and remount a fresh widget.
      const status = resolveApiError(err).status
      setError(status === 502 ? m['auth.phone_otp.send_failed']() : getErrorMessage(err))
      setTurnstileToken('')
      setSendAttempt((n) => n + 1)
    },
  })

  const verifyMutation = useSmsOtpVerifyMutation({
    realmId,
    onSuccess: () => {
      clearCountdown()
      onSuccess()
    },
    onSecondFactorRequired: (tempToken, secondFactors) => {
      clearCountdown()
      onSecondFactorRequired?.(tempToken, secondFactors)
    },
    onConflict: (result) => {
      // Same 409 control-flow outcome as send (account anonymized between
      // send and verify): drop the countdown and consumed token, branch into
      // the not-registered guidance view.
      clearCountdown()
      setTurnstileToken('')
      setConflict(result)
    },
    onError: (err) => {
      const status = resolveApiError(err).status
      // 429 is the only status with a bespoke message; every other failure
      // (401 wrong-code/expired/exhausted/disabled, 400, etc.) surfaces the
      // authoritative backend message verbatim so the user can tell retry-able
      // wrong-code from "must resend" exhaustion.
      setError(status === 429 ? m['auth.phone_otp.rate_limited']() : getErrorMessage(err))
      // Clear the code so the user re-enters after an error (one-time-consumed
      // semantics on the backend mean the previous code is dead regardless),
      // and remount a fresh Turnstile widget: verify consumed this attempt's
      // token (siteverify runs before the code check), so a retry with the
      // old token would be rejected before the code is even read.
      setCode('')
      setTurnstileToken('')
      setSendAttempt((n) => n + 1)
    },
  })

  function handleSend(phone: string) {
    setError(null)
    setConflict(null)
    sendMutation.mutate({
      phone: phone.trim(),
      clientId,
      turnstileToken: turnstileToken || undefined,
    })
  }

  function handleVerify() {
    if (code.length !== CODE_LENGTH) return
    setError(null)
    verifyMutation.mutate({
      phone: form.getFieldValue('phone').trim(),
      code,
      clientId,
      turnstileToken: turnstileToken || undefined,
    })
  }

  function handleResend() {
    if (countdown !== null) return
    handleSend(form.getFieldValue('phone'))
  }

  const isNotRegisteredConflict = conflict?.code === 'phone_not_registered'

  if (isNotRegisteredConflict) {
    return (
      <div className="space-y-4" data-testid="phone-otp-login-form">
        <div
          className="p-3 bg-warning/10 border border-warning/20 rounded text-warning text-sm"
          data-testid="phone-otp-not-registered-message"
        >
          {conflict?.message ?? m['auth.phone_otp.not_registered_guidance']()}
        </div>
        <Button
          type="button"
          variant="outline"
          className="w-full"
          data-testid="phone-otp-back-after-conflict-button"
          onClick={() => {
            // Full reset to the phone step: the button promises "different
            // phone", so landing back on the old number's code step (codeSent
            // still true) would loop the user into failing verifies.
            setConflict(null)
            setError(null)
            setCodeSent(false)
            setCode('')
            setMaskedPhone(null)
            setTurnstileToken('')
            clearCountdown()
          }}
        >
          {m['auth.phone_otp.try_different_phone']()}
        </Button>
        <div className="text-sm">
          <Link
            to={registerPath}
            className="font-medium text-primary hover:text-primary/80"
            data-testid="phone-otp-register-link"
          >
            {m['auth.phone_otp.register_link']()}
          </Link>
        </div>
      </div>
    )
  }

  if (codeSent) {
    const resendDisabled = countdown !== null || sendMutation.isPending || verifyMutation.isPending
    return (
      <div className="space-y-4" data-testid="phone-otp-login-form">
        <div>
          <h3 className="font-semibold">{m['auth.phone_otp.code_title']()}</h3>
          <p className="text-sm text-muted-foreground">
            {m['auth.phone_otp.code_description']({
              maskedPhone: maskedPhone ?? '',
            })}
          </p>
        </div>

        {error && (
          <div
            className="p-3 bg-destructive/10 border border-destructive/20 rounded text-destructive text-sm"
            data-testid="phone-otp-error-message"
          >
            {error}
          </div>
        )}

        {/* Turnstile runs on verify too: a fresh widget here issues the second
        single-use token, mirroring the phone step. Keyed by the send attempt
        so a resend remounts it — a solved widget never issues another token. */}
        {turnstileEnabled && turnstileSiteKey && (
          <TurnstileWidget
            key={sendAttempt}
            siteKey={turnstileSiteKey}
            onTokenChange={(token) => setTurnstileToken(token || '')}
            onError={(err) => console.error('Turnstile error:', err)}
          />
        )}

        <div>
          <Label htmlFor="phone-otp-code-input">{m['auth.phone_otp.code_label']()}</Label>
          <OtpCodeInput
            value={code}
            onChange={setCode}
            numInputs={CODE_LENGTH}
            testIdBase="phone-otp-code"
            ariaLabel={m['auth.phone_otp.code_label']()}
          />
        </div>

        <Button
          type="button"
          className="w-full"
          disabled={code.length !== CODE_LENGTH || verifyMutation.isPending}
          data-testid="phone-otp-verify-btn"
          onClick={handleVerify}
        >
          {verifyMutation.isPending
            ? m['auth.phone_otp.verifying']()
            : m['auth.phone_otp.verify']()}
        </Button>

        <div className="flex items-center gap-2 text-sm">
          {countdown !== null ? (
            <span className="text-muted-foreground" data-testid="phone-otp-resend-countdown">
              {m['auth.phone_otp.resend_in']({ countdown })}
            </span>
          ) : (
            <Button
              type="button"
              variant="link"
              className="h-auto p-0"
              disabled={resendDisabled}
              data-testid="phone-otp-resend-btn"
              onClick={handleResend}
            >
              {m['auth.phone_otp.resend']()}
            </Button>
          )}
        </div>

        <Button
          type="button"
          variant="ghost"
          className="w-full"
          data-testid="phone-otp-back-to-phone-button"
          onClick={() => {
            setCodeSent(false)
            setCode('')
            setError(null)
            setMaskedPhone(null)
            // Drop the code-step token: the phone step renders its own widget
            // and a stale token must never ride the next send.
            setTurnstileToken('')
            clearCountdown()
          }}
        >
          {m['auth.phone_otp.back_to_phone']()}
        </Button>
      </div>
    )
  }

  return (
    <div className="space-y-4" data-testid="phone-otp-login-form">
      <div>
        <h3 className="font-semibold">{m['auth.phone_otp.title']()}</h3>
        <p className="text-sm text-muted-foreground">{m['auth.phone_otp.description']()}</p>
      </div>

      {error && (
        <div
          className="p-3 bg-destructive/10 border border-destructive/20 rounded text-destructive text-sm"
          data-testid="phone-otp-error-message"
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
        <form.Field name="phone" validators={{ onChange: phoneSchema }}>
          {(field) => (
            <div>
              <Label htmlFor="phone-otp-phone">{m['auth.phone_otp.phone_label']()}</Label>
              <Input
                id="phone-otp-phone"
                type="tel"
                autoComplete="tel"
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(e) => field.handleChange(e.target.value)}
                disabled={sendMutation.isPending}
                data-testid="phone-otp-phone-input"
                placeholder="+86 138 0013 8000"
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
            key={sendAttempt}
            siteKey={turnstileSiteKey}
            onTokenChange={(token) => setTurnstileToken(token || '')}
            onError={(err) => console.error('Turnstile error:', err)}
          />
        )}

        <Button
          type="submit"
          className="w-full"
          disabled={sendMutation.isPending}
          data-testid="phone-otp-send-btn"
        >
          {sendMutation.isPending ? m['auth.phone_otp.sending']() : m['auth.phone_otp.send']()}
        </Button>
      </form>

      <Button
        type="button"
        variant="ghost"
        className="w-full"
        data-testid="phone-otp-back-button"
        onClick={onBack}
      >
        {m['auth.phone_otp.use_password_instead']()}
      </Button>
    </div>
  )
}
