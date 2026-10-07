import { createFileRoute } from '@tanstack/react-router'
import {
  StatisticsRoute,
  statisticsSearchSchema,
} from '@/routes/$realmId/manage/billing/statistics'

export const Route = createFileRoute('/manage/billing/statistics')({
  validateSearch: statisticsSearchSchema,
  component: StatisticsRoute,
})
