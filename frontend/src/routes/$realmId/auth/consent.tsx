import { createFileRoute, useRouter } from '@tanstack/react-router'
import { useMutation, useQuery } from '@tanstack/react-query'
import { useEffect, useRef, useState } from 'react'
import { oauthConsentDecision, oauthConsentProbe, recordConsent } from '@/lib/api-generated'
import type { LegalAgreementSummary } from '@/lib/api-generated'
import { consentSearchSchema, type ConsentSearchParams } from '@/lib/schemas/search-params'
import { getErrorMessage } from '@/lib/error-utils'
import { validateOAuthParams } from '@/lib/auth-utils'
import { useIsAuthenticated, useRealmId } from '@/stores/auth-store'
import { AuthPageWrapper } from '@/components/auth/auth-page-wrapper'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { AuthorizeConfirm } from '@/components/device/authorize-confirm'
import { ConsentAgreementsPanel } from '@/components/legal/ConsentAgreementsPanel'
import { publicConfigQueryOptions } from '@/data/query-options'
import { m } from '@/paraglide/messages'
import {
  realmPath,
  useCurrentSearch,
  useOptionalRouteParams,
  useResolvedRealmContext,
} from '@/lib/realm-routing'

// react-hooks/immutability forbids assigning window.location.href inside
// callbacks passed to hooks; route it through a module-level helper (same
// pattern as the login page).
function navigateExternally(url: string): void {
  window.location.href = url
}

export const Route = createFileRoute('/$realmId/auth/consent')({
  component: OAuthConsentPage,
  validateSearch: (search) => consentSearchSchema.parse(search),
  // NOTE: Authentication branching lives in the component, not __root.tsx —
  // this is the one /auth route that mounts FOR authenticated users (the
  // root guard exempts it), and it forwards anonymous and cross-realm
  // visitors to the login page with the OAuth continuation params preserved.
})

/**
 * Landing page for third-party Authorization Code + PKCE flows. The backend
 * `/authorize` redirects here because the navigation carries no Herald
 * credential (sessions are SPA-held Bearer tokens); this page is where the
 * three-way branch the flow needs actually happens:
 *
 * - anonymous, or a session minted for ANOTHER realm → forwarded to
 *   /auth/login with all params (the login page already completes the OAuth
 *   flow after a credential login; a cross-realm session cannot consent
 *   here — the backend enforces identity.realm_id == flow realm);
 * - authenticated in this realm + remembered grant (or first-party client)
 *   → silently approves and follows the redirect carrying the code;
 * - authenticated in this realm + no grant → shows the authorize/deny UI,
 *   then follows the redirect (deny arrives back as `error=access_denied`).
 */
