import { useState } from 'react'
import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { webhooksQueryOptions, emailStatusQueryOptions, queryKeys } from '@/data/query-options'
import { webhooksSearchSchema } from '@/lib/schemas/search-params'
import { DeleteWebhookDialog } from '@/components/delete-webhook-dialog'
import { RotateWebhookSecretDialog } from '@/components/rotate-webhook-secret-dialog'
import { WebhookSubscriptionTable } from '@/components/webhook-subscription-table'
import { getWebhookStatusLabel } from '@/lib/constants/webhook'
import { ListPagination, AccessDenied, PageHeader } from '@/components/shared'
import { Plus, CircleAlert } from 'lucide-react'
import { useDialogManager } from '@/hooks/use-dialog-state'
import {
  deleteWebhookSubscription,
  rotateWebhookSecret,
  setWebhookSubscriptionStatus,
} from '@/lib/api-generated'
import type {
  WebhookSubscriptionItem,
  WebhookSubscriptionItemWithSecret,
} from '@/lib/api-generated'
import { useFormMutation } from '@/hooks/use-form-mutation'
import { usePermission } from '@/hooks/use-permission'
import { PERMISSION } from '@/lib/constants/auth-constants'
import type { WebhooksSearchParams } from '@/lib/schemas/search-params'
import { Card, CardContent } from '@/components/ui/card'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { m } from '@/paraglide/messages'
import { realmPath, useCurrentSearch, useResolvedRealmContext } from '@/lib/realm-routing'

export const Route = createFileRoute('/$realmId/manage/webhooks/')({
  component: WebhooksPage,
  validateSearch: (search): WebhooksSearchParams => {
    const parsed = webhooksSearchSchema.parse(search)
    return {
      page: parsed.page,
      pageSize: parsed.pageSize,
    }
  },
})

