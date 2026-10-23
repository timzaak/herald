import { useState } from 'react'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { PasswordInput } from '@/components/ui/password-input'
import { Label } from '@/components/ui/label'
import { getFieldErrorMessage } from '@/lib/form-utils'
import { withTimeout } from '@/lib/totp-utils'
import {
  isWebAuthnSupported,
  prepareCreationOptions,
  serializeAttestation,
} from '@/lib/passkey-utils'
import {
  handleBeginPasskeyRegistration,
  handleFinishPasskeyRegistration,
} from '@/lib/api-generated'
import { obtainReauthToken } from '@/lib/reauth-flow'
import type { BeginRegistrationResponse } from '@/lib/api-generated'
import { useFormMutation } from '@/hooks/use-form-mutation'
import { queryKeys } from '@/data/query-options'
import { z } from 'zod'
import { m } from '@/paraglide/messages'

interface PasskeyRegisterFormProps {
  onSuccess: () => void
  onCancel: () => void
}

type RegisterStep = 'confirm' | 'name'

const step1Schema = z.object({
  password: z.string().min(1, m['profile.passkey_register_password_required']()),
})

const step2Schema = z.object({
  nickname: z.string().min(1).max(128),
})

/**
 * Two-step Passkey registration form.
 *
 * Step 1 (confirm): verify current password → call registration/begin to get
 *   the WebAuthn creation options → invoke `navigator.credentials.create` →
 *   serialise the resulting attestation.
 * Step 2 (name): let the user name the device → call registration/finish with
 *   the regToken + attestation → invalidate the passkey list query.
 *
 * All generated API calls are wrapped in `withTimeout`. Errors are mapped to a
 * generic "registration failed" message — backend details are never surfaced.
 * A user-initiated cancellation of the browser prompt is treated silently.
 */
