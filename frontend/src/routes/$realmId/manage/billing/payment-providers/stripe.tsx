import { createFileRoute } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { lazy, Suspense } from 'react'
import { SpinnerFallback } from '@/components/shared'
import { listPaymentProviders } from '@/lib/api-generated'
import { listRealmConfigs } from '@/lib/api-generated/sdk.gen'
import { parseStripeConfig } from '@/lib/stripe-config-utils'
import type { StripeConfigForm } from '@/lib/schemas/stripe-config'
import { useResolvedRealmId } from '@/lib/realm-routing'

const StripeConfigFormPage = lazy(() =>
  import('@/components/billing/StripeConfigForm').then((m) => ({ default: m.StripeConfigFormPage }))
)

export const Route = createFileRoute('/$realmId/manage/billing/payment-providers/stripe')({
  component: StripeConfigRoute,
})

export function StripeConfigRoute() {
  const realmId = useResolvedRealmId()

  const { data: providers, isLoading } = useQuery({
    queryKey: ['payment-providers', realmId],
    queryFn: async () => {
      const result = await listPaymentProviders({})
      return result.data?.providers ?? []
    },
  })

  const stripeProvider = providers?.find((p) => p.platform === 'stripe')
  const mode = stripeProvider ? 'edit' : 'create'

  const { data: configData } = useQuery({
    queryKey: ['stripe-config', realmId],
    queryFn: async () => {
      if (!stripeProvider) return null
      const result = await listRealmConfigs({})
      return parseStripeConfig(result.data ?? [])
    },
    enabled: !!stripeProvider,
  })

  const initialValues: Partial<StripeConfigForm> | undefined = configData
    ? {
        publishableKey: configData.publishableKey,
        secretKey: '',
        webhookSecret: '',
        asyncPointsStrategy: configData.asyncPointsStrategy,
      }
    : undefined

  if (isLoading) {
    return <SpinnerFallback />
  }

  return (
    <Suspense fallback={<SpinnerFallback />}>
      <StripeConfigFormPage realmId={realmId} mode={mode} initialValues={initialValues} />
    </Suspense>
  )
}
