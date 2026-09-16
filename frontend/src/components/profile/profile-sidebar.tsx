import { Link, useLocation } from '@tanstack/react-router'
import { useCallback, useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import { usePermissions, useRealmId } from '@/stores/auth-store'
import { logoutFlow } from '@/lib/auth-utils'
import { hasAdminPermission } from '@/lib/constants/auth-constants'
import { userFeatureAvailabilityQueryOptions } from '@/data/query-options'
import { m } from '@/paraglide/messages'
import { LanguageSwitcher } from '@/components/shared/language-switcher'
import { realmPath, resolvedRealmFromPath } from '@/lib/realm-routing'

interface MenuItem {
  name: string
  path: string
  visible?: boolean
}

export function ProfileSidebar() {
  const location = useLocation()
  const storeRealmId = useRealmId()
  const realmContext = resolvedRealmFromPath(location.pathname)
  const realmId = realmContext.realmId || storeRealmId || 'admin'
  const permissions = usePermissions()
  const canAccessAdminConsole = hasAdminPermission(permissions)
  const { data: features } = useQuery(userFeatureAvailabilityQueryOptions)
  const userFeatures = features?.user

  /** Maps profile menu item name to its translated display label. */
  const getProfileNavLabel = useCallback((name: string): string => {
    const map: Record<string, () => string> = {
      Profile: m['nav_profile.profile'],
      Security: m['nav_profile.security'],
      Points: m['nav_profile.points'],
      PurchaseRecords: m['nav_profile.purchase_records'],
      Invoices: m['nav_profile.invoices'],
    }
    return map[name]?.() ?? name
  }, [])

  // Memoize menu items to prevent infinite re-renders
  const menuItems: MenuItem[] = useMemo(
    () => [
      {
        name: 'Profile',
        path: realmPath({ ...realmContext, realmId }, '/user/profile'),
      },
      {
        name: 'Security',
        path: realmPath({ ...realmContext, realmId }, '/user/security'),
      },
      {
        name: 'Points',
        path: realmPath({ ...realmContext, realmId }, '/user/points'),
        visible: userFeatures?.pointsVisible === true,
      },
      {
        name: 'PurchaseRecords',
        path: realmPath({ ...realmContext, realmId }, '/user/subscription-history'),
        visible: userFeatures?.pointsVisible === true,
      },
      {
        name: 'Invoices',
        path: realmPath({ ...realmContext, realmId }, '/user/invoices'),
        visible: userFeatures?.invoicesVisible === true,
      },
    ],
    [realmContext, realmId, userFeatures]
  )

  const isActive = (path: string) => location.pathname === path

  const handleLogout = useCallback(async () => {
    await logoutFlow(realmId)
  }, [realmId])

  return (
    <aside
      data-testid="profile-sidebar"
      className="flex w-full flex-col border-b border-border md:w-64 md:border-b-0 md:border-r md:px-6 md:py-8"
    >
      {/* <md: title row doubling as the mobile toolbar (admin entry + language);
          ≥md: plain block title, toolbar items live in the bottom block below. */}
      <div className="flex items-center justify-between gap-2 px-4 pt-4 md:block md:px-0 md:pt-0">
        <h1 className="text-base font-semibold tracking-tight text-foreground md:text-lg">
          {m['nav_profile.profile']()}
        </h1>
        <div className="flex items-center gap-3 md:hidden">
          {canAccessAdminConsole && (
            <a
              href={realmPath({ ...realmContext, realmId }, '/manage')}
              data-testid="profile-admin-console-link-mobile"
              className="py-1.5 text-sm text-muted-foreground hover:text-foreground transition-colors"
            >
              {m['nav.dashboard']()}
            </a>
          )}
          <LanguageSwitcher />
        </div>
      </div>

      <nav
        data-testid="profile-sidebar-nav"
        className="flex gap-1 overflow-x-auto px-4 pb-3 md:mt-8 md:flex-1 md:flex-col md:space-y-1 md:overflow-visible md:px-0 md:pb-0"
      >
        {menuItems
          .filter((item) => item.visible !== false)
          .map((item) => (
            <Link
              key={item.name}
              to={item.path}
              data-testid={`profile-menu-${item.name.toLowerCase()}`}
              className={`whitespace-nowrap rounded-md px-3 py-2 text-sm transition-colors md:block md:px-0 md:py-1.5 md:rounded-none ${
                isActive(item.path)
                  ? 'bg-muted font-medium text-foreground md:bg-transparent'
                  : 'text-muted-foreground hover:text-foreground'
              }`}
            >
              {getProfileNavLabel(item.name)}
            </Link>
          ))}
      </nav>

      <div className="hidden space-y-2 md:block">
        {canAccessAdminConsole && (
          <a
            href={realmPath({ ...realmContext, realmId }, '/manage')}
            data-testid="profile-admin-console-link"
            className="block py-1.5 text-sm text-muted-foreground hover:text-foreground transition-colors"
          >
            {m['nav.dashboard']()}
          </a>
        )}
        <LanguageSwitcher />
      </div>

      <div className="mt-4 hidden border-t border-border pt-4 md:block">
        <button
          data-testid="profile-logout-button"
          onClick={handleLogout}
          className="block py-1.5 text-sm text-muted-foreground hover:text-foreground transition-colors"
        >
          {m['user_menu.logout']()}
        </button>
      </div>
    </aside>
  )
}
