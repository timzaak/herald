import { createFileRoute } from '@tanstack/react-router'
import { MySubscriptionsPage } from '@/components/billing/my-subscriptions-page'
import { useResolvedRealmId } from '@/lib/realm-routing'

export const Route = createFileRoute('/$realmId/user/my-subscriptions/')({
  component: MySubscriptionsIndexRoute,
})

// Resolves the realm from the URL/session context (not Route.useParams) so
// the session-scoped mirror route (`/user/my-subscriptions`) can reuse this
// component — the sibling user-tree pages follow the same convention (see
// frontend/README.md route-tree conventions).
export function MySubscriptionsIndexRoute() {
  const realmId = useResolvedRealmId()

  return <MySubscriptionsPage realmId={realmId} />
}
