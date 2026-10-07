import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { z } from 'zod'
import { CreditBucketOverviewPage } from '@/components/billing/credit-bucket/credit-bucket-overview-page'
import { realmPath, useCurrentSearch, useResolvedRealmContext } from '@/lib/realm-routing'

const creditTypeKeySchema = z.enum([
  'topup',
  'subscription',
  'registration',
  'freePeriodic',
  'granted',
])

// eslint-disable-next-line react-refresh/only-export-components
export const overviewSearchSchema = z.object({
  enabledOnly: z.boolean().optional(),
  creditTypes: z.array(creditTypeKeySchema).optional(),
})

export type CreditTypeKey = z.infer<typeof creditTypeKeySchema>

export type OverviewSearch = z.infer<typeof overviewSearchSchema>

export const Route = createFileRoute('/$realmId/manage/billing/credit-buckets/overview')({
  validateSearch: overviewSearchSchema,
  component: CreditBucketOverviewRoute,
})

export function CreditBucketOverviewRoute() {
  const realmContext = useResolvedRealmContext()
  const realmId = realmContext.realmId
  const search = useCurrentSearch<OverviewSearch>()
  const navigate = useNavigate()

  function handleSearchChange(next: OverviewSearch) {
    navigate({
      to: realmPath(realmContext, '/manage/billing/credit-buckets/overview'),
      search: () => ({
        enabledOnly: next.enabledOnly,
        creditTypes: next.creditTypes,
      }),
      replace: true,
    })
  }

  return (
    <CreditBucketOverviewPage
      realmId={realmId}
      search={search}
      onSearchChange={handleSearchChange}
    />
  )
}
