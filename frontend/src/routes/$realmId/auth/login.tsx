import { createFileRoute, useRouter } from '@tanstack/react-router'
import { useForm } from '@tanstack/react-form'
import { useMutation, useQuery } from '@tanstack/react-query'
import type {
  LoginRequestPayload,
  VerifyTotpResponse,
  PasskeyVerifyResponse,
  LegalAgreementSummary,
  AuthConsentAgreement,
  OneTapDirectResponse,
} from '@/lib/api-generated'
import { loginSchema } from '@/lib/schemas/common'
import { loginSearchSchema, type LoginSearchParams } from '@/lib/schemas/search-params'
import { getErrorMessage, getFieldErrorMessage } from '@/lib/error-utils'
import {
  loginFlow,
  completeLoginAfterTotp,
  completeLoginAfterPasskey,
  completeLoginAfterEmailOtp,
  completeLoginAfterOneTap,
  isConsentRequired,
  resolveLdapLoginError,
  getSafeRedirect,
  validateOAuthParams,
} from '@/lib/auth-utils'
import { performLdapLogin } from '@/lib/auth-service'
import { firstPartyClientForPath } from '@/lib/constants/auth-constants'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { AuthPageWrapper } from '@/components/auth/auth-page-wrapper'
import { TotpVerificationForm } from '@/components/auth/totp-verification-form'
import { PasskeyLoginForm } from '@/components/auth/passkey-login-form'
import { Passkey2FaForm } from '@/components/auth/passkey-2fa-form'
import { EmailOtpLoginForm } from '@/components/auth/email-otp-login-form'
import { LdapLoginForm, type LdapLoginFormValues } from '@/components/auth/ldap-login-form'
import { OneTapLogin } from '@/components/auth/one-tap-login'
import { TurnstileWidget } from '@/components/auth/turnstile-widget'
import {
  publicConfigQueryOptions,
  toAuthConsentAgreements,
  turnstileStatusQueryOptions,
  emailOtpStatusQueryOptions,
  passkeyStatusQueryOptions,
  ldapStatusQueryOptions,
} from '@/data/query-options'
import { Link } from '@tanstack/react-router'
import { useOAuthLogin } from '@/hooks/use-oauth-login'
import { useState } from 'react'
import { toast } from 'sonner'
import { m } from '@/paraglide/messages'
import { AgreementLinks } from '@/components/legal/AgreementLinks'
import { formatDate } from '@/lib/date-utils'
import {
  realmPath,
  useCurrentSearch,
  useOptionalRouteParams,
  useResolvedRealmContext,
} from '@/lib/realm-routing'

// react-hooks/immutability forbids assigning window.location.href inside
// callbacks passed to hooks; route it through a module-level helper.
function navigateExternally(url: string): void {
  window.location.href = url
}

interface TotpStep {
  tempToken: string
}

/** Which first factor produced the pending login/consent submission. */
type LoginFactor = 'password' | 'ldap'

/** Values accepted by the shared `loginMutation` (both first factors). */
interface LoginMutationValues {
  factor: LoginFactor
  username: string
  password: string
  agreements?: AuthConsentAgreement[]
  turnstileToken?: string
}

interface ConsentStep {
  agreements: LegalAgreementSummary[]
  /**
   * Verbatim first-factor values that triggered the consent gate; replayed
   * with `agreements` attached. The forms are not editable while the consent
   * view is mounted, so the snapshot equals the current field values.
   */
  originalPayload: LoginMutationValues
}

/**
 * Passkey second-factor step. Reached when a password login returns
 * `secondFactors` containing `"passkey"`. Carries the temp token plus the full
 * `secondFactors` list so the form can show the TOTP fallback link only when
 * `"totp"` is also present.
 */
interface PasskeySecondFactorStep {
  tempToken: string
  secondFactors: string[]
}

