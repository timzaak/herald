import { useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { Rocket } from 'lucide-react'
import { signupStatusQueryOptions } from '@/data/query-options'
import { ADMIN_REALM_ID } from '@/lib/constants/auth-constants'
import { realmPath, type ResolvedRealmContext } from '@/lib/realm-routing'
import { Button } from '@/components/ui/button'
import { m } from '@/paraglide/messages'

interface SignupCtaBlockProps {
  /** Realm context of the login page, used for the signup link target. */
  realmContext: ResolvedRealmContext
}

/**
 * "Create your own Realm" guidance block for the platform storefront login
 * page. Visibility is strictly fail-closed: it renders only on an explicit
 * `enabled === true` from the public platform-signup status — loading,
 * disabled and failed queries all stay silent, exposing no error details to
 * visitors. The audience gate is enforced here rather than left to call
 * sites: the platform-branded copy must never render outside the admin
 * (platform storefront) realm.
 */
export function SignupCtaBlock({ realmContext }: SignupCtaBlockProps) {
  const { data: signupStatus } = useQuery(signupStatusQueryOptions(ADMIN_REALM_ID))

  if (realmContext.realmId !== ADMIN_REALM_ID || signupStatus?.enabled !== true) {
    return null
  }

  return (
    <div
      data-testid="signup-cta-block"
      className="mt-6 w-full rounded-lg border bg-card p-5 shadow-sm"
    >
      <div className="flex items-center gap-2">
        <Rocket className="size-4 text-primary" aria-hidden="true" />
        <h2 className="text-sm font-semibold tracking-tight text-foreground">
          {m['auth.login.signup_cta_title']()}
        </h2>
      </div>
      <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
        {m['auth.login.signup_cta_description']()}
      </p>
      <Button asChild className="mt-4 h-11 w-full" data-testid="signup-cta-link">
        <Link to={realmPath(realmContext, '/auth/signup')}>
          {m['auth.login.signup_cta_link']()}
        </Link>
      </Button>
    </div>
  )
}
