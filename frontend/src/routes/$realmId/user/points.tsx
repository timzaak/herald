import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { UserPointsPage } from '@/components/points/UserPointsPage'
import { useUser } from '@/stores/auth-store'
import { requireUserFeature } from '@/data/query-options'
import { initializeAuth } from '@/lib/auth-utils'
import { USER_ACCOUNT_CENTER_CLIENT_ID } from '@/lib/constants/auth-constants'
import { transactionBucketSearchSchema } from '@/lib/schemas/points-forms'
import { useCurrentSearch, useResolvedRealmContext } from '@/lib/realm-routing'

export const Route = createFileRoute('/$realmId/user/points')({
  beforeLoad: async ({ context, params }) => {
    // Route beforeLoads run ahead of the __root loader, so on a cold reload
    // the feature query below would fire before initializeAuth restores the
    // Bearer token (401 → route error boundary). Idempotent: short-circuits
    // once the realm/client is initialized.
    await initializeAuth(params.realmId, USER_ACCOUNT_CENTER_CLIENT_ID)
    await requireUserFeature(context.queryClient, (f) => f.user.pointsVisible, {
      to: '/$realmId/user/profile',
      params: { realmId: params.realmId },
    })
  },
  // `bucketId` is the shareable transaction-bucket filter
  // (`?bucketId=`). Parsing + URL ↔ filter sync covered by the
  // frontend/test slot.
  validateSearch: transactionBucketSearchSchema,
  component: UserPointsWrapper,
})

export function UserPointsWrapper() {
  const realmContext = useResolvedRealmContext()
  const realmId = realmContext.realmId
  const user = useUser()
  // Get userId from auth store since this is user's own points page
  const userId = user?.id || ''
  const search = useCurrentSearch<{ bucketId?: string }>()
  const navigate = useNavigate()

  function handleBucketIdChange(bucketId: string | undefined) {
    // `to: '.'` targets the current route match, keeping the current params
    // and only updating search. This wrapper backs BOTH /$realmId/user/points
    // and /user/points; a fixed `/user/points` target migrated realm-prefixed
    // visitors to the session-scoped route match, remounting the page and
    // dropping the local type/date filter state.
    navigate({
      to: '.',
      search: () => ({ bucketId }),
      replace: true,
    })
  }

  return (
    <UserPointsPage
      realmId={realmId}
      userId={userId}
      bucketId={search.bucketId}
      onBucketIdChange={handleBucketIdChange}
    />
  )
}
