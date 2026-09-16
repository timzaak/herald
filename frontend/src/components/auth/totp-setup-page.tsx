import { useState, useRef, useCallback } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { useRealmId } from '@/stores/auth-store'
import { QRCodeCanvas } from 'qrcode.react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Checkbox } from '@/components/ui/checkbox'
import { BackupCodesDisplay } from '@/components/profile/totp/backup-codes-display'
import { handleEnableTotp, handleVerifyTotpSetup } from '@/lib/api-generated'
import { obtainReauthToken } from '@/lib/reauth-flow'
import { useFormMutation } from '@/hooks/use-form-mutation'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import { getFieldErrorMessage } from '@/lib/form-utils'
import { withTimeout, type TotpData } from '@/lib/totp-utils'
import { QUERY_KEYS } from '@/lib/constants'
import { z } from 'zod'
import { Loader2, ArrowLeft } from 'lucide-react'
import type { EnableTotpResponse, VerifyTotpSetupResponse } from '@/lib/api-generated'

type SetupStep = 'password' | 'qr-code' | 'verify'

const STEP_NUMBER: Record<SetupStep, number> = {
  password: 1,
  'qr-code': 2,
  verify: 3,
}

const passwordSchema = z.object({
  password: z.string().min(1, 'Password is required'),
})

