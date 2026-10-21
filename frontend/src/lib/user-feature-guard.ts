import type { QueryClient } from '@tanstack/react-query'
import type { UserFeatureAvailabilityResponse } from '@/lib/api-generated'
import { requireUserFeature } from '@/data/query-options'
import { initializeAuth } from '@/lib/auth-utils'
import { USER_ACCOUNT_CENTER_CLIENT_ID } from '@/lib/constants/auth-constants'
import { realmPath, type ResolvedRealmContext } from '@/lib/realm-routing'

/**
 * beforeLoad feature gate for custom-domain mirror routes (`/user/**`,
 * `/subscription/**`).
 *
 * The mirror tree mounts the same components as the `$realmId` tree but its
 * URL carries no realm — the realm comes from the root's Host context. Feature
 * gating must stay isomorphic across both route forms (see frontend/README.md
 * route-tree conventions): a feature-hidden page redirects to the user
 * profile (or another user-facing fallback) regardless of which URL form is
 * visited. Authentication is already restored by the root beforeLoad.
 */
export function userFeatureGuard(
  check: (features: UserFeatureAvailabilityResponse) => boolean,
  redirectTo: '/user/profile' | '/user/points' = '/user/profile'
) {
  return async ({
    context,
  }: {
    context: { queryClient: QueryClient; realmContext: ResolvedRealmContext }
  }) => {
    const realmId = context.realmContext.realmId
    await initializeAuth(realmId, USER_ACCOUNT_CENTER_CLIENT_ID)
    await requireUserFeature(context.queryClient, check, {
      to: realmPath(context.realmContext, redirectTo),
    })
  }
}
