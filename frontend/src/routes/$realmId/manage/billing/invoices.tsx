import { createFileRoute, Outlet } from '@tanstack/react-router'
import { requireFeature } from '@/data/query-options'
import { initializeAuth } from '@/lib/auth-utils'
import { ADMIN_WEB_CONSOLE_CLIENT_ID } from '@/lib/constants/auth-constants'

export const Route = createFileRoute('/$realmId/manage/billing/invoices')({
  beforeLoad: async ({ context, params }) => {
    // Route beforeLoads run ahead of the __root loader, so on a cold reload
    // the feature query below would fire before initializeAuth restores the
    // Bearer token (401 → route error boundary). Idempotent: short-circuits
    // once the realm/client is initialized.
    await initializeAuth(params.realmId, ADMIN_WEB_CONSOLE_CLIENT_ID)
    await requireFeature(context.queryClient, params.realmId, (f) => f.admin.invoicesVisible, {
      to: '/$realmId/manage',
      params: { realmId: params.realmId },
      search: { status: 'all' },
    })
  },
  component: () => <Outlet />,
})
