import { createFileRoute } from '@tanstack/react-router'
import { BillingStatisticsPage } from '@/components/billing/statistics/billing-statistics-page'
import { useResolvedRealmId } from '@/lib/realm-routing'

export const Route = createFileRoute('/$realmId/manage/billing/statistics')({
  component: StatisticsRoute,
})

export function StatisticsRoute() {
  const realmId = useResolvedRealmId()

  return <BillingStatisticsPage realmId={realmId} />
}
