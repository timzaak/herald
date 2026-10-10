import { useState } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { Label } from '@/components/ui/label'
import { Button } from '@/components/ui/button'
import { profileQueryOptions, publicConfigQueryOptions } from '@/data/query-options'
import { useResolvedRealmId } from '@/lib/realm-routing'
import { PageHeader } from '@/components/shared'
import { NicknameEditForm } from '@/components/profile/nickname-edit-form'
import { ChangeEmailDialog } from '@/components/profile/change-email-dialog'
import { m } from '@/paraglide/messages'

export const Route = createFileRoute('/$realmId/user/profile')({
  component: ProfileIndex,
})

export function ProfileIndex() {
  const realmId = useResolvedRealmId()
  const { data: profile, isLoading } = useQuery(profileQueryOptions)
  const {
    data: publicConfig,
    isLoading: configLoading,
    isError: configError,
  } = useQuery(publicConfigQueryOptions(realmId))
  const [changeEmailOpen, setChangeEmailOpen] = useState(false)
  // Without a usable email channel the confirmation mail could never be sent,
  // so the entry must not appear; while the flag is loading or failed neither
  // the entry nor the note renders (the backend 400 gate is the final guard).
  const configKnown = !configLoading && !configError && publicConfig !== undefined
  const emailConfigured = publicConfig?.emailChannelConfigured === true

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
            <div className="flex items-center justify-between gap-2">
              <Label>{m['profile.email_label']()}</Label>
              {configKnown && emailConfigured && (
                <Button
                  variant="link"
                  size="sm"
                  className="h-auto p-0"
                  data-testid="change-email-button"
                  onClick={() => setChangeEmailOpen(true)}
                >
                  {m['profile.change_email_entry']()}
                </Button>
              )}
            </div>
            <p className="text-sm text-muted-foreground" data-testid="email-display">
              {profile.email}
            </p>
            {configKnown && !emailConfigured && (
              <p
                className="text-sm text-muted-foreground"
                data-testid="change-email-unavailable-note"
              >
                {m['profile.change_email_unavailable']()}
              </p>
            )}
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

      {changeEmailOpen && (
        <ChangeEmailDialog currentEmail={profile.email} onClose={() => setChangeEmailOpen(false)} />
      )}
    </div>
  )
}
