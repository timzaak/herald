import { createFileRoute } from '@tanstack/react-router'
import {
  SubscriptionsRoute,
  subscriptionsSearchSchema,
} from '@/routes/$realmId/manage/billing/subscriptions'
import { manageFeatureGuard } from '@/lib/manage-feature-guard'

export const Route = createFileRoute('/manage/billing/subscriptions')({
  beforeLoad: manageFeatureGuard((f) => f.admin.entitlementMappingsVisible),
  validateSearch: subscriptionsSearchSchema,
  component: SubscriptionsRoute,
})
