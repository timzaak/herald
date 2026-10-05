import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { z } from 'zod'
import { BillingStatisticsPage } from '@/components/billing/statistics/billing-statistics-page'
import { useCurrentSearch, useResolvedRealmId } from '@/lib/realm-routing'

// The statistics view (section tab + stats window) lives in the querystring
// so a refresh or shared deep link restores the exact view.
const statisticsSearchSchema = z.object({
  tab: z.enum(['payment', 'points']).optional(),
  days: z.union([z.literal(7), z.literal(30)]).optional(),
})

export const Route = createFileRoute('/$realmId/manage/billing/statistics')({
  validateSearch: statisticsSearchSchema,
  component: StatisticsRoute,
})

export function StatisticsRoute() {
  const realmId = useResolvedRealmId()
  const search = useCurrentSearch<z.infer<typeof statisticsSearchSchema>>()
  const navigate = useNavigate()

  // `to: '.'` keeps the update on the CURRENT route match — the family
  // convention for manage pages (see manage/users, manage/realms).
  function handleSearchChange(patch: Partial<z.infer<typeof statisticsSearchSchema>>) {
    navigate({ to: '.', search: (prev) => ({ ...prev, ...patch }) })
  }

  return (
    <BillingStatisticsPage realmId={realmId} search={search} onSearchChange={handleSearchChange} />
  )
}
