import { useEffect, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import { KeyRound } from 'lucide-react'
import { m } from '@/paraglide/messages'
import { withTimeout } from '@/lib/totp-utils'
import { isConsentRequired } from '@/lib/auth-utils'
import { isWebAuthnSupported, prepareRequestOptions, serializeAssertion } from '@/lib/passkey-utils'
import type { AssertionResultJSON } from '@herald/web'
import { mapLoginResultToResponse } from '@/lib/auth-service'
import { ensureHeraldClient } from '@/lib/herald-client'
import type {
  PasskeyVerifyResponse,
  PasskeyOAuthRequest,
  LegalAgreementSummary,
} from '@/lib/api-generated'
import { AgreementLinks } from '@/components/legal/AgreementLinks'
import { toAuthConsentAgreements } from '@/data/query-options'
import { formatDate } from '@/lib/date-utils'

export interface PasskeyLoginFormProps {
  realmId: string
  clientId: string
  turnstileToken?: string
  /** Forwarded OAuth context (complete triple) when present in the login URL. */
  oauth?: PasskeyOAuthRequest | null
  /**
   * Called after a successful verify that does NOT require consent. The consent
   * interlock (`isConsentRequired`) is re-checked inside this component and, if
   * consent is required, an inline re-consent UI is shown — mirroring the TOTP
   * form — so this callback only fires for a fully-completed login.
   */
  onSuccess: (response: PasskeyVerifyResponse) => void
  /**
   * Called once during mount when the realm has passkey disabled (options 404)
   * or the browser does not support WebAuthn, so the parent can hide the
   * Passkey entry point. Failures are silent — the password form remains.
   */
  onUnavailable?: () => void
}

/**
 * Passkey first-factor login (usernameless / conditional UI).
 *
 * On mount it fetches the WebAuthn challenge (`/login/passkey/options`) and
 * immediately arms the *conditional* UI (`navigator.credentials.get` with
 * `mediation: 'conditional'`), which stays pending until the user interacts
 * with the username autofill. The explicit "Use Passkey" button re-arms the
 * same challenge with `mediation: 'optional'`.
 *
 * The single `authToken` / `options` pair from the begin call is reused for
 * both paths. After the user selects a credential it is serialised and POSTed
 * to `/login/passkey/verify`; the consent interlock (`isConsentRequired`) is
 * re-checked — if consent is required, the last assertion is replayed with the
 * accepted agreements (identical to the TOTP form's re-consent flow) before
 * `onSuccess` fires.
 *
 * Error mapping: every failure (401 challenge mismatch, realm disabled 404,
 * assertion error, browser abort) collapses to the single user-facing
 * "Passkey verification failed" message — backend detail is never surfaced.
 */
export function PasskeyLoginForm({
  realmId,
  clientId,
  turnstileToken,
  oauth,
  onSuccess,
  onUnavailable,
}: PasskeyLoginFormProps) {
  const webAuthnSupported = isWebAuthnSupported()
  const [error, setError] = useState<string | null>(null)
  const [verifying, setVerifying] = useState(false)
  const [pendingConsent, setPendingConsent] = useState<LegalAgreementSummary[] | null>(null)
  // Reactive flag mirroring optionsRef.current so the "Use Passkey" button's
  // disabled state updates once the begin challenge has loaded (refs alone do
  // not trigger a re-render, so the button would stay disabled forever).
  const [optionsReady, setOptionsReady] = useState(false)

  // Hold the begin-challenge in refs so the conditional UI and the explicit
  // button share the same authToken/options without re-fetching.
  const authTokenRef = useRef<string | null>(null)
  const optionsRef = useRef<unknown>(null)
  // Keep the last serialised assertion so the re-consent path can replay the
  // verify with agreements (the assertion itself is single-use on the server,
  // but the backend re-issues the challenge tied to the same authToken).
  const lastAssertionRef = useRef<unknown>(null)
  const mountedRef = useRef(true)

  useEffect(() => {
    mountedRef.current = true
    return () => {
      mountedRef.current = false
    }
  }, [])

  // Abort controller for the pending conditional-UI request, cancelled on
  // unmount so a late credential selection after navigation does nothing.
  const abortRef = useRef<AbortController | null>(null)

  /**
   * POST the serialised assertion to verify. When `agreements` is supplied it
   * re-runs the same verify to complete the re-consent acceptance.
   */
  async function postVerify(
    assertion: unknown,
    agreements?: ReturnType<typeof toAuthConsentAgreements>
  ): Promise<PasskeyVerifyResponse | null> {
    const authToken = authTokenRef.current
    if (!authToken) return null

    const result = await withTimeout(
      ensureHeraldClient(realmId).passkey.loginFinish({
        authToken,
        assertion: assertion as AssertionResultJSON,
        ...(agreements ? { agreements } : {}),
      })
    )

    if (!mountedRef.current) return null
    // passkey verify returns the multi-branch login body; the SDK applies the
    // token set itself on the success branch and throws on HTTP errors, so map
    // the discriminated result back to the legacy shape the route consumes.
    return mapLoginResultToResponse(result) as unknown as PasskeyVerifyResponse
  }

  /**
   * Run the full verify sequence for a freshly-selected credential:
   * serialise → POST verify → consent interlock → success / consent UI.
   * Shared by both the conditional-UI and the explicit-button paths.
   */
  async function runVerify(credential: PublicKeyCredential): Promise<void> {
    const assertion = serializeAssertion(credential)
    lastAssertionRef.current = assertion

    setError(null)
    setVerifying(true)
    try {
      const data = await postVerify(assertion)
      if (!data) return

      if (isConsentRequired(data)) {
        setPendingConsent(data.agreements ?? [])
        return
      }
      setPendingConsent(null)
      onSuccess(data)
    } catch {
      if (mountedRef.current) {
        setError(m['auth.login.passkey_verification_failed']())
      }
    } finally {
      if (mountedRef.current) setVerifying(false)
    }
  }

  /**
   * Arm the conditional (autofill) UI once the begin challenge is available.
   * Stays pending until the user picks a credential via the autofill prompt;
   * any AbortError / no-selection is swallowed silently.
   */
  async function armConditional(): Promise<void> {
    const options = optionsRef.current
    if (!options) return

    const controller = new AbortController()
    abortRef.current?.abort()
    abortRef.current = controller

    try {
      const credential = await navigator.credentials.get({
        ...prepareRequestOptions(options, 'conditional'),
        signal: controller.signal,
      })
      if (!credential) return
      await runVerify(credential as PublicKeyCredential)
    } catch {
      // Conditional UI is best-effort: an abort (unmount / button press) or a
      // user dismissing the autofill is expected and must NOT show an error.
    }
  }

  /**
   * Begin: fetch the WebAuthn challenge, then arm the conditional UI. A 404
   * (realm passkey disabled) or any other error notifies the parent to hide
   * the entry point; the password form keeps working regardless.
   */
  async function beginAndArm(signal: AbortSignal): Promise<void> {
    try {
      const herald = ensureHeraldClient(realmId)
      // The parent resolved the product client (console vs account center) for
      // this flow — rebind the SDK's request-body clientId accordingly.
      herald.tokens.bindClientId(clientId)
      const data = await withTimeout(
        herald.passkey.loginBegin({
          ...(turnstileToken ? { turnstileToken } : {}),
          ...(oauth ? { oauth } : {}),
        })
      )

      if (!mountedRef.current || signal.aborted) return

      authTokenRef.current = data.authToken
      optionsRef.current = data.options
      if (mountedRef.current) setOptionsReady(true)

      await armConditional()
    } catch {
      // 404 (realm passkey disabled), network, or timeout — passkey is
      // unavailable; fall back silently.
      if (mountedRef.current) onUnavailable?.()
    }
  }

  useEffect(() => {
    if (!webAuthnSupported) {
      onUnavailable?.()
      return
    }

    const controller = new AbortController()
    void beginAndArm(controller.signal)
    return () => {
      controller.abort()
      abortRef.current?.abort()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [realmId, clientId])

  /**
   * Explicit "Use Passkey" button: re-arm the same challenge with
   * `mediation: 'optional'` to surface the native picker modal.
   */
  async function handleUsePasskey(): Promise<void> {
    const options = optionsRef.current
    if (!options || verifying) return

    abortRef.current?.abort()
    setError(null)
    const controller = new AbortController()
    abortRef.current = controller

    try {
      const credential = await navigator.credentials.get({
        ...prepareRequestOptions(options, 'optional'),
        signal: controller.signal,
      })
      if (!credential) return
      await runVerify(credential as PublicKeyCredential)
    } catch {
      // Dismissal / abort is silent. Genuine verify failures surface inside
      // runVerify with the unified message.
    }
  }

  /** Re-consent: replay the last assertion with the accepted agreements. */
  async function handleConsentAgree(): Promise<void> {
    if (!pendingConsent || !lastAssertionRef.current) return
    setError(null)
    setVerifying(true)
    try {
      const data = await postVerify(
        lastAssertionRef.current,
        toAuthConsentAgreements(pendingConsent)
      )
      if (!data) return
      setPendingConsent(null)
      onSuccess(data)
    } catch {
      if (mountedRef.current) {
        setError(m['auth.login.passkey_verification_failed']())
      }
    } finally {
      if (mountedRef.current) setVerifying(false)
    }
  }

  if (!webAuthnSupported) {
    return (
      <p className="text-sm text-muted-foreground" data-testid="passkey-unsupported-message">
        {m['auth.login.passkey_unsupported']()}
      </p>
    )
  }

  if (pendingConsent) {
    return (
      <div className="space-y-4" data-testid="passkey-login-form">
        <h3 className="font-semibold">{m['auth.login.reconsent_title']()}</h3>
        <p className="text-sm text-muted-foreground">{m['auth.login.reconsent_description']()}</p>
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
                agreementType={agreement.agreement_type as 'terms_of_service' | 'privacy_policy'}
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
          className="w-full"
          disabled={verifying}
          data-testid="passkey-agree-and-continue-button"
          onClick={handleConsentAgree}
        >
          {verifying ? m['common.loading']() : m['auth.login.agree_and_continue']()}
        </Button>
      </div>
    )
  }

  return (
    <div className="space-y-2" data-testid="passkey-login-form">
      <Button
        type="button"
        variant="outline"
        className="w-full"
        onClick={handleUsePasskey}
        disabled={verifying || !optionsReady}
        data-testid="passkey-login-button"
      >
        <KeyRound className="mr-2 h-4 w-4" />
        {m['auth.login.passkey_use_button']()}
      </Button>

      {error && (
        <p className="text-sm text-destructive" data-testid="passkey-verification-error">
          {error}
        </p>
      )}
    </div>
  )
}
