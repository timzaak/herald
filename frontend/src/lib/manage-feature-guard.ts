import type { QueryClient } from '@tanstack/react-query'
import { requireFeature, type FeatureAvailabilityResponse } from '@/data/query-options'
import { initializeAuth } from '@/lib/auth-utils'
import { ADMIN_WEB_CONSOLE_CLIENT_ID } from '@/lib/constants/auth-constants'
import { useAuthStore } from '@/stores/auth-store'

/**
 * beforeLoad feature gate for session-scoped mirror routes (`/manage/**`).
 *
 * The mirror tree mounts the same components as the `$realmId` tree but its
 * URL carries no realm — the realm comes from the session store. Realms'
 * feature gating must stay isomorphic across both route forms (see
 * frontend/README.md route-tree conventions): a feature-hidden page redirects
 * to `/manage` regardless of which URL form is visited. Route beforeLoads run
 * ahead of the __root loader, so initializeAuth runs here first (idempotent).
 */
export function manageFeatureGuard(
  check: (features: FeatureAvailabilityResponse) => boolean,
  redirectSearch?: Record<string, unknown>
) {
  return async ({ context }: { context: { queryClient: QueryClient } }) => {
    const realmId = useAuthStore.getState().realmId || 'admin'
    await initializeAuth(realmId, ADMIN_WEB_CONSOLE_CLIENT_ID)
    await requireFeature(context.queryClient, realmId, check, {
      to: '/manage',
      ...(redirectSearch ? { search: redirectSearch } : {}),
    })
  }
}
