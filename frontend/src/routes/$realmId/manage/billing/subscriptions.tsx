import { createFileRoute } from '@tanstack/react-router'
import { z } from 'zod'
import { requireFeature } from '@/data/query-options'
import { initializeAuth } from '@/lib/auth-utils'
import { ADMIN_WEB_CONSOLE_CLIENT_ID } from '@/lib/constants/auth-constants'
import { AdminSubscriptionListPage } from '@/components/billing/admin-subscription-list-page'
import { useCurrentSearch, useResolvedRealmId } from '@/lib/realm-routing'

const subscriptionsSearchSchema = z.object({
  page: z.number().int().min(0).optional(),
  pageSize: z.number().int().min(1).max(100).optional(),
  entitlementKey: z.string().optional(),
  status: z.string().optional(),
  paymentProvider: z.string().optional(),
})

export const Route = createFileRoute('/$realmId/manage/billing/subscriptions')({
  beforeLoad: async ({ context, params }) => {
    // Route beforeLoads run ahead of the __root loader, so on a cold reload
    // the feature query below would fire before initializeAuth restores the
    // Bearer token (401 → route error boundary). Idempotent: short-circuits
    // once the realm/client is initialized.
    await initializeAuth(params.realmId, ADMIN_WEB_CONSOLE_CLIENT_ID)
    await requireFeature(
      context.queryClient,
      params.realmId,
      (f) => f.admin.entitlementMappingsVisible,
      {
        to: '/$realmId/manage',
        params: { realmId: params.realmId },
      }
    )
  },
  validateSearch: subscriptionsSearchSchema,
  component: SubscriptionsRoute,
})

export function SubscriptionsRoute() {
  const realmId = useResolvedRealmId()
  const search = useCurrentSearch<z.infer<typeof subscriptionsSearchSchema>>()

  return <AdminSubscriptionListPage realmId={realmId} search={search} />
}
