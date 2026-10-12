import { createFileRoute } from '@tanstack/react-router'
import { MySubscriptionsIndexRoute } from '@/routes/$realmId/user/my-subscriptions/'

export const Route = createFileRoute('/user/my-subscriptions/')({
  component: MySubscriptionsIndexRoute,
})
