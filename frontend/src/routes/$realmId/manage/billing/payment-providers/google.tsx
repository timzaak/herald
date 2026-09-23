import { createFileRoute } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { lazy, Suspense } from 'react'
import { SpinnerFallback } from '@/components/shared'
import { listPaymentProviders } from '@/lib/api-generated'
import { listRealmConfigs } from '@/lib/api-generated/sdk.gen'
import { parseGoogleConfig } from '@/lib/google-config-utils'
import type { GooglePlayConfigForm } from '@/lib/schemas/google-config'
import { useResolvedRealmId } from '@/lib/realm-routing'

const GooglePlayConfigFormPage = lazy(() =>
  import('@/components/billing/GooglePlayConfigForm').then((m) => ({
    default: m.GooglePlayConfigFormPage,
  }))
)

export const Route = createFileRoute('/$realmId/manage/billing/payment-providers/google')({
  component: GoogleConfigRoute,
})

export function GoogleConfigRoute() {
  const realmId = useResolvedRealmId()

  const { data: providers, isLoading } = useQuery({
    queryKey: ['payment-providers', realmId],
    queryFn: async () => {
      const result = await listPaymentProviders({})
      return result.data?.providers ?? []
    },
  })

  const googleProvider = providers?.find((p) => p.platform === 'google')
  const mode = googleProvider ? 'edit' : 'create'

  const { data: configData } = useQuery({
    queryKey: ['google-config', realmId],
    queryFn: async () => {
      if (!googleProvider) return null
      const result = await listRealmConfigs({})
      return parseGoogleConfig(result.data ?? [])
    },
    enabled: !!googleProvider,
  })

  const initialValues: Partial<GooglePlayConfigForm> | undefined = configData
    ? {
        packageName: configData.packageName,
        // Sensitive: leave blank on edit so it can be retained.
        serviceAccountJson: '',
      }
    : undefined

  if (isLoading) {
    return <SpinnerFallback />
  }

  return (
    <Suspense fallback={<SpinnerFallback />}>
      <GooglePlayConfigFormPage realmId={realmId} mode={mode} initialValues={initialValues} />
    </Suspense>
  )
}
