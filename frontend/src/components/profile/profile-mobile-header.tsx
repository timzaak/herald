import { Link, useLocation } from '@tanstack/react-router'
import { ArrowLeft } from 'lucide-react'
import { m } from '@/paraglide/messages'
import { realmPath } from '@/lib/realm-routing'
import { useProfileMenu } from '@/components/profile/profile-nav'

/**
 * Mobile-only header for the six account-center section pages: back-to-hub
 * arrow plus the section title. Placed above the scrolling <main> in
 * ProfileLayout, so it stays pinned without sticky positioning.
 *
 * Sub-flows (TOTP setup, purchase, invoice apply, subscription history
 * detail) are intentionally not matched — they ship their own in-page back
 * affordances and a second header would stack two back buttons.
 */
export function ProfileMobileHeader() {
  const location = useLocation()
  const { realmContext, realmId, menuItems } = useProfileMenu()

  // Matched by path only: this header is a section page's only mobile chrome,
  // so it must not depend on feature-gate visibility — a loading/failed
  // feature query or a later entitlement flip would otherwise strip the
  // page's only navigation.
  const current = menuItems.find((item) => location.pathname === item.path)

  if (!current) return null

  return (
    <header
      data-testid="profile-mobile-header"
      className="flex items-center gap-1 border-b border-border bg-background px-2 py-2 md:hidden"
    >
      <Link
        to={realmPath({ ...realmContext, realmId }, '/user')}
        aria-label={m['profile_hub.back']()}
        data-testid="profile-mobile-back"
        className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg text-foreground transition-colors hover:bg-accent"
      >
        <ArrowLeft className="size-5" />
      </Link>
      <h1
        className="min-w-0 truncate px-1 text-base font-semibold tracking-tight"
        data-testid="profile-mobile-title"
      >
        {current.label()}
      </h1>
    </header>
  )
}
