import { createFileRoute } from '@tanstack/react-router'
import { NewWebhookPage } from '@/routes/$realmId/manage/webhooks.new'

export const Route = createFileRoute('/manage/webhooks/new')({
  component: NewWebhookPage,
})
