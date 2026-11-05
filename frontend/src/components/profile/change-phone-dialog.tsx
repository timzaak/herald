/**
 * Phone bind/rebind dialog.
 *
 * State machine (mirrors `ChangeEmailDialog`):
 *   loading → no-factors → begin-error → reauth (`change_phone`) → phone →
 *   code (6-digit + countdown) → done.
 *
 * Bind and rebind share the dialog: the entry label differs (绑定/换绑), the
 * backend treats "already bound" as rebind semantics on the same endpoints.
 * The old binding stays live until confirm succeeds (backend writes the new
 * phone in a single UPDATE), so abandoning mid-flow never loses the old number.
 *
 * Reauth ticket lifecycle: `bind/send` consumes the single-use ticket on every
 * call, so the code step's resend restarts the whole reauth (fresh begin →
 * verify → phone step pre-filled) instead of re-posting with a dead ticket —
 * the same restart semantics `ChangeEmailDialog` applies to its sent step.
 *
 * Error matrix:
 *   - 409 `phone_already_in_use` (send or confirm; any writing form of a
 *     number already bound in the realm) → localized "in use" message, flow
 *     does not advance.
 *   - 401/409 ticket-class failures → restart reauth.
 *   - 429 → localized rate-limited message.
 *   - 400 invalid phone / channel unusable, 502 upstream → backend message
 *     (server is the format/authority, uniform `invalid_phone` copy).
 */

