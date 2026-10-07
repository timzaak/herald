import { createFileRoute } from '@tanstack/react-router'
import { transactionBucketSearchSchema } from '@/lib/schemas/points-forms'
import { UserPointsWrapper } from '@/routes/$realmId/user/points'
import { userFeatureGuard } from '@/lib/user-feature-guard'

export const Route = createFileRoute('/user/points')({
  beforeLoad: userFeatureGuard((f) => f.user.pointsVisible),
  validateSearch: transactionBucketSearchSchema,
  component: UserPointsWrapper,
})
