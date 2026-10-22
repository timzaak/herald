import { createFileRoute } from '@tanstack/react-router'
import { ProfileHub } from '@/components/profile/profile-hub'
import { ProfileIndex } from './profile'

export const Route = createFileRoute('/$realmId/user/')({
  component: UserIndexRoute,
})

// The account center's landing page: below md it is the mobile hub (identity
// card + grouped menu, self-hidden above md); from md up the sidebar is the
// navigation, so the profile section renders inline exactly as /user/profile
// does.
export function UserIndexRoute() {
  return (
    <>
      <ProfileHub />
      <div className="hidden md:block">
        <ProfileIndex />
      </div>
    </>
  )
}
