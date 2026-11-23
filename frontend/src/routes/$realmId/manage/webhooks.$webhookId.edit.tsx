import { createFileRoute } from '@tanstack/react-router'
import { useSuspenseQuery } from '@tanstack/react-query'
import { lazy, Suspense } from 'react'
import { SpinnerFallback } from '@/components/shared'
import { webhookQueryOptions } from '@/data/query-options'
import { useLastPathSegment, useResolvedRealmId } from '@/lib/realm-routing'

const WebhookSubscriptionFormPage = lazy(() =>
  import('@/components/webhook-subscription-form-page').then((m) => ({
    default: m.WebhookSubscriptionFormPage,
  }))
)

export const Route = createFileRoute('/$realmId/manage/webhooks/$webhookId/edit')({
  component: EditWebhookPage,
})

export function EditWebhookPage() {
  const realmId = useResolvedRealmId()
  const webhookId = useLastPathSegment(1)
  const { data: webhook } = useSuspenseQuery(webhookQueryOptions(realmId, webhookId))

  return (
    <div className="container mx-auto max-w-4xl px-6 py-6">
      <Suspense fallback={<SpinnerFallback dataTestId="webhook-form-loading" />}>
        <WebhookSubscriptionFormPage mode="edit" realmId={realmId} webhook={webhook} />
      </Suspense>
    </div>
  )
}
