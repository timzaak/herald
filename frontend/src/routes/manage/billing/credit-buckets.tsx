import { createFileRoute, Outlet } from '@tanstack/react-router'
import { manageFeatureGuard } from '@/lib/manage-feature-guard'

export const Route = createFileRoute('/manage/billing/credit-buckets')({
  beforeLoad: manageFeatureGuard((f) => f.admin.pointsVisible),
  component: () => <Outlet />,
})
