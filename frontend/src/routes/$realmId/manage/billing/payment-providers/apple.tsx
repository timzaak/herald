import { createFileRoute } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { lazy, Suspense } from 'react'
import { SpinnerFallback } from '@/components/shared'
import { listPaymentProviders } from '@/lib/api-generated'
import { listRealmConfigs } from '@/lib/api-generated/sdk.gen'
import { parseAppleConfig } from '@/lib/apple-config-utils'
import type { AppleIapConfigForm } from '@/lib/schemas/apple-config'
import { useResolvedRealmId } from '@/lib/realm-routing'

const AppleIapConfigFormPage = lazy(() =>
  import('@/components/billing/AppleIapConfigForm').then((m) => ({
    default: m.AppleIapConfigFormPage,
  }))
)

export const Route = createFileRoute('/$realmId/manage/billing/payment-providers/apple')({
  component: AppleConfigRoute,
})

export function AppleConfigRoute() {
  const realmId = useResolvedRealmId()

  const { data: providers, isLoading } = useQuery({
    queryKey: ['payment-providers', realmId],
    queryFn: async () => {
      const result = await listPaymentProviders({})
      return result.data?.providers ?? []
    },
  })

  const appleProvider = providers?.find((p) => p.platform === 'apple')
  const mode = appleProvider ? 'edit' : 'create'

  const { data: configData } = useQuery({
    queryKey: ['apple-config', realmId],
    queryFn: async () => {
      if (!appleProvider) return null
      const result = await listRealmConfigs({})
      return parseAppleConfig(result.data ?? [])
    },
    enabled: !!appleProvider,
  })

  const initialValues: Partial<AppleIapConfigForm> | undefined = configData
    ? {
        bundleId: configData.bundleId,
        issuerId: configData.issuerId,
        keyId: configData.keyId,
        // Sensitive: leave blank on edit so it can be retained.
        privateKeyP8: '',
        environment: configData.environment,
      }
    : undefined

  if (isLoading) {
    return <SpinnerFallback />
  }

  return (
    <Suspense fallback={<SpinnerFallback />}>
      <AppleIapConfigFormPage realmId={realmId} mode={mode} initialValues={initialValues} />
    </Suspense>
  )
}
