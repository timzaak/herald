import type { QueryClient } from '@tanstack/react-query'
import type { UserFeatureAvailabilityResponse } from '@/lib/api-generated'
import { requireUserFeature } from '@/data/query-options'
import { initializeAuth } from '@/lib/auth-utils'
import { USER_ACCOUNT_CENTER_CLIENT_ID } from '@/lib/constants/auth-constants'
import { useAuthStore } from '@/stores/auth-store'

/**
 * beforeLoad feature gate for session-scoped mirror routes (`/user/**`,
 * `/subscription/**`).
 *
 * The mirror tree mounts the same components as the `$realmId` tree but its
 * URL carries no realm — the realm comes from the session store. Feature
 * gating must stay isomorphic across both route forms (see frontend/README.md
 * route-tree conventions): a feature-hidden page redirects to the user
 * profile (or another user-facing fallback) regardless of which URL form is
 * visited. Route beforeLoads run ahead of the __root loader, so
 * initializeAuth runs here first (idempotent).
 */
export function userFeatureGuard(
  check: (features: UserFeatureAvailabilityResponse) => boolean,
  redirectTo: '/user/profile' | '/user/points' = '/user/profile'
) {
  return async ({ context }: { context: { queryClient: QueryClient } }) => {
    const realmId = useAuthStore.getState().realmId || 'admin'
    await initializeAuth(realmId, USER_ACCOUNT_CENTER_CLIENT_ID)
    await requireUserFeature(context.queryClient, check, { to: redirectTo })
  }
}
