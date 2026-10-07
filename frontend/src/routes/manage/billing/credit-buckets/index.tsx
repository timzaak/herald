import { createFileRoute } from '@tanstack/react-router'
import {
  CreditBucketsIndexRoute,
  creditBucketsSearchSchema,
} from '@/routes/$realmId/manage/billing/credit-buckets/index'

export const Route = createFileRoute('/manage/billing/credit-buckets/')({
  validateSearch: creditBucketsSearchSchema,
  component: CreditBucketsIndexRoute,
})