export function OAuthConsentPage() {
  const router = useRouter()
  const routeParams = useOptionalRouteParams<{ realmId?: string }>(Route)
  const resolvedRealmContext = useResolvedRealmContext()
  const realmContext = routeParams.realmId
    ? { ...resolvedRealmContext, realmId: routeParams.realmId, isCustomDomain: false }
    : resolvedRealmContext
  const { realmId } = realmContext
  const search = useCurrentSearch<ConsentSearchParams>()
  const isAuthenticated = useIsAuthenticated()
  const sessionRealmId = useRealmId()
  // The consent endpoints enforce identity.realm_id == flow realm (403
  // otherwise), so a session minted for another realm — e.g. an admin-console
  // session arriving at a third-party realm's authorize — is functionally
  // anonymous here: forward it to this realm's login instead of probing
  // into the generic invalid-link dead end.
  const flowAuthenticated = isAuthenticated && sessionRealmId === realmId

  const { oauthParams, hasPartialOAuth } = validateOAuthParams(search)
  const { data: publicConfig } = useQuery(publicConfigQueryOptions(realmId))

  // Set when the backend legal consent gate blocks an approval: the pending
  // authorize state was NOT consumed, so recording consent and re-submitting
  // the decision completes the flow.
  const [consentAgreements, setConsentAgreements] = useState<LegalAgreementSummary[] | null>(null)
  const [error, setError] = useState<string | null>(null)

  // Anonymous or cross-realm visitors: forward to the login page with every
  // continuation param preserved — the login form's post-login redirect
  // completes the OAuth flow. Idempotent under StrictMode double-effects.
  useEffect(() => {
    if (!flowAuthenticated && !hasPartialOAuth && !oauthParams) {
      // No OAuth context at all: the plain login page is the right target.
      router.navigate({ to: realmPath(realmContext, '/auth/login'), params: { realmId } })
      return
    }
    if (!flowAuthenticated && oauthParams) {
      router.navigate({
        to: realmPath(realmContext, '/auth/login'),
        params: { realmId },
        search: {
          ...(search.clientId ? { clientId: search.clientId } : {}),
          oauthClientId: oauthParams.oauthClientId,
          redirectUri: oauthParams.redirectUri,
          state: oauthParams.state,
        },
      })
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [flowAuthenticated])

  const probeQuery = useQuery({
    queryKey: ['oauth-consent-probe', realmId, oauthParams?.state],
    queryFn: async () => {
      if (!oauthParams) throw new Error('missing OAuth params')
      const response = await oauthConsentProbe({
        path: { realmId },
        query: {
          oauth_client_id: oauthParams.oauthClientId,
          redirect_uri: oauthParams.redirectUri,
          state: oauthParams.state,
        },
      })
      if (response.error) throw response.error
      return response.data
    },
    enabled: flowAuthenticated && !!oauthParams,
    staleTime: Infinity,
    retry: false,
  })

  const decisionMutation = useMutation({
    mutationFn: async (approved: boolean) => {
      if (!oauthParams) throw new Error('missing OAuth params')
      const response = await oauthConsentDecision({
        path: { realmId },
        body: {
          oauth_client_id: oauthParams.oauthClientId,
          redirect_uri: oauthParams.redirectUri,
          state: oauthParams.state,
          approved,
        },
      })
      if (response.error) throw response.error
      return response.data
    },
    onSuccess: (data) => {
      if (data.status === 'consent_required' && data.agreements && data.agreements.length > 0) {
        setConsentAgreements(data.agreements)
        return
      }
      setConsentAgreements(null)
      if (data.redirect_to) {
        navigateExternally(data.redirect_to)
      } else {
        setError(m['oauth_consent.invalid_link']())
      }
    },
    onError: (err: unknown) => {
      setError(getErrorMessage(err))
    },
  })

  const recordConsentMutation = useMutation({
    mutationFn: async (agreements: LegalAgreementSummary[]) => {
      const response = await recordConsent({
        body: {
          agreements: agreements.map((a) => ({
            agreement_type: a.agreement_type,
            version_id: a.version_id,
          })),
        },
      })
      if (response.error) throw response.error
    },
    onSuccess: () => {
      setConsentAgreements(null)
      decisionMutation.mutate(true)
    },
    onError: (err: unknown) => {
      setError(getErrorMessage(err))
    },
  })

  // Remembered grant (or first-party client): the probe says no consent is
  // needed, so complete the flow without any interaction. The ref guards
  // against a double submission (React StrictMode re-runs effects).
  const autoApprovedRef = useRef(false)
  useEffect(() => {
    if (probeQuery.data && !probeQuery.data.consent_required && !autoApprovedRef.current) {
      autoApprovedRef.current = true
      decisionMutation.mutate(true)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [probeQuery.data])

  const invalidLink = hasPartialOAuth || !oauthParams

  return (
    <AuthPageWrapper whiteLabel={publicConfig?.whiteLabel} realmName={publicConfig?.realmName}>
      <Card className="w-full max-w-md" data-testid="oauth-consent-card">
        <CardHeader>
          <CardTitle data-testid="oauth-consent-title">{m['oauth_consent.title']()}</CardTitle>
        </CardHeader>
        <CardContent>
          {error && (
            <div
              className="mb-4 p-3 bg-destructive/10 border border-destructive/20 rounded text-destructive text-sm"
              data-testid="oauth-consent-error"
            >
              {error}
            </div>
          )}

          {invalidLink && !error && (
            <p className="text-sm text-muted-foreground text-center">
              {m['auth.oauth_params_incomplete']()}
            </p>
          )}

          {!invalidLink && !flowAuthenticated && (
            <p className="py-8 text-center text-muted-foreground">
              {m['oauth_consent.redirecting_to_login']()}
            </p>
          )}

          {!invalidLink && flowAuthenticated && probeQuery.isPending && (
            <p className="py-8 text-center text-muted-foreground">
              {m['oauth_consent.continuing']()}
            </p>
          )}

          {!invalidLink && flowAuthenticated && probeQuery.isError && !error && (
            <p
              className="py-4 text-center text-destructive text-sm"
              data-testid="oauth-consent-probe-error"
            >
              {m['oauth_consent.invalid_link']()}
            </p>
          )}

          {!invalidLink && flowAuthenticated && consentAgreements && (
            <ConsentAgreementsPanel
              realmId={realmId}
              agreements={consentAgreements}
              isPending={recordConsentMutation.isPending}
              title={m['oauth_consent.consent_title']()}
              description={m['oauth_consent.consent_description']()}
              agreeLabel={m['oauth_consent.agree_and_continue']()}
              declineLabel={m['oauth_consent.consent_cancel']()}
              onAgree={() => recordConsentMutation.mutate(consentAgreements)}
              onDecline={() => setConsentAgreements(null)}
              testIdPrefix="oauth-consent"
            />
          )}

          {!invalidLink && flowAuthenticated && probeQuery.data && !consentAgreements && (
            <AuthorizeConfirm
              clientAppName={probeQuery.data.client_app_name}
              clientAppIconUrl={probeQuery.data.client_app_icon_url}
              onConfirm={(approved) => decisionMutation.mutate(approved)}
              isLoading={decisionMutation.isPending}
            />
          )}
        </CardContent>
      </Card>
    </AuthPageWrapper>
  )
}
