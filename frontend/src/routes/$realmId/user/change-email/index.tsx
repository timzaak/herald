import { createFileRoute } from '@tanstack/react-router'
import { PageHeader } from '@/components/shared'
import { ChangeEmailForm } from '@/components/profile/change-email-form'
import { m } from '@/paraglide/messages'

export const Route = createFileRoute('/$realmId/user/change-email/')({
  component: ChangeEmailIndex,
})

export function ChangeEmailIndex() {
  return (
    <div className="space-y-8">
      <PageHeader
        title={m['profile.change_email_page_title']()}
        subtitle={m['profile.change_email_description']()}
      />
      <ChangeEmailForm />
    </div>
  )
}
