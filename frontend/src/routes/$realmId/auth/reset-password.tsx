import { useState } from 'react'
import { createFileRoute, Link, useNavigate } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { resetPasswordConfirm } from '@/lib/api-generated'
import { getErrorMessage } from '@/lib/error-utils'
import { resetPasswordSearchSchema } from '@/lib/schemas/search-params'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { AuthPageWrapper } from '@/components/auth/auth-page-wrapper'
import { TurnstileWidget } from '@/components/auth/turnstile-widget'
import { publicConfigQueryOptions, turnstileStatusQueryOptions } from '@/data/query-options'
import { toast } from 'sonner'
import { m } from '@/paraglide/messages'
import { realmPath, resolvedRealmFromPath } from '@/lib/realm-routing'

export const Route = createFileRoute('/$realmId/auth/reset-password')({
  component: ResetPasswordPage,
  validateSearch: (search) => resetPasswordSearchSchema.parse(search),
})

export function ResetPasswordPage() {
  const realmContext = resolvedRealmFromPath(window.location.pathname)
  const { realmId } = realmContext
  const { code } = resetPasswordSearchSchema.parse(
    Object.fromEntries(new URLSearchParams(window.location.search))
  )
  const navigate = useNavigate()

  const [newPassword, setNewPassword] = useState('')
  const [confirmPassword, setConfirmPassword] = useState('')
  const [turnstileToken, setTurnstileToken] = useState<string | null>(null)
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const { data: turnstileStatus, isLoading: loadingTurnstile } = useQuery(
    turnstileStatusQueryOptions(realmId)
  )
  const { data: publicConfig } = useQuery(publicConfigQueryOptions(realmId))
  // Per-realm white-label config (FE-D03). Generic variant: logo/accent/
  // background/footer only, never login/register copy.
  const whiteLabel = publicConfig?.whiteLabel ?? null

  const passwordsMatch = newPassword.length >= 8 && newPassword === confirmPassword

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (isSubmitting || !passwordsMatch) return

    setIsSubmitting(true)
    setError(null)

    try {
      await resetPasswordConfirm({
        path: { realmId, resetCode: code },
        body: { newPass: newPassword, turnstileToken },
        throwOnError: true,
      })
      toast.success(m['auth.reset_password.success']())
      navigate({ to: realmPath(realmContext, '/auth/login') })
    } catch (err) {
      setError(getErrorMessage(err))
    } finally {
      setIsSubmitting(false)
    }
  }

  const showMismatchError = confirmPassword.length > 0 && newPassword !== confirmPassword

  return (
    <AuthPageWrapper whiteLabel={whiteLabel} realmName={publicConfig?.realmName}>
      <div className="w-full pt-8" data-testid="reset-password-card">
        <h1 data-testid="reset-password-title" className="text-xl font-semibold tracking-tight">
          {m['auth.reset_password.title']()}
        </h1>
        <p className="mt-1 text-sm text-muted-foreground">
          {m['auth.reset_password.description']()}
        </p>
        <div className="mt-6">
          <form onSubmit={handleSubmit} className="space-y-4" data-testid="reset-password-form">
            {error && (
              <div
                className="p-3 bg-destructive/10 border border-destructive/20 rounded text-destructive text-sm"
                data-testid="reset-password-error"
              >
                {error}
              </div>
            )}

            <div>
              <Label htmlFor="new-password">{m['auth.reset_password.new_password_label']()}</Label>
              <Input
                id="new-password"
                type="password"
                value={newPassword}
                onChange={(e) => setNewPassword(e.target.value)}
                disabled={isSubmitting}
                required
                minLength={8}
                autoFocus
                className="mt-1"
                data-testid="reset-password-new-input"
              />
              {newPassword.length > 0 && newPassword.length < 8 && (
                <p className="text-sm text-destructive mt-1">{m['auth.password_min_length']()}</p>
              )}
            </div>

            <div>
              <Label htmlFor="confirm-password">
                {m['auth.reset_password.confirm_password_label']()}
              </Label>
              <Input
                id="confirm-password"
                type="password"
                value={confirmPassword}
                onChange={(e) => setConfirmPassword(e.target.value)}
                disabled={isSubmitting}
                required
                minLength={8}
                className="mt-1"
                data-testid="reset-password-confirm-input"
              />
              {showMismatchError && (
                <p className="text-sm text-destructive mt-1">{m['auth.passwords_dont_match']()}</p>
              )}
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
              disabled={isSubmitting || !passwordsMatch}
              className="h-11 w-full md:h-10"
              data-testid="reset-password-submit-button"
            >
              {isSubmitting
                ? m['auth.reset_password.submitting']()
                : m['auth.reset_password.submit']()}
            </Button>
          </form>

          <div className="mt-4">
            <Link
              to={realmPath(realmContext, '/auth/login')}
              className="text-sm font-medium text-primary hover:text-primary/80"
              data-testid="reset-password-back-link"
            >
              {m['auth.reset_password.back_to_login']()}
            </Link>
          </div>
        </div>
      </div>
    </AuthPageWrapper>
  )
}
