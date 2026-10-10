import { useEffect, useState } from 'react'
import { useMutation } from '@tanstack/react-query'
import { z } from 'zod'
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
import { Label } from '@/components/ui/label'
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import { TextField } from '@/components/shared/form-fields/text-field'
import {
  changeEmailRequest,
  handleBeginReauth,
  handleVerifyReauth,
  type ReauthFactor,
} from '@/lib/api-generated'
import { prepareRequestOptions, serializeAssertion } from '@/lib/passkey-utils'
import { resolveApiError } from '@/lib/error-utils'
import { useResolvedRealmId } from '@/lib/realm-routing'
import { emailSchema } from '@/lib/schemas/common'
import { m } from '@/paraglide/messages'

/** The begin response's `challenge` payload when the passkey factor is available. */
interface PasskeyChallenge {
  challengeToken: string
  options: unknown
}

const FACTOR_LABELS: Record<ReauthFactor, () => string> = {
  password: m['profile.change_email_factor_password'],
  totp: m['profile.change_email_factor_totp'],
  passkey: m['profile.change_email_factor_passkey'],
}

type DialogStep = 'loading' | 'no-factors' | 'begin-error' | 'reauth' | 'email' | 'sent'

interface ChangeEmailDialogProps {
  currentEmail: string
  onClose: () => void
}

function verifyFailureMessage(factor: ReauthFactor, status: number | undefined): string {
  switch (status) {
    case 401:
      return factor === 'password'
        ? m['reauth.wrong_password']()
        : m['profile.change_email_error_verify_failed']()
    case 409:
      return m['reauth.expired']()
    default:
      return m['reauth.failed']()
  }
}

function requestFailureMessage(status: number | undefined): string {
  switch (status) {
    case 400:
      return m['profile.change_email_error_not_configured']()
    case 429:
      return m['profile.change_email_error_rate_limited']()
    default:
      return m['profile.change_email_error_send_failed']()
  }
}

