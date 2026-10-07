import { createFileRoute } from '@tanstack/react-router'
import { MySubscriptionsRoute } from '@/routes/$realmId/subscription/my-subscriptions'
import { userFeatureGuard } from '@/lib/user-feature-guard'

export const Route = createFileRoute('/subscription/my-subscriptions')({
  beforeLoad: userFeatureGuard((f) => f.user.subscriptionVisible),
  component: MySubscriptionsRoute,
})