export function TotpSetupPage() {
  const navigate = useNavigate()
  const realmId = useRealmId()
  const queryClient = useQueryClient()

  const [step, setStep] = useState<SetupStep>('password')
  const [setupData, setSetupData] = useState<TotpData | null>(null)
  const [savedBackupCodes, setSavedBackupCodes] = useState(false)
  const [verificationCode, setVerificationCode] = useState('')

  // Step 1: Password confirmation to generate TOTP secret
  const generateMutation = useFormMutation<EnableTotpResponse, { password: string }>({
    mutationFn: async (data) => {
      // Bind-authenticator reauth: obtain a single-use ticket with the user's
      // password, then enable TOTP with it.
      const reauth_token = await obtainReauthToken('bind_authenticator', data.password)
      const response = await withTimeout(handleEnableTotp({ body: { ...data, reauth_token } }))
      if (response.error) {
        throw response.error
      }
      return response.data as EnableTotpResponse
    },
    getSuccessMessage: () => 'TOTP secret generated successfully',
    onSuccess: (data) => {
      setSetupData({
        secret: data.secret,
        qrCodeUrl: data.qrCodeUrl,
        backupCodes: data.backupCodes,
        tempToken: data.tempToken,
      })
      setStep('qr-code')
    },
  })

  const passwordForm = useAppForm({
    schema: passwordSchema,
    defaultValues: { password: '' },
    onSubmit: async ({ value }) => {
      void generateMutation.mutate(value)
    },
  })

  // Step 3: Verify TOTP code
  const verifyMutation = useFormMutation<
    VerifyTotpSetupResponse,
    { code: string; tempToken: string }
  >({
    mutationFn: async (data) => {
      const response = await withTimeout(handleVerifyTotpSetup({ body: data }))
      if (response.error) {
        throw response.error
      }
      return response.data as VerifyTotpSetupResponse
    },
    invalidateQueries: [[QUERY_KEYS.TOTP_STATUS]],
    getSuccessMessage: () => 'TOTP enabled successfully',
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [QUERY_KEYS.TOTP_STATUS] })
      navigate({ to: '/$realmId/user/security', params: { realmId } })
    },
  })

  const handleCheckboxChange = useCallback((checked: boolean | string) => {
    setSavedBackupCodes(checked === true)
  }, [])

  const isVerifyDisabled =
    verificationCode.length !== 6 || !savedBackupCodes || verifyMutation.isSubmitting

  const currentStepNumber = STEP_NUMBER[step]

  return (
    <div
      className="container max-w-2xl mx-auto py-6 px-0 space-y-6 md:px-6"
      data-testid="totp-setup-page"
    >
      {/* Header */}
      <div className="flex items-center gap-4">
        <Button
          variant="ghost"
          size="icon"
          onClick={() => navigate({ to: '/$realmId/user/security', params: { realmId } })}
          data-testid="totp-setup-back-to-security"
          aria-label="Back to security settings"
        >
          <ArrowLeft className="h-5 w-5" />
        </Button>
        <div className="flex-1">
          <h1 className="text-xl font-semibold tracking-tight" data-testid="totp-setup-page-title">
            Set Up Two-Factor Authentication
          </h1>
          <p
            className="text-sm text-muted-foreground mt-1"
            data-testid="totp-setup-page-description"
          >
            Step {currentStepNumber} of 3
          </p>
        </div>
      </div>

      {/* Step indicator */}
      <div
        className="flex gap-2"
        role="progressbar"
        aria-valuenow={currentStepNumber}
        aria-valuemin={1}
        aria-valuemax={3}
        data-testid="totp-setup-step-indicator"
      >
        {[1, 2, 3].map((s) => (
          <div
            key={s}
            className={`h-1.5 flex-1 rounded-full transition-colors ${
              s <= currentStepNumber ? 'bg-primary' : 'bg-muted'
            }`}
          />
        ))}
      </div>

      {/* Main content area */}
      <div role="region" aria-live="polite" aria-atomic="true">
        {/* Step 1: Password Confirmation */}
        {step === 'password' && (
          <div
            className="space-y-4 animate-in fade-in slide-in-from-bottom-2 duration-200"
            data-testid="totp-setup-step-password"
            role="group"
            aria-labelledby="totp-password-heading"
          >
            <p id="totp-password-heading" className="text-sm text-muted-foreground">
              Enter your current password to begin setting up two-factor authentication.
            </p>
            <AppForm>
              <form
                onSubmit={(e) => {
                  e.preventDefault()
                  e.stopPropagation()
                  passwordForm.handleSubmit()
                }}
                className="space-y-4"
              >
                <passwordForm.Field name="password">
                  {(field) => (
                    <div className="space-y-2">
                      <Label htmlFor="totp-password">Current Password</Label>
                      <Input
                        id="totp-password"
                        type="password"
                        value={field.state.value ?? ''}
                        onChange={(e) => field.handleChange(e.target.value)}
                        data-testid="totp-setup-password-input"
                        autoFocus
                        aria-describedby="totp-password-hint"
                        aria-invalid={field.state.meta.errors.length > 0}
                        aria-required="true"
                      />
                      <p id="totp-password-hint" className="text-xs text-muted-foreground">
                        Enter your password to generate a TOTP secret
                      </p>
                      {(field.state.meta.isTouched || passwordForm.state.isSubmitted) &&
                        field.state.meta.errors.length > 0 && (
                          <p
                            className="text-sm text-destructive"
                            data-testid="totp-password-error"
                            role="alert"
                          >
                            {getFieldErrorMessage(field.state.meta)}
                          </p>
                        )}
                    </div>
                  )}
                </passwordForm.Field>
              </form>
            </AppForm>
          </div>
        )}

        {/* Step 2: QR Code Display */}
        {step === 'qr-code' && setupData && (
          <div
            className="space-y-6 animate-in fade-in slide-in-from-bottom-2 duration-200"
            data-testid="totp-setup-step-qr-code"
            role="group"
            aria-labelledby="totp-qr-heading"
          >
            <p id="totp-qr-heading" className="text-sm text-muted-foreground">
              Scan the QR code below with your authenticator app (e.g., Google Authenticator,
              Authy).
            </p>

            {/* QR Code Card */}
            <div className="flex justify-center">
              <div
                className="inline-flex items-center justify-center p-4 bg-white rounded-xl border-2 border-border shadow-sm"
                data-testid="totp-qr-code-container"
                data-secret={setupData.secret}
                role="img"
                aria-label="QR code for TOTP setup"
              >
                <QRCodeCanvas
                  value={setupData.qrCodeUrl}
                  size={200}
                  level="M"
                  data-testid="totp-qr-code"
                />
              </div>
            </div>

            {/* Backup Codes */}
            <BackupCodesDisplay backupCodes={setupData.backupCodes} />

            {/* Backup Codes Confirmation */}
            <div className="flex items-center space-x-2">
              <Checkbox
                id="saved-backup-codes"
                checked={savedBackupCodes}
                onCheckedChange={handleCheckboxChange}
                data-testid="totp-saved-backup-codes-checkbox"
                aria-required="true"
                aria-describedby="saved-backup-codes-label"
              />
              <Label
                htmlFor="saved-backup-codes"
                data-testid="totp-saved-backup-codes-label"
                id="saved-backup-codes-label"
              >
                I have saved my backup codes in a secure location
              </Label>
            </div>
          </div>
        )}

        {/* Step 3: Verification Code */}
        {step === 'verify' && (
          <div
            className="space-y-6 animate-in fade-in slide-in-from-bottom-2 duration-200"
            data-testid="totp-setup-step-verify"
            role="group"
            aria-labelledby="totp-verify-heading"
          >
            <p id="totp-verify-heading" className="text-sm text-muted-foreground">
              Enter the 6-digit verification code from your authenticator app to complete setup.
            </p>

            <div className="space-y-2">
              <Label htmlFor="totp-verification-code">Verification Code</Label>
              <OtpCodeInput
                value={verificationCode}
                onChange={setVerificationCode}
                disabled={verifyMutation.isSubmitting}
              />
            </div>

            {verifyMutation.isSubmitting && (
              <div
                className="flex items-center gap-2 text-sm text-muted-foreground"
                data-testid="totp-verify-loading"
                role="status"
                aria-live="polite"
              >
                <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
                Verifying code...
              </div>
            )}
          </div>
        )}
      </div>

      {/* Footer navigation */}
      <div
        className="flex flex-wrap justify-between gap-2 pt-4"
        role="navigation"
        aria-label="TOTP setup navigation"
      >
        {step === 'password' && (
          <>
            <div />
            <Button
              onClick={() => passwordForm.handleSubmit()}
              disabled={generateMutation.isSubmitting}
              data-testid="totp-setup-generate-button"
              aria-label="Generate QR code for TOTP setup"
            >
              {generateMutation.isSubmitting ? (
                <>
                  <Loader2 className="h-4 w-4 mr-2 animate-spin" aria-hidden="true" />
                  Generating...
                </>
              ) : (
                'Generate QR Code'
              )}
            </Button>
          </>
        )}

        {step === 'qr-code' && (
          <>
            <Button
              variant="outline"
              onClick={() => {
                setStep('password')
                setSetupData(null)
                setSavedBackupCodes(false)
              }}
              data-testid="totp-setup-back-button"
              aria-label="Go back to password step"
            >
              Back
            </Button>
            <Button
              onClick={() => setStep('verify')}
              disabled={!savedBackupCodes}
              data-testid="totp-setup-next-button"
              aria-label="Proceed to verification step"
            >
              Next
            </Button>
          </>
        )}

        {step === 'verify' && (
          <>
            <Button
              variant="outline"
              onClick={() => setStep('qr-code')}
              data-testid="totp-verify-back-button"
              aria-label="Go back to QR code step"
            >
              Back
            </Button>
            <Button
              onClick={() => {
                if (setupData) {
                  verifyMutation.mutate({
                    code: verificationCode,
                    tempToken: setupData.tempToken,
                  })
                }
              }}
              disabled={isVerifyDisabled}
              data-testid="totp-verify-submit-button"
              aria-label="Verify and enable TOTP authentication"
            >
              {verifyMutation.isSubmitting ? (
                <>
                  <Loader2 className="h-4 w-4 mr-2 animate-spin" aria-hidden="true" />
                  Verifying...
                </>
              ) : (
                'Verify & Enable'
              )}
            </Button>
          </>
        )}
      </div>
    </div>
  )
}

