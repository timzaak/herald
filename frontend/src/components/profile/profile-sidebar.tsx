import { Link, useLocation } from '@tanstack/react-router'
import {
  User,
  ShieldCheck,
  CreditCard,
  Coins,
  History,
  FileText,
  LayoutDashboard,
  LogOut,
} from 'lucide-react'
import { useCallback, useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import { usePermissions, useRealmId } from '@/stores/auth-store'
import { logoutFlow } from '@/lib/auth-utils'
import { hasAdminPermission } from '@/lib/constants/auth-constants'
import { userFeatureAvailabilityQueryOptions } from '@/data/query-options'
import { m } from '@/paraglide/messages'
import { LanguageSwitcher } from '@/components/shared/language-switcher'
import { SidebarMenuLabel } from '@/components/shared/sidebar-menu-label'
import {
  SIDEBAR_ITEM_ICON_CLASS,
  sidebarItemClassName,
} from '@/components/shared/sidebar-item-styles'
import { realmPath, resolvedRealmFromPath } from '@/lib/realm-routing'
import type { LucideIcon } from 'lucide-react'

interface MenuItem {
  name: string
  path: string
  icon: LucideIcon
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
      Subscription: m['nav_profile.subscription'],
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
        icon: User,
      },
      {
        name: 'Security',
        path: realmPath({ ...realmContext, realmId }, '/user/security'),
        icon: ShieldCheck,
      },
      {
        name: 'Subscription',
        path: realmPath({ ...realmContext, realmId }, '/user/my-subscriptions'),
        icon: CreditCard,
        // US-BI-009 scenario 0: the entry follows the realm's subscription
        // capability (enabled entitlement mappings), not mere history records.
        visible: userFeatures?.subscriptionVisible === true,
      },
      {
        name: 'Points',
        path: realmPath({ ...realmContext, realmId }, '/user/points'),
        icon: Coins,
        visible: userFeatures?.pointsVisible === true,
      },
      {
        name: 'PurchaseRecords',
        path: realmPath({ ...realmContext, realmId }, '/user/subscription-history'),
        icon: History,
        visible: userFeatures?.pointsVisible === true,
      },
      {
        name: 'Invoices',
        path: realmPath({ ...realmContext, realmId }, '/user/invoices'),
        icon: FileText,
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
      className="flex w-full flex-col border-b border-sidebar-border bg-sidebar md:w-64 md:border-b-0 md:border-r"
    >
      {/* <md: title row doubling as the mobile toolbar (admin entry + language);
          ≥md: plain block title, toolbar items live in the bottom block below. */}
      <div className="flex items-center justify-between gap-2 px-4 pt-4 md:block md:px-5 md:pt-6">
        <h1 className="text-base font-semibold tracking-tight text-sidebar-foreground md:text-lg">
          {m['nav_profile.profile']()}
        </h1>
        <div className="flex items-center gap-3 md:hidden">
          {canAccessAdminConsole && (
            <a
              href={realmPath({ ...realmContext, realmId }, '/manage')}
              data-testid="profile-admin-console-link-mobile"
              className="py-1.5 text-sm text-sidebar-foreground/70 hover:text-sidebar-foreground transition-colors"
            >
              {m['nav_profile.admin_console']()}
            </a>
          )}
          <LanguageSwitcher />
          <button
            onClick={handleLogout}
            data-testid="profile-logout-button-mobile"
            aria-label={m['user_menu.logout']()}
            className="flex h-8 w-8 items-center justify-center text-sidebar-foreground/70 hover:text-sidebar-foreground transition-colors"
          >
            <LogOut className="h-4 w-4" />
          </button>
        </div>
      </div>

      <nav
        data-testid="profile-sidebar-nav"
        className="sidebar-scroll flex gap-1 overflow-x-auto px-2 pb-3 md:mt-4 md:min-h-0 md:flex-1 md:flex-col md:space-y-0.5 md:overflow-y-auto md:px-2 md:pb-4"
      >
        {menuItems
          .filter((item) => item.visible !== false)
          .map((item) => {
            const Icon = item.icon
            return (
              <Link
                key={item.name}
                to={item.path}
                data-testid={`profile-menu-${item.name.toLowerCase()}`}
                className={sidebarItemClassName(isActive(item.path))}
              >
                <Icon className={SIDEBAR_ITEM_ICON_CLASS} />
                <SidebarMenuLabel label={getProfileNavLabel(item.name)} className="truncate" />
              </Link>
            )
          })}
      </nav>

      <div className="hidden space-y-2 md:block md:px-4">
        {canAccessAdminConsole && (
          <a
            href={realmPath({ ...realmContext, realmId }, '/manage')}
            data-testid="profile-admin-console-link"
            className={sidebarItemClassName(false)}
          >
            <LayoutDashboard className={SIDEBAR_ITEM_ICON_CLASS} />
            <SidebarMenuLabel label={m['nav_profile.admin_console']()} className="truncate" />
          </a>
        )}
        <LanguageSwitcher />
      </div>

      <div className="mx-4 mt-4 hidden h-px bg-sidebar-border md:block" />
      <div className="hidden pt-4 pb-4 md:block md:px-4">
        <button
          data-testid="profile-logout-button"
          onClick={handleLogout}
          className={`${sidebarItemClassName(false)} w-full`}
        >
          <LogOut className={SIDEBAR_ITEM_ICON_CLASS} />
          <SidebarMenuLabel label={m['user_menu.logout']()} className="truncate" />
        </button>
      </div>
    </aside>
  )
}
