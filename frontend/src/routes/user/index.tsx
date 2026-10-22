import { createFileRoute } from '@tanstack/react-router'
import { UserIndexRoute } from '@/routes/$realmId/user/index'

export const Route = createFileRoute('/user/')({
  component: UserIndexRoute,
})