/**
 * Whether the realm exposes a passkey login entry point for the current
 * browser. Lazily flipped to false when the begin-options call 404s (realm
 * passkey disabled) or the browser lacks WebAuthn, so the entry is hidden
 * without affecting the password form.
 */

export const Route = createFileRoute('/$realmId/auth/login')({
  component: LoginPage,
  validateSearch: (search) => loginSearchSchema.parse(search),
  // NOTE: Authentication and redirect logic is handled by __root.tsx
  // This allows us to use cached auth data and avoid redundant API calls
})

export function LoginPage() {
  const router = useRouter()
  const routeParams = useOptionalRouteParams<{ realmId?: string }>(Route)
  const resolvedRealmContext = useResolvedRealmContext()
  const realmContext = routeParams.realmId
    ? { ...resolvedRealmContext, realmId: routeParams.realmId, isCustomDomain: false }
    : resolvedRealmContext
  const { realmId } = realmContext
  const search = loginSearchSchema.parse(useCurrentSearch()) as LoginSearchParams
  const { initiateOAuthLogin } = useOAuthLogin()

  const [totpStep, setTotpStep] = useState<TotpStep | null>(null)
  const [consentStep, setConsentStep] = useState<ConsentStep | null>(null)
  const [globalError, setGlobalError] = useState<string | null>(null)
  // Passkey second-factor step (reached when a password login returns
  // secondFactors containing "passkey"). The first-factor entry point does
  // not need a dedicated step — PasskeyLoginForm manages its own conditional
  // UI lifecycle and is mounted alongside the password form.
  const [passkeySecondFactor, setPasskeySecondFactor] = useState<PasskeySecondFactorStep | null>(
    null
  )
  // Tracks whether the mounted PasskeyLoginForm can still serve an entry for
  // this browser. Defaults to true and is flipped to false only by the form's
  // onUnavailable callback (begin-options 404 or unsupported browser). Acts as
  // a defensive secondary fallback behind `passkeyEnabled` (the primary gate
  // from /passkey/status) — see the comment near the PasskeyLoginForm render.
  const [passkeyAvailable, setPasskeyAvailable] = useState(true)
  // Google One Tap entry visibility. Defaults to true and is flipped to false
  // when the GIS script fails to load or `window.google` is absent, hiding the
  // entry without affecting the password/OAuth options. Separate from
  // `googleProvider` (which gates on realm config) so a load failure still
  // degrades gracefully even when the realm has Google enabled.
  const [oneTapAvailable, setOneTapAvailable] = useState(true)
  // Email-OTP mode toggle. `false` (default) shows the password form; `true`
  // swaps the card body for `EmailOtpLoginForm`. The toggle is only rendered
  // when the public OTP-status query reports `enabled`, so the
  // entry is hidden entirely for realms with OTP login off.
  const [otpMode, setOtpMode] = useState(false)
  // LDAP (corporate account) mode toggle. Same shape as `otpMode`: swapped-in
  // `LdapLoginForm` card body, back button returns to the password form. The
  // entry is rendered only on an explicit `enabled === true` from the public
  // LDAP status query — loading and failed queries keep it hidden (fail-closed).
  const [ldapMode, setLdapMode] = useState(false)

  const { data: publicConfig, isLoading } = useQuery(publicConfigQueryOptions(realmId))
  // Resolved Client App id used for both the password form's Turnstile status
  // and (when OTP mode is active) the OTP form. The Turnstile status endpoint
  // is clientId-keyed.
  const resolvedClientId = search.clientId || firstPartyClientForPath(search.redirect)
  const { data: turnstileStatus, isLoading: loadingTurnstile } = useQuery(
    turnstileStatusQueryOptions(realmId, resolvedClientId)
  )
  // Public OTP-login enablement flag. Gates the "Email code" entry visibility.
  // Anonymous; safe to query unconditionally.
  const { data: emailOtpStatus } = useQuery(emailOtpStatusQueryOptions(realmId))
  const emailOtpEnabled = emailOtpStatus?.enabled === true

  // Public corporate-directory (LDAP) login enablement flag. Gates the
  // "corporate account" entry visibility. Strictly `=== true` — loading and
  // failed queries must NOT surface the entry (fail-closed). Anonymous; safe
  // to query unconditionally.
  const { data: ldapStatus } = useQuery(ldapStatusQueryOptions(realmId))
  const ldapEnabled = ldapStatus?.enabled === true

  // Public Passkey enablement flag. The PRIMARY gate for the passkey entry:
  // when the realm has passkey disabled we skip mounting PasskeyLoginForm
  // entirely (so the begin-options probe request is never fired). The
  // `passkeyAvailable` state below remains as a defensive secondary fallback
  // (e.g. browser unsupported, or a rare race where the flag and the begin
  // endpoint disagree). Anonymous; safe to query unconditionally.
  const { data: passkeyStatus } = useQuery(passkeyStatusQueryOptions(realmId))
  // `!== false` keeps the entry optimistically visible while the query loads
  // (matches the pre-flag UX); it is hidden only on an explicit `enabled:false`.
  const passkeyEnabled = passkeyStatus?.enabled !== false

  // Per-realm white-label config. Derived once so every auth
  // sub-state (consent, TOTP, passkey 2FA, main form) reuses the same brand
  // presentation — missing one would silently drop the brand.
  const whiteLabel = publicConfig?.whiteLabel ?? null

  const oauthProviders = publicConfig?.oauthProviders ?? []
  const isRegistrationAllowed = publicConfig?.registration?.enabled === true

  const { oauthParams, hasPartialOAuth } = validateOAuthParams(search)

  // Google One Tap is offered only when the realm has the Google provider
  // enabled with a client_id exposed via publicConfig (the GIS init client_id),
  // AND this is not a third-party OAuth downstream login (oauthParams present).
  // In the downstream case the One Tap direct-session mode would mint a
  // first-party token, conflicting with the Code+PKCE grant the third party is
  // waiting on — that path is covered by the redirect OAuth buttons with state.
  const googleProvider = oauthProviders.find((p) => p.name === 'google' && p.enabled && p.clientId)
  const oneTapEligible = Boolean(googleProvider?.clientId) && !oauthParams

  // Shared second-factor routing for every first factor (password, LDAP via
  // the same mutation, email-OTP): a passkey-capable user lands on the
  // Passkey second-factor form (which offers a TOTP fallback when the list
  // also contains "totp"), anything else degrades to the TOTP step.
  const routeSecondFactor = (tempToken: string, secondFactors: string[]) => {
    if (secondFactors.includes('passkey')) {
      setPasskeySecondFactor({ tempToken, secondFactors })
    } else {
      setTotpStep({ tempToken })
    }
  }

  const loginMutation = useMutation({
    mutationFn: async (values: LoginMutationValues) => {
      const isLdap = values.factor === 'ldap'
      // LDAP usernames are directory identifiers — never split into email.
      const isEmail = !isLdap && values.username.includes('@')
      const clientId = resolvedClientId

      const loginData: LoginRequestPayload = {
        clientId,
        email: isEmail ? values.username : undefined,
        username: isEmail ? undefined : values.username,
        password: values.password,
        turnstileToken: values.turnstileToken || null,
        ...(values.agreements ? { agreements: values.agreements } : {}),
        ...(oauthParams ?? {}),
      }

      // Both first factors share this flow: PKCE bootstrap, consent
      // early-return, token exchange, and post-login hydration are
      // factor-agnostic; only the credential performer differs.
      const result = await loginFlow(realmId, loginData, {
        performer: isLdap ? performLdapLogin : undefined,
      })
      return { result, values }
    },
    onSuccess: async (data) => {
      setGlobalError(null)
      setConsentStep(null)
      const { response } = data.result

      // --- Second-factor routing (backward compatible) ----------
      // Read order: prefer `secondFactors` when present and non-empty; only
      // when it is ABSENT do we fall back to the legacy `requiresTotp` path.
      // This keeps the existing password+TOTP login 100% unchanged for any
      // backend that does not yet return `secondFactors`.
      const secondFactors =
        response.secondFactors && response.secondFactors.length > 0 ? response.secondFactors : null

      if (secondFactors) {
        if (!response.tempToken) {
          // Defensive: secondFactors without a tempToken cannot proceed to any
          // 2FA form. Fall through to consent / direct-login handling below.
        } else {
          routeSecondFactor(response.tempToken, secondFactors)
          return
        }
      } else if (response.requiresTotp && response.tempToken) {
        // Legacy fallback (unchanged behaviour): backend without secondFactors.
        setTotpStep({ tempToken: response.tempToken })
        setConsentStep(null)
        return
      }

      if (isConsentRequired(response)) {
        const agreements = response.agreements ?? []
        if (agreements.length > 0) {
          setConsentStep({ agreements, originalPayload: data.values })
          return
        }
      }

      if (response.redirectTo) {
        navigateExternally(response.redirectTo)
        return
      }

      toast.success(m['auth.login.login_successful']())

      const userRealmId = response.realmId || realmId
      let redirectPath = search.redirect || data.result.redirectPath

      // Prevent open redirect attacks
      redirectPath = getSafeRedirect(redirectPath)

      if (redirectPath === '/') {
        redirectPath = data.result.redirectPath
      }

      if (redirectPath.startsWith('http://') || redirectPath.startsWith('https://')) {
        navigateExternally(redirectPath)
        return
      }

      await router.navigate({
        to: realmPath({ ...realmContext, realmId: userRealmId }, redirectPath),
        params: { realmId: userRealmId },
      })
    },
  })

  /**
   * Shared error surface for every `loginMutation.mutate` call site: the
   * factor selects its error mapping (LDAP gets the dedicated 503/429 copy),
   * then the message lands in both the toast and the shared error region.
   */
  function handleLoginError(error: unknown, factor: LoginFactor) {
    const message = factor === 'ldap' ? resolveLdapLoginError(error) : getErrorMessage(error)
    toast.error(message)
    setGlobalError(message)
  }

  const form = useForm({
    defaultValues: { username: '', password: '', turnstileToken: '' },
    onSubmit: async ({ value }) => {
      setGlobalError(null)
      if (hasPartialOAuth) return
      loginMutation.mutate(
        {
          factor: 'password',
          username: value.username,
          password: value.password,
          turnstileToken: value.turnstileToken || undefined,
        },
        {
          onError: (error: unknown) => handleLoginError(error, 'password'),
        }
      )
    },
  })

  async function handleConsentAgree() {
    if (!consentStep) return
    setGlobalError(null)

    const agreements = toAuthConsentAgreements(consentStep.agreements)

    // Replay the exact first-factor submission with the agreements attached.
    // The factor snapshot selects the same performer (password vs LDAP) and
    // the same error mapping as the original attempt.
    loginMutation.mutate(
      { ...consentStep.originalPayload, agreements },
      {
        onError: (error: unknown) => handleLoginError(error, consentStep.originalPayload.factor),
      }
    )
  }

  function handleConsentDecline() {
    setConsentStep(null)
    setGlobalError(null)
  }

  /**
   * LDAP first-factor submission. Errors route through
   * `resolveLdapLoginError`: 503/429 get dedicated localized messages; every
   * other failure surfaces the backend message verbatim (the 401
   * anti-enumeration copy is shared with password login).
   */
  function handleLdapSubmit(values: LdapLoginFormValues) {
    setGlobalError(null)
    if (hasPartialOAuth) return
    loginMutation.mutate(
      {
        factor: 'ldap',
        username: values.username,
        password: values.password,
        turnstileToken: values.turnstileToken,
      },
      {
        onError: (error: unknown) => handleLoginError(error, 'ldap'),
      }
    )
  }

  /**
   * Resolve the post-login redirect for the verify-success handlers (TOTP /
   * Passkey / Email-OTP) and navigate: apply the safe-redirect guard, the
   * '/' → admin/user home fallback, then the internal-vs-external rule. The
   * OAuth/password path (`loginMutation`) does NOT use this — it may land the
   * user in a different home realm.
   */
  async function navigateAfterLoginSuccess(redirectPath: string | undefined): Promise<void> {
    // Prevent open redirect attacks
    let safeRedirectPath = getSafeRedirect(search.redirect, redirectPath)

    if (safeRedirectPath === '/') {
      safeRedirectPath = redirectPath || '/user/profile'
    }

    if (safeRedirectPath.startsWith('http://') || safeRedirectPath.startsWith('https://')) {
      navigateExternally(safeRedirectPath)
      return
    }

    await router.navigate({
      to: realmPath(realmContext, safeRedirectPath),
      params: { realmId },
    })
  }

  async function handleTotpSuccess(verifyResponse: VerifyTotpResponse): Promise<void> {
    toast.success(m['auth.login.login_successful']())

    const { redirectPath, redirectTo } = await completeLoginAfterTotp(realmId, verifyResponse)

    if (redirectTo) {
      navigateExternally(redirectTo)
      return
    }

    await navigateAfterLoginSuccess(redirectPath)
  }

  /**
   * Shared completion handler for a Passkey login that has already passed the
   * consent interlock (handled inside the passkey forms). Behaviour mirrors
   * `handleTotpSuccess`: fetch auth data, store it, redirect safely. Used by
   * both the first-factor form and the second-factor form.
   */
  async function handlePasskeySuccess(verifyResponse: PasskeyVerifyResponse): Promise<void> {
    toast.success(m['auth.login.login_successful']())

    const { redirectPath, redirectTo } = await completeLoginAfterPasskey(realmId, verifyResponse)

    if (redirectTo) {
      navigateExternally(redirectTo)
      return
    }

    await navigateAfterLoginSuccess(redirectPath)
  }

  /**
   * Completion handler for an Email-OTP login. Mirrors `handlePasskeySuccess`
   * minus the PKCE/`redirectTo` branch — OTP verify runs through the Herald
   * SDK, which applies the issued token set itself, so
   * `completeLoginAfterEmailOtp` only rebinds the routing clientId + hydrates.
   * The `EmailOtpLoginForm` notified the route via its argument-less
   * `onSuccess` prop.
   */
  async function handleEmailOtpSuccess(): Promise<void> {
    toast.success(m['auth.login.login_successful']())

    const { redirectPath } = await completeLoginAfterEmailOtp(realmId, resolvedClientId)

    await navigateAfterLoginSuccess(redirectPath)
  }

  /**
   * Completion handler for a Google One Tap login. Mirrors
   * `handleEmailOtpSuccess`: the One Tap direct-session endpoint returns a
   * flattened `BrowserTokenSet` (`OneTapDirectResponse`) with no PKCE /
   * `redirectTo` branch, so only the safe-internal-redirect path applies. The
   * route owns token storage (`completeLoginAfterOneTap`) + navigation; the
   * `OneTapLogin` component handed up the raw response via its `onSuccess` prop.
   */
  async function handleOneTapSuccess(tokenResponse: OneTapDirectResponse): Promise<void> {
    toast.success(m['auth.login.login_successful']())

    const { redirectPath } = await completeLoginAfterOneTap(
      realmId,
      tokenResponse,
      resolvedClientId
    )

    await navigateAfterLoginSuccess(redirectPath)
  }

  if (consentStep) {
    return (
      <AuthPageWrapper whiteLabel={whiteLabel} realmName={publicConfig?.realmName}>
        <div className="w-full pt-8" data-testid="login-reconsent-view">
          <h1 data-testid="login-reconsent-title" className="text-xl font-semibold tracking-tight">
            {m['auth.login.reconsent_title']()}
          </h1>
          <p className="mt-1 text-sm text-muted-foreground">
            {m['auth.login.reconsent_description']()}
          </p>
          <div className="mt-6 space-y-4">
            {consentStep.agreements.map((agreement) => (
              <div
                key={agreement.version_id}
                className="rounded border p-3"
                data-testid={`login-reconsent-agreement-${agreement.agreement_type}`}
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
                  data-testid={`login-reconsent-agreement-${agreement.agreement_type}-version`}
                >
                  {m['legal.version_label']()}: {agreement.version_no} •{' '}
                  {m['legal.effective_date_label']()}: {formatDate(agreement.effective_at)}
                </div>
              </div>
            ))}
            <Button
              type="button"
              disabled={loginMutation.isPending}
              className="w-full"
              data-testid="login-agree-and-continue-button"
              onClick={handleConsentAgree}
            >
              {loginMutation.isPending
                ? m['auth.login.logging_in']()
                : m['auth.login.agree_and_continue']()}
            </Button>
            <Button
              type="button"
              variant="outline"
              className="w-full"
              data-testid="login-decline-back-button"
              onClick={handleConsentDecline}
            >
              {m['auth.login.decline_back_to_login']()}
            </Button>
          </div>
        </div>
      </AuthPageWrapper>
    )
  }

  if (totpStep) {
    return (
      <AuthPageWrapper whiteLabel={whiteLabel} realmName={publicConfig?.realmName}>
        <TotpVerificationForm
          realmId={realmId}
          tempToken={totpStep.tempToken}
          onSuccess={handleTotpSuccess}
          onBack={() => setTotpStep(null)}
        />
      </AuthPageWrapper>
    )
  }

  if (passkeySecondFactor) {
    return (
      <AuthPageWrapper whiteLabel={whiteLabel} realmName={publicConfig?.realmName}>
        <Passkey2FaForm
          realmId={realmId}
          tempToken={passkeySecondFactor.tempToken}
          secondFactors={passkeySecondFactor.secondFactors}
          onSuccess={handlePasskeySuccess}
          onBack={() => setPasskeySecondFactor(null)}
          // Only offer the TOTP fallback when the user actually has TOTP.
          onSwitchToTotp={
            passkeySecondFactor.secondFactors.includes('totp')
              ? () => {
                  setTotpStep({ tempToken: passkeySecondFactor.tempToken })
                  setPasskeySecondFactor(null)
                }
              : undefined
          }
        />
      </AuthPageWrapper>
    )
  }

  // Brand name lives in the AuthPageWrapper header; the form heading is the
  // functional title unless the tenant white-label provides a custom one, so
  // the default skin never prints the brand twice on one screen.
  const loginSubtitle = whiteLabel?.loginSubtitle ?? publicConfig?.realmDescription ?? null

  return (
    <AuthPageWrapper whiteLabel={whiteLabel} realmName={publicConfig?.realmName}>
      <div className="w-full pt-8" data-testid="login-card">
        <h1 data-testid="login-title" className="text-xl font-semibold tracking-tight">
          {whiteLabel?.loginTitle ?? m['auth.login.login_to_account']()}
        </h1>
        {loginSubtitle && <p className="mt-1 text-sm text-muted-foreground">{loginSubtitle}</p>}
        <div className="mt-6">
          {globalError && (
            <div
              className="mb-4 p-3 bg-destructive/10 border border-destructive/20 rounded text-destructive text-sm"
              data-testid="login-error-message"
            >
              {globalError}
            </div>
          )}

          {hasPartialOAuth && (
            <div
              className="mb-4 p-3 bg-destructive/10 border border-destructive/20 rounded text-destructive text-sm"
              data-testid="oauth-incomplete-error"
            >
              {m['auth.oauth_params_incomplete']()}
            </div>
          )}

          {otpMode ? (
            <EmailOtpLoginForm
              realmId={realmId}
              clientId={resolvedClientId}
              turnstileStatus={turnstileStatus}
              onSuccess={handleEmailOtpSuccess}
              // OTP verified but the user has an enabled TOTP/passkey: hand
              // the temp token to the SAME second-factor step the password
              // login uses (backend mirrors login.rs temp-session shape).
              onSecondFactorRequired={(tempToken, secondFactors) => {
                setOtpMode(false)
                routeSecondFactor(tempToken, secondFactors)
              }}
              onBack={() => setOtpMode(false)}
              registerPath={realmPath(realmContext, '/auth/register')}
            />
          ) : ldapMode ? (
            <LdapLoginForm
              realmId={realmId}
              isPending={loginMutation.isPending}
              hasPartialOAuth={hasPartialOAuth}
              turnstileStatus={turnstileStatus}
              onSubmit={handleLdapSubmit}
              onBack={() => setLdapMode(false)}
            />
          ) : (
            <>
              {emailOtpEnabled && (
                <div className="mb-4">
                  <Button
                    type="button"
                    variant="outline"
                    className="w-full"
                    onClick={() => setOtpMode(true)}
                    disabled={loginMutation.isPending}
                    data-testid="email-otp-toggle"
                  >
                    {m['auth.email_otp.toggle_entry']()}
                  </Button>
                </div>
              )}

              {ldapEnabled && (
                <div className="mb-4">
                  <Button
                    type="button"
                    variant="outline"
                    className="w-full"
                    onClick={() => setLdapMode(true)}
                    disabled={loginMutation.isPending}
                    data-testid="ldap-toggle"
                  >
                    {m['auth.ldap.toggle_entry']()}
                  </Button>
                </div>
              )}

              <form
                onSubmit={(e) => {
                  e.preventDefault()
                  form.handleSubmit()
                }}
                className="space-y-4"
                data-testid="login-form"
              >
                <form.Field name="username" validators={{ onChange: loginSchema.shape.username }}>
                  {(field) => (
                    <div>
                      <Label htmlFor="username">{m['auth.login.username_or_email']()}</Label>
                      <Input
                        id="username"
                        type="text"
                        value={field.state.value}
                        onBlur={field.handleBlur}
                        onChange={(e) => field.handleChange(e.target.value)}
                        disabled={loginMutation.isPending}
                        data-testid="email-input"
                      />
                      {field.state.meta.errors.length > 0 && (
                        <p className="text-sm text-destructive mt-1">
                          {getFieldErrorMessage(field.state.meta.errors[0])}
                        </p>
                      )}
                    </div>
                  )}
                </form.Field>

                <form.Field name="password" validators={{ onChange: loginSchema.shape.password }}>
                  {(field) => (
                    <div>
                      <div className="flex items-center justify-between">
                        <Label htmlFor="password">{m['auth.login.password']()}</Label>
                        <Link
                          to={realmPath(realmContext, '/auth/forgot-password')}
                          className="text-sm font-medium text-primary hover:text-primary/80"
                          data-testid="forgot-password-link"
                        >
                          {m['auth.forgot_password.forgot_link']()}
                        </Link>
                      </div>
                      <Input
                        id="password"
                        type="password"
                        value={field.state.value}
                        onBlur={field.handleBlur}
                        onChange={(e) => field.handleChange(e.target.value)}
                        disabled={loginMutation.isPending}
                        data-testid="password-input"
                      />
                      {field.state.meta.errors.length > 0 && (
                        <p className="text-sm text-destructive mt-1">
                          {getFieldErrorMessage(field.state.meta.errors[0])}
                        </p>
                      )}
                    </div>
                  )}
                </form.Field>

                {!loadingTurnstile && turnstileStatus?.enabled && (
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
                  disabled={loginMutation.isPending || hasPartialOAuth}
                  className="w-full"
                  data-testid="login-submit-button"
                >
                  {loginMutation.isPending
                    ? m['auth.login.logging_in']()
                    : m['auth.login.submit']()}
                </Button>

                <div
                  className="pt-2 text-xs leading-relaxed text-muted-foreground"
                  data-testid="login-consent-statement"
                >
                  {m['auth.login.consent_statement']()}
                  <AgreementLinks
                    realmId={realmId}
                    beforeText=" "
                    linkClassName="text-primary hover:text-primary/80 underline underline-offset-2"
                  />
                </div>
              </form>

              {/* Passkey first-factor entry. The PRIMARY gate is `passkeyEnabled`
              (from GET /passkey/status): when the realm has passkey disabled we
              skip mounting the form so the begin-options probe is never fired.
              `passkeyAvailable` is a defensive secondary fallback (browser
              unsupported, or a begin-challenge 404) — when it flips to false
              after mount we hide the entry without touching the password form. */}
              {passkeyEnabled && passkeyAvailable && (
                <div className="mt-4">
                  <PasskeyLoginForm
                    realmId={realmId}
                    clientId={resolvedClientId}
                    turnstileToken={form.getFieldValue('turnstileToken') || undefined}
                    oauth={
                      oauthParams
                        ? {
                            clientId: oauthParams.oauthClientId,
                            redirectUri: oauthParams.redirectUri,
                            state: oauthParams.state,
                          }
                        : null
                    }
                    onSuccess={handlePasskeySuccess}
                    onUnavailable={() => setPasskeyAvailable(false)}
                  />
                </div>
              )}

              {/* Google One Tap prompt entry. Only offered
              when the realm has Google enabled with a client_id and this is
              not a third-party OAuth downstream login. The GIS prompt overlay
              is rendered/positioned by Google; the component emits only an
              anchor. On script-load failure or `window.google` absence it
              calls onUnavailable and is hidden without affecting other
              entries (PRD §7 silent degradation). */}
              {oneTapEligible && oneTapAvailable && (
                <div className="mt-4">
                  <OneTapLogin
                    realmId={realmId}
                    clientId={resolvedClientId}
                    googleClientId={googleProvider!.clientId!}
                    onSuccess={handleOneTapSuccess}
                    onUnavailable={() => setOneTapAvailable(false)}
                  />
                </div>
              )}

              {!isLoading && oauthProviders.length > 0 && (
                <div className="space-y-3 mt-6">
                  <div className="relative">
                    <div className="absolute inset-0 flex items-center">
                      <span className="w-full border-t" />
                    </div>
                    <div className="relative flex justify-center text-xs uppercase">
                      <span className="bg-background px-2 text-muted-foreground">
                        {m['auth.login.or_continue_with']()}
                      </span>
                    </div>
                  </div>

                  <div className="grid grid-cols-2 gap-3">
                    {oauthProviders.map((provider) => (
                      <Button
                        key={provider.name}
                        variant="outline"
                        onClick={() =>
                          initiateOAuthLogin(realmId, provider.name, oauthParams?.state)
                        }
                        disabled={loginMutation.isPending}
                        data-testid={`oauth-login-button-${provider.name}`}
                      >
                        {provider.displayName}
                      </Button>
                    ))}
                  </div>
                </div>
              )}

              {isRegistrationAllowed && (
                <div className="mt-4">
                  <span className="text-sm text-muted-foreground">
                    {m['auth.login.no_account']()}{' '}
                  </span>
                  <Link
                    to={realmPath(realmContext, '/auth/register')}
                    className="text-sm font-medium text-primary hover:text-primary/80"
                    data-testid="register-link"
                  >
                    {m['auth.login.register_link']()}
                  </Link>
                </div>
              )}
            </>
          )}
        </div>
      </div>
    </AuthPageWrapper>
  )
}
