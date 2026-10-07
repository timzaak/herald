import { createFileRoute, Outlet } from '@tanstack/react-router'
import { userFeatureGuard } from '@/lib/user-feature-guard'

export const Route = createFileRoute('/user/invoices')({
  beforeLoad: userFeatureGuard((f) => f.user.invoicesVisible),
  component: Outlet,
})
