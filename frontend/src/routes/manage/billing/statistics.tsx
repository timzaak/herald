import { createFileRoute } from '@tanstack/react-router'
import { StatisticsRoute } from '@/routes/$realmId/manage/billing/statistics'

export const Route = createFileRoute('/manage/billing/statistics')({
  component: StatisticsRoute,
})
