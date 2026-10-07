import { createFileRoute } from '@tanstack/react-router'
import { apiKeysSearchSchema } from '@/lib/schemas/search-params'
import { ApiKeysPage } from '@/routes/$realmId/manage/api-keys/index'

export const Route = createFileRoute('/manage/api-keys/')({
  validateSearch: apiKeysSearchSchema,
  component: ApiKeysPage,
})
