import { useState, useMemo, useCallback } from 'react'
import { Link } from '@tanstack/react-router'
import {
  LayoutDashboard,
  Users,
  Shield,
  Settings,
  Globe,
  Key,
  ChevronDown,
  Briefcase,
  CreditCard,
  History,
  FileText,
  ScrollText,
  BarChart3,
  Github,
} from 'lucide-react'
import { useQuery } from '@tanstack/react-query'
import { useAuth } from '@/hooks/use-auth'
import { useRealmId } from '@/stores/auth-store'
import { PERMISSION } from '@/lib/constants/auth-constants'
import { APP_VERSION, BRAND_NAME, GITHUB_REPO_URL } from '@/lib/constants'
import { realmQueryOptions, featureAvailabilityQueryOptions } from '@/data/query-options'
import { filterByPermission } from '@/lib/utils/filter-by-permission'
import { m } from '@/paraglide/messages'
import { LanguageSwitcher } from '@/components/shared/language-switcher'
import { SidebarMenuLabel } from '@/components/shared/sidebar-menu-label'
import {
  SIDEBAR_ITEM_ACTIVE_CLASS,
  SIDEBAR_ITEM_ICON_CLASS,
  sidebarItemClassName,
} from '@/components/shared/sidebar-item-styles'
import type { LucideIcon } from 'lucide-react'
import { useLocation } from '@tanstack/react-router'
import { realmPath, resolvedRealmFromPath } from '@/lib/realm-routing'

interface MenuItem {
  id: string
  name: string
  path?: string
  icon: LucideIcon
  permission: string | string[] | null
  visible?: boolean
  children?: MenuItem[]
}

