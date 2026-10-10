import { createFileRoute } from '@tanstack/react-router'
import { ChangeEmailIndex } from '@/routes/$realmId/user/change-email/index'

export const Route = createFileRoute('/user/change-email/')({
  component: ChangeEmailIndex,
})
