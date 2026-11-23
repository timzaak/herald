import { createFileRoute } from '@tanstack/react-router'
import { lazy, Suspense } from 'react'
import { SpinnerFallback } from '@/components/shared'
import { useResolvedRealmId } from '@/lib/realm-routing'

const WebhookSubscriptionFormPage = lazy(() =>
  import('@/components/webhook-subscription-form-page').then((m) => ({
    default: m.WebhookSubscriptionFormPage,
  }))
)

export const Route = createFileRoute('/$realmId/manage/webhooks/new')({
  component: NewWebhookPage,
})

export function NewWebhookPage() {
  const realmId = useResolvedRealmId()

  return (
    <div className="container mx-auto max-w-4xl px-6 py-6">
      <Suspense fallback={<SpinnerFallback dataTestId="webhook-form-loading" />}>
        <WebhookSubscriptionFormPage mode="create" realmId={realmId} />
      </Suspense>
    </div>
  )
}
