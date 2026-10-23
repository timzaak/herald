import { Link } from '@tanstack/react-router'
import { useCallback } from 'react'
import { useQuery } from '@tanstack/react-query'
import { ChevronRight, Globe, LayoutDashboard, LogOut } from 'lucide-react'
import { usePermissions } from '@/stores/auth-store'
import { logoutFlow } from '@/lib/auth-utils'
import { hasAdminPermission } from '@/lib/constants/auth-constants'
import { profileQueryOptions, publicConfigQueryOptions } from '@/data/query-options'
import { m } from '@/paraglide/messages'
import { LanguageSwitcher } from '@/components/shared/language-switcher'
import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Skeleton } from '@/components/ui/skeleton'
import { realmPath } from '@/lib/realm-routing'
import { PROFILE_MENU_GROUP_ORDER, useProfileMenu } from '@/components/profile/profile-nav'
import type { ProfileMenuGroup } from '@/components/profile/profile-nav'

const GROUP_LABELS: Record<ProfileMenuGroup, () => string> = {
  account: m['profile_hub.account_group'],
  billing: m['profile_hub.billing_group'],
}

const ROW_CLASS =
  'flex min-h-12 items-center gap-3 px-4 py-2.5 text-sm font-medium transition-colors hover:bg-accent'
const ROW_ICON_CLASS = 'size-[18px] shrink-0 text-muted-foreground'
const ROW_CHEVRON_CLASS = 'ml-auto size-4 shrink-0 text-muted-foreground/70'

/**
 * Mobile home of the account center: identity card + grouped menu rows +
 * a footer that keeps the destructive logout action isolated from navigation.
 * Rendered only below `md`; desktop keeps the sidebar (profile-sidebar).
 */
export function ProfileHub() {
  const { realmContext, realmId, menuItems } = useProfileMenu()
  const permissions = usePermissions()
  const canAccessAdminConsole = hasAdminPermission(permissions)
  const { data: profile, isLoading: profileLoading } = useQuery(profileQueryOptions)
  const { data: publicConfig } = useQuery(publicConfigQueryOptions(realmId))

  const handleLogout = useCallback(async () => {
    await logoutFlow(realmId)
  }, [realmId])

  const displayName = profile?.nickname || profile?.email || ''
  // The realm label is the fallback identity line when no nickname exists —
  // on a multi-tenant system the user must always see which realm they act in.
  const identitySubtitle = profile?.nickname ? profile.email : (publicConfig?.realmName ?? realmId)
  const initial = displayName.trim().charAt(0).toUpperCase()

  return (
    <div data-testid="profile-hub" className="space-y-5 pb-6 md:hidden">
      <Link
        to={realmPath({ ...realmContext, realmId }, '/user/profile')}
        data-testid="profile-hub-identity"
        className="flex items-center gap-4 rounded-xl border border-border bg-card p-4 transition-colors hover:bg-accent/50"
      >
        <Avatar className="size-14">
          <AvatarFallback className="text-lg">{initial}</AvatarFallback>
        </Avatar>
        <div className="min-w-0 flex-1">
          {profileLoading ? (
            <>
              <Skeleton className="h-5 w-28" />
              <Skeleton className="mt-2 h-4 w-40" />
            </>
          ) : !profile ? (
            // A failed profile query must surface, not silently render an
            // empty identity line — same notice as the Profile section page.
            <p className="text-sm text-muted-foreground" data-testid="profile-hub-identity-error">
              {m['profile.failed_to_load']()}
            </p>
          ) : (
            <>
              <p
                className="truncate text-base font-semibold"
                data-testid="profile-hub-identity-name"
              >
                {displayName}
              </p>
              <p className="mt-0.5 truncate text-sm text-muted-foreground">{identitySubtitle}</p>
            </>
          )}
        </div>
        <ChevronRight className="size-4 shrink-0 text-muted-foreground/70" />
      </Link>

      {PROFILE_MENU_GROUP_ORDER.map((group) => {
        const items = menuItems.filter((item) => item.group === group && item.visible !== false)
        if (items.length === 0) return null
        return (
          <section key={group} data-testid={`profile-hub-group-${group}`}>
            <h2 className="px-1 pb-2 text-xs font-medium uppercase tracking-wider text-muted-foreground">
              {GROUP_LABELS[group]()}
            </h2>
            <div className="divide-y divide-border overflow-hidden rounded-xl border border-border bg-card">
              {items.map((item) => {
                const Icon = item.icon
                return (
                  <Link
                    key={item.name}
                    to={item.path}
                    data-testid={`profile-hub-menu-${item.name.toLowerCase()}`}
                    className={ROW_CLASS}
                  >
                    <Icon className={ROW_ICON_CLASS} />
                    <span className="flex-1 truncate">{item.label()}</span>
                    {item.name === 'Security' && (
                      <span className="truncate text-xs font-normal text-muted-foreground/80">
                        {m['profile_hub.security_hint']()}
                      </span>
                    )}
                    <ChevronRight className={ROW_CHEVRON_CLASS} />
                  </Link>
                )
              })}
            </div>
          </section>
        )
      })}

      <section data-testid="profile-hub-footer">
        <div className="divide-y divide-border overflow-hidden rounded-xl border border-border bg-card">
          {canAccessAdminConsole && (
            <a
              href={realmPath({ ...realmContext, realmId }, '/manage')}
              data-testid="profile-hub-admin-console"
              className={ROW_CLASS}
            >
              <LayoutDashboard className={ROW_ICON_CLASS} />
              <span className="flex-1 truncate">{m['nav_profile.admin_console']()}</span>
              <ChevronRight className={ROW_CHEVRON_CLASS} />
            </a>
          )}
          <div className="flex min-h-12 items-center justify-between gap-3 px-4 py-2.5">
            {/* shrink-0 keeps the CJK label on one line: the default w-full
                select trigger would otherwise claim the whole row and squeeze
                the label to per-character wrapping. */}
            <span className="flex shrink-0 items-center gap-3 text-sm font-medium">
              <Globe className={ROW_ICON_CLASS} />
              {m['profile_hub.language']()}
            </span>
            <LanguageSwitcher className="w-auto" />
          </div>
        </div>
        {/* Logout sits alone in its own destructive card: it must be
            deliberately hard to hit by accident while browsing menu rows. */}
        <div className="mt-3 overflow-hidden rounded-xl border border-border bg-card">
          <button
            onClick={handleLogout}
            data-testid="profile-hub-logout"
            className="flex min-h-12 w-full items-center gap-3 px-4 py-2.5 text-sm font-medium text-destructive transition-colors hover:bg-destructive/10"
          >
            <LogOut className="size-[18px] shrink-0" />
            <span className="flex-1 text-left">{m['user_menu.logout']()}</span>
          </button>
        </div>
      </section>
    </div>
  )
}
