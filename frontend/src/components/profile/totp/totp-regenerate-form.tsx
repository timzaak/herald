import { useState } from 'react'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import { handleRegenerateTotp, handleVerifyTotpSetup } from '@/lib/api-generated'
import { obtainReauthToken } from '@/lib/reauth-flow'
import { useFormMutation } from '@/hooks/use-form-mutation'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { PasswordInput } from '@/components/ui/password-input'
import { Label } from '@/components/ui/label'
import { QRCodeCanvas } from 'qrcode.react'
import { BackupCodesDisplay } from './backup-codes-display'
import { getFieldErrorMessage } from '@/lib/form-utils'
import { withTimeout, type TotpData } from '@/lib/totp-utils'
import { z } from 'zod'
import { m } from '@/paraglide/messages'
import type { RegenerateTotpResponse, VerifyTotpSetupResponse } from '@/lib/api-generated'

const regenerateTotpStep1Schema = z.object({
  password: z.string().min(1, m['profile.totp_password_required']()),
})

const regenerateTotpStep2Schema = z.object({
  code: z.string().length(6, m['profile.totp_code_must_be_6_digits']()),
})

interface TotpRegenerateFormProps {
  onSuccess: () => void
  onCancel: () => void
}

type RegenerateStep = 'confirm' | 'verify'

function getSubmitButtonText(isSubmitting: boolean, step: RegenerateStep): string {
  if (step === 'confirm') {
    return isSubmitting ? m['profile.totp_regenerating']() : m['profile.totp_regenerate_button']()
  }
  return isSubmitting ? m['profile.totp_verifying']() : m['profile.totp_verify_new_title']()
}

export function TotpRegenerateForm({ onSuccess, onCancel }: TotpRegenerateFormProps) {
  const [step, setStep] = useState<RegenerateStep>('confirm')
  const [regeneratedData, setRegeneratedData] = useState<TotpData | null>(null)

  const regenerateMutation = useFormMutation({
    mutationFn: async (data: { password: string }) => {
      // Bind-authenticator reauth: obtain a single-use ticket with the user's
      // password, then regenerate the TOTP secret with it.
      const reauth_token = await obtainReauthToken('bind_authenticator', data.password)
      const response = await withTimeout(handleRegenerateTotp({ body: { ...data, reauth_token } }))
      return response.data as RegenerateTotpResponse
    },
    getSuccessMessage: () => m['profile.totp_regenerated_success'](),
    onSuccess: (data) => {
      setRegeneratedData({
        secret: data.secret,
        qrCodeUrl: data.qrCodeUrl,
        backupCodes: data.backupCodes,
        tempToken: data.tempToken,
      })
      setStep('verify')
    },
  })

  const confirmForm = useAppForm({
    schema: regenerateTotpStep1Schema,
    defaultValues: { password: '' },
    onSubmit: async ({ value }) => {
      void regenerateMutation.mutate(value)
    },
  })

  const verifyMutation = useFormMutation({
    mutationFn: async (data: { code: string; tempToken: string }) => {
      const response = await withTimeout(handleVerifyTotpSetup({ body: data }))
      return response.data as VerifyTotpSetupResponse
    },
    getSuccessMessage: () => m['profile.totp_verified_success'](),
    onSuccess: () => {
      onSuccess()
    },
  })

  const verifyForm = useAppForm({
    schema: regenerateTotpStep2Schema,
    defaultValues: { code: '' },
    onSubmit: async ({ value }) => {
      if (!regeneratedData) return
      void verifyMutation.mutate({
        code: value.code,
        tempToken: regeneratedData.tempToken,
      })
    },
  })

  if (step === 'confirm') {
    return (
      <div className="space-y-4" data-testid="totp-regenerate-form-confirm">
        <h2 className="text-2xl font-bold">{m['profile.totp_regenerate_title']()}</h2>
        <p className="text-muted-foreground">{m['profile.totp_regenerate_description']()}</p>

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
                  <Label htmlFor="password">{m['profile.current_password_label']()}</Label>
                  <PasswordInput
                    id="password"
                    value={field.state.value ?? ''}
                    onChange={(e) => field.handleChange(e.target.value)}
                    data-testid="totp-regenerate-password-input"
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
                data-testid="totp-regenerate-cancel-button"
              >
                {m['common.cancel']()}
              </Button>
              <Button
                type="submit"
                disabled={regenerateMutation.isSubmitting}
                data-testid="totp-regenerate-submit-button"
              >
                {getSubmitButtonText(regenerateMutation.isSubmitting, 'confirm')}
              </Button>
            </div>
          </form>
        </AppForm>
      </div>
    )
  }

  return (
    <div className="space-y-4" data-testid="totp-regenerate-form-verify">
      <h2 className="text-2xl font-bold">{m['profile.totp_verify_new_title']()}</h2>
      <p className="text-muted-foreground">{m['profile.totp_verify_new_description']()}</p>

      {regeneratedData && (
        <div className="flex justify-center" data-testid="totp-regenerate-qr-code-container">
          <QRCodeCanvas
            value={regeneratedData.qrCodeUrl}
            size={200}
            data-testid="totp-regenerate-qr-code"
          />
        </div>
      )}

      {regeneratedData && <BackupCodesDisplay backupCodes={regeneratedData.backupCodes} />}

      <AppForm>
        <form
          onSubmit={(e) => {
            e.preventDefault()
            e.stopPropagation()
            verifyForm.handleSubmit()
          }}
          className="space-y-4"
        >
          <verifyForm.Field name="code">
            {(field) => (
              <div className="space-y-2">
                <Label htmlFor="code">{m['profile.verification_code_label']()}</Label>
                <Input
                  id="code"
                  type="text"
                  inputMode="numeric"
                  pattern="[0-9]{6}"
                  maxLength={6}
                  value={field.state.value ?? ''}
                  onChange={(e) => field.handleChange(e.target.value)}
                  data-testid="totp-regenerate-verify-code-input"
                  placeholder="000000"
                  autoFocus
                />
                {(field.state.meta.isTouched || verifyForm.state.isSubmitted) &&
                  field.state.meta.errors.length > 0 && (
                    <p className="text-sm text-destructive">
                      {getFieldErrorMessage(field.state.meta)}
                    </p>
                  )}
              </div>
            )}
          </verifyForm.Field>

          <div className="flex space-x-2">
            <Button
              type="button"
              variant="outline"
              onClick={onCancel}
              data-testid="totp-regenerate-verify-cancel-button"
            >
              {m['common.cancel']()}
            </Button>
            <Button
              type="submit"
              disabled={verifyMutation.isSubmitting || !verifyForm.state.canSubmit}
              data-testid="totp-regenerate-verify-button"
            >
              {getSubmitButtonText(verifyMutation.isSubmitting, 'verify')}
            </Button>
          </div>
        </form>
      </AppForm>
    </div>
  )
}
