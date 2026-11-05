/**
 * Admin user-phone management dialog (US-PL-005).
 *
 * Sub-machines:
 *   - view: masked bound phone + pending-rebind badge (data snapshotted from
 *     `get_user` at dialog open; a pending record that expires server-side
 *     surfaces as a 400 on confirm, never as stale UI).
 *   - unbind: shared `ConfirmDialog` second confirmation → DELETE (audit is
 *     server-side; DEC-phone-login-007).
 *   - rebind: phone → send (registers the pending record) → 6-digit code +
 *     countdown → confirm. The old binding stays live until confirm
 *     succeeds; abandoning mid-flow just leaves the pending record to
 *     expire (5min TTL).
 *
 * Not gated on the phone-login switch: admin disposition is an account-data
 * operation (DEC-phone-login-003), unlike the self-service Security entry.
 *
 * Error matrix: 409 in-use (send/confirm) → localized; 400 no-pending →
 * "send first" hint; code failures → backend message + cleared input.
 */

import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { ConfirmDialog, OtpCodeInput } from '@/components/shared'
import { useCountdown } from '@/hooks/use-countdown'
import {
  adminUserPhoneRebindConfirm,
  adminUserPhoneRebindSend,
  adminUserPhoneUnbind,
} from '@/lib/api-generated'
import { resolveApiError, getErrorMessage } from '@/lib/error-utils'
import { adminUserQueryOptions, queryKeys } from '@/data/query-options'
import { m } from '@/paraglide/messages'

const CODE_LENGTH = 6

type DialogStep = 'view' | 'rebind-phone' | 'rebind-code' | 'rebind-done'

interface UserPhoneManageDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  realmId: string
  userId: string
  userEmail: string
}

