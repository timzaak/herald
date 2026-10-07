import type { QueryClient } from '@tanstack/react-query'
import { requireFeature, type FeatureAvailabilityResponse } from '@/data/query-options'
import { initializeAuth } from '@/lib/auth-utils'
import { ADMIN_WEB_CONSOLE_CLIENT_ID } from '@/lib/constants/auth-constants'
import { useAuthStore } from '@/stores/auth-store'

/**
 * beforeLoad feature gate for `/manage/**` mirror routes and their
 * `/$realmId/manage/**` real-tree twins (same page, two URL forms).
 *
 * In the real tree the URL supplies the realm (`params.realmId`) and a
 * feature-hidden page redirects to `/$realmId/manage`; in the mirror tree the
 * realm comes from the session store and the redirect targets `/manage`.
 * Feature gating must stay isomorphic across both route forms (see
 * frontend/README.md route-tree conventions). Route beforeLoads run ahead of
 * the __root loader, so initializeAuth runs here first (idempotent).
 */
export function manageFeatureGuard(
  check: (features: FeatureAvailabilityResponse) => boolean,
  redirectSearch?: Record<string, unknown>
) {
  return async ({
    context,
    params,
  }: {
    context: { queryClient: QueryClient }
    params?: { realmId?: string }
  }) => {
    const realmId = params?.realmId || useAuthStore.getState().realmId || 'admin'
    await initializeAuth(realmId, ADMIN_WEB_CONSOLE_CLIENT_ID)
    await requireFeature(
      context.queryClient,
      realmId,
      check,
      params?.realmId
        ? { to: '/$realmId/manage', params: { realmId: params.realmId } }
        : { to: '/manage', ...(redirectSearch ? { search: redirectSearch } : {}) }
    )
  }
}
