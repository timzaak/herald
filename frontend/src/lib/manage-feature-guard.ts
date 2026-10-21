import type { QueryClient } from '@tanstack/react-query'
import { requireFeature, type FeatureAvailabilityResponse } from '@/data/query-options'
import { initializeAuth } from '@/lib/auth-utils'
import { ADMIN_WEB_CONSOLE_CLIENT_ID } from '@/lib/constants/auth-constants'
import { realmPath, type ResolvedRealmContext } from '@/lib/realm-routing'

/**
 * beforeLoad feature gate for `/manage/**` mirror routes and their
 * `/$realmId/manage/**` real-tree twins (same page, two URL forms).
 *
 * Both trees consume the Host/URL context established by the root beforeLoad.
 * Main-domain redirects retain the realm; custom-domain redirects stay bare.
 */
export function manageFeatureGuard(
  check: (features: FeatureAvailabilityResponse) => boolean,
  redirectSearch?: Record<string, unknown>
) {
  return async ({
    context,
    params,
  }: {
    context: { queryClient: QueryClient; realmContext: ResolvedRealmContext }
    params?: { realmId?: string }
  }) => {
    const { realmContext } = context
    const realmId = realmContext.realmId
    await initializeAuth(realmId, ADMIN_WEB_CONSOLE_CLIENT_ID)
    await requireFeature(
      context.queryClient,
      realmId,
      check,
      params?.realmId && !realmContext.isCustomDomain
        ? { to: '/$realmId/manage', params: { realmId: params.realmId } }
        : {
            to: realmPath(realmContext, '/manage'),
            ...(redirectSearch ? { search: redirectSearch } : {}),
          }
    )
  }
}
