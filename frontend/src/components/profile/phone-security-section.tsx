/**
 * Security-page phone tab container.
 *
 * Gating matrix (extracted as its own component to keep the page file thin):
 *   - `canManage = phoneLoginEnabled && smsConfigured` → full management
 *     (bind / rebind / unbind entries).
 *   - bound && !canManage → read-only retained state: show the masked number
 *     with a "kept, admin disabled" note and NO action buttons (the server
 *     would reject them; never offer a button that must fail).
 *   - !bound && !canManage → the tab itself is hidden by the page, so this
 *     component is not even mounted.
 */

import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ChangePhoneDialog } from '@/components/profile/change-phone-dialog'
import { PhoneUnbindDialog } from '@/components/profile/phone-unbind-dialog'
import { userPhoneStatusQueryOptions } from '@/data/query-options'
import { m } from '@/paraglide/messages'

export function PhoneSecuritySection() {
  const [bindDialogOpen, setBindDialogOpen] = useState(false)
  const [unbindDialogOpen, setUnbindDialogOpen] = useState(false)
  // Masked number snapshotted when the unbind dialog opens: the dialog must
  // be mounted on the switch alone. Keying the mount on the live query would
  // unmount it mid-flow when unbind invalidates the status query (the done
  // panel vanishing on refetch) and resurrect it after a later rebind flips
  // the state back.
  const [unbindTarget, setUnbindTarget] = useState('')

  const { data: status } = useQuery(userPhoneStatusQueryOptions)

  // Fail-closed while loading: render nothing rather than a half-gated card
  // (the tab may already be visible from the page's own status query; the
  // two share the cache, so this settles together with it).
  if (!status) return null

  const bound = status.bound
  const canManage = status.phoneLoginEnabled && status.smsConfigured

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-base font-semibold">{m['profile.phone_title']()}</h2>
        <p className="text-sm text-muted-foreground">{m['profile.phone_description']()}</p>
      </div>

      <div className="rounded-lg border p-4" data-testid="phone-status-card">
        {bound ? (
          <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
            <div className="space-y-1">
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium">{m['profile.phone_bound_label']()}</span>
                <span className="font-mono text-sm" data-testid="phone-bound-value">
                  {status.maskedPhone}
                </span>
                <Badge variant="secondary" data-testid="phone-verified-badge">
                  {m['profile.phone_verified_badge']()}
                </Badge>
              </div>
              {!canManage && (
                <p className="text-sm text-muted-foreground" data-testid="phone-disabled-note">
                  {m['profile.phone_disabled_note']()}
                </p>
              )}
            </div>
            {canManage && (
              <div className="flex gap-2">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => setBindDialogOpen(true)}
                  data-testid="phone-bind-button"
                >
                  {m['profile.phone_rebind_entry']()}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => {
                    setUnbindTarget(status.maskedPhone ?? '')
                    setUnbindDialogOpen(true)
                  }}
                  data-testid="phone-unbind-button"
                >
                  {m['profile.phone_unbind_entry']()}
                </Button>
              </div>
            )}
          </div>
        ) : (
          <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
            <p className="text-sm text-muted-foreground" data-testid="phone-unavailable-note">
              {canManage
                ? m['profile.phone_unbound_note']()
                : m['profile.phone_unavailable_note']()}
            </p>
            {canManage && (
              <Button
                type="button"
                size="sm"
                onClick={() => setBindDialogOpen(true)}
                data-testid="phone-bind-button"
              >
                {m['profile.phone_bind_entry']()}
              </Button>
            )}
          </div>
        )}
      </div>

      {bindDialogOpen && (
        <ChangePhoneDialog
          currentMaskedPhone={bound ? (status.maskedPhone ?? null) : null}
          onClose={() => setBindDialogOpen(false)}
        />
      )}
      {unbindDialogOpen && (
        <PhoneUnbindDialog maskedPhone={unbindTarget} onClose={() => setUnbindDialogOpen(false)} />
      )}
    </div>
  )
}
