import { createFileRoute } from '@tanstack/react-router'
import { purchasePointsSearchSchema } from '@/lib/schemas/search-params'
import { PurchasePointsRoute } from '@/routes/$realmId/user/purchase-points'
import { userFeatureGuard } from '@/lib/user-feature-guard'

export const Route = createFileRoute('/user/purchase-points')({
  beforeLoad: userFeatureGuard((f) => f.user.pointsVisible, '/user/points'),
  validateSearch: purchasePointsSearchSchema,
  component: PurchasePointsRoute,
})
