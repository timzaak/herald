import { createFileRoute, Outlet } from '@tanstack/react-router'
import { manageFeatureGuard } from '@/lib/manage-feature-guard'

export const Route = createFileRoute('/manage/billing/invoices')({
  beforeLoad: manageFeatureGuard((f) => f.admin.invoicesVisible, { status: 'all' }),
  component: () => <Outlet />,
})
