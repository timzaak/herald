import { Outlet } from '@tanstack/react-router'
import { ProfileSidebar } from '@/components/profile/profile-sidebar'
import { ProfileHeader } from '@/components/profile/profile-header'

export function ProfileLayout() {
  return (
    <div className="flex h-screen flex-col bg-background md:flex-row">
      <ProfileSidebar />
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <ProfileHeader />
        <main className="flex-1 overflow-y-auto">
          <div className="mx-auto w-full max-w-2xl px-4 py-6 md:px-8 md:py-10">
            <Outlet />
          </div>
        </main>
      </div>
    </div>
  )
}
