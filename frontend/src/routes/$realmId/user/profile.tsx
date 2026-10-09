import { createFileRoute } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { Label } from '@/components/ui/label'
import { profileQueryOptions } from '@/data/query-options'
import { PageHeader } from '@/components/shared'
import { NicknameEditForm } from '@/components/profile/nickname-edit-form'
import { m } from '@/paraglide/messages'

export const Route = createFileRoute('/$realmId/user/profile')({
  component: ProfileIndex,
})

export function ProfileIndex() {
  const { data: profile, isLoading } = useQuery(profileQueryOptions)

  if (isLoading) {
    return <div>{m['profile.loading']()}</div>
  }

  if (!profile) {
    return <div>{m['profile.failed_to_load']()}</div>
  }

  return (
    <div className="space-y-8">
      <PageHeader title={m['profile.page_title']()} />

      <section>
        <h2 className="text-base font-semibold">{m['profile.info_card_title']()}</h2>
        <div className="mt-4 space-y-4 border-t border-border pt-6">
          <div className="space-y-1">
            <Label>{m['profile.email_label']()}</Label>
            <p className="text-sm text-muted-foreground" data-testid="email-display">
              {profile.email}
            </p>
          </div>
          {/* TanStack Form reads defaultValues only at mount: keying on the
              server nickname re-mounts the form when a refetch brings a new
              value, so a stale tab cannot save its outdated nickname back. */}
          <NicknameEditForm key={profile.nickname ?? ''} initialNickname={profile.nickname ?? ''} />
          <div className="space-y-1">
            <Label>{m['profile.status_label']()}</Label>
            <p className="text-sm text-muted-foreground" data-testid="status-display">
              {profile.status === 1 ? m['profile.status_normal']() : m['profile.status_other']()}
            </p>
          </div>
        </div>
      </section>
    </div>
  )
}