export function Sidebar() {
  const storeRealmId = useRealmId()
  const location = useLocation()
  const realmContext = resolvedRealmFromPath(location.pathname)
  const realmId = realmContext.realmId || storeRealmId || 'admin'
  const customDomainPath = useCallback(
    (path: string) => realmPath({ ...realmContext, realmId }, path),
    [realmContext, realmId]
  )
  const { permissions } = useAuth()
  const { data: realm } = useQuery(realmQueryOptions(realmId))
  const { data: features } = useQuery(featureAvailabilityQueryOptions(realmId))
  const [openMenus, setOpenMenus] = useState<Set<string>>(new Set(['Authorization']))
  const adminFeatures = features?.admin

  /** Maps menu item id to its translated display label. */
  const getNavLabel = useCallback((id: string): string => {
    const map: Record<string, () => string> = {
      dashboard: m['nav.dashboard'],
      realms: m['nav.realms'],
      clients: m['nav.clients'],
      users: m['nav.users'],
      authorization: m['nav.authorization'],
      permissions: m['nav.permissions'],
      roles: m['nav.roles'],
      'api-keys': m['nav.api_keys'],
      'products-payments': m['nav.products_payments'],
      'payment-providers': m['nav.payment_providers'],
      'subscription-plans': m['nav.subscription_plans'],
      'entitlement-mappings': m['nav.entitlement_mappings'],
      'points-registration-rules': m['nav.points_registration_rules'],
      'credit-buckets': m['nav.credit_buckets'],
      transactions: m['nav.transactions'],
      invoices: m['nav.invoices'],
      'subscription-history': m['nav.subscription_history'],
      'points-wallets': m['nav.points_wallets'],
      statistics: m['nav.statistics'],
      'audit-log': m['nav.audit_log'],
      settings: m['nav.settings'],
    }
    return map[id]?.() ?? id
  }, [])

  const toggleMenu = useCallback((name: string) => {
    setOpenMenus((prev) => {
      const next = new Set(prev)
      if (next.has(name)) {
        next.delete(name)
      } else {
        next.add(name)
      }
      return next
    })
  }, [])

  const menuItems: MenuItem[] = useMemo(
    () => [
      {
        id: 'dashboard',
        name: 'Dashboard',
        path: customDomainPath('/manage'),
        icon: LayoutDashboard,
        permission: PERMISSION.DASHBOARD_VIEW,
      },
      {
        id: 'realms',
        name: 'Realms',
        path: customDomainPath('/manage/realms'),
        icon: Globe,
        permission: PERMISSION.REALM_VIEW,
      },
      {
        id: 'clients',
        name: 'Clients',
        path: customDomainPath('/manage/client-apps'),
        icon: Briefcase,
        permission: PERMISSION.CLIENTS_VIEW,
      },
      {
        id: 'users',
        name: 'Users',
        path: customDomainPath('/manage/users'),
        icon: Users,
        permission: PERMISSION.USERS_VIEW,
      },
      {
        id: 'authorization',
        name: 'Authorization',
        icon: Shield,
        permission: null,
        children: [
          {
            id: 'permissions',
            name: 'Permissions',
            path: customDomainPath('/manage/permissions'),
            icon: Key,
            permission: PERMISSION.PERMISSIONS_VIEW,
          },
          {
            id: 'roles',
            name: 'Roles',
            path: customDomainPath('/manage/roles'),
            icon: Shield,
            permission: PERMISSION.ROLES_VIEW,
          },
          {
            id: 'api-keys',
            name: 'API Keys',
            path: customDomainPath('/manage/api-keys'),
            icon: Key,
            permission: PERMISSION.API_KEYS_VIEW,
          },
        ],
      },
      {
        id: 'products-payments',
        name: 'Products & Payments',
        icon: Briefcase,
        permission: null,
        children: [
          {
            id: 'payment-providers',
            name: 'Payment Providers',
            path: customDomainPath('/manage/billing/payment-providers'),
            icon: CreditCard,
            permission: PERMISSION.BILLING_VIEW,
            visible: adminFeatures?.billingConfigVisible ?? true,
          },
          {
            id: 'entitlement-mappings',
            name: 'Entitlement Mappings',
            path: customDomainPath('/manage/billing/entitlement-mappings'),
            icon: CreditCard,
            permission: PERMISSION.BILLING_VIEW,
            visible: adminFeatures?.entitlementMappingsVisible ?? true,
          },
          {
            id: 'points-registration-rules',
            name: 'Registration Rules',
            path: customDomainPath('/manage/points/registration-rules'),
            icon: Settings,
            permission: PERMISSION.POINTS_VIEW,
            visible: adminFeatures?.pointsVisible ?? true,
          },
          {
            id: 'credit-buckets',
            name: 'Credit Buckets',
            path: customDomainPath('/manage/billing/credit-buckets'),
            icon: CreditCard,
            permission: PERMISSION.POINTS_VIEW,
            visible: adminFeatures?.pointsVisible ?? true,
          },
        ],
      },
      {
        id: 'transactions',
        name: 'Transactions',
        icon: FileText,
        permission: null,
        children: [
          {
            id: 'invoices',
            name: 'Invoices',
            path: customDomainPath('/manage/billing/invoices'),
            icon: FileText,
            permission: PERMISSION.BILLING_VIEW,
            visible: adminFeatures?.invoicesVisible ?? true,
          },
          {
            id: 'subscription-history',
            name: 'Subscription History',
            path: customDomainPath('/manage/subscription-history'),
            icon: History,
            permission: PERMISSION.BILLING_VIEW,
            visible: adminFeatures?.subscriptionHistoryVisible ?? true,
          },
          {
            id: 'points-wallets',
            name: 'Points Wallets',
            path: customDomainPath('/manage/points/wallets'),
            icon: Users,
            permission: PERMISSION.POINTS_VIEW,
            visible: adminFeatures?.pointsVisible ?? true,
          },
          {
            id: 'statistics',
            name: 'Statistics',
            path: customDomainPath('/manage/billing/statistics'),
            icon: BarChart3,
            permission: [PERMISSION.BILLING_VIEW, PERMISSION.POINTS_VIEW],
          },
        ],
      },
      {
        id: 'audit-log',
        name: 'Audit Log',
        path: customDomainPath('/manage/audit'),
        icon: ScrollText,
        permission: PERMISSION.AUDIT_VIEW,
      },
      {
        id: 'settings',
        name: 'Settings',
        path: customDomainPath('/manage/settings'),
        icon: Settings,
        permission: PERMISSION.SETTINGS_VIEW,
      },
    ],
    [adminFeatures, customDomainPath]
  )

  const filteredMenuItems = useMemo(
    () => filterByPermission(menuItems, permissions, realmId),
    [menuItems, permissions, realmId]
  )

  const renderMenuItem = (item: MenuItem, level: number = 0) => {
    const hasChildren = item.children && item.children.length > 0
    const isOpen = openMenus.has(item.name)
    const Icon = item.icon
    const label = getNavLabel(item.id)

    if (item.visible === false) {
      return null
    }

    const visibleChildren = hasChildren
      ? item.children!.filter((child) => child.visible !== false)
      : []

    if (hasChildren && visibleChildren.length === 0) {
      return null
    }

    const paddingLeft = level > 0 ? 'pl-12 pr-4' : 'px-4'

    return (
      <div key={item.name}>
        {!hasChildren && item.path ? (
          <Link
            to={item.path}
            className={sidebarItemClassName(false, paddingLeft)}
            activeProps={{ className: SIDEBAR_ITEM_ACTIVE_CLASS }}
            activeOptions={{ exact: true }}
            data-testid={`sidebar-menu-${item.name.toLowerCase().replace(/\s+/g, '-')}`}
          >
            <Icon className={SIDEBAR_ITEM_ICON_CLASS} />
            <SidebarMenuLabel label={label} className="truncate" />
          </Link>
        ) : (
          <div
            onClick={() => hasChildren && toggleMenu(item.name)}
            className={`group flex items-center gap-3 rounded-lg py-2.5 text-sm font-medium text-sidebar-foreground/60 cursor-pointer transition-all duration-150 hover:bg-sidebar-accent hover:text-sidebar-foreground/90 px-4`}
            data-testid={`sidebar-menu-${item.name.toLowerCase().replace(/\s+/g, '-')}`}
            aria-expanded={hasChildren ? isOpen : undefined}
          >
            <Icon className={SIDEBAR_ITEM_ICON_CLASS} />
            <SidebarMenuLabel label={label} className="flex-1 truncate" />
            {hasChildren && (
              <ChevronDown
                className={`size-4 shrink-0 text-sidebar-foreground/40 transition-transform duration-200 ${isOpen ? 'rotate-0' : '-rotate-90'}`}
              />
            )}
          </div>
        )}

        {hasChildren && isOpen && (
          <div
            className="mt-0.5 space-y-px"
            data-testid={`sidebar-submenu-${item.name.toLowerCase().replace(/\s+/g, '-')}`}
          >
            {visibleChildren.map((child) => renderMenuItem(child, level + 1))}
          </div>
        )}
      </div>
    )
  }

  return (
    <div
      className="flex h-full min-h-0 w-64 flex-col bg-sidebar border-r border-sidebar-border"
      data-testid="admin-sidebar"
    >
      <div className="shrink-0 px-5 pt-6 pb-4">
        <h1 className="font-display text-xl font-light uppercase tracking-[0.18em] text-sidebar-foreground">
          {BRAND_NAME}
        </h1>
        <p className="mt-1 text-[11px] font-medium uppercase tracking-[0.1em] text-instrument-muted">
          {realm?.name ?? realmId}
        </p>
      </div>

      <div className="mx-4 mb-3 h-px bg-sidebar-border" />

      <nav
        className="sidebar-scroll min-h-0 flex-1 overflow-y-auto px-2 pb-4"
        data-testid="sidebar-nav"
      >
        <div className="space-y-0.5">{filteredMenuItems.map((item) => renderMenuItem(item))}</div>
      </nav>

      <div className="mx-4 mb-2 h-px bg-sidebar-border" />
      <div className="px-4 pb-4">
        <LanguageSwitcher />
        <div className="mt-2 flex items-center justify-between" data-testid="sidebar-footer-meta">
          <span
            className="text-xs font-medium text-sidebar-foreground/40"
            data-testid="sidebar-version"
          >
            v{APP_VERSION}
          </span>
          <a
            href={GITHUB_REPO_URL}
            target="_blank"
            rel="noopener noreferrer"
            aria-label={m['nav.github_repo']()}
            className="inline-flex items-center rounded-md text-sidebar-foreground/40 transition-colors hover:text-sidebar-foreground"
            data-testid="sidebar-github-link"
          >
            <Github className="size-3.5" />
          </a>
        </div>
      </div>
    </div>
  )
}
