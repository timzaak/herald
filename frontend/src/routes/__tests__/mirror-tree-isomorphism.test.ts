/**
 * @vitest-environment jsdom
 */
import { describe, expect, it } from 'vitest'

import { Route as RealUsersRoute } from '../$realmId/manage/users'
import { Route as RealRealmsRoute } from '../$realmId/manage/realms'
import { Route as RealRolesRoute } from '../$realmId/manage/roles'
import { Route as RealPermissionsRoute } from '../$realmId/manage/permissions'
import { Route as RealSettingsRoute } from '../$realmId/manage/settings'
import { Route as RealAuditIndexRoute } from '../$realmId/manage/audit/index'
import { Route as RealApiKeysIndexRoute } from '../$realmId/manage/api-keys/index'
import { Route as RealClientAppsIndexRoute } from '../$realmId/manage/client-apps/index'
import { Route as RealStatisticsRoute } from '../$realmId/manage/billing/statistics'
import { Route as RealSubscriptionsRoute } from '../$realmId/manage/billing/subscriptions'
import { Route as RealCreditBucketsRoute } from '../$realmId/manage/billing/credit-buckets'
import { Route as RealCreditBucketsIndexRoute } from '../$realmId/manage/billing/credit-buckets/index'
import { Route as RealCreditBucketsOverviewRoute } from '../$realmId/manage/billing/credit-buckets/overview'
import { Route as RealEntitlementMappingsRoute } from '../$realmId/manage/billing/entitlement-mappings'
import { Route as RealEntitlementMappingsIndexRoute } from '../$realmId/manage/billing/entitlement-mappings/index'
import { Route as RealInvoicesRoute } from '../$realmId/manage/billing/invoices'
import { Route as RealInvoicesIndexRoute } from '../$realmId/manage/billing/invoices/index'
import { Route as RealPaymentProvidersRoute } from '../$realmId/manage/billing/payment-providers'
import { Route as RealPointsRoute } from '../$realmId/manage/points'
import { Route as RealManageSubscriptionHistoryRoute } from '../$realmId/manage/subscription-history'
import { Route as RealUserPointsRoute } from '../$realmId/user/points'
import { Route as RealPurchasePointsRoute } from '../$realmId/user/purchase-points'
import { Route as RealUserSubscriptionHistoryRoute } from '../$realmId/user/subscription-history'
import { Route as RealUserInvoicesRoute } from '../$realmId/user/invoices'
import { Route as RealMySubscriptionsRoute } from '../$realmId/subscription/my-subscriptions'

import { Route as MirrorUsersRoute } from '../manage/users'
import { Route as MirrorRealmsRoute } from '../manage/realms'
import { Route as MirrorRolesRoute } from '../manage/roles'
import { Route as MirrorPermissionsRoute } from '../manage/permissions'
import { Route as MirrorSettingsRoute } from '../manage/settings'
import { Route as MirrorAuditIndexRoute } from '../manage/audit/index'
import { Route as MirrorApiKeysIndexRoute } from '../manage/api-keys/index'
import { Route as MirrorClientAppsIndexRoute } from '../manage/client-apps/index'
import { Route as MirrorStatisticsRoute } from '../manage/billing/statistics'
import { Route as MirrorSubscriptionsRoute } from '../manage/billing/subscriptions'
import { Route as MirrorCreditBucketsRoute } from '../manage/billing/credit-buckets'
import { Route as MirrorCreditBucketsIndexRoute } from '../manage/billing/credit-buckets/index'
import { Route as MirrorCreditBucketsOverviewRoute } from '../manage/billing/credit-buckets/overview'
import { Route as MirrorEntitlementMappingsRoute } from '../manage/billing/entitlement-mappings'
import { Route as MirrorEntitlementMappingsIndexRoute } from '../manage/billing/entitlement-mappings/index'
import { Route as MirrorInvoicesRoute } from '../manage/billing/invoices'
import { Route as MirrorInvoicesIndexRoute } from '../manage/billing/invoices/index'
import { Route as MirrorPaymentProvidersRoute } from '../manage/billing/payment-providers'
import { Route as MirrorPointsRoute } from '../manage/points'
import { Route as MirrorManageSubscriptionHistoryRoute } from '../manage/subscription-history'
import { Route as MirrorUserPointsRoute } from '../user/points'
import { Route as MirrorPurchasePointsRoute } from '../user/purchase-points'
import { Route as MirrorUserSubscriptionHistoryRoute } from '../user/subscription-history'
import { Route as MirrorUserInvoicesRoute } from '../user/invoices'
import { Route as MirrorMySubscriptionsRoute } from '../subscription/my-subscriptions'

