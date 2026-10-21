import { realmPath, useResolvedRealmContext } from '@/lib/realm-routing'
import { Sidebar } from '@/components/admin/sidebar'
import { Header } from '@/components/admin/header'
import { Outlet, useLocation, useNavigate } from '@tanstack/react-router'
import { useEffect } from 'react'
import { usePermission } from '@/hooks/use-permission'

export function RealmDashboardLayout() {
  const { hasAdminPermission: hasAdminPermissionValue, isLoading } = usePermission()
  const location = useLocation()
  const navigate = useNavigate()
  const { realmId, isCustomDomain } = useResolvedRealmContext()

  // Define admin page patterns
  const adminPages = [
    /\/users/,
    /\/client-apps/,
    /\/roles/,
    /\/permissions/,
    /\/realms/,
    /\/settings/,
  ]

  // Check if accessing admin page
  const isAccessingAdminPage = adminPages.some((regex) => regex.test(location.pathname))

  // Redirect unauthorized users to profile
  // Only redirect when NOT loading and NOT authorized
  useEffect(() => {
    if (isAccessingAdminPage && !isLoading && !hasAdminPermissionValue) {
      navigate({ to: realmPath({ realmId, isCustomDomain }, '/user/profile') })
    }
  }, [isAccessingAdminPage, hasAdminPermissionValue, isLoading, realmId, isCustomDomain, navigate])

  return (
    <div className="flex h-screen bg-muted">
      <Sidebar />
      <div className="flex-1 flex flex-col overflow-hidden">
        <Header />
        <main className="flex-1 overflow-y-auto p-6">
          <Outlet />
        </main>
      </div>
    </div>
  )
}