export function UserPhoneManageDialog({
  open,
  onOpenChange,
  realmId,
  userId,
  userEmail,
}: UserPhoneManageDialogProps) {
  const queryClient = useQueryClient()
  const [step, setStep] = useState<DialogStep>('view')
  const [phone, setPhone] = useState('')
  const [code, setCode] = useState('')
  const [stepError, setStepError] = useState<string | null>(null)
  const [unbindConfirmOpen, setUnbindConfirmOpen] = useState(false)
  const { countdown, startCountdown, clearCountdown } = useCountdown()

  // Open-time snapshot of the user's phone state (masked values + pending
  // badge). `staleTime: 5min` on the underlying options is fine: the dialog
  // is mounted fresh per open and mutations below invalidate the key.
  const { data: user } = useQuery(adminUserQueryOptions(realmId, userId))

  const invalidateAfterChange = () => {
    void queryClient.invalidateQueries({ queryKey: queryKeys.adminUser(realmId, userId) })
    void queryClient.invalidateQueries({ queryKey: queryKeys.usersList(realmId) })
  }

  const unbindMutation = useMutation({
    mutationFn: async () => {
      const result = await adminUserPhoneUnbind({ path: { userId } })
      if (result.error) throw result.error
      return result.data
    },
    onSuccess: () => {
      setUnbindConfirmOpen(false)
      setStepError(null)
      invalidateAfterChange()
      setStep('view')
    },
    onError: (error: unknown) => {
      setUnbindConfirmOpen(false)
      // Unbind sends no code — generic fallback.
      setStepError(getErrorMessage(error) ?? m['error.generic']())
    },
  })

  const sendMutation = useMutation({
    mutationFn: async (payload: { phone: string }) => {
      const result = await adminUserPhoneRebindSend({
        path: { userId },
        body: { phone: payload.phone },
      })
      if (result.error) throw result.error
      return result.data
    },
    onSuccess: (data) => {
      setStepError(null)
      setCode('')
      setStep('rebind-code')
      startCountdown(data.expiresInSeconds)
      // The send registered a pending rebind server-side: refresh the user
      // snapshot so reopening the dialog shows the pending badge even when
      // the cached query is still fresh.
      invalidateAfterChange()
    },
    onError: (error: unknown) => {
      const status = resolveApiError(error).status
      if (status === 409) {
        setStepError(m['users.phone_error_in_use']())
        return
      }
      if (status === 429) {
        setStepError(m['users.phone_error_rate_limited']())
        return
      }
      setStepError(getErrorMessage(error) ?? m['users.phone_error_send_failed']())
    },
  })

  const confirmMutation = useMutation({
    mutationFn: async (payload: { code: string }) => {
      const result = await adminUserPhoneRebindConfirm({
        path: { userId },
        body: { code: payload.code },
      })
      if (result.error) throw result.error
      return result.data
    },
    onSuccess: () => {
      setStepError(null)
      clearCountdown()
      invalidateAfterChange()
      setStep('rebind-done')
    },
    onError: (error: unknown) => {
      const status = resolveApiError(error).status
      if (status === 409) {
        setStepError(m['users.phone_error_in_use']())
        return
      }
      // 400 no_pending_rebind (TTL expired) and code failures surface the
      // backend message; the resend path re-registers a fresh pending record.
      // Confirm verifies a code, it sends none — generic fallback.
      setStepError(getErrorMessage(error) ?? m['error.generic']())
      setCode('')
    },
  })

  const handleClose = (nextOpen: boolean) => {
    if (!nextOpen) {
      setStep('view')
      setPhone('')
      setCode('')
      setStepError(null)
      clearCountdown()
      onOpenChange(false)
    }
  }

  const maskedPhone = user?.phoneMasked ?? null
  const pendingPhoneMasked =
    user && 'pendingPhoneMasked' in user && user.pendingPhoneMasked ? user.pendingPhoneMasked : null

  return (
    <Dialog open={open} onOpenChange={handleClose}>
      <DialogContent className="sm:max-w-[425px]" data-testid="user-phone-manage-dialog">
        <DialogHeader>
          <DialogTitle data-testid="user-phone-manage-dialog-title">
            {m['users.phone_manage_title']()}
          </DialogTitle>
          <DialogDescription>{userEmail}</DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          {stepError && (
            <Alert variant="destructive">
              <AlertDescription data-testid="user-phone-error-message">
                {stepError}
              </AlertDescription>
            </Alert>
          )}

          {step === 'view' && (
            <div className="space-y-4" data-testid="user-phone-view-panel">
              <div className="space-y-1">
                <span className="text-sm font-medium">{m['users.phone_bound_label']()}</span>
                <div className="flex items-center gap-2">
                  <span className="font-mono text-sm" data-testid="user-phone-bound-value">
                    {maskedPhone ?? '-'}
                  </span>
                  {pendingPhoneMasked && (
                    <Badge variant="secondary" data-testid="user-phone-pending-badge">
                      {m['users.phone_pending_badge']({
                        maskedPhone: pendingPhoneMasked,
                      })}
                    </Badge>
                  )}
                </div>
                {pendingPhoneMasked && (
                  <p className="text-xs text-muted-foreground">{m['users.phone_pending_note']()}</p>
                )}
              </div>

              <div className="flex flex-wrap gap-2">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={unbindMutation.isPending}
                  onClick={() => setUnbindConfirmOpen(true)}
                  data-testid="user-phone-unbind-button"
                >
                  {m['users.phone_unbind_button']()}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => {
                    setStepError(null)
                    setPhone('')
                    setStep('rebind-phone')
                  }}
                  data-testid="user-phone-rebind-button"
                >
                  {m['users.phone_rebind_button']()}
                </Button>
              </div>
            </div>
          )}

          {step === 'rebind-phone' && (
            <div className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="user-phone-rebind-phone-input">
                  {m['users.phone_rebind_phone_label']()}
                </Label>
                <Input
                  id="user-phone-rebind-phone-input"
                  type="tel"
                  autoComplete="tel"
                  value={phone}
                  onChange={(e) => setPhone(e.target.value)}
                  disabled={sendMutation.isPending}
                  data-testid="user-phone-rebind-phone-input"
                />
                <p className="text-xs text-muted-foreground">{m['users.phone_input_hint']()}</p>
              </div>
              <div className="flex gap-2">
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => setStep('view')}
                  disabled={sendMutation.isPending}
                  data-testid="user-phone-rebind-back-button"
                >
                  {m['common.cancel']()}
                </Button>
                <Button
                  type="button"
                  onClick={() => sendMutation.mutate({ phone: phone.trim() })}
                  disabled={sendMutation.isPending || phone.trim().length === 0}
                  loading={sendMutation.isPending}
                  data-testid="user-phone-rebind-send-btn"
                >
                  {sendMutation.isPending
                    ? m['users.phone_sending']()
                    : m['users.phone_rebind_send']()}
                </Button>
              </div>
            </div>
          )}

          {step === 'rebind-code' && (
            <div className="space-y-4" data-testid="user-phone-rebind-code-step">
              <p className="text-sm text-muted-foreground">
                {m['users.phone_rebind_code_description']({
                  maskedPhone: sendMutation.data?.maskedNewPhone ?? '',
                })}
              </p>
              <div>
                <Label htmlFor="user-phone-rebind-code-input">
                  {m['users.phone_code_label']()}
                </Label>
                <OtpCodeInput
                  value={code}
                  onChange={setCode}
                  numInputs={CODE_LENGTH}
                  testIdBase="user-phone-rebind-code"
                  ariaLabel={m['users.phone_code_label']()}
                />
              </div>
              <div className="flex items-center gap-2 text-sm">
                {countdown !== null ? (
                  <span className="text-muted-foreground" data-testid="user-phone-rebind-countdown">
                    {m['users.phone_resend_in']({ countdown })}
                  </span>
                ) : (
                  <Button
                    type="button"
                    variant="link"
                    className="h-auto p-0"
                    disabled={sendMutation.isPending || confirmMutation.isPending}
                    onClick={() => sendMutation.mutate({ phone: phone.trim() })}
                    data-testid="user-phone-rebind-resend-btn"
                  >
                    {m['users.phone_resend']()}
                  </Button>
                )}
              </div>
              <div className="flex gap-2">
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => {
                    setCode('')
                    setStepError(null)
                    clearCountdown()
                    setStep('rebind-phone')
                  }}
                  disabled={confirmMutation.isPending}
                  data-testid="user-phone-rebind-back-to-phone-button"
                >
                  {m['users.phone_back_button']()}
                </Button>
                <Button
                  type="button"
                  className="flex-1"
                  disabled={code.length !== CODE_LENGTH || confirmMutation.isPending}
                  loading={confirmMutation.isPending}
                  onClick={() => confirmMutation.mutate({ code })}
                  data-testid="user-phone-rebind-confirm-btn"
                >
                  {confirmMutation.isPending
                    ? m['users.phone_verifying']()
                    : m['users.phone_rebind_confirm']()}
                </Button>
              </div>
            </div>
          )}

          {step === 'rebind-done' && (
            <div className="space-y-3" data-testid="user-phone-rebind-done-panel">
              <p className="text-sm">{m['users.phone_rebind_done']()}</p>
              <Button type="button" onClick={() => handleClose(false)}>
                {m['users.phone_rebind_done']()}
              </Button>
            </div>
          )}
        </div>

        {unbindConfirmOpen && (
          <ConfirmDialog
            open={unbindConfirmOpen}
            onOpenChange={setUnbindConfirmOpen}
            title={m['users.phone_unbind_confirm_title']()}
            description={m['users.phone_unbind_confirm_body']({
              maskedPhone: maskedPhone ?? '',
            })}
            onConfirm={() => unbindMutation.mutate()}
            isPending={unbindMutation.isPending}
            contentTestId="user-phone-unbind-confirm-dialog"
            confirmTestId="confirm-unbind-user-phone-button"
            cancelTestId="cancel-unbind-user-phone-button"
          />
        )}
      </DialogContent>
    </Dialog>
  )
}
