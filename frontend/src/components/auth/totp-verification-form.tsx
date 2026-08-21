import { useState, useCallback } from 'react'
import { useMutation } from '@tanstack/react-query'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { RefreshCw } from 'lucide-react'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import { getFieldErrorMessage } from '@/lib/form-utils'
import { withTimeout } from '@/lib/totp-utils'
import { z } from 'zod'
import type {
  VerifyTotpResponse,
  LegalAgreementSummary,
  AuthConsentAgreement,
} from '@/lib/api-generated'
import { mapLoginResultToResponse } from '@/lib/auth-service'
import { ensureHeraldClient } from '@/lib/herald-client'
import { AgreementLinks } from '@/components/legal/AgreementLinks'
import { toAuthConsentAgreements } from '@/data/query-options'
import { formatDate } from '@/lib/date-utils'
import { m } from '@/paraglide/messages'
import { getErrorMessage } from '@/lib/error-utils'

const totpCodeSchema = z.object({
  code: z.string().length(6, 'Code must be 6 digits'),
})

const backupCodeSchema = z.object({
  code: z.string().length(8, 'Code must be 8 characters'),
})

interface TotpVerificationFormProps {
  realmId: string
  tempToken: string
  onSuccess: (response: VerifyTotpResponse) => void
  onBack?: () => void
}

type CodeType = 'totp' | 'backup'

const MAX_ATTEMPTS = 5
const TOTP_CODE_LENGTH = 6
const BACKUP_CODE_LENGTH = 8
const LOCKED_MESSAGE = 'Too many failed attempts. Please try again in 15 minutes.'

function isConsentRequired(response: {
  consentRequired?: boolean | null
  consent_required?: boolean | null
}): boolean {
  return (
    !!response.consentRequired ||
    !!(response as { consent_required?: boolean | null }).consent_required
  )
}

