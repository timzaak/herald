import { createFileRoute } from '@tanstack/react-router'
import { lazy, Suspense } from 'react'
import { SpinnerFallback } from '@/components/shared'
import { useResolvedRealmId } from '@/lib/realm-routing'

const WebhookSecretRevealPage = lazy(() =>
  import('@/components/webhook-secret-reveal-page').then((m) => ({
    default: m.WebhookSecretRevealPage,
  }))
)

export const Route = createFileRoute('/$realmId/manage/webhooks/reveal')({
  component: RevealWebhookSecretPage,
})

export function RevealWebhookSecretPage() {
  const realmId = useResolvedRealmId()

  return (
    <div className="container mx-auto max-w-4xl px-6 py-6">
      <Suspense fallback={<SpinnerFallback dataTestId="webhook-reveal-loading" />}>
        <WebhookSecretRevealPage realmId={realmId} />
      </Suspense>
    </div>
  )
}
