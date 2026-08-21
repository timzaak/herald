/**
 * Email-OTP login send/verify mutations (design §4.1, §4.2, §4.4.2).
 *
 * TanStack `useMutation` over the Herald SDK client
 * (`client.loginWithEmailOtp.send/verify`, DEC-js-sdk-014), with errors mapped
 * through `getErrorMessage` (`src/lib/error-utils`) and surfaced via `sonner`
 * toasts + `@/paraglide/messages` (`m`).
 *
 * Send's two 409 control-flow outcomes arrive as the SDK's `conflict` branch
 * (NOT errors):
 *   - `consent_required` (auto-register ON but consent missing) → the form
 *     renders the `agreements` list and re-sends with the accepted agreements
 *     (built via `toAuthConsentAgreements`);
 *   - `email_not_registered` (auto-register OFF) → the form shows the localized
 *     guidance message and the explicit-register link.
 * The mutation exposes them via the returned `EmailOtpSendResult` shape
 * instead of throwing, so the component can branch without a try/catch.
 *
 * Verify runs through the SDK as well: on success the SDK applies the issued
 * token set itself, so `onSuccess` carries no token payload — the route's
 * `completeLoginAfterEmailOtp` only rebinds the routing clientId and hydrates.
 */

import { useMutation } from '@tanstack/react-query'
import { toast } from 'sonner'
import type { ConsentAgreement } from '@herald/web'
import type { EmailOtpSendResponse, LegalAgreementSummary } from '@/lib/api-generated'
import { ensureHeraldClient } from '@/lib/herald-client'
import { m } from '@/paraglide/messages'

/**
 * Conflict outcome surfaced to the form when send returns 409.
 *
 * - `code: 'consent_required'` → the form renders the `agreements` list and
 *   re-sends with the accepted agreements (built via `toAuthConsentAgreements`).
 * - `code: 'email_not_registered'` → the form shows the localized guidance
 *   message and the explicit-register link.
 */
export interface EmailOtpSendConflict {
  code: string
  consentRequired?: boolean | null
  agreements?: LegalAgreementSummary[] | null
  message: string
}

/**
 * Successful send result. On conflict, `conflict` is set and `data` is null
 * (no code was sent). On other errors the mutation throws and `onError` runs.
 */
export interface EmailOtpSendResult {
  data: EmailOtpSendResponse | null
  conflict: EmailOtpSendConflict | null
}

/**
 * Restore the raw snake_case agreement summaries the consent UI renders from
 * the SDK's normalized `ConsentAgreement` (whose `raw` field carries the
 * backend summary through — DEC-js-sdk-013).
 */
function conflictAgreements(agreements: ConsentAgreement[]): LegalAgreementSummary[] {
  return agreements.map(
    (a) =>
      (a.raw ?? {
        agreement_type: a.agreementType,
        version_id: a.versionId,
      }) as LegalAgreementSummary
  )
}

export interface UseEmailOtpSendMutationOptions {
  realmId: string
  onSuccess?: (result: EmailOtpSendResult) => void
  onError?: (error: unknown) => void
}

/**
 * Send mutation. Surfaces the 409 conflict branch via the returned
 * `EmailOtpSendResult` (data=null, conflict set) — it does NOT throw on 409,
 * so the form can branch on `code`/`agreements`/`message` without try/catch.
 * All other errors throw normally and flow to `onError`.
 */
export function useEmailOtpSendMutation({
  realmId,
  onSuccess,
  onError,
}: UseEmailOtpSendMutationOptions) {
  return useMutation({
    mutationFn: async (payload: {
      email: string
      clientId: string
      turnstileToken?: string | null
      agreements?: Array<{ agreementType: string; versionId: string }>
    }): Promise<EmailOtpSendResult> => {
      const herald = ensureHeraldClient(realmId)
      herald.tokens.bindClientId(payload.clientId)
      const result = await herald.loginWithEmailOtp.send({
        email: payload.email,
        ...(payload.turnstileToken ? { turnstileToken: payload.turnstileToken } : {}),
        ...(payload.agreements ? { agreements: payload.agreements } : {}),
      })
      if (result.kind === 'conflict') {
        return {
          data: null,
          conflict: {
            code: result.code,
            consentRequired: result.consentRequired,
            agreements: result.agreements.length > 0 ? conflictAgreements(result.agreements) : null,
            message: result.message,
          },
        }
      }
      return {
        data: {
          message: result.message,
          expiresInSeconds: result.expiresInSeconds,
        } as EmailOtpSendResponse,
        conflict: null,
      }
    },
    onSuccess: (result) => {
      // Only toast on a real send (not on conflict). Conflict handling is the
      // form's job (render gate / guidance), not a toast.
      if (result.data) {
        toast.success(m['auth.email_otp.send_success']())
      }
      onSuccess?.(result)
    },
    onError: (error) => {
      onError?.(error)
    },
  })
}

export interface UseEmailOtpVerifyMutationOptions {
  realmId: string
  onSuccess?: () => void
  onError?: (error: unknown) => void
}

/**
 * Verify mutation. On 200 the Herald SDK applies the issued token set itself
 * (AT in its holder, RT in its storage), so `onSuccess` carries no payload —
 * the route owns `completeLoginAfterEmailOtp` (clientId rebind + hydration)
 * and navigation (component/route boundary; mirror `PasskeyLoginForm` →
 * `handlePasskeySuccess`). Verify never returns `redirectTo`/PKCE, so no
 * exchange branch is needed here.
 */
export function useEmailOtpVerifyMutation({
  realmId,
  onSuccess,
  onError,
}: UseEmailOtpVerifyMutationOptions) {
  return useMutation({
    mutationFn: async (payload: {
      email: string
      code: string
      clientId: string
      agreements?: Array<{ agreementType: string; versionId: string }>
    }): Promise<void> => {
      const herald = ensureHeraldClient(realmId)
      herald.tokens.bindClientId(payload.clientId)
      const result = await herald.loginWithEmailOtp.verify({
        email: payload.email,
        code: payload.code,
        ...(payload.agreements ? { agreements: payload.agreements } : {}),
      })
      // The backend's verify 200 is always a BrowserTokenResponse (mapped to
      // the SDK's `success` branch, tokens applied internally). Any other
      // branch is outside the contract — surface it loudly.
      if (result.kind !== 'success') {
        throw new Error(`Unexpected email-OTP verify result: ${result.kind}`)
      }
    },
    onSuccess: () => {
      onSuccess?.()
    },
    onError: (error) => {
      onError?.(error)
    },
  })
}
