/**
 * Phone unbind dialog.
 *
 * State machine: confirm (show masked number + warning) → reauth
 * (`remove_phone`) → done. Deliberately separate from `ChangePhoneDialog`:
 * unbind is "confirm → reauth → single request" with no OTP/business step, and
 * merging the two would make a double-headed state machine.
 *
 * Error matrix: 401/409 ticket-class failures restart reauth; 400 (no
 * binding left / switch off) and the rest surface the backend message.
 */

import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Input } from '@/components/ui/input'
import { PasswordInput } from '@/components/ui/password-input'
import { Label } from '@/components/ui/label'
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group'
import {
  handleBeginReauth,
  handleVerifyReauth,
  userPhoneUnbind,
  type ReauthFactor,
} from '@/lib/api-generated'
import { prepareRequestOptions, serializeAssertion } from '@/lib/passkey-utils'
import { resolveApiError, getErrorMessage } from '@/lib/error-utils'
import { queryKeys } from '@/data/query-options'
import { m } from '@/paraglide/messages'

/** The begin response's `challenge` payload when the passkey factor is available. */
interface PasskeyChallenge {
  challengeToken: string
  options: unknown
}

const FACTOR_LABELS: Record<ReauthFactor, () => string> = {
  password: m['profile.phone_factor_password'],
  totp: m['profile.phone_factor_totp'],
  passkey: m['profile.phone_factor_passkey'],
}

type DialogStep = 'confirm' | 'reauth' | 'done'

interface PhoneUnbindDialogProps {
  maskedPhone: string
  onClose: () => void
}

