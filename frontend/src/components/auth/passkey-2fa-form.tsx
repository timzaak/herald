import { useEffect, useRef, useState } from 'react'
import { useMutation } from '@tanstack/react-query'
import { Button } from '@/components/ui/button'
import { KeyRound, RefreshCw } from 'lucide-react'
import { m } from '@/paraglide/messages'
import { withTimeout } from '@/lib/totp-utils'
import { isConsentRequired } from '@/lib/auth-utils'
import { isWebAuthnSupported, prepareRequestOptions, serializeAssertion } from '@/lib/passkey-utils'
import type { AssertionResultJSON } from '@herald/web'
import { mapLoginResultToResponse } from '@/lib/auth-service'
import { ensureHeraldClient } from '@/lib/herald-client'
import type {
  PasskeyVerifyResponse,
  AuthConsentAgreement,
  LegalAgreementSummary,
} from '@/lib/api-generated'
import { AgreementLinks } from '@/components/legal/AgreementLinks'
import { toAuthConsentAgreements } from '@/data/query-options'
import { formatDate } from '@/lib/date-utils'

export interface Passkey2FaFormProps {
  realmId: string
  tempToken: string
  /** Second factors the user has available — drives the TOTP fallback link. */
  secondFactors?: string[] | null
  onSuccess: (response: PasskeyVerifyResponse) => void
  onBack?: () => void
  /** Switch to the TOTP verification form (shown when secondFactors has totp). */
  onSwitchToTotp?: () => void
}

/**
 * Passkey second-factor verification form (password already verified, holds a
 * `tempToken`). Mirrors `TotpVerificationForm`:
 *
 * - begin: POST `/login/passkey/2fa/options` → `{ authToken, options }`
 * - verify: `navigator.credentials.get` → `serializeAssertion` → POST
 *   `/login/passkey/2fa/verify` with `{ tempToken, authToken, assertion }`
 * - consent interlock: `isConsentRequired` re-checked after verify (re-consent
 *   rendered inline, identical UX to the TOTP form)
 * - "Use TOTP instead" fallback (only when `secondFactors` also has `totp`)
 *
 * All failures collapse to "Passkey verification failed"; a user dismissing
 * the native prompt is treated as a silent cancellation.
 */
