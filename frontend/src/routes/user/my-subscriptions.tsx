import { createFileRoute, Outlet } from '@tanstack/react-router'
import { userFeatureGuard } from '@/lib/user-feature-guard'

export const Route = createFileRoute('/user/my-subscriptions')({
  beforeLoad: userFeatureGuard((f) => f.user.subscriptionVisible),
  component: Outlet,
})
