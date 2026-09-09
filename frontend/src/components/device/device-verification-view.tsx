import { useState, useEffect } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { deviceVerify, deviceConfirm, recordConsent } from '@/lib/api-generated'
import type { DeviceVerifyResponse, LegalAgreementSummary } from '@/lib/api-generated'
import { AuthPageWrapper } from '@/components/auth/auth-page-wrapper'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { CodeInput } from '@/components/device/code-input'
import { AuthorizeConfirm } from '@/components/device/authorize-confirm'
import { ConsentAgreementsPanel } from '@/components/legal/ConsentAgreementsPanel'
import { getErrorMessage } from '@/lib/error-utils'
import { filterAndFormat, toBackendCode } from './device-code-utils'
import { m } from '@/paraglide/messages'
import { publicConfigQueryOptions } from '@/data/query-options'

type PageState = 'input' | 'verifying' | 'confirmed' | 'result'

interface DeviceVerificationViewProps {
  realmId: string
  initialCode?: string
}

export function DeviceVerificationView({ realmId, initialCode }: DeviceVerificationViewProps) {
  const { data: publicConfig } = useQuery(publicConfigQueryOptions(realmId))
  const [pageState, setPageState] = useState<PageState>(initialCode ? 'verifying' : 'input')
  const [error, setError] = useState<string | null>(null)
  const [verifyResponse, setVerifyResponse] = useState<DeviceVerifyResponse | null>(null)
  const [resultCode, setResultCode] = useState<'approved' | 'denied' | null>(null)
  const [userCode, setUserCode] = useState(initialCode ?? '')
  // Set when the backend consent gate blocks an approval: the device stays
  // verified, so recording consent and re-confirming completes the flow.
  const [consentAgreements, setConsentAgreements] = useState<LegalAgreementSummary[] | null>(null)

  const verifyMutation = useMutation({
    mutationFn: async (code: string) => {
      const response = await deviceVerify({
        body: { user_code: code },
        path: { realmId },
      })
      if (response.error) throw response.error
      return response.data
    },
    onSuccess: (data) => {
      setError(null)
      setVerifyResponse(data)
      setPageState('confirmed')
    },
    onError: (err: unknown) => {
      setError(getErrorMessage(err))
      setPageState('input')
    },
  })

  const confirmMutation = useMutation({
    mutationFn: async (approved: boolean) => {
      const response = await deviceConfirm({
        body: { user_code: userCode, approved },
        path: { realmId },
      })
      if (response.error) throw response.error
      return response.data
    },
    onSuccess: (data) => {
      setError(null)
      if (data.consent_required && data.agreements && data.agreements.length > 0) {
        setConsentAgreements(data.agreements)
        return
      }
      setConsentAgreements(null)
      setResultCode(data.status === 'authorized' ? 'approved' : 'denied')
      setPageState('result')
    },
    onError: (err: unknown) => {
      setError(getErrorMessage(err))
    },
  })

  const consentMutation = useMutation({
    mutationFn: async (agreements: LegalAgreementSummary[]) => {
      const response = await recordConsent({
        body: {
          agreements: agreements.map((a) => ({
            agreement_type: a.agreement_type,
            version_id: a.version_id,
          })),
        },
        path: { realmId },
      })
      if (response.error) throw response.error
    },
    onSuccess: () => {
      setConsentAgreements(null)
      confirmMutation.mutate(true)
    },
    onError: (err: unknown) => {
      setError(getErrorMessage(err))
    },
  })

  // Auto-submit verify on mount when initialCode is provided
  useEffect(() => {
    if (initialCode) {
      const formatted = filterAndFormat(initialCode)
      setUserCode(formatted)
      verifyMutation.mutate(toBackendCode(formatted))
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initialCode])

  function handleCodeSubmit(code: string) {
    setError(null)
    setUserCode(code)
    setPageState('verifying')
    verifyMutation.mutate(code)
  }

  function handleConfirm(approved: boolean) {
    confirmMutation.mutate(approved)
  }

  function handleConsentDecline() {
    setConsentAgreements(null)
    setError(null)
  }

  return (
    <AuthPageWrapper whiteLabel={publicConfig?.whiteLabel} realmName={publicConfig?.realmName}>
      <Card className="w-full max-w-md" data-testid="device-verification-card">
        <CardHeader>
          <CardTitle data-testid="device-verification-title">{m['device.title']()}</CardTitle>
        </CardHeader>
        <CardContent>
          {error && (
            <div
              className="mb-4 p-3 bg-destructive/10 border border-destructive/20 rounded text-destructive text-sm"
              data-testid="device-verification-error"
            >
              {error}
            </div>
          )}

          {pageState === 'input' && (
            <div className="space-y-4">
              <p className="text-sm text-muted-foreground text-center">
                {m['device.enter_code_description']()}
              </p>
              <CodeInput onSubmit={handleCodeSubmit} defaultValue={initialCode} />
            </div>
          )}

          {pageState === 'verifying' && (
            <div className="py-8 text-center text-muted-foreground">
              {m['device.verifying_code']()}
            </div>
          )}

          {pageState === 'confirmed' && verifyResponse && !consentAgreements && (
            <AuthorizeConfirm
              clientAppName={verifyResponse.client_app_name}
              clientAppIconUrl={verifyResponse.client_app_icon_url}
              onConfirm={handleConfirm}
              isLoading={confirmMutation.isPending}
            />
          )}

          {pageState === 'confirmed' && consentAgreements && (
            <ConsentAgreementsPanel
              realmId={realmId}
              agreements={consentAgreements}
              isPending={consentMutation.isPending}
              title={m['device.consent_title']()}
              description={m['device.consent_description']()}
              agreeLabel={m['device.agree_and_continue']()}
              declineLabel={m['device.consent_cancel']()}
              onAgree={() => consentMutation.mutate(consentAgreements)}
              onDecline={handleConsentDecline}
              testIdPrefix="device"
            />
          )}

          {pageState === 'result' && (
            <div className="py-4 text-center" data-testid="device-verification-result">
              {resultCode === 'approved' ? (
                <div className="space-y-2">
                  <p className="text-success font-medium">
                    {m['device.authorization_successful']()}
                  </p>
                  <p className="text-sm text-muted-foreground">{m['device.return_to_device']()}</p>
                </div>
              ) : (
                <p className="text-destructive font-medium">{m['device.authorization_denied']()}</p>
              )}
            </div>
          )}
        </CardContent>
      </Card>
    </AuthPageWrapper>
  )
}
