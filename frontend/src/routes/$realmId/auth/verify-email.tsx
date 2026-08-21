import { useState, useEffect } from 'react'
import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { verifyEmailConfirm } from '@/lib/api-generated'
import { FIRST_PARTY_CLIENT_ID } from '@/lib/auth-utils'
import { ensureHeraldClient } from '@/lib/herald-client'
import { getErrorMessage } from '@/lib/error-utils'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { TurnstileWidget } from '@/components/auth/turnstile-widget'
import { AuthPageWrapper } from '@/components/auth/auth-page-wrapper'
import { publicConfigQueryOptions, turnstileStatusQueryOptions } from '@/data/query-options'
import { toast } from 'sonner'
import { m } from '@/paraglide/messages'
import { realmPath, resolvedRealmFromPath } from '@/lib/realm-routing'

const VERIFICATION_CODE_LENGTH = 6
const RESEND_COUNTDOWN_SECONDS = 60

function getResendButtonText(isResending: boolean, canResend: boolean, countdown: number): string {
  if (isResending) return m['auth.verify_email.sending']()
  if (canResend) return m['auth.verify_email.resend_button']()
  return m['auth.verify_email.resend_in']({ countdown })
}

export const Route = createFileRoute('/$realmId/auth/verify-email')({
  component: VerifyEmailPage,
})

export function VerifyEmailPage() {
  const realmContext = resolvedRealmFromPath(window.location.pathname)
  const { realmId } = realmContext
  const navigate = useNavigate()

  const [code, setCode] = useState('')
  const [email, setEmail] = useState('')
  const [turnstileToken, setTurnstileToken] = useState<string | null>(null)
  const [countdown, setCountdown] = useState(RESEND_COUNTDOWN_SECONDS)
  const [canResend, setCanResend] = useState(false)
  const [verificationError, setVerificationError] = useState<string | null>(null)
  const [isVerifying, setIsVerifying] = useState(false)
  const [isResending, setIsResending] = useState(false)

  const { data: turnstileStatus, isLoading: loadingTurnstile } = useQuery(
    turnstileStatusQueryOptions(realmId)
  )
  const { data: publicConfig } = useQuery(publicConfigQueryOptions(realmId))

  async function handleVerify(e: React.FormEvent) {
    e.preventDefault()
    if (code.length !== VERIFICATION_CODE_LENGTH) return

    setIsVerifying(true)
    setVerificationError(null)

    try {
      const response = await verifyEmailConfirm({
        path: { realmId, emailVerificationCode: code },
        throwOnError: true,
      })

      if (response.data) {
        toast.success(m['auth.verify_email.success']())
        navigate({ to: realmPath(realmContext, '/auth/login') })
      }
    } catch (error) {
      setVerificationError(getErrorMessage(error))
    } finally {
      setIsVerifying(false)
    }
  }

  async function handleResend() {
    if (!canResend || isResending || !email) return

    setIsResending(true)
    setVerificationError(null)

    try {
      const herald = ensureHeraldClient(realmId)
      herald.tokens.bindClientId(FIRST_PARTY_CLIENT_ID)
      await herald.triggerVerifyEmail({
        email,
        ...(turnstileToken ? { turnstileToken } : {}),
      })

      toast.success(m['auth.verify_email.resend_success']())
      setCountdown(RESEND_COUNTDOWN_SECONDS)
      setCanResend(false)
    } catch (error) {
      setVerificationError(getErrorMessage(error))
    } finally {
      setIsResending(false)
    }
  }

  useEffect(() => {
    if (countdown > 0) {
      const timer = setTimeout(() => setCountdown(countdown - 1), 1000)
      return () => clearTimeout(timer)
    } else {
      setCanResend(true)
    }
  }, [countdown])

  return (
    <AuthPageWrapper whiteLabel={publicConfig?.whiteLabel} realmName={publicConfig?.realmName}>
      <div className="w-full pt-8">
        <h1 data-testid="verify-email-title" className="text-xl font-semibold tracking-tight">
          {m['auth.verify_email.title']()}
        </h1>
        <p className="mt-1 text-sm text-muted-foreground">{m['auth.verify_email.description']()}</p>

        <form onSubmit={handleVerify} className="mt-6 space-y-6">
          <div>
            <Label htmlFor="email">{m['auth.verify_email.email_label']()}</Label>
            <Input
              id="email"
              type="email"
              data-testid="verify-email-input"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder="your@email.com"
              className="mt-1"
              required
            />
          </div>

          <div>
            <Label htmlFor="verification-code">{m['auth.verify_email.code_label']()}</Label>
            <Input
              id="verification-code"
              type="text"
              data-testid="verification-code-input"
              value={code}
              onChange={(e) =>
                setCode(e.target.value.replace(/\D/g, '').slice(0, VERIFICATION_CODE_LENGTH))
              }
              placeholder="123456"
              maxLength={VERIFICATION_CODE_LENGTH}
              className="mt-1"
              required
            />
          </div>

          <Button
            type="submit"
            data-testid="verify-button"
            disabled={code.length !== VERIFICATION_CODE_LENGTH || isVerifying}
            className="w-full"
          >
            {isVerifying
              ? m['auth.verify_email.verifying']()
              : m['auth.verify_email.verify_button']()}
          </Button>

          {verificationError && (
            <div className="bg-destructive/10 border border-destructive/20 text-destructive px-4 py-3 rounded-lg">
              {verificationError}
            </div>
          )}

          {canResend && !loadingTurnstile && turnstileStatus?.enabled && (
            <TurnstileWidget
              siteKey={turnstileStatus.siteKey || ''}
              onTokenChange={setTurnstileToken}
              onError={(error) => console.error('Turnstile error:', error)}
            />
          )}

          <div>
            <Button
              type="button"
              variant="ghost"
              data-testid="resend-button"
              onClick={handleResend}
              disabled={!canResend || isResending}
            >
              {getResendButtonText(isResending, canResend, countdown)}
            </Button>
          </div>
        </form>
      </div>
    </AuthPageWrapper>
  )
}
