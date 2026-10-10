import { useEffect } from 'react'
import { createFileRoute, Link } from '@tanstack/react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { changeEmailConfirmQueryOptions, queryKeys } from '@/data/query-options'
import { changeEmailConfirmSearchSchema } from '@/lib/schemas/search-params'
import { getErrorMessage } from '@/lib/error-utils'
import { realmPath, resolvedRealmFromPath } from '@/lib/realm-routing'
import { PageHeader } from '@/components/shared'
import { m } from '@/paraglide/messages'

export const Route = createFileRoute('/$realmId/user/change-email/confirm')({
  component: ChangeEmailConfirmPage,
  validateSearch: (search) => changeEmailConfirmSearchSchema.parse(search),
})

export function ChangeEmailConfirmPage() {
  // Shared by both route trees (realm-prefixed and session-scoped), so the
  // realm and the code are read from the raw location like the reset-password
  // page does instead of tree-specific hooks.
  const realmContext = resolvedRealmFromPath(window.location.pathname)
  const { realmId } = realmContext
  const { code } = changeEmailConfirmSearchSchema.parse(
    Object.fromEntries(new URLSearchParams(window.location.search))
  )

  // The query commits the email change exactly once (see the options def for
  // why it must never retry or refetch).
  const { isPending, isError, isSuccess, error } = useQuery(
    changeEmailConfirmQueryOptions(realmId, code)
  )
  const queryClient = useQueryClient()

  useEffect(() => {
    if (isSuccess) {
      void queryClient.invalidateQueries({ queryKey: queryKeys.profile() })
    }
  }, [isSuccess, queryClient])

  return (
    <div className="space-y-8">
      <PageHeader title={m['profile.change_email_confirm_title']()} />
      <section className="max-w-lg space-y-4" data-testid="change-email-confirm-card">
        {isPending && (
          <p className="text-sm text-muted-foreground" data-testid="change-email-confirm-pending">
            {m['profile.change_email_confirming']()}
          </p>
        )}
        {isError && (
          <div className="space-y-2">
            <div
              className="p-3 bg-destructive/10 border border-destructive/20 rounded text-destructive text-sm"
              data-testid="change-email-confirm-error"
            >
              {getErrorMessage(error)}
            </div>
            <p
              className="text-sm text-muted-foreground"
              data-testid="change-email-confirm-error-hint"
            >
              {m['profile.change_email_confirm_error_hint']()}
            </p>
          </div>
        )}
        {isSuccess && (
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
