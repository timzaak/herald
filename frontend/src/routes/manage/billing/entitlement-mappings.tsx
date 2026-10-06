import { createFileRoute, Outlet } from '@tanstack/react-router'
import { manageFeatureGuard } from '@/lib/manage-feature-guard'

export const Route = createFileRoute('/manage/billing/entitlement-mappings')({
  beforeLoad: manageFeatureGuard((f) => f.admin.entitlementMappingsVisible),
  component: () => <Outlet />,
})
