import { createFileRoute } from '@tanstack/react-router'
import { webhooksSearchSchema } from '@/lib/schemas/search-params'
import { WebhooksPage } from '@/routes/$realmId/manage/webhooks.index'

export const Route = createFileRoute('/manage/webhooks/')({
  validateSearch: webhooksSearchSchema,
  component: WebhooksPage,
})
