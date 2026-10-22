import { Link, useLocation } from '@tanstack/react-router'
import { LayoutDashboard, LogOut } from 'lucide-react'
import { useCallback } from 'react'
import { usePermissions } from '@/stores/auth-store'
import { logoutFlow } from '@/lib/auth-utils'
import { hasAdminPermission } from '@/lib/constants/auth-constants'
import { m } from '@/paraglide/messages'
import { LanguageSwitcher } from '@/components/shared/language-switcher'
import { SidebarMenuLabel } from '@/components/shared/sidebar-menu-label'
import {
  SIDEBAR_ITEM_ICON_CLASS,
  sidebarItemClassName,
} from '@/components/shared/sidebar-item-styles'
import { realmPath } from '@/lib/realm-routing'
import { useProfileMenu } from '@/components/profile/profile-nav'

export function ProfileSidebar() {
  const location = useLocation()
  const { realmContext, realmId, menuItems } = useProfileMenu()
  const permissions = usePermissions()
  const canAccessAdminConsole = hasAdminPermission(permissions)

  const userHomePath = realmPath({ ...realmContext, realmId }, '/user')
  const profilePath = realmPath({ ...realmContext, realmId }, '/user/profile')
  const isActive = (path: string) =>
    location.pathname === path ||
    // The /user landing renders the Profile section inline, so it highlights
    // Profile there the way /manage highlights Dashboard on its index.
    (path === profilePath && location.pathname.replace(/\/+$/, '') === userHomePath)

  const handleLogout = useCallback(async () => {
    await logoutFlow(realmId)
  }, [realmId])

  return (
    // Desktop-only rail: below md the account center is navigated as the
    // mobile hub page (profile-hub) with per-section back headers.
    <aside
      data-testid="profile-sidebar"
      className="hidden w-64 flex-col border-r border-sidebar-border bg-sidebar md:flex"
    >
      <div className="px-5 pt-6">
        <h1 className="text-lg font-semibold tracking-tight text-sidebar-foreground">
          {m['nav_profile.account_center']()}
        </h1>
      </div>

      <nav
        data-testid="profile-sidebar-nav"
        className="mt-4 flex min-h-0 flex-1 flex-col space-y-0.5 overflow-y-auto px-2 pb-4"
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
                <SidebarMenuLabel label={item.label()} className="truncate" />
              </Link>
            )
          })}
      </nav>

      <div className="space-y-2 px-4">
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

      <div className="mx-4 mt-4 h-px bg-sidebar-border" />
      <div className="px-4 pb-4 pt-4">
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
