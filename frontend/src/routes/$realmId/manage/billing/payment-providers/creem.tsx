import { createFileRoute } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { lazy, Suspense } from 'react'
import { SpinnerFallback } from '@/components/shared'
import { listPaymentProviders } from '@/lib/api-generated'
import { listRealmConfigs } from '@/lib/api-generated/sdk.gen'
import { parseCreemConfig } from '@/lib/creem-config-utils'
import type { CreemConfigForm } from '@/lib/schemas/creem-config'
import { useResolvedRealmId } from '@/lib/realm-routing'

const CreemConfigFormPage = lazy(() =>
  import('@/components/billing/CreemConfigForm').then((m) => ({ default: m.CreemConfigFormPage }))
)

export const Route = createFileRoute('/$realmId/manage/billing/payment-providers/creem')({
  component: CreemConfigRoute,
})

export function CreemConfigRoute() {
  const realmId = useResolvedRealmId()

  const { data: providers, isLoading } = useQuery({
    queryKey: ['payment-providers', realmId],
    queryFn: async () => {
      const result = await listPaymentProviders({})
      return result.data?.providers ?? []
    },
  })

  const creemProvider = providers?.find((p) => p.platform === 'creem')
  const mode = creemProvider ? 'edit' : 'create'

  const { data: configData } = useQuery({
    queryKey: ['creem-config', realmId],
    queryFn: async () => {
      if (!creemProvider) return null
      const result = await listRealmConfigs({})
      return parseCreemConfig(result.data ?? [])
    },
    enabled: !!creemProvider,
  })

  const initialValues: Partial<CreemConfigForm> | undefined = configData
    ? {
        apiKey: '',
        timeout: configData.timeout,
        webhookSecret: '',
      }
    : undefined

  if (isLoading) {
    return <SpinnerFallback />
  }

  return (
    <Suspense fallback={<SpinnerFallback />}>
      <CreemConfigFormPage realmId={realmId} mode={mode} initialValues={initialValues} />
    </Suspense>
  )
}
