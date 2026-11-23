import { createFileRoute } from '@tanstack/react-router'
import { RevealWebhookSecretPage } from '@/routes/$realmId/manage/webhooks.reveal'

export const Route = createFileRoute('/manage/webhooks/reveal')({
  component: RevealWebhookSecretPage,
})
