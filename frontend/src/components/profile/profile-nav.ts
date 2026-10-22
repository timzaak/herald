import { User, ShieldCheck, CreditCard, Coins, History, FileText } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import { useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useRealmId } from '@/stores/auth-store'
import { userFeatureAvailabilityQueryOptions } from '@/data/query-options'
import { m } from '@/paraglide/messages'
import { realmPath, useResolvedRealmContext } from '@/lib/realm-routing'
import type { ResolvedRealmContext } from '@/lib/realm-routing'

/** Menu sections of the account center. `account` always exists; `billing`
 * entries follow the realm's feature availability. */
export type ProfileMenuGroup = 'account' | 'billing'

export interface ProfileMenuItem {
  /** Uppercase name kept stable for the `profile-menu-*` / `profile-hub-menu-*` testids. */
  name: string
  group: ProfileMenuGroup
  path: string
  icon: LucideIcon
  /** Carried on the item so adding a section never needs a parallel name→label map. */
  label: () => string
  visible?: boolean
}

export interface UserFeatureVisibility {
  subscriptionVisible?: boolean
  pointsVisible?: boolean
  invoicesVisible?: boolean
}

export const PROFILE_MENU_GROUP_ORDER: readonly ProfileMenuGroup[] = ['account', 'billing']

/**
 * Single source of the account-center menu: item order, feature gates and
 * grouping are shared by the desktop sidebar and the mobile hub so the two
 * navigation surfaces can never drift apart.
 */
export function buildProfileMenuItems(
  realmContext: ResolvedRealmContext,
  realmId: string,
  userFeatures?: UserFeatureVisibility
): ProfileMenuItem[] {
  const to = (path: string) => realmPath({ ...realmContext, realmId }, path)
  return [
    {
      name: 'Profile',
      group: 'account',
      path: to('/user/profile'),
      icon: User,
      label: m['nav_profile.profile'],
    },
    {
      name: 'Security',
      group: 'account',
      path: to('/user/security'),
      icon: ShieldCheck,
      label: m['nav_profile.security'],
    },
    {
      name: 'Subscription',
      group: 'billing',
      path: to('/user/my-subscriptions'),
      icon: CreditCard,
      label: m['nav_profile.subscription'],
      // US-BI-009 scenario 0: the entry follows the realm's subscription
      // capability (enabled entitlement mappings), not mere history records.
      visible: userFeatures?.subscriptionVisible === true,
    },
    {
      name: 'Points',
      group: 'billing',
      path: to('/user/points'),
      icon: Coins,
      label: m['nav_profile.points'],
      visible: userFeatures?.pointsVisible === true,
    },
    {
      name: 'PurchaseRecords',
      group: 'billing',
      path: to('/user/subscription-history'),
      icon: History,
      label: m['nav_profile.purchase_records'],
      visible: userFeatures?.pointsVisible === true,
    },
    {
      name: 'Invoices',
      group: 'billing',
      path: to('/user/invoices'),
      icon: FileText,
      label: m['nav_profile.invoices'],
      visible: userFeatures?.invoicesVisible === true,
    },
  ]
}

/**
 * Shared preamble of every account-center navigation surface (desktop
 * sidebar, mobile hub, mobile section header): resolve the realm from the
 * URL, load the feature gates and build the menu from the single source.
 */
export function useProfileMenu() {
  const realmContext = useResolvedRealmContext()
  const storeRealmId = useRealmId()
  const realmId = realmContext.realmId || storeRealmId || 'admin'
  const { data: features } = useQuery(userFeatureAvailabilityQueryOptions)
  const userFeatures = features?.user

  const menuItems = useMemo(
    () => buildProfileMenuItems(realmContext, realmId, userFeatures),
    [realmContext, realmId, userFeatures]
  )
  return { realmContext, realmId, menuItems }
}