export function PhoneUnbindDialog({ maskedPhone, onClose }: PhoneUnbindDialogProps) {
  const queryClient = useQueryClient()
  const [step, setStep] = useState<DialogStep>('confirm')
  const [factors, setFactors] = useState<ReauthFactor[]>([])
  const [passkeyChallenge, setPasskeyChallenge] = useState<PasskeyChallenge | undefined>()
  const [selectedFactor, setSelectedFactor] = useState<ReauthFactor>('password')
  const [credential, setCredential] = useState('')
  const [stepError, setStepError] = useState<string | null>(null)

  const beginMutation = useMutation({
    mutationFn: () => handleBeginReauth({ body: { targetOperation: 'remove_phone' } }),
    onSuccess: (response) => {
      if (response.error) {
        setStepError(m['reauth.failed']())
        return
      }
      const available = response.data?.availableFactors ?? []
      setFactors(available)
      setPasskeyChallenge(
        (response.data?.challenge as PasskeyChallenge | undefined | null) ?? undefined
      )
      if (available.length === 0) {
        setStepError(m['profile.change_email_no_factors']())
        return
      }
      setSelectedFactor(available[0])
      setStep('reauth')
    },
    onError: () => setStepError(m['reauth.failed']()),
  })
  const beginReauth = beginMutation.mutate

  const restartReauth = () => {
    setCredential('')
    // stepError is intentionally kept: the only caller (a ticket-class 401/409)
    // sets the "verify again" message right before restarting, and the user
    // needs it on the reauth step (mirrors ChangePhoneDialog.restartReauth).
    setStep('reauth')
    beginReauth()
  }

  const unbindMutation = useMutation({
    mutationFn: (reauthToken: string) => userPhoneUnbind({ body: { reauthToken } }),
    onSuccess: (response) => {
      if (response.error) {
        const { status } = resolveApiError(response.error)
        if (status === 401 || status === 409) {
          setStepError(m['profile.phone_error_reauth_needed']())
          restartReauth()
          return
        }
        setStepError(getErrorMessage(response.error))
        return
      }
      // The binding is gone: refresh the Security-page gating matrix
      // immediately.
      void queryClient.invalidateQueries({ queryKey: queryKeys.userPhoneStatus() })
      setStepError(null)
      setStep('done')
    },
    // Unbind sends no code — a generic fallback here (onError only fires on
    // client-side faults; HTTP errors arrive as response.error above).
    onError: () => setStepError(m['error.generic']()),
  })

  const verifyMutation = useMutation({
    mutationFn: (payload: {
      factor: ReauthFactor
      password?: string
      totpCode?: string
      passkeyAssertion?: { assertion: unknown; challengeToken: string }
    }) =>
      handleVerifyReauth({
        body: {
          targetOperation: 'remove_phone',
          factor: payload.factor,
          password: payload.password ?? null,
          totpCode: payload.totpCode ?? null,
          passkeyAssertion: payload.passkeyAssertion ?? null,
        },
      }),
    onSuccess: (response, payload) => {
      if (response.error) {
        const status = resolveApiError(response.error).status
        setStepError(
          status === 401 && payload.factor === 'password'
            ? m['reauth.wrong_password']()
            : status === 409
              ? m['reauth.expired']()
              : m['reauth.failed']()
        )
        return
      }
      const token = response.data?.reauthToken
      if (!token) {
        setStepError(m['reauth.failed']())
        return
      }
      setCredential('')
      setStepError(null)
      unbindMutation.mutate(token)
    },
    onError: () => setStepError(m['reauth.failed']()),
  })

  // The parent mounts this dialog on open and unmounts it on close.
  const handleOpenChange = (nextOpen: boolean) => {
    if (!nextOpen) onClose()
  }

  const handlePasskeyVerify = async () => {
    setStepError(null)
    if (!passkeyChallenge) {
      setStepError(m['profile.phone_error_verify_failed']())
      return
    }
    try {
      const credential = await navigator.credentials.get(
        prepareRequestOptions(passkeyChallenge.options, 'optional')
      )
      if (!credential) return
      const assertion = serializeAssertion(credential as PublicKeyCredential)
      verifyMutation.mutate({
        factor: 'passkey',
        passkeyAssertion: {
          assertion,
          challengeToken: passkeyChallenge.challengeToken,
        },
      })
    } catch {
      // A dismissed native prompt is a silent cancellation; genuine verify
      // failures surface through verifyMutation error handling.
      return
    }
  }

  const handleVerify = () => {
    setStepError(null)
    if (selectedFactor === 'passkey') {
      void handlePasskeyVerify()
      return
    }
    if (credential.length === 0) {
      setStepError(
        selectedFactor === 'password'
          ? m['reauth.password_required']()
          : m['profile.phone_totp_required']()
      )
      return
    }
    if (selectedFactor === 'totp') {
      verifyMutation.mutate({ factor: 'totp', totpCode: credential })
    } else {
      verifyMutation.mutate({ factor: 'password', password: credential })
    }
  }

  const handleSelectFactor = (factor: string) => {
    setSelectedFactor(factor as ReauthFactor)
    setCredential('')
    setStepError(null)
  }

  return (
    <Dialog open onOpenChange={handleOpenChange}>
      <DialogContent className="sm:max-w-[425px]" data-testid="phone-unbind-dialog">
        <DialogHeader>
          <DialogTitle data-testid="phone-unbind-dialog-title">
            {m['profile.phone_unbind_dialog_title']()}
          </DialogTitle>
          <DialogDescription>{m['profile.phone_unbind_dialog_description']()}</DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          {stepError && (
            <Alert variant="destructive">
              <AlertDescription data-testid="phone-unbind-error-message">
                {stepError}
              </AlertDescription>
            </Alert>
          )}

          {step === 'confirm' && (
            <div className="space-y-4">
              <p className="text-sm">{m['profile.phone_unbind_warning']({ maskedPhone })}</p>
              <div className="flex gap-2">
                <Button
                  type="button"
                  variant="outline"
                  className="flex-1"
                  data-testid="phone-unbind-cancel-button"
                  onClick={() => handleOpenChange(false)}
                >
                  {m['common.cancel']()}
                </Button>
                <Button
                  type="button"
                  variant="destructive"
                  className="flex-1"
                  data-testid="confirm-unbind-phone-button"
                  onClick={() => {
                    setStepError(null)
                    beginReauth()
                  }}
                  disabled={beginMutation.isPending}
                  loading={beginMutation.isPending}
                >
                  {m['profile.phone_unbind_confirm']()}
                </Button>
              </div>
            </div>
          )}

          {step === 'reauth' && (
            <div className="space-y-4">
              {factors.length > 1 && (
                <RadioGroup
                  value={selectedFactor}
                  onValueChange={handleSelectFactor}
                  className="gap-2"
                >
                  {factors.map((factor) => (
                    <div key={factor} className="flex items-center gap-2">
                      <RadioGroupItem
                        value={factor}
                        id={`phone-unbind-factor-${factor}`}
                        data-testid={`phone-unbind-factor-${factor}-radio`}
                      />
                      <Label htmlFor={`phone-unbind-factor-${factor}`}>
                        {FACTOR_LABELS[factor]()}
                      </Label>
                    </div>
                  ))}
                </RadioGroup>
              )}

              {selectedFactor === 'password' && (
                <div className="space-y-2">
                  <Label htmlFor="phone-unbind-password-input">
                    {m['profile.phone_factor_password']()}
                  </Label>
                  <PasswordInput
                    id="phone-unbind-password-input"
                    autoComplete="current-password"
                    value={credential}
                    onChange={(e) => setCredential(e.target.value)}
                    data-testid="phone-unbind-password-input"
                  />
                </div>
              )}

              {selectedFactor === 'totp' && (
                <div className="space-y-2">
                  <Label htmlFor="phone-unbind-totp-input">
                    {m['profile.phone_factor_totp']()}
                  </Label>
                  <Input
                    id="phone-unbind-totp-input"
                    inputMode="numeric"
                    maxLength={6}
                    autoComplete="one-time-code"
                    value={credential}
                    onChange={(e) => setCredential(e.target.value)}
                    data-testid="phone-unbind-totp-input"
                  />
                </div>
              )}

              {selectedFactor === 'passkey' ? (
                <Button
                  type="button"
                  variant="outline"
                  data-testid="phone-unbind-passkey-button"
                  onClick={() => void handlePasskeyVerify()}
                  disabled={verifyMutation.isPending}
                >
                  {m['profile.phone_factor_passkey']()}
                </Button>
              ) : (
                <Button
                  type="button"
                  variant="destructive"
                  data-testid="phone-unbind-verify-button"
                  onClick={handleVerify}
                  disabled={verifyMutation.isPending}
                  loading={verifyMutation.isPending}
                >
                  {verifyMutation.isPending
                    ? m['profile.phone_verifying_factor']()
                    : m['profile.phone_verify_factor_button']()}
                </Button>
              )}
            </div>
          )}

          {step === 'done' && (
            <div className="space-y-3" data-testid="phone-unbind-done-panel">
              <p className="text-sm">{m['profile.phone_unbind_done']()}</p>
              <Button
                type="button"
                data-testid="phone-unbind-done-button"
                onClick={() => handleOpenChange(false)}
              >
                {m['profile.phone_done_button']()}
              </Button>
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}