/**
 * Guard-scheme isomorphism between the `$realmId/**` real tree and the
 * session-scoped mirror tree (frontend/README.md route-tree conventions):
 * every `validateSearch` and `beforeLoad` feature gate the real tree
 * declares must also be declared by its mirror alias, so the same page
 * cannot enforce different URL parsing or feature gating depending on
 * which URL form (/acme/manage/... vs /manage/...) it was visited
 * through.
 */
const MIRROR_PAIRS = [
  { path: 'manage/users', real: RealUsersRoute, mirror: MirrorUsersRoute },
  { path: 'manage/realms', real: RealRealmsRoute, mirror: MirrorRealmsRoute },
  { path: 'manage/roles', real: RealRolesRoute, mirror: MirrorRolesRoute },
  { path: 'manage/permissions', real: RealPermissionsRoute, mirror: MirrorPermissionsRoute },
  { path: 'manage/settings', real: RealSettingsRoute, mirror: MirrorSettingsRoute },
  { path: 'manage/audit/index', real: RealAuditIndexRoute, mirror: MirrorAuditIndexRoute },
  { path: 'manage/api-keys/index', real: RealApiKeysIndexRoute, mirror: MirrorApiKeysIndexRoute },
  {
    path: 'manage/client-apps/index',
    real: RealClientAppsIndexRoute,
    mirror: MirrorClientAppsIndexRoute,
  },
  { path: 'manage/billing/statistics', real: RealStatisticsRoute, mirror: MirrorStatisticsRoute },
  {
    path: 'manage/billing/subscriptions',
    real: RealSubscriptionsRoute,
    mirror: MirrorSubscriptionsRoute,
  },
  {
    path: 'manage/billing/credit-buckets',
    real: RealCreditBucketsRoute,
    mirror: MirrorCreditBucketsRoute,
  },
  {
    path: 'manage/billing/credit-buckets/index',
    real: RealCreditBucketsIndexRoute,
    mirror: MirrorCreditBucketsIndexRoute,
  },
  {
    path: 'manage/billing/credit-buckets/overview',
    real: RealCreditBucketsOverviewRoute,
    mirror: MirrorCreditBucketsOverviewRoute,
  },
  {
    path: 'manage/billing/entitlement-mappings',
    real: RealEntitlementMappingsRoute,
    mirror: MirrorEntitlementMappingsRoute,
  },
  {
    path: 'manage/billing/entitlement-mappings/index',
    real: RealEntitlementMappingsIndexRoute,
    mirror: MirrorEntitlementMappingsIndexRoute,
  },
  {
    path: 'manage/billing/invoices',
    real: RealInvoicesRoute,
    mirror: MirrorInvoicesRoute,
  },
  {
    path: 'manage/billing/invoices/index',
    real: RealInvoicesIndexRoute,
    mirror: MirrorInvoicesIndexRoute,
  },
  {
    path: 'manage/billing/payment-providers',
    real: RealPaymentProvidersRoute,
    mirror: MirrorPaymentProvidersRoute,
  },
  { path: 'manage/points', real: RealPointsRoute, mirror: MirrorPointsRoute },
  {
    path: 'manage/subscription-history',
    real: RealManageSubscriptionHistoryRoute,
    mirror: MirrorManageSubscriptionHistoryRoute,
  },
  { path: 'user/points', real: RealUserPointsRoute, mirror: MirrorUserPointsRoute },
  {
    path: 'user/purchase-points',
    real: RealPurchasePointsRoute,
    mirror: MirrorPurchasePointsRoute,
  },
  {
    path: 'user/subscription-history',
    real: RealUserSubscriptionHistoryRoute,
    mirror: MirrorUserSubscriptionHistoryRoute,
  },
  { path: 'user/invoices', real: RealUserInvoicesRoute, mirror: MirrorUserInvoicesRoute },
  {
    path: 'subscription/my-subscriptions',
    real: RealMySubscriptionsRoute,
    mirror: MirrorMySubscriptionsRoute,
  },
] as const

type RouteOptions = { validateSearch?: unknown; beforeLoad?: unknown }

function optionsOf(route: unknown): RouteOptions {
  // createFileRoute nests the declared config under `.options`.
  return (route as { options?: RouteOptions }).options ?? {}
}

describe('mirror tree stays isomorphic with the $realmId tree', () => {
  it.each(MIRROR_PAIRS)('$path: mirror declares every guard the real tree declares', (pair) => {
    const real = optionsOf(pair.real)
    const mirror = optionsOf(pair.mirror)

    if (real.validateSearch !== undefined) {
      expect(mirror.validateSearch, `${pair.path}: mirror lacks validateSearch`).toBeDefined()
    }
    if (real.beforeLoad !== undefined) {
      expect(mirror.beforeLoad, `${pair.path}: mirror lacks beforeLoad`).toBeDefined()
    }
  })
})
