import { createFileRoute } from '@tanstack/react-router'
import { MySubscriptionsPage } from '@/components/billing/my-subscriptions-page'
import { requireUserFeature } from '@/data/query-options'
import { initializeAuth } from '@/lib/auth-utils'
import { USER_ACCOUNT_CENTER_CLIENT_ID } from '@/lib/constants/auth-constants'

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

function MySubscriptionsRoute() {
  const { realmId } = Route.useParams()

  return <MySubscriptionsPage realmId={realmId} />
}