export function WebhooksPage() {
  const realmContext = useResolvedRealmContext()
  const realmId = realmContext.realmId
  const navigate = useNavigate()
  const search = useCurrentSearch<WebhooksSearchParams>()
  const queryClient = useQueryClient()
  const { hasPermission } = usePermission()

  const canManage = hasPermission(PERMISSION.WEBHOOKS_MANAGE)
  const canView = hasPermission(PERMISSION.WEBHOOKS_VIEW)

  const deleteDialog = useDialogManager<WebhookSubscriptionItem>()
  const rotateDialog = useDialogManager<WebhookSubscriptionItem>()
  // Per-row pending ids: a single value would let one row's completion
  // re-enable another row's still-in-flight status button.
  const [statusPendingIds, setStatusPendingIds] = useState<ReadonlySet<string>>(() => new Set())

  const { data, isLoading, error } = useQuery(
    webhooksQueryOptions(realmId, {
      page: search.page,
      pageSize: search.pageSize,
    })
  )

  const items = data?.items ?? []
  const hasPaused = items.some((item) => item.status === 'paused')

  // Fail-safe: on query error the note is simply not shown — the status column
  // stays authoritative regardless of email channel configuration.
  const { data: emailStatus } = useQuery({
    ...emailStatusQueryOptions(realmId),
    enabled: hasPaused,
  })

  const { mutate: deleteMutate } = useFormMutation({
    // The single-subscription key is dynamic (deleted id); return it so
    // onSuccess can drop the cached edit page for a deleted subscription.
    mutationFn: (subscription: WebhookSubscriptionItem) =>
      deleteWebhookSubscription({
        path: { subscriptionId: subscription.id },
      }).then((response) => {
        if (response.error) throw response.error
        return { id: subscription.id }
      }),
    getSuccessMessage: () => m['webhooks.deleted_success'](),
    invalidateQueries: [queryKeys.webhooksList(realmId)],
    onSuccess: (data) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.webhook(realmId, data.id) })
    },
  })

  const { mutate: setStatusMutate } = useFormMutation({
    mutationFn: (input: { subscription: WebhookSubscriptionItem; status: 'active' | 'disabled' }) =>
      setWebhookSubscriptionStatus({
        path: { subscriptionId: input.subscription.id },
        body: { status: input.status },
      }).then((response) => {
        if (response.error) throw response.error
        return response.data
      }),
    getSuccessMessage: (item: WebhookSubscriptionItem) =>
      m['webhooks.status_changed']({
        url: item.endpointUrl,
        status: getWebhookStatusLabel(item.status),
      }),
    invalidateQueries: [queryKeys.webhooksList(realmId)],
    // Like rotate below: refresh the dynamic single key so a cached edit page
    // reflects the new status instead of a stale badge for its staleTime.
    onSuccess: (item: WebhookSubscriptionItem) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.webhook(realmId, item.id) })
    },
  })

  const { mutate: rotateMutate } = useFormMutation({
    mutationFn: (subscription: WebhookSubscriptionItem) =>
      rotateWebhookSecret({
        path: { subscriptionId: subscription.id },
      }).then((response) => {
        if (response.error) throw response.error
        return response.data as WebhookSubscriptionItemWithSecret
      }),
    getSuccessMessage: () => m['webhooks.rotated_success'](),
    invalidateQueries: [queryKeys.webhooksList(realmId)],
    onSuccess: (data) => {
      // The single-subscription key is dynamic; refresh it here so a cached
      // edit page reflects the rotated secretHint.
      void queryClient.invalidateQueries({ queryKey: queryKeys.webhook(realmId, data.id) })
      void navigate({
        to: realmPath(realmContext, '/manage/webhooks/reveal'),
        state: { secretData: data } as unknown as Parameters<typeof navigate>[0]['state'],
      })
    },
  })

  const handleSetStatus = (
    subscription: WebhookSubscriptionItem,
    status: 'active' | 'disabled'
  ) => {
    setStatusPendingIds((prev) => new Set(prev).add(subscription.id))
    setStatusMutate({ subscription, status })
      .catch(() => {})
      .finally(() =>
        setStatusPendingIds((prev) => {
          if (!prev.has(subscription.id)) return prev
          const next = new Set(prev)
          next.delete(subscription.id)
          return next
        })
      )
  }

  const handlePageChange = (newPage: number) => {
    navigate({
      to: realmPath(realmContext, '/manage/webhooks'),
      search: { ...search, page: newPage },
    })
  }

  if (!canView) {
    return <AccessDenied message={m['webhooks.access_denied']()} />
  }

  return (
    <div className="space-y-6" data-testid="webhooks-page">
      <PageHeader
        title={m['webhooks.page_title']()}
        subtitle={m['webhooks.page_subtitle']()}
        headingTestId="webhooks-heading"
        action={
          canManage
            ? {
                label: m['webhooks.add_button'](),
                onClick: () => navigate({ to: realmPath(realmContext, '/manage/webhooks/new') }),
                testId: 'add-webhook-button',
                icon: <Plus className="mr-2 h-4 w-4" />,
              }
            : undefined
        }
      />

      {hasPaused && emailStatus?.configured === false && (
        <Alert data-testid="webhook-email-unavailable-alert">
          <CircleAlert className="h-4 w-4" />
          <AlertDescription>{m['webhooks.email_unavailable_note']()}</AlertDescription>
        </Alert>
      )}

      <Card>
        <CardContent className="space-y-4 pt-6">
          <WebhookSubscriptionTable
            data={items}
            isLoading={isLoading}
            error={error}
            onEdit={(subscription) =>
              navigate({
                to: realmPath(realmContext, `/manage/webhooks/${subscription.id}/edit`),
              })
            }
            onSetStatus={handleSetStatus}
            onRotateSecret={(subscription) => rotateDialog.open(subscription)}
            onDelete={(subscription) => deleteDialog.open(subscription)}
            canManage={canManage}
            statusPendingIds={statusPendingIds}
          />
        </CardContent>
      </Card>

      {data && (
        <ListPagination
          page={data.page}
          pageSize={data.pageSize}
          total={data.total}
          onPageChange={handlePageChange}
          testIdPrefix="webhooks-pagination"
        />
      )}

      {deleteDialog.selectedItem && (
        <DeleteWebhookDialog
          open={deleteDialog.isOpen}
          onOpenChange={deleteDialog.onOpenChange}
          onConfirm={() => {
            if (deleteDialog.selectedItem) {
              deleteMutate(deleteDialog.selectedItem)
              deleteDialog.close()
            }
          }}
          endpointUrl={deleteDialog.selectedItem.endpointUrl}
        />
      )}

      {rotateDialog.selectedItem && (
        <RotateWebhookSecretDialog
          open={rotateDialog.isOpen}
          onOpenChange={rotateDialog.onOpenChange}
          onConfirm={() => {
            if (rotateDialog.selectedItem) {
              rotateMutate(rotateDialog.selectedItem)
              rotateDialog.close()
            }
          }}
          endpointUrl={rotateDialog.selectedItem.endpointUrl}
        />
      )}
    </div>
  )
}
