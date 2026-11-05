/**
 * SMS-OTP login send/verify mutations.
 *
 * TanStack `useMutation` over the Herald SDK client
 * (`client.loginWithSmsOtp.send/verify`), with errors mapped through
 * `getErrorMessage` (`src/lib/error-utils`) and surfaced via `sonner` toasts +
 * `@/paraglide/messages` (`m`).
 *
 * Send's single 409 control-flow outcome arrives as the SDK's `conflict`
 * branch (NOT an error):
 *   - `phone_not_registered` (no account matches the number; no code is sent
 *     and none is created, DEC-phone-login-001) → the form shows the localized
 *     guidance message and the explicit-register link.
 * SMS OTP has no auto-register consent branch (DEC-phone-login-001), unlike
 * `email-otp-mutations` — there are no agreements to carry.
 *
 * Verify runs through the SDK as well: on success the SDK applies the issued
 * token set itself, so `onSuccess` carries no token payload — the route's
 * `completeLoginAfterEmailOtp` only rebinds the routing clientId and hydrates.
 */

import { useMutation } from '@tanstack/react-query'
import { toast } from 'sonner'
import type { SmsOtpSendResponse } from '@/lib/api-generated'
import { ensureHeraldClient } from '@/lib/herald-client'
import { m } from '@/paraglide/messages'

/**
 * Conflict outcome surfaced to the form when send returns 409. Only
 * `phone_not_registered` exists (no consent gate for SMS OTP).
 */
export interface SmsOtpSendConflict {
  code: string
  message: string
}

/**
 * Successful send result. On conflict, `conflict` is set and `data` is null
 * (no code was sent). On other errors the mutation throws and `onError` runs.
 */
export interface SmsOtpSendResult {
  data: SmsOtpSendResponse | null
  conflict: SmsOtpSendConflict | null
}

export interface UseSmsOtpSendMutationOptions {
  realmId: string
  onSuccess?: (result: SmsOtpSendResult) => void
  onError?: (error: unknown) => void
}

/**
 * Send mutation. Surfaces the 409 conflict branch via the returned
 * `SmsOtpSendResult` (data=null, conflict set) — it does NOT throw on 409,
 * so the form can branch on `code`/`message` without try/catch. All other
 * errors throw normally and flow to `onError`.
 */
export function useSmsOtpSendMutation({
  realmId,
  onSuccess,
  onError,
}: UseSmsOtpSendMutationOptions) {
  return useMutation({
    mutationFn: async (payload: {
      phone: string
      clientId: string
      turnstileToken?: string | null
    }): Promise<SmsOtpSendResult> => {
      const herald = ensureHeraldClient(realmId)
      herald.tokens.bindClientId(payload.clientId)
      const result = await herald.loginWithSmsOtp.send({
        phone: payload.phone,
        ...(payload.turnstileToken ? { turnstileToken: payload.turnstileToken } : {}),
      })
      if (result.kind === 'conflict') {
        return {
          data: null,
          conflict: {
            code: result.code,
            message: result.message,
          },
        }
      }
      return {
        data: {
          message: result.message,
          expiresInSeconds: result.expiresInSeconds,
          maskedPhone: result.maskedPhone,
        } as SmsOtpSendResponse,
        conflict: null,
      }
    },
    onSuccess: (result) => {
      // Only toast on a real send (not on conflict). Conflict handling is the
      // form's job (guidance + register link), not a toast.
      if (result.data) {
        toast.success(m['auth.phone_otp.send_success']())
      }
      onSuccess?.(result)
    },
    onError: (error) => {
      onError?.(error)
    },
  })
}

export interface UseSmsOtpVerifyMutationOptions {
  realmId: string
  onSuccess?: () => void
  /**
   * Called when verify 200 answered `requires-second-factor` (user with an
   * enabled TOTP/passkey logged in via OTP). No tokens were issued; the
   * caller routes the user to the shared second-factor step with the
   * returned temp token (mirrors the password-login branch).
   */
  onSecondFactorRequired?: (tempToken: string, secondFactors: string[]) => void
  /**
   * Called when verify answered 409 `phone_not_registered` (the account was
   * anonymized between send and verify). Same conflict shape as send; the
   * form shows the not-registered guidance view.
   */
  onConflict?: (conflict: SmsOtpSendConflict) => void
  onError?: (error: unknown) => void
}

/**
 * Verify outcome resolved by the mutationFn. The `requires-second-factor`
 * branch must NOT resolve as a plain success: TanStack fires the hook-level
 * `onSuccess` for every resolved mutationFn, so the branch is carried as data
 * and dispatched in `onSuccess` — second-factor routing and login completion
 * stay mutually exclusive. The 409 conflict branch is likewise carried as
 * data (mirrors send).
 */
type SmsOtpVerifyOutcome =
  | { kind: 'success' }
  | { kind: 'second-factor'; tempToken: string; secondFactors: string[] }
  | { kind: 'conflict'; conflict: SmsOtpSendConflict }

/**
 * Verify mutation. On 200 the Herald SDK applies the issued token set itself
 * (AT in its holder, RT in its storage), so `onSuccess` carries no payload —
 * the route owns `completeLoginAfterEmailOtp` (clientId rebind + hydration)
 * and navigation. Verify never returns `redirectTo`/PKCE, so no exchange
 * branch is needed here.
 *
 * The one non-token branch is `requires-second-factor` (a user with an
 * enabled TOTP/passkey must complete the second factor before a session is
 * issued); it is surfaced through `onSecondFactorRequired` instead of
 * throwing.
 */
export function useSmsOtpVerifyMutation({
  realmId,
  onSuccess,
  onSecondFactorRequired,
  onConflict,
  onError,
}: UseSmsOtpVerifyMutationOptions) {
  return useMutation({
    mutationFn: async (payload: {
      phone: string
      code: string
      clientId: string
      turnstileToken?: string | null
    }): Promise<SmsOtpVerifyOutcome> => {
      const herald = ensureHeraldClient(realmId)
      herald.tokens.bindClientId(payload.clientId)
      const result = await herald.loginWithSmsOtp.verify({
        phone: payload.phone,
        code: payload.code,
        // The backend enforces Turnstile on verify too — a fresh token from
        // the code step's own widget, since send already consumed the
        // phone-step one.
        ...(payload.turnstileToken ? { turnstileToken: payload.turnstileToken } : {}),
      })
      if (result.kind === 'requires-second-factor' && result.tempToken) {
        return {
          kind: 'second-factor',
          tempToken: result.tempToken,
          secondFactors: result.secondFactors,
        }
      }
      if (result.kind === 'conflict') {
        return { kind: 'conflict', conflict: { code: result.code, message: result.message } }
      }
      // The backend's verify 200 is always a BrowserTokenResponse (mapped to
      // the SDK's `success` branch, tokens applied internally). Any other
      // branch is outside the contract — surface it loudly.
      if (result.kind !== 'success') {
        throw new Error(`Unexpected sms-OTP verify result: ${result.kind}`)
      }
      return { kind: 'success' }
    },
    onSuccess: (outcome) => {
      if (outcome.kind === 'second-factor') {
        onSecondFactorRequired?.(outcome.tempToken, outcome.secondFactors)
        return
      }
      if (outcome.kind === 'conflict') {
        onConflict?.(outcome.conflict)
        return
      }
      onSuccess?.()
    },
    onError: (error) => {
      onError?.(error)
    },
  })
}
