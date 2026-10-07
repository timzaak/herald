import { createFileRoute } from '@tanstack/react-router'
import { PurchaseRecordsRoute } from '@/routes/$realmId/user/subscription-history'
import { userFeatureGuard } from '@/lib/user-feature-guard'

export const Route = createFileRoute('/user/subscription-history')({
  beforeLoad: userFeatureGuard((f) => f.user.pointsVisible),
  component: PurchaseRecordsRoute,
})
