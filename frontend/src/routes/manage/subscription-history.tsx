import { createFileRoute } from '@tanstack/react-router'
import { SubscriptionHistoryRoute } from '@/routes/$realmId/manage/subscription-history'
import { manageFeatureGuard } from '@/lib/manage-feature-guard'

export const Route = createFileRoute('/manage/subscription-history')({
  beforeLoad: manageFeatureGuard((f) => f.admin.subscriptionHistoryVisible),
  component: SubscriptionHistoryRoute,
})
