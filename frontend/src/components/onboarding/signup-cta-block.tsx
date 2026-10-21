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
  const { data: signupStatus } = useQuery({
    ...signupStatusQueryOptions(ADMIN_REALM_ID),
    enabled: !realmContext.isCustomDomain,
  })

  // Creating a realm leaves the current realm, so this platform entry belongs
  // on the main host; a bound custom host cannot serve the new realm's console.
  if (
    realmContext.isCustomDomain ||
    realmContext.realmId !== ADMIN_REALM_ID ||
    signupStatus?.enabled !== true
  ) {
    return null
  }

  // Paper-side inset panel (paper-deep inset + rule hairline, DESIGN.md §2.1
  // 次级底/内嵌面): the storefront moment stays subordinate to the login form —
  // on auth vendors' login pages the signup entry is a quiet secondary action,
  // never an auto-opening dialog interrupting the admin who came to sign in.
  // Gold stays foil: only the Rocket icon carries kinpaku-deep, the sole
  // readable gold for decorative detail on paper (§2.2); gold text, gold
  // borders and LED glow belong to dark instrument panels only (§1 金箔法则,
  // §2.6 scopes those panels to the sidebar and code/key surfaces). The
  // standard outline button keeps the login submit below the view's single
  // solid primary (§1.2 一个赢家).
  return (
    <div
      data-testid="signup-cta-block"
      className="mb-6 w-full rounded-lg border border-border bg-secondary p-5"
    >
      <div className="flex items-center gap-2">
        <Rocket className="size-4 text-gold-deep" aria-hidden="true" />
        <h2 className="text-sm font-semibold tracking-tight text-foreground">
          {m['auth.login.signup_cta_title']()}
        </h2>
      </div>
      <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
        {m['auth.login.signup_cta_description']()}
      </p>
      <Button asChild variant="outline" className="mt-4 h-11 w-full" data-testid="signup-cta-link">
        <Link to={realmPath(realmContext, '/auth/signup')}>
          {m['auth.login.signup_cta_link']()}
        </Link>
      </Button>
    </div>
  )
}
