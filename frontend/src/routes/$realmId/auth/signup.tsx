import { createFileRoute, useNavigate, Link } from '@tanstack/react-router'
import { AuthPageWrapper } from '@/components/auth/auth-page-wrapper'
import { SignupForm } from '@/components/auth/signup-form'
import { publicConfigQueryOptions, signupStatusQueryOptions } from '@/data/query-options'
import { useQuery } from '@tanstack/react-query'
import { m } from '@/paraglide/messages'
import { ADMIN_REALM_ID } from '@/lib/constants/auth-constants'
import { realmPath, resolvedRealmFromPath } from '@/lib/realm-routing'
import { markSignupSignal } from '@/components/onboarding/onboarding-storage'

export const Route = createFileRoute('/$realmId/auth/signup')({
  component: SignupPage,
})

// Self-service realm signup is an admin-realm-only public entry (DEC-001): the
// backend rejects any `realmId !== "admin"`. Branding still reads the admin
// realm's public config so the page presents the platform brand regardless of
// the URL it was opened under (main domain `/admin/auth/signup`, mirror
// `/auth/signup`, or a custom domain).
export function SignupPage() {
  const navigate = useNavigate()
  // Reused by the mirror `/auth/signup` route (same component), so resolve the
  // realm from the URL for link/navigation context — but the signup API and
  // branding are always the admin realm.
  const pathname = window.location.pathname
  const realmContext = resolvedRealmFromPath(pathname)

  const { data: publicConfig, isLoading: publicConfigLoading } = useQuery(
    publicConfigQueryOptions(ADMIN_REALM_ID)
  )
  const {
    data: signupStatus,
    isLoading: statusLoading,
    error: statusError,
  } = useQuery(signupStatusQueryOptions(ADMIN_REALM_ID))
  const whiteLabel = publicConfig?.whiteLabel ?? null

  const isLoading = publicConfigLoading || statusLoading
  const signupEnabled = signupStatus?.enabled === true

  function handleSignupSuccess(redirectPath: string, realmId: string): void {
    // One-shot signal for the console's first-login guidance: keyed to the
    // new realm, and it only selects the welcome-copy variant.
    markSignupSignal(realmId)
    // Navigate into the NEW realm's management console (DEC-012).
    navigate({ to: realmPath({ realmId, isCustomDomain: false }, redirectPath) })
  }

  if (isLoading) {
    return (
      <AuthPageWrapper whiteLabel={whiteLabel}>
        <div className="text-muted-foreground">{m['common.loading']()}</div>
      </AuthPageWrapper>
    )
  }

  // Fail-closed: a failed/absent status query means signup is treated as
  // disabled (DEC-013), matching the backend's missing-config → false behavior.
  if (statusError || !signupEnabled) {
    return (
      <AuthPageWrapper whiteLabel={whiteLabel}>
        <div className="w-full">
          <h1 data-testid="signup-disabled-notice" className="text-xl font-semibold tracking-tight">
            {m['auth.signup.disabled_title']()}
          </h1>
          <p className="mt-1 text-sm text-muted-foreground">
            {m['auth.signup.disabled_description']()}
          </p>
          <div className="mt-6">
            <Link
              to={realmPath(realmContext, '/auth/login')}
              className="text-sm font-medium text-primary hover:text-primary/80"
            >
              {m['auth.signup.return_to_login']()}
            </Link>
          </div>
        </div>
      </AuthPageWrapper>
    )
  }

  return (
    <AuthPageWrapper whiteLabel={whiteLabel}>
      <div className="w-full" data-testid="signup-card">
        <h1 data-testid="signup-title" className="text-xl font-semibold tracking-tight">
          {m['auth.signup.title']()}
        </h1>
        <p className="mt-1 text-sm text-muted-foreground" data-testid="signup-subtitle">
          {m['auth.signup.subtitle']()}
        </p>
        <div className="mt-6">
          <SignupForm onSuccess={handleSignupSuccess} />
        </div>
      </div>
    </AuthPageWrapper>
  )
}