export function ChangeEmailDialog({ currentEmail, onClose }: ChangeEmailDialogProps) {
  const realmId = useResolvedRealmId()
  const [step, setStep] = useState<DialogStep>('loading')
  const [factors, setFactors] = useState<ReauthFactor[]>([])
  const [passkeyChallenge, setPasskeyChallenge] = useState<PasskeyChallenge | undefined>()
  const [selectedFactor, setSelectedFactor] = useState<ReauthFactor>('password')
  const [credential, setCredential] = useState('')
  const [reauthToken, setReauthToken] = useState<string | null>(null)
  const [stepError, setStepError] = useState<string | null>(null)
  const [sentTo, setSentTo] = useState<string | null>(null)
  // The last submitted address: a resend restart re-opens the email step
  // pre-filled with it once verification passes again.
  const [lastEmail, setLastEmail] = useState('')

  const beginMutation = useMutation({
    mutationFn: () => handleBeginReauth({ body: { targetOperation: 'change_email' } }),
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
    setStep('reauth')
    beginReauth()
  }

  const requestMutation = useMutation({
    mutationFn: (newEmail: string) =>
      changeEmailRequest({
        path: { realmId },
        body: { newEmail, reauthToken: reauthToken ?? '' },
      }),
    onSuccess: (response, newEmail) => {
      if (response.error) {
        const { status } = resolveApiError(response.error)
        if (status === 401 || status === 409) {
          // The request endpoint has no email-conflict precheck (conflict is
          // surfaced only by the confirm step), so a 409 here is always
          // ticket-class: the single-use ticket was consumed or mismatched.
          setReauthToken(null)
          setStepError(m['profile.change_email_error_reauth_needed']())
          restartReauth()
          return
        }
        setStepError(requestFailureMessage(status))
        return
      }
      setLastEmail(newEmail)
      setSentTo(newEmail)
      setStepError(null)
      setStep('sent')
    },
    onError: () => setStepError(m['profile.change_email_error_send_failed']()),
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
          targetOperation: 'change_email',
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
      setStep('email')
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
      setStepError(m['profile.change_email_error_verify_failed']())
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
          : m['profile.change_email_totp_required']()
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
      <DialogContent className="sm:max-w-lg" data-testid="change-email-dialog">
        <DialogHeader>
          <DialogTitle data-testid="change-email-dialog-title">
            {m['profile.change_email_dialog_title']()}
          </DialogTitle>
          <DialogDescription>{m['profile.change_email_dialog_description']()}</DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          {stepError && (
            <Alert variant="destructive">
              <AlertDescription data-testid="change-email-error-message">
                {stepError}
              </AlertDescription>
            </Alert>
          )}

          {step === 'loading' && (
            <p className="text-sm text-muted-foreground" data-testid="change-email-factor-loading">
              {m['profile.change_email_factor_loading']()}
            </p>
          )}

          {step === 'no-factors' && (
            <p className="text-sm text-muted-foreground" data-testid="change-email-no-factors">
              {m['profile.change_email_no_factors']()}
            </p>
          )}

          {step === 'begin-error' && (
            <div className="space-y-3">
              <p className="text-sm text-muted-foreground" data-testid="change-email-begin-error">
                {m['profile.change_email_begin_error']()}
              </p>
              <Button
                type="button"
                variant="outline"
                size="sm"
                data-testid="change-email-retry-button"
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
                        id={`change-email-factor-${factor}`}
                        data-testid={`change-email-factor-${factor}-radio`}
                      />
                      <Label htmlFor={`change-email-factor-${factor}`}>
                        {FACTOR_LABELS[factor]()}
                      </Label>
                    </div>
                  ))}
                </RadioGroup>
              )}

              {selectedFactor === 'password' && (
                <div className="space-y-2">
                  <Label htmlFor="change-email-password-input">
                    {m['profile.change_email_factor_password']()}
                  </Label>
                  <Input
                    id="change-email-password-input"
                    type="password"
                    autoComplete="current-password"
                    value={credential}
                    onChange={(e) => setCredential(e.target.value)}
                    data-testid="change-email-password-input"
                  />
                </div>
              )}

              {selectedFactor === 'totp' && (
                <div className="space-y-2">
                  <Label htmlFor="change-email-totp-input">
                    {m['profile.change_email_factor_totp']()}
                  </Label>
                  <Input
                    id="change-email-totp-input"
                    inputMode="numeric"
                    maxLength={6}
                    autoComplete="one-time-code"
                    value={credential}
                    onChange={(e) => setCredential(e.target.value)}
                    data-testid="change-email-totp-input"
                  />
                </div>
              )}

              {selectedFactor === 'passkey' ? (
                <Button
                  type="button"
                  variant="outline"
                  data-testid="change-email-passkey-button"
                  onClick={() => void handlePasskeyVerify()}
                  disabled={verifyMutation.isPending}
                >
                  {m['profile.change_email_factor_passkey']()}
                </Button>
              ) : (
                <Button
                  type="button"
                  data-testid="change-email-verify-button"
                  onClick={handleVerify}
                  disabled={verifyMutation.isPending}
                  loading={verifyMutation.isPending}
                >
                  {verifyMutation.isPending
                    ? m['profile.change_email_verifying']()
                    : m['profile.change_email_verify_button']()}
                </Button>
              )}
            </div>
          )}

          {step === 'email' && (
            <div className="space-y-4">
              <div
                className="rounded-md border bg-muted/40 px-3 py-2 text-sm text-muted-foreground"
                data-testid="change-email-verified-banner"
              >
                {m['profile.change_email_verified_as']({
                  factor: FACTOR_LABELS[selectedFactor](),
                })}
              </div>
              {/* Mounts fresh each time the step is (re)entered, so a resend
                  restart pre-fills the last submitted address through
                  defaultValues — form.reset(values) would overwrite the
                  defaultValues slot and race the next render's form update. */}
              <EmailStep
                currentEmail={currentEmail}
                initialEmail={lastEmail}
                submitting={requestMutation.isPending}
                onSubmitEmail={(newEmail) => {
                  setStepError(null)
                  requestMutation.mutate(newEmail)
                }}
              />
            </div>
          )}

          {step === 'sent' && (
            <div className="space-y-3" data-testid="change-email-sent-panel">
              <p className="text-sm">
                {m['profile.change_email_sent_notice']({ email: sentTo ?? '' })}
              </p>
              <p className="text-sm text-muted-foreground">
                {m['profile.change_email_old_mailbox_note']()}
              </p>
              <div className="flex gap-2">
                <Button
                  type="button"
                  variant="outline"
                  data-testid="change-email-resend-button"
                  onClick={() => {
                    setStepError(null)
                    restartReauth()
                  }}
                >
                  {m['profile.change_email_resend_button']()}
                </Button>
                <Button
                  type="button"
                  data-testid="change-email-done-button"
                  onClick={() => handleOpenChange(false)}
                >
                  {m['profile.change_email_done_button']()}
                </Button>
              </div>
            </div>
          )}
        </div>

        <div>
          <Button
            type="button"
            variant="outline"
            data-testid="change-email-cancel-button"
            onClick={() => handleOpenChange(false)}
            disabled={requestMutation.isPending || verifyMutation.isPending}
          >
            {m['common.cancel']()}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}

function EmailStep({
  currentEmail,
  initialEmail,
  submitting,
  onSubmitEmail,
}: {
  currentEmail: string
  initialEmail: string
  submitting: boolean
  onSubmitEmail: (newEmail: string) => void
}) {
  const form = useAppForm({
    schema: z.object({
      newEmail: emailSchema.refine(
        (value) => value.trim().toLowerCase() !== currentEmail.trim().toLowerCase(),
        { error: () => m['profile.change_email_error_same_email']() }
      ),
    }),
    defaultValues: { newEmail: initialEmail },
    onSubmit: async ({ value }) => onSubmitEmail(value.newEmail),
  })

  return (
    <AppForm>
      <form
        onSubmit={(e) => {
          e.preventDefault()
          e.stopPropagation()
          form.handleSubmit()
        }}
        className="space-y-4"
      >
        <TextField
          form={form}
          name="newEmail"
          label={m['profile.change_email_new_email_label']()}
          type="email"
          required
          dataTestId="change-email-new-email-input"
        />
        <Button
          type="submit"
          data-testid="change-email-submit-button"
          disabled={submitting}
          loading={submitting}
        >
          {submitting
            ? m['profile.change_email_submitting']()
            : m['profile.change_email_submit_button']()}
        </Button>
      </form>
    </AppForm>
  )
}
