import { useState } from 'react'
import { createFileRoute, Link, useNavigate } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { FIRST_PARTY_CLIENT_ID } from '@/lib/auth-utils'
import { ensureHeraldClient } from '@/lib/herald-client'
import { getErrorMessage } from '@/lib/error-utils'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { AuthPageWrapper } from '@/components/auth/auth-page-wrapper'
import { TurnstileWidget } from '@/components/auth/turnstile-widget'
import { publicConfigQueryOptions, turnstileStatusQueryOptions } from '@/data/query-options'
import { toast } from 'sonner'
import { m } from '@/paraglide/messages'
import { realmPath, resolvedRealmFromPath } from '@/lib/realm-routing'

export const Route = createFileRoute('/$realmId/auth/forgot-password')({
  component: ForgotPasswordPage,
})

export function ForgotPasswordPage() {
  const navigate = useNavigate()
  const realmContext = resolvedRealmFromPath(window.location.pathname)
  const { realmId } = realmContext

  const [email, setEmail] = useState('')
  const [turnstileToken, setTurnstileToken] = useState<string | null>(null)
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [sent, setSent] = useState(false)

  const { data: turnstileStatus, isLoading: loadingTurnstile } = useQuery(
    turnstileStatusQueryOptions(realmId)
  )
  const { data: publicConfig } = useQuery(publicConfigQueryOptions(realmId))
  // Per-realm white-label config (FE-D03). Generic variant: logo/accent/
  // background/footer only, never login/register copy.
  const whiteLabel = publicConfig?.whiteLabel ?? null

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (isSubmitting || !email) return

    setIsSubmitting(true)
    setError(null)

    try {
      const herald = ensureHeraldClient(realmId)
      herald.tokens.bindClientId(FIRST_PARTY_CLIENT_ID)
      await herald.requestPasswordReset({
        email,
        ...(turnstileToken ? { turnstileToken } : {}),
      })
      setSent(true)
      toast.success(m['auth.forgot_password.success']())
    } catch (err) {
      setError(getErrorMessage(err))
    } finally {
      setIsSubmitting(false)
    }
  }

  return (
    <AuthPageWrapper whiteLabel={whiteLabel} realmName={publicConfig?.realmName}>
      <div className="w-full pt-8" data-testid="forgot-password-card">
        <h1 data-testid="forgot-password-title" className="text-xl font-semibold tracking-tight">
          {m['auth.forgot_password.title']()}
        </h1>
        <p className="mt-1 text-sm text-muted-foreground">
          {m['auth.forgot_password.description']()}
        </p>
        <div className="mt-6">
          {sent ? (
            <div className="space-y-4" data-testid="forgot-password-success">
              <div className="p-3 bg-success/10 border border-success/20 rounded text-success text-sm">
                {m['auth.forgot_password.success']()}
              </div>
              <Button
                type="button"
                variant="outline"
                className="w-full"
                onClick={() => navigate({ to: realmPath(realmContext, '/auth/login') })}
              >
                {m['auth.forgot_password.back_to_login']()}
              </Button>
            </div>
          ) : (
            <form onSubmit={handleSubmit} className="space-y-4" data-testid="forgot-password-form">
              {error && (
                <div
                  className="p-3 bg-destructive/10 border border-destructive/20 rounded text-destructive text-sm"
                  data-testid="forgot-password-error"
                >
                  {error}
                </div>
              )}

              <div>
                <Label htmlFor="email">{m['auth.forgot_password.email_label']()}</Label>
                <Input
                  id="email"
                  type="email"
                  value={email}
                  onChange={(e) => setEmail(e.target.value)}
                  disabled={isSubmitting}
                  placeholder="your@email.com"
                  required
                  autoFocus
                  className="mt-1"
                  data-testid="forgot-password-email-input"
                />
              </div>

              {!loadingTurnstile && turnstileStatus?.enabled && (
                <TurnstileWidget
                  siteKey={turnstileStatus.siteKey || ''}
                  onTokenChange={setTurnstileToken}
                  onError={(error) => console.error('Turnstile error:', error)}
                />
              )}

              <Button
                type="submit"
                disabled={isSubmitting || !email}
                className="w-full"
                data-testid="forgot-password-submit-button"
              >
                {isSubmitting
                  ? m['auth.forgot_password.submitting']()
                  : m['auth.forgot_password.submit']()}
              </Button>
            </form>
          )}

          <div className="mt-4">
            <Link
              to={realmPath(realmContext, '/auth/login')}
              className="text-sm font-medium text-primary hover:text-primary/80"
              data-testid="forgot-password-back-link"
            >
              {m['auth.forgot_password.back_to_login']()}
            </Link>
          </div>
        </div>
      </div>
    </AuthPageWrapper>
  )
}