import { useEffect, useState } from 'react'
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
import { OtpCodeInput } from '@/components/shared'
import { useCountdown } from '@/hooks/use-countdown'
import {
  handleBeginReauth,
  handleVerifyReauth,
  userPhoneBindConfirm,
  userPhoneBindSend,
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

const CODE_LENGTH = 6

type DialogStep = 'loading' | 'no-factors' | 'begin-error' | 'reauth' | 'phone' | 'code' | 'done'

interface ChangePhoneDialogProps {
  /** Current masked binding, when rebinding (`null` = first bind). */
  currentMaskedPhone: string | null
  onClose: () => void
}

function verifyFailureMessage(factor: ReauthFactor, status: number | undefined): string {
  switch (status) {
    case 401:
      return factor === 'password'
        ? m['reauth.wrong_password']()
        : m['profile.phone_error_verify_failed']()
    case 409:
      return m['reauth.expired']()
    default:
      return m['reauth.failed']()
  }
}

export function ChangePhoneDialog({ currentMaskedPhone, onClose }: ChangePhoneDialogProps) {
  const queryClient = useQueryClient()
  const [step, setStep] = useState<DialogStep>('loading')
  const [phone, setPhone] = useState('')
  const [factors, setFactors] = useState<ReauthFactor[]>([])
  const [passkeyChallenge, setPasskeyChallenge] = useState<PasskeyChallenge | undefined>()
  const [selectedFactor, setSelectedFactor] = useState<ReauthFactor>('password')
  const [credential, setCredential] = useState('')
  const [reauthToken, setReauthToken] = useState<string | null>(null)
  const [stepError, setStepError] = useState<string | null>(null)
  // Backend-derived masked number shown on the code step; never computed here.
  const [maskedPhone, setMaskedPhone] = useState<string | null>(null)
  // The last submitted phone: a resend restart re-opens the phone step
  // pre-filled with it once verification passes again.
  const [lastPhone, setLastPhone] = useState('')
  const [code, setCode] = useState('')
  const { countdown, startCountdown, clearCountdown } = useCountdown()

  const beginMutation = useMutation({
    mutationFn: () => handleBeginReauth({ body: { targetOperation: 'change_phone' } }),
    onSuccess: (response) => {
      if (response.error) {
        setStep('begin-error')
        return
      }
      const available = response.data?.availableFactors ?? []
      setFactors(available)
      setPasskeyChallenge(
        (response.data?.challenge as PasskeyChallenge | undefined | null) ?? undefined
      )
      if (available.length === 0) {
        setStep('no-factors')
        return
      }
      setSelectedFactor(available[0])
      setStep('reauth')
    },
    onError: () => setStep('begin-error'),
  })
  const beginReauth = beginMutation.mutate

  // Every restart needs a fresh begin: the passkey challenge and factor
  // inventory are issued per attempt, and the previous ticket is consumed.
  const restartReauth = () => {
    setCredential('')
    setCode('')
    clearCountdown()
    setStep('reauth')
    beginReauth()
  }

  const sendMutation = useMutation({
    mutationFn: (phone: string) =>
      userPhoneBindSend({ body: { phone, reauthToken: reauthToken ?? '' } }),
    onSuccess: (response, phone) => {
      if (response.error) {
        const failure = resolveApiError(response.error)
        if (failure.status === 401 || failure.status === 409) {
          // Both 409 variants burn the single-use ticket (the backend
          // consumes it before the occupancy precheck), so every 401/409
          // restarts the reauth. The message distinguishes occupancy (try a
          // different number) from ticket-class (verify again).
          setReauthToken(null)
          setStepError(
            failure.code === 'phone_already_in_use'
              ? m['profile.phone_error_in_use']()
              : m['profile.phone_error_reauth_needed']()
          )
          restartReauth()
          return
        }
        if (failure.status === 429) {
          setStepError(m['profile.phone_error_rate_limited']())
          return
        }
        setStepError(getErrorMessage(response.error))
        return
      }
      setLastPhone(phone)
      setMaskedPhone(response.data?.maskedPhone ?? null)
      setStepError(null)
      setCode('')
      setStep('code')
      startCountdown(response.data?.expiresInSeconds ?? 0)
    },
    onError: () => setStepError(m['profile.phone_error_send_failed']()),
  })

  const confirmMutation = useMutation({
    mutationFn: (payload: { phone: string; code: string }) =>
      userPhoneBindConfirm({ body: { phone: payload.phone, code: payload.code } }),
    onSuccess: (response) => {
      if (response.error) {
        const { status } = resolveApiError(response.error)
        if (status === 409) {
          // Occupancy conflict (concurrent bind race landed first); the user
          // can go back and enter a different number.
          setStepError(m['profile.phone_error_in_use']())
          return
        }
        if (status === 429) {
          setStepError(m['profile.phone_error_rate_limited']())
          return
        }
        setStepError(getErrorMessage(response.error))
        return
      }
      setMaskedPhone(response.data?.maskedPhone ?? null)
      setStepError(null)
      clearCountdown()
      // The binding changed: refresh the Security-page gating matrix
      // (masked value + entry states) immediately.
      void queryClient.invalidateQueries({ queryKey: queryKeys.userPhoneStatus() })
      setStep('done')
    },
    // Confirm verifies a code, it sends none — generic fallback (onError only
    // fires on client-side faults; HTTP errors arrive as response.error above).
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
          targetOperation: 'change_phone',
          factor: payload.factor,
          password: payload.password ?? null,
          totpCode: payload.totpCode ?? null,
          passkeyAssertion: payload.passkeyAssertion ?? null,
        },
      }),
    onSuccess: (response, payload) => {
      if (response.error) {
        setStepError(verifyFailureMessage(payload.factor, resolveApiError(response.error).status))
        return
      }
      const token = response.data?.reauthToken
      if (!token) {
        setStepError(m['reauth.failed']())
        return
      }
      setReauthToken(token)
      setCredential('')
      setStepError(null)
      setStep('phone')
    },
    onError: () => setStepError(m['reauth.failed']()),
  })

  // The parent mounts this dialog on open and unmounts it on close, so every
  // open starts from the initial step state; the effect only fires the begin
  // call (state updates happen in the mutation callbacks).
  useEffect(() => {
    beginReauth()
  }, [beginReauth])

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

  const handleSendCode = () => {
    setStepError(null)
    sendMutation.mutate(phone.trim())
  }

  const handleConfirmCode = () => {
    if (code.length !== CODE_LENGTH) return
    setStepError(null)
    confirmMutation.mutate({ phone: lastPhone, code })
  }

  const isRebind = currentMaskedPhone !== null

  return (
    <Dialog open onOpenChange={handleOpenChange}>
      <DialogContent className="sm:max-w-lg" data-testid="phone-bind-dialog">
        <DialogHeader>
          <DialogTitle data-testid="phone-bind-dialog-title">
            {isRebind
              ? m['profile.phone_rebind_dialog_title']()
              : m['profile.phone_bind_dialog_title']()}
          </DialogTitle>
          <DialogDescription>{m['profile.phone_bind_dialog_description']()}</DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          {stepError && (
            <Alert variant="destructive">
              <AlertDescription data-testid="phone-bind-error-message">
                {stepError}
              </AlertDescription>
            </Alert>
          )}

          {step === 'loading' && (
            <p className="text-sm text-muted-foreground" data-testid="phone-bind-factor-loading">
              {m['profile.change_email_factor_loading']()}
            </p>
          )}

          {step === 'no-factors' && (
            <p className="text-sm text-muted-foreground" data-testid="phone-bind-no-factors">
              {m['profile.change_email_no_factors']()}
            </p>
          )}

          {step === 'begin-error' && (
            <div className="space-y-3">
              <p className="text-sm text-muted-foreground" data-testid="phone-bind-begin-error">
                {m['profile.change_email_begin_error']()}
              </p>
              <Button
                type="button"
                variant="outline"
                size="sm"
                data-testid="phone-bind-retry-button"
                onClick={() => {
                  setStepError(null)
                  beginReauth()
                }}
                disabled={beginMutation.isPending}
              >
                {m['common.retry']()}
              </Button>
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
                        id={`phone-bind-factor-${factor}`}
                        data-testid={`phone-bind-factor-${factor}-radio`}
                      />
                      <Label htmlFor={`phone-bind-factor-${factor}`}>
                        {FACTOR_LABELS[factor]()}
                      </Label>
                    </div>
                  ))}
                </RadioGroup>
              )}

              {selectedFactor === 'password' && (
                <div className="space-y-2">
                  <Label htmlFor="phone-bind-password-input">
                    {m['profile.phone_factor_password']()}
                  </Label>
                  <PasswordInput
                    id="phone-bind-password-input"
                    autoComplete="current-password"
                    value={credential}
                    onChange={(e) => setCredential(e.target.value)}
                    data-testid="phone-bind-password-input"
                  />
                </div>
              )}

              {selectedFactor === 'totp' && (
                <div className="space-y-2">
                  <Label htmlFor="phone-bind-totp-input">{m['profile.phone_factor_totp']()}</Label>
                  <Input
                    id="phone-bind-totp-input"
                    inputMode="numeric"
                    maxLength={6}
                    autoComplete="one-time-code"
                    value={credential}
                    onChange={(e) => setCredential(e.target.value)}
                    data-testid="phone-bind-totp-input"
                  />
                </div>
              )}

              {selectedFactor === 'passkey' ? (
                <Button
                  type="button"
                  variant="outline"
                  data-testid="phone-bind-passkey-button"
                  onClick={() => void handlePasskeyVerify()}
                  disabled={verifyMutation.isPending}
                >
                  {m['profile.phone_factor_passkey']()}
                </Button>
              ) : (
                <Button
                  type="button"
                  data-testid="phone-bind-verify-button"
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

          {step === 'phone' && (
            <div className="space-y-4">
              <div
                className="rounded-md border bg-muted/40 px-3 py-2 text-sm text-muted-foreground"
                data-testid="phone-bind-verified-banner"
              >
                {m['profile.phone_verified_as']({ factor: FACTOR_LABELS[selectedFactor]() })}
              </div>
              <div className="space-y-2">
                <Label htmlFor="phone-bind-phone-input">
                  {m['profile.phone_new_phone_label']()}
                </Label>
                <Input
                  id="phone-bind-phone-input"
                  type="tel"
                  autoComplete="tel"
                  value={phone}
                  onChange={(e) => setPhone(e.target.value)}
                  disabled={sendMutation.isPending}
                  data-testid="phone-bind-phone-input"
                />
                <p className="text-xs text-muted-foreground">{m['profile.phone_input_hint']()}</p>
              </div>
              <Button
                type="button"
                data-testid="phone-bind-send-btn"
                onClick={handleSendCode}
                disabled={sendMutation.isPending || phone.trim().length === 0}
                loading={sendMutation.isPending}
              >
                {sendMutation.isPending
                  ? m['profile.phone_sending']()
                  : m['profile.phone_send_code']()}
              </Button>
            </div>
          )}

          {step === 'code' && (
            <div className="space-y-4" data-testid="phone-bind-code-step">
              <div>
                <p className="text-sm text-muted-foreground">
                  {m['profile.phone_code_description']({ maskedPhone: maskedPhone ?? '' })}
                </p>
              </div>
              <div>
                <Label htmlFor="phone-bind-code-input">{m['profile.phone_code_label']()}</Label>
                <OtpCodeInput
                  value={code}
                  onChange={setCode}
                  numInputs={CODE_LENGTH}
                  testIdBase="phone-bind-code"
                  ariaLabel={m['profile.phone_code_label']()}
                />
              </div>
              <Button
                type="button"
                className="w-full"
                disabled={code.length !== CODE_LENGTH || confirmMutation.isPending}
                data-testid="phone-bind-confirm-btn"
                onClick={handleConfirmCode}
                loading={confirmMutation.isPending}
              >
                {confirmMutation.isPending
                  ? m['profile.phone_verifying']()
                  : m['profile.phone_confirm_bind']()}
              </Button>
              <div className="flex items-center gap-2 text-sm">
                {countdown !== null ? (
                  <span className="text-muted-foreground" data-testid="phone-bind-resend-countdown">
                    {m['profile.phone_resend_in']({ countdown })}
                  </span>
                ) : (
                  <Button
                    type="button"
                    variant="link"
                    className="h-auto p-0"
                    disabled={sendMutation.isPending || confirmMutation.isPending}
                    data-testid="phone-bind-resend-btn"
                    onClick={() => {
                      // bind/send consumes the single-use reauth ticket on
                      // every call — a resend needs a fresh one, so restart
                      // the reauth (lands back on the phone step pre-filled).
                      setStepError(null)
                      restartReauth()
                    }}
                  >
                    {m['profile.phone_resend']()}
                  </Button>
                )}
              </div>
              <Button
                type="button"
                variant="ghost"
                className="w-full"
                data-testid="phone-bind-back-to-phone-button"
                onClick={() => {
                  setCode('')
                  setStepError(null)
                  clearCountdown()
                  setStep('phone')
                }}
              >
                {m['profile.phone_back_to_phone']()}
              </Button>
            </div>
          )}

          {step === 'done' && (
            <div className="space-y-3" data-testid="phone-bind-done-panel">
              <p className="text-sm">
                {m['profile.phone_bind_done']({ maskedPhone: maskedPhone ?? '' })}
              </p>
              <Button
                type="button"
                data-testid="phone-bind-done-button"
                onClick={() => handleOpenChange(false)}
              >
                {m['profile.phone_done_button']()}
              </Button>
            </div>
          )}
        </div>

        {step !== 'done' && (
          <div>
            <Button
              type="button"
              variant="outline"
              data-testid="phone-bind-cancel-button"
              onClick={() => handleOpenChange(false)}
              disabled={
                sendMutation.isPending || confirmMutation.isPending || verifyMutation.isPending
              }
            >
              {m['common.cancel']()}
            </Button>
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}