export function TotpVerificationForm({
  realmId,
  tempToken,
  onSuccess,
  onBack,
}: TotpVerificationFormProps) {
  const [codeType, setCodeType] = useState<CodeType>('totp')
  const [attempts, setAttempts] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const [pendingConsent, setPendingConsent] = useState<LegalAgreementSummary[] | null>(null)
  const [lastSubmitted, setLastSubmitted] = useState<{
    code: string
    backupCode: string | null
  } | null>(null)

  const totpForm = useAppForm({
    schema: totpCodeSchema,
    defaultValues: { code: '' },
    onSubmit: async ({ value }) => {
      if (attempts >= MAX_ATTEMPTS) return
      setError(null)
      setLastSubmitted({ code: value.code, backupCode: null })
      verifyMutation.mutate({
        code: value.code,
        backupCode: null,
      })
    },
  })

  const backupForm = useAppForm({
    schema: backupCodeSchema,
    defaultValues: { code: '' },
    onSubmit: async ({ value }) => {
      if (attempts >= MAX_ATTEMPTS) return
      setError(null)
      setLastSubmitted({ code: '', backupCode: value.code.toUpperCase() })
      verifyMutation.mutate({
        code: '',
        backupCode: value.code.toUpperCase(),
      })
    },
  })

  const verifyMutation = useMutation({
    mutationFn: async (data: {
      code: string
      backupCode: string | null
      agreements?: AuthConsentAgreement[]
    }) => {
      const result = await withTimeout(
        ensureHeraldClient(realmId).verifyTotp({
          tempToken,
          ...(data.backupCode ? { backupCode: data.backupCode } : { code: data.code }),
          ...(data.agreements ? { agreements: data.agreements } : {}),
        })
      )
      // The SDK applies the token set itself on the success branch and throws
      // on HTTP errors; map the discriminated result back to the legacy branch
      // shape the route consumes (`completeLoginAfterTotp`).
      return mapLoginResultToResponse(result) as unknown as VerifyTotpResponse
    },
    onSuccess: (data) => {
      if (isConsentRequired(data)) {
        setPendingConsent(data.agreements ?? [])
        return
      }
      setPendingConsent(null)
      onSuccess(data)
    },
    onError: (err: unknown) => {
      setAttempts((prev) => prev + 1)
      setError(getErrorMessage(err))
    },
  })

  const currentForm = codeType === 'backup' ? backupForm : totpForm
  const codeLength = codeType === 'backup' ? BACKUP_CODE_LENGTH : TOTP_CODE_LENGTH
  const remainingAttempts = MAX_ATTEMPTS - attempts
  const isLocked = attempts >= MAX_ATTEMPTS

  const handleCodeChange = useCallback(
    (value: string) => {
      const processedValue = codeType === 'backup' ? value.toUpperCase() : value
      currentForm.setFieldValue('code', processedValue)

      if (processedValue.length === codeLength && !verifyMutation.isPending && !isLocked) {
        setError(null)
        currentForm.handleSubmit()
      }
    },
    [codeType, codeLength, currentForm, verifyMutation.isPending, isLocked]
  )

  const switchToBackupCode = useCallback(() => {
    setCodeType('backup')
    setError(null)
    backupForm.reset()
  }, [backupForm])

  const switchToTotpCode = useCallback(() => {
    setCodeType('totp')
    setError(null)
    totpForm.reset()
  }, [totpForm])

  const getInputType = useCallback((): 'text' | 'numeric' => {
    return codeType === 'backup' ? 'text' : 'numeric'
  }, [codeType])

  const getInputPattern = useCallback((): string => {
    return codeType === 'backup' ? '[A-Z0-9]{8}' : '[0-9]{6}'
  }, [codeType])

  const getPlaceholder = useCallback((): string => {
    return codeType === 'backup' ? 'XXXXXXXX' : '000000'
  }, [codeType])

  const getLabelText = useCallback((): string => {
    return codeType === 'backup' ? 'Backup Code' : 'Verification Code'
  }, [codeType])

  const getDescriptionText = useCallback((): string => {
    return codeType === 'backup'
      ? 'Enter one of your backup recovery codes'
      : 'Enter the 6-digit code from your authenticator app'
  }, [codeType])

  const getAttemptsText = useCallback((): string | null => {
    if (isLocked || remainingAttempts <= 0 || remainingAttempts >= MAX_ATTEMPTS) return null
    return `${remainingAttempts} attempt${remainingAttempts > 1 ? 's' : ''} remaining`
  }, [isLocked, remainingAttempts])

  async function handleConsentAgree() {
    if (!pendingConsent || !lastSubmitted) return
    setError(null)
    verifyMutation.mutate({
      code: lastSubmitted.backupCode ? '' : lastSubmitted.code,
      backupCode: lastSubmitted.backupCode,
      agreements: toAuthConsentAgreements(pendingConsent),
    })
  }

  function handleConsentDecline() {
    setPendingConsent(null)
    if (onBack) {
      onBack()
    }
  }

  return (
    <div className="w-full pt-8" data-testid="totp-verification-form">
      <h1 className="text-xl font-semibold tracking-tight">Two-Factor Authentication</h1>
      <p className="mt-1 text-sm text-muted-foreground">{getDescriptionText()}</p>
      <div className="mt-6 space-y-4">
        {error && (
          <div className="text-sm text-destructive" data-testid="totp-verification-error">
            {error}
          </div>
        )}

        {isLocked && (
          <div className="text-sm text-destructive" data-testid="totp-verification-locked">
            {LOCKED_MESSAGE}
          </div>
        )}

        {pendingConsent && (
          <div className="space-y-4" data-testid="totp-reconsent-view">
            <h3 className="font-semibold">{m['auth.login.reconsent_title']()}</h3>
            <p className="text-sm text-muted-foreground">
              {m['auth.login.reconsent_description']()}
            </p>
            {pendingConsent.map((agreement) => (
              <div
                key={agreement.version_id}
                className="rounded border p-3"
                data-testid={`totp-reconsent-agreement-${agreement.agreement_type}`}
              >
                <div className="font-medium">
                  <AgreementLinks
                    realmId={realmId}
                    agreements={[agreement]}
                    agreementType={
                      agreement.agreement_type as 'terms_of_service' | 'privacy_policy'
                    }
                  />
                </div>
                <div
                  className="text-sm text-muted-foreground"
                  data-testid={`totp-reconsent-agreement-${agreement.agreement_type}-version`}
                >
                  {m['legal.version_label']()}: {agreement.version_no} •{' '}
                  {m['legal.effective_date_label']()}: {formatDate(agreement.effective_at)}
                </div>
              </div>
            ))}
            <Button
              type="button"
              disabled={verifyMutation.isPending}
              className="w-full"
              data-testid="totp-agree-and-continue-button"
              onClick={handleConsentAgree}
            >
              {verifyMutation.isPending
                ? m['common.loading']()
                : m['auth.login.agree_and_continue']()}
            </Button>
            {onBack && (
              <Button
                type="button"
                variant="outline"
                className="w-full"
                data-testid="totp-decline-back-button"
                onClick={handleConsentDecline}
              >
                {m['auth.login.decline_back_to_login']()}
              </Button>
            )}
          </div>
        )}

        {!pendingConsent && (
          <AppForm>
            <form
              onSubmit={(e) => {
                e.preventDefault()
                e.stopPropagation()
                currentForm.handleSubmit()
              }}
              className="space-y-4"
            >
              <currentForm.Field name="code">
                {(field) => (
                  <div className="space-y-2">
                    <Label htmlFor="code">{getLabelText()}</Label>
                    <Input
                      id="code"
                      type={getInputType()}
                      inputMode={getInputType()}
                      pattern={getInputPattern()}
                      maxLength={codeLength}
                      value={field.state.value ?? ''}
                      onChange={(e) => handleCodeChange(e.target.value)}
                      disabled={isLocked || verifyMutation.isPending}
                      data-testid="totp-verification-code-input"
                      placeholder={getPlaceholder()}
                      autoFocus
                    />
                    {(field.state.meta.isTouched || currentForm.state.isSubmitted) &&
                      field.state.meta.errors.length > 0 && (
                        <p className="text-sm text-destructive">
                          {getFieldErrorMessage(field.state.meta)}
                        </p>
                      )}
                  </div>
                )}
              </currentForm.Field>
            </form>
          </AppForm>
        )}

        {!pendingConsent && codeType === 'totp' && !isLocked && (
          <button
            type="button"
            onClick={switchToBackupCode}
            className="text-sm text-primary hover:underline"
            data-testid="totp-use-backup-code-link"
          >
            Use a backup code instead
          </button>
        )}

        {!pendingConsent && codeType === 'backup' && !isLocked && (
          <button
            type="button"
            onClick={switchToTotpCode}
            className="text-sm text-primary hover:underline"
            data-testid="totp-use-totp-code-link"
          >
            Use TOTP code instead
          </button>
        )}

        {!pendingConsent && getAttemptsText() && (
          <div className="text-sm text-muted-foreground" data-testid="totp-remaining-attempts">
            {getAttemptsText()}
          </div>
        )}

        {!pendingConsent && onBack && (
          <Button
            type="button"
            variant="ghost"
            onClick={onBack}
            className="w-full"
            data-testid="totp-verification-back-button"
          >
            <RefreshCw className="mr-2 h-4 w-4" />
            Back to Login
          </Button>
        )}
      </div>
    </div>
  )
}
