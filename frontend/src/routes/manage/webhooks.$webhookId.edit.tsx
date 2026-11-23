import { createFileRoute } from '@tanstack/react-router'
import { EditWebhookPage } from '@/routes/$realmId/manage/webhooks.$webhookId.edit'

export const Route = createFileRoute('/manage/webhooks/$webhookId/edit')({
  component: EditWebhookPage,
})