export function Passkey2FaForm({
  realmId,
  tempToken,
  secondFactors,
  onSuccess,
  onBack,
  onSwitchToTotp,
}: Passkey2FaFormProps) {
  const webAuthnSupported = isWebAuthnSupported()
  const [error, setError] = useState<string | null>(null)
  const [pendingConsent, setPendingConsent] = useState<LegalAgreementSummary[] | null>(null)
  const [lastAssertion, setLastAssertion] = useState<unknown>(null)

  const authTokenRef = useRef<string | null>(null)
  const optionsRef = useRef<unknown>(null)
  const abortRef = useRef<AbortController | null>(null)

  const canSwitchToTotp = !!onSwitchToTotp && (secondFactors ?? []).includes('totp')

  useEffect(() => {
    const controller = new AbortController()
    abortRef.current?.abort()
    abortRef.current = controller
    return () => {
      controller.abort()
      abortRef.current?.abort()
    }
  }, [])

  const beginMutation = useMutation({
    mutationFn: async () => {
      // The SDK throws on HTTP errors (including the 404 "2FA passkey not
      // available" case), which onError collapses to the unified message.
      return withTimeout(ensureHeraldClient(realmId).passkey.loginBegin({ tempToken }))
    },
    onSuccess: (data) => {
      authTokenRef.current = data.authToken
      optionsRef.current = data.options
    },
    onError: () => {
      setError(m['auth.login.passkey_verification_failed']())
    },
  })

  // Fetch the challenge as soon as the form mounts.
  useEffect(() => {
    if (!webAuthnSupported) return
    beginMutation.mutate()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const verifyMutation = useMutation({
    mutationFn: async (data: { assertion: unknown; agreements?: AuthConsentAgreement[] }) => {
      const authToken = authTokenRef.current
      if (!authToken) {
        throw new Error(m['auth.login.passkey_verification_failed']())
      }
      const result = await withTimeout(
        ensureHeraldClient(realmId).passkey.loginFinish({
          authToken,
          assertion: data.assertion as AssertionResultJSON,
          tempToken,
          ...(data.agreements ? { agreements: data.agreements } : {}),
        })
      )
      // passkey verify returns the multi-branch login body; the SDK applies
      // the token set itself on the success branch and throws on HTTP errors,
      // so map the discriminated result back to the legacy shape.
      return mapLoginResultToResponse(result) as unknown as PasskeyVerifyResponse
    },
    onSuccess: (data) => {
      setError(null)
      if (isConsentRequired(data)) {
        setPendingConsent(data.agreements ?? [])
        return
      }
      setPendingConsent(null)
      onSuccess(data)
    },
    onError: () => {
      setPendingConsent(null)
      setError(m['auth.login.passkey_verification_failed']())
    },
  })

  /**
   * Surface the native credential picker (mediation 'optional' — no autofill
   * in the second-factor context). Re-arms on each press so the user can
   * retry after a dismissal.
   */
  async function handleUsePasskey(): Promise<void> {
    const options = optionsRef.current
    if (!options || verifyMutation.isPending || beginMutation.isPending) return

    setError(null)
    abortRef.current?.abort()
    const controller = new AbortController()
    abortRef.current = controller

    try {
      const credential = await navigator.credentials.get({
        ...prepareRequestOptions(options),
        signal: controller.signal,
      })
      if (!credential) return
      const assertion = serializeAssertion(credential as PublicKeyCredential)
      setLastAssertion(assertion)
      verifyMutation.mutate({ assertion })
    } catch {
      // Dismissal / abort is silent. Genuine verify failures are surfaced by
      // verifyMutation.onError with the unified message.
    }
  }

  async function handleConsentAgree(): Promise<void> {
    if (!pendingConsent || !lastAssertion) return
    setError(null)
    verifyMutation.mutate({
      assertion: lastAssertion,
      agreements: toAuthConsentAgreements(pendingConsent),
    })
  }

  function handleConsentDecline(): void {
    setPendingConsent(null)
    onBack?.()
  }

  if (!webAuthnSupported) {
    return (
      <div className="w-full pt-8" data-testid="passkey-2fa-form">
        <h1 className="text-xl font-semibold tracking-tight">
          {m['auth.login.passkey_2fa_title']()}
        </h1>
        <div className="mt-6 space-y-4">
          <p className="text-sm text-muted-foreground" data-testid="passkey-unsupported-message">
            {m['auth.login.passkey_unsupported']()}
          </p>
          {canSwitchToTotp && (
            <button
              type="button"
              onClick={onSwitchToTotp}
              className="text-sm text-primary hover:underline"
              data-testid="passkey-use-totp-link"
            >
              {m['auth.login.passkey_use_totp_instead']()}
            </button>
          )}
          {onBack && (
            <Button
              type="button"
              variant="ghost"
              onClick={onBack}
              className="w-full"
              data-testid="passkey-use-password-link"
            >
              <RefreshCw className="mr-2 h-4 w-4" />
              {m['auth.login.passkey_use_password_instead']()}
            </Button>
          )}
        </div>
      </div>
    )
  }

  return (
    <div className="w-full pt-8" data-testid="passkey-2fa-form">
      <h1 className="text-xl font-semibold tracking-tight">
        {m['auth.login.passkey_2fa_title']()}
      </h1>
      <p className="mt-1 text-sm text-muted-foreground">
        {m['auth.login.passkey_2fa_description']()}
      </p>
      <div className="mt-6 space-y-4">
        {error && (
          <div className="text-sm text-destructive" data-testid="passkey-verification-error">
            {error}
          </div>
        )}

        {pendingConsent && (
          <div className="space-y-4" data-testid="passkey-reconsent-view">
            <h3 className="font-semibold">{m['auth.login.reconsent_title']()}</h3>
            <p className="text-sm text-muted-foreground">
              {m['auth.login.reconsent_description']()}
            </p>
            {pendingConsent.map((agreement) => (
              <div
                key={agreement.version_id}
                className="rounded border p-3"
                data-testid={`passkey-reconsent-agreement-${agreement.agreement_type}`}
              >
                <div className="font-medium">
                  <AgreementLinks
                    realmId={realmId}
                    agreements={[agreement]}
                    agreementType={
                      agreement.agreement_type as 'terms_of_service' | 'privacy_policy'
                    }
                  />
                </div>
                <div
                  className="text-sm text-muted-foreground"
                  data-testid={`passkey-reconsent-agreement-${agreement.agreement_type}-version`}
                >
                  {m['legal.version_label']()}: {agreement.version_no} •{' '}
                  {m['legal.effective_date_label']()}: {formatDate(agreement.effective_at)}
                </div>
              </div>
            ))}
            <Button
              type="button"
              disabled={verifyMutation.isPending}
              className="w-full"
              data-testid="passkey-agree-and-continue-button"
              onClick={handleConsentAgree}
            >
              {verifyMutation.isPending
                ? m['common.loading']()
                : m['auth.login.agree_and_continue']()}
            </Button>
            {onBack && (
              <Button
                type="button"
                variant="outline"
                className="w-full"
                data-testid="passkey-decline-back-button"
                onClick={handleConsentDecline}
              >
                {m['auth.login.decline_back_to_login']()}
              </Button>
            )}
          </div>
        )}

        {!pendingConsent && (
          <>
            <Button
              type="button"
              className="w-full"
              onClick={handleUsePasskey}
              disabled={verifyMutation.isPending || beginMutation.isPending || !optionsRef.current}
              data-testid="passkey-login-button"
            >
              <KeyRound className="mr-2 h-4 w-4" />
              {m['auth.login.passkey_use_button']()}
            </Button>

            {canSwitchToTotp && (
              <button
                type="button"
                onClick={onSwitchToTotp}
                className="block w-full text-center text-sm text-primary hover:underline"
                data-testid="passkey-use-totp-link"
              >
                {m['auth.login.passkey_use_totp_instead']()}
              </button>
            )}

            {onBack && (
              <Button
                type="button"
                variant="ghost"
                onClick={onBack}
                className="w-full"
                data-testid="passkey-use-password-link"
              >
                <RefreshCw className="mr-2 h-4 w-4" />
                {m['auth.login.passkey_use_password_instead']()}
              </Button>
            )}
          </>
        )}
      </div>
    </div>
  )
}
