import { createFileRoute } from '@tanstack/react-router'
import {
  CreditBucketOverviewRoute,
  overviewSearchSchema,
} from '@/routes/$realmId/manage/billing/credit-buckets/overview'

export const Route = createFileRoute('/manage/billing/credit-buckets/overview')({
  validateSearch: overviewSearchSchema,
  component: CreditBucketOverviewRoute,
})
