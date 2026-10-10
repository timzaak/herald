import { createFileRoute } from '@tanstack/react-router'
import { ChangeEmailConfirmPage } from '@/routes/$realmId/user/change-email/confirm'
import { changeEmailConfirmSearchSchema } from '@/lib/schemas/search-params'

export const Route = createFileRoute('/user/change-email/confirm')({
  component: ChangeEmailConfirmPage,
  validateSearch: (search) => changeEmailConfirmSearchSchema.parse(search),
})
