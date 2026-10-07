import { createFileRoute, Outlet } from '@tanstack/react-router'
import { manageFeatureGuard } from '@/lib/manage-feature-guard'

export const Route = createFileRoute('/$realmId/manage/billing/payment-providers')({
  beforeLoad: manageFeatureGuard((f) => f.admin.billingConfigVisible),
  component: () => <Outlet />,
})
