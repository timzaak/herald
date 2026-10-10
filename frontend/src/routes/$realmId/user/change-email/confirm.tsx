import { useEffect } from 'react'
import { createFileRoute, Link } from '@tanstack/react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { changeEmailConfirmQueryOptions, queryKeys } from '@/data/query-options'
import { changeEmailSearchSchema } from '@/lib/schemas/search-params'
import { resolveApiError } from '@/lib/error-utils'
import { realmPath, resolvedRealmFromPath } from '@/lib/realm-routing'
import { Button } from '@/components/ui/button'
import { PageHeader } from '@/components/shared'
import { m } from '@/paraglide/messages'

export const Route = createFileRoute('/$realmId/user/change-email/confirm')({
  component: ChangeEmailConfirmPage,
  validateSearch: (search) => changeEmailSearchSchema.parse(search),
})

function confirmFailureMessage(status: number | undefined): string {
  switch (status) {
    case 400:
      return m['profile.change_email_confirm_error_invalid']()
    case 401:
      return m['profile.change_email_confirm_error_relogin']()
    case 403:
      return m['profile.change_email_confirm_error_mismatch']()
    case 409:
      return m['profile.change_email_confirm_error_conflict']()
    case 429:
      return m['profile.change_email_confirm_error_rate_limited']()
    default:
      return m['error.server_error']()
  }
}

export function ChangeEmailConfirmPage() {
  // Shared by both route trees (realm-prefixed and session-scoped), so the
  // realm and the code are read from the raw location like the reset-password
  // page does instead of tree-specific hooks.
  const realmContext = resolvedRealmFromPath(window.location.pathname)
  const { realmId } = realmContext
  const search = changeEmailSearchSchema.parse(
    Object.fromEntries(new URLSearchParams(window.location.search))
  )
  // A code dropped by the login redirect (or an empty ?code=) renders the
  // invalid-link state: the schema keeps `code` optional so validateSearch
  // does not throw the route into its error boundary.
  const code = search.code && search.code.length > 0 ? search.code : undefined

  // The query commits the email change exactly once (see the options def for
  // why it must never retry or refetch); the manual 429 retry below is the
  // only safe re-fire (the code was not consumed on a rate-limited attempt).
  const { isPending, isError, isSuccess, error, refetch } = useQuery({
    ...changeEmailConfirmQueryOptions(realmId, code ?? ''),
    enabled: code !== undefined,
  })
  const errorStatus = isError ? resolveApiError(error).status : undefined
  const queryClient = useQueryClient()

  useEffect(() => {
    if (isSuccess) {
      void queryClient.invalidateQueries({ queryKey: queryKeys.profile() })
    }
  }, [isSuccess, queryClient])

  return (
    <div className="space-y-8">
      <PageHeader
        title={m['profile.change_email_confirm_title']()}
        headingTestId="change-email-confirm-title"
      />
      <section className="max-w-lg space-y-4" data-testid="change-email-confirm-card">
        {!code && (
          <div
            className="p-3 bg-destructive/10 border border-destructive/20 rounded text-destructive text-sm"
            data-testid="change-email-confirm-error"
          >
            {m['profile.change_email_confirm_error_invalid']()}
          </div>
        )}
        {code && isPending && (
          <p className="text-sm text-muted-foreground" data-testid="change-email-confirm-loading">
            {m['profile.change_email_confirming']()}
          </p>
        )}
        {code && isError && (
          <div className="space-y-2">
            <div
              className="p-3 bg-destructive/10 border border-destructive/20 rounded text-destructive text-sm"
              data-testid="change-email-confirm-error"
            >
              {confirmFailureMessage(errorStatus)}
            </div>
            {errorStatus === 429 && (
              <div>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  data-testid="change-email-confirm-retry-button"
                  onClick={() => void refetch()}
                >
                  {m['common.retry']()}
                </Button>
              </div>
            )}
          </div>
        )}
        {code && isSuccess && (
          <p className="text-sm text-muted-foreground" data-testid="change-email-confirm-success">
            {m['profile.change_email_confirm_success']()}
          </p>
        )}
        <div>
          <Link
            to={realmPath(realmContext, '/user/profile')}
            className="text-sm font-medium text-primary hover:text-primary/80"
            data-testid="change-email-confirm-back-link"
          >
            {m['profile.change_email_back_to_profile']()}
          </Link>
        </div>
      </section>
    </div>
  )
}
