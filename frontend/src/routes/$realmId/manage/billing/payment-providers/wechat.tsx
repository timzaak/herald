import { createFileRoute } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { lazy, Suspense } from 'react'
import { SpinnerFallback } from '@/components/shared'
import { listPaymentProviders } from '@/lib/api-generated'
import { listRealmConfigs } from '@/lib/api-generated/sdk.gen'
import { parseWechatConfig } from '@/lib/wechat-config-utils'
import type { WechatConfigForm } from '@/lib/schemas/wechat-config'
import { useResolvedRealmId } from '@/lib/realm-routing'

const WechatConfigFormPage = lazy(() =>
  import('@/components/billing/WechatConfigForm').then((m) => ({
    default: m.WechatConfigFormPage,
  }))
)

export const Route = createFileRoute('/$realmId/manage/billing/payment-providers/wechat')({
  component: WechatConfigRoute,
})

export function WechatConfigRoute() {
  const realmId = useResolvedRealmId()

  const { data: providers, isLoading } = useQuery({
    queryKey: ['payment-providers', realmId],
    queryFn: async () => {
      const result = await listPaymentProviders({})
      return result.data?.providers ?? []
    },
  })

  const wechatProvider = providers?.find((p) => p.platform === 'wechat')
  const mode = wechatProvider ? 'edit' : 'create'

  const { data: configData } = useQuery({
    queryKey: ['wechat-config', realmId],
    queryFn: async () => {
      if (!wechatProvider) return null
      const result = await listRealmConfigs({})
      return parseWechatConfig(result.data ?? [])
    },
    enabled: !!wechatProvider,
  })

  const initialValues: Partial<WechatConfigForm> | undefined = configData
    ? {
        appId: configData.appId,
        mchId: configData.mchId,
        serialNo: configData.serialNo,
        notifyUrl: configData.notifyUrl,
        platformPublicKey: configData.platformPublicKey,
        // Sensitive: leave blank on edit so it can be retained.
        privateKey: '',
        v3Key: '',
      }
    : undefined

  if (isLoading) {
    return <SpinnerFallback />
  }

  return (
    <Suspense fallback={<SpinnerFallback />}>
      <WechatConfigFormPage realmId={realmId} mode={mode} initialValues={initialValues} />
    </Suspense>
  )
}
