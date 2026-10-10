import { createFileRoute } from '@tanstack/react-router'
import { ChangeEmailConfirmPage } from '@/routes/$realmId/user/change-email/confirm'
import { changeEmailSearchSchema } from '@/lib/schemas/search-params'

export const Route = createFileRoute('/user/change-email/confirm')({
  component: ChangeEmailConfirmPage,
  validateSearch: (search) => changeEmailSearchSchema.parse(search),
})