export function PasskeyRegisterForm({ onSuccess, onCancel }: PasskeyRegisterFormProps) {
  const webAuthnSupported = isWebAuthnSupported()
  const [step, setStep] = useState<RegisterStep>('confirm')
  const [regToken, setRegToken] = useState<string>('')
  const [attestation, setAttestation] = useState<unknown>(null)
  // The user's current password is carried from the confirm step to the name
  // step so the finish call can obtain its OWN single-use reauth ticket. The
  // backend consumes the ticket on BOTH begin and finish (one ticket each), so
  // a ticket obtained for begin CANNOT be reused for finish (design §4.2.2).
  const [password, setPassword] = useState<string>('')

  const beginMutation = useFormMutation({
    mutationFn: async (data: { password: string }) => {
      // Bind-authenticator reauth: obtain a single-use ticket with the user's
      // password, then begin passkey registration with it.
      const token = await obtainReauthToken('bind_authenticator', data.password)
      const response = await withTimeout(
        handleBeginPasskeyRegistration({ body: { reauthToken: token } })
      )
      if (response.error) {
        // Map every backend failure (401 bad password, 422, 409, …) to a single
        // generic message — never surface backend details.
        throw new Error(m['profile.passkey_register_failed']())
      }
      return response.data as BeginRegistrationResponse
    },
    // Suppress the default success toast here — the credential prompt is silent
    // and a premature "success" would be confusing. Finish shows the real one.
    getSuccessMessage: () => '',
    onSuccess: async (data) => {
      try {
        const credential = await navigator.credentials.create(prepareCreationOptions(data.options))
        if (!credential) {
          // No credential returned — treat like a silent cancellation.
          return
        }
        setRegToken(data.regToken)
        setAttestation(serializeAttestation(credential as PublicKeyCredential))
        setStep('name')
      } catch {
        // User cancelled the browser prompt (or it was otherwise aborted).
        // Swallow silently per US-PK-004 — do not surface an error.
      }
    },
  })

  const finishMutation = useFormMutation({
    mutationFn: async (data: { regToken: string; attestation: unknown }) => {
      // Finish obtains its OWN fresh single-use ticket — the begin ticket was
      // already consumed by the backend's begin handler.
      const token = await obtainReauthToken('bind_authenticator', password)
      const response = await withTimeout(
        handleFinishPasskeyRegistration({
          body: { regToken: data.regToken, attestation: data.attestation, reauthToken: token },
        })
      )
      if (response.error) {
        throw new Error(m['profile.passkey_register_failed']())
      }
      return response.data
    },
    getSuccessMessage: () => m['profile.passkey_register_success'](),
    invalidateQueries: [queryKeys.passkeyList()],
    onSuccess: () => {
      onSuccess()
    },
  })

  const confirmForm = useAppForm({
    schema: step1Schema,
    defaultValues: { password: '' },
    onSubmit: async ({ value }) => {
      setPassword(value.password)
      void beginMutation.mutate(value)
    },
  })

  const nameForm = useAppForm({
    schema: step2Schema,
    defaultValues: { nickname: '' },
    onSubmit: async () => {
      void finishMutation.mutate({ regToken, attestation })
    },
  })

  if (!webAuthnSupported) {
    return (
      <div className="space-y-4" data-testid="passkey-register-form">
        <h2 className="text-base font-semibold">{m['profile.passkey_register_title']()}</h2>
        <p className="text-sm text-muted-foreground" data-testid="passkey-unsupported-message">
          {m['profile.passkey_unsupported']()}
        </p>
        <Button type="button" variant="outline" onClick={onCancel}>
          {m['profile.passkey_register_cancel']()}
        </Button>
      </div>
    )
  }

  if (step === 'confirm') {
    return (
      <div className="space-y-4" data-testid="passkey-register-form">
        <h2 className="text-base font-semibold">{m['profile.passkey_register_title']()}</h2>
        <p className="text-muted-foreground">{m['profile.passkey_register_description']()}</p>

        <AppForm>
          <form
            onSubmit={(e) => {
              e.preventDefault()
              e.stopPropagation()
              confirmForm.handleSubmit()
            }}
            className="space-y-4"
          >
            <confirmForm.Field name="password">
              {(field) => (
                <div className="space-y-2">
                  <Label htmlFor="passkey-password">
                    {m['profile.passkey_register_password_label']()}
                  </Label>
                  <PasswordInput
                    id="passkey-password"
                    value={field.state.value ?? ''}
                    onChange={(e) => field.handleChange(e.target.value)}
                    data-testid="passkey-register-password-input"
                  />
                  {(field.state.meta.isTouched || confirmForm.state.isSubmitted) &&
                    field.state.meta.errors.length > 0 && (
                      <p className="text-sm text-destructive">
                        {getFieldErrorMessage(field.state.meta)}
                      </p>
                    )}
                </div>
              )}
            </confirmForm.Field>

            <div className="flex space-x-2">
              <Button
                type="button"
                variant="outline"
                onClick={onCancel}
                data-testid="passkey-register-cancel-button"
              >
                {m['profile.passkey_register_cancel']()}
              </Button>
              <Button
                type="submit"
                disabled={beginMutation.isSubmitting}
                data-testid="passkey-register-submit-button"
              >
                {beginMutation.isSubmitting
                  ? m['profile.passkey_register_registering']()
                  : m['profile.passkey_register_submit']()}
              </Button>
            </div>
          </form>
        </AppForm>
      </div>
    )
  }

  // Step 2: name the passkey, then finish.
  return (
    <div className="space-y-4" data-testid="passkey-register-form">
      <h2 className="text-base font-semibold">{m['profile.passkey_register_step2']()}</h2>
      <p className="text-muted-foreground">{m['profile.passkey_register_description']()}</p>

      <AppForm>
        <form
          onSubmit={(e) => {
            e.preventDefault()
            e.stopPropagation()
            nameForm.handleSubmit()
          }}
          className="space-y-4"
        >
          <nameForm.Field name="nickname">
            {(field) => (
              <div className="space-y-2">
                <Label htmlFor="passkey-nickname">
                  {m['profile.passkey_register_nickname_label']()}
                </Label>
                <Input
                  id="passkey-nickname"
                  type="text"
                  value={field.state.value ?? ''}
                  onChange={(e) => field.handleChange(e.target.value)}
                  placeholder={m['profile.passkey_register_nickname_placeholder']()}
                  data-testid="passkey-rename-input"
                  autoFocus
                />
                {(field.state.meta.isTouched || nameForm.state.isSubmitted) &&
                  field.state.meta.errors.length > 0 && (
                    <p className="text-sm text-destructive">
                      {getFieldErrorMessage(field.state.meta)}
                    </p>
                  )}
              </div>
            )}
          </nameForm.Field>

          <div className="flex space-x-2">
            <Button
              type="button"
              variant="outline"
              onClick={onCancel}
              data-testid="passkey-register-cancel-button"
            >
              {m['profile.passkey_register_cancel']()}
            </Button>
            <Button
              type="submit"
              disabled={finishMutation.isSubmitting}
              data-testid="passkey-register-submit-button"
            >
              {finishMutation.isSubmitting
                ? m['profile.passkey_register_finishing']()
                : m['profile.passkey_register_finish']()}
            </Button>
          </div>
        </form>
      </AppForm>
    </div>
  )
}
