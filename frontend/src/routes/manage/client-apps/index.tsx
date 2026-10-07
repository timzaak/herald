import { createFileRoute } from '@tanstack/react-router'
import { clientAppsSearchSchema } from '@/lib/schemas/search-params'
import { ClientAppsPage } from '@/routes/$realmId/manage/client-apps/index'

export const Route = createFileRoute('/manage/client-apps/')({
  validateSearch: clientAppsSearchSchema,
  component: ClientAppsPage,
})
