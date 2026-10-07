import { createFileRoute } from '@tanstack/react-router'
import { MySubscriptionsPage } from '@/components/billing/my-subscriptions-page'
import { requireUserFeature } from '@/data/query-options'
import { initializeAuth } from '@/lib/auth-utils'
import { USER_ACCOUNT_CENTER_CLIENT_ID } from '@/lib/constants/auth-constants'
import { useResolvedRealmId } from '@/lib/realm-routing'

export const Route = createFileRoute('/$realmId/subscription/my-subscriptions')({
  beforeLoad: async ({ context, params }) => {
    // Route beforeLoads run ahead of the __root loader, so on a cold reload
    // the feature query below would fire before initializeAuth restores the
    // Bearer token (401 → route error boundary). Idempotent: short-circuits
    // once the realm/client is initialized.
    await initializeAuth(params.realmId, USER_ACCOUNT_CENTER_CLIENT_ID)
    await requireUserFeature(context.queryClient, (f) => f.user.subscriptionVisible, {
      to: '/$realmId/user/profile',
      params: { realmId: params.realmId },
    })
  },
  component: MySubscriptionsRoute,
})

// Resolves the realm from the URL/session context (not Route.useParams) so
// the session-scoped mirror route (`/subscription/my-subscriptions`) can
// reuse this component — the sibling user-tree pages follow the same
// convention (see frontend/README.md route-tree conventions).
export function MySubscriptionsRoute() {
  const realmId = useResolvedRealmId()

  return <MySubscriptionsPage realmId={realmId} />
}