/**
 * 6-digit OTP code input with individual character fields and auto-focus.
 */
function OtpCodeInput({
  value,
  onChange,
  disabled = false,
}: {
  value: string
  onChange: (value: string) => void
  disabled?: boolean
}) {
  const inputRefs = useRef<(HTMLInputElement | null)[]>([])
  const length = 6

  const handleChange = useCallback(
    (index: number, char: string) => {
      // Only accept single digit
      const digit = char.replace(/\D/g, '').slice(0, 1)
      if (char && !digit) return

      const newCode = value.padEnd(length, ' ').split('')
      newCode[index] = digit
      const nextValue = newCode.join('').replace(/ /g, '')
      onChange(nextValue)

      // Auto-focus next input
      if (digit && index < length - 1) {
        inputRefs.current[index + 1]?.focus()
      }
    },
    [value, onChange, length]
  )

  const handleKeyDown = useCallback(
    (index: number, e: React.KeyboardEvent<HTMLInputElement>) => {
      if (e.key === 'Backspace') {
        if (!value[index] && index > 0) {
          // If current field is empty, move to previous
          const newCode = value.padEnd(length, ' ').split('')
          newCode[index - 1] = ''
          onChange(newCode.join('').replace(/ /g, ''))
          inputRefs.current[index - 1]?.focus()
        } else {
          const newCode = value.padEnd(length, ' ').split('')
          newCode[index] = ''
          onChange(newCode.join('').replace(/ /g, ''))
        }
      } else if (e.key === 'ArrowLeft' && index > 0) {
        inputRefs.current[index - 1]?.focus()
      } else if (e.key === 'ArrowRight' && index < length - 1) {
        inputRefs.current[index + 1]?.focus()
      }
    },
    [value, onChange, length]
  )

  const handlePaste = useCallback(
    (e: React.ClipboardEvent) => {
      e.preventDefault()
      const pasted = e.clipboardData.getData('text').replace(/\D/g, '').slice(0, length)
      if (pasted) {
        onChange(pasted)
        // Focus the field after the last pasted digit, or the last field
        const focusIndex = Math.min(pasted.length, length - 1)
        inputRefs.current[focusIndex]?.focus()
      }
    },
    [onChange, length]
  )

  return (
    <div className="flex gap-2 justify-center" data-testid="totp-otp-input">
      {Array.from({ length }, (_, index) => (
        <input
          key={index}
          ref={(el) => {
            inputRefs.current[index] = el
          }}
          type="text"
          inputMode="numeric"
          maxLength={1}
          value={value[index] ?? ''}
          onChange={(e) => handleChange(index, e.target.value)}
          onKeyDown={(e) => handleKeyDown(index, e)}
          onPaste={handlePaste}
          disabled={disabled}
          data-testid={`totp-otp-digit-${index}`}
          className="w-10 h-12 text-center text-xl font-semibold border-2 border-input rounded-lg
            focus:border-primary focus:ring-2 focus:ring-primary/20 focus:outline-none
            transition-all disabled:cursor-not-allowed disabled:border-transparent disabled:bg-muted disabled:text-muted-foreground
            md:w-12 md:h-14 md:text-2xl"
          aria-label={`Digit ${index + 1}`}
        />
      ))}
    </div>
  )
}
