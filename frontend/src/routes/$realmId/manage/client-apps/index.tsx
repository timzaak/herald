import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useState } from 'react'
import { clientAppsQueryOptions, queryKeys } from '@/data/query-options'
import { clientAppsSearchSchema } from '@/lib/schemas/search-params'
import { DeleteClientAppDialog } from '@/components/client-apps/delete-client-app-dialog'
import { ClientAppTable } from '@/components/client-apps/client-app-table'
import { ListPagination } from '@/components/shared'
import { Plus } from 'lucide-react'
import { useDialogManager } from '@/hooks/use-dialog-state'
import { deleteClientApp, updateClientApp } from '@/lib/api-generated'
import { useFormMutation } from '@/hooks/use-form-mutation'
import { usePermission } from '@/hooks/use-permission'
import { PERMISSION } from '@/lib/constants/auth-constants'
import type { ClientAppItem } from '@/lib/api-generated'
import type { ClientAppsSearchParams } from '@/lib/schemas/search-params'
import { Card, CardContent } from '@/components/ui/card'
import { PageHeader } from '@/components/shared'
import { m } from '@/paraglide/messages'
import { realmPath, useCurrentSearch, useResolvedRealmContext } from '@/lib/realm-routing'

export const Route = createFileRoute('/$realmId/manage/client-apps/')({
  component: ClientAppsPage,
  validateSearch: (search): ClientAppsSearchParams => {
    const parsed = clientAppsSearchSchema.parse(search)
    return {
      page: parsed.page,
      pageSize: parsed.pageSize,
    }
  },
})

export function ClientAppsPage() {
  const realmContext = useResolvedRealmContext()
  const realmId = realmContext.realmId
  const navigate = useNavigate()
  const search = useCurrentSearch<ClientAppsSearchParams>()
  const { hasPermission } = usePermission()

  const canCreate = hasPermission(PERMISSION.CLIENTS_MANAGE)
  const canUpdate = hasPermission(PERMISSION.CLIENTS_MANAGE)
  const canDelete = hasPermission(PERMISSION.CLIENTS_MANAGE)

  const deleteDialog = useDialogManager<ClientAppItem>()

  const queryClient = useQueryClient()
  // Rows whose server-side enabled state could not be re-confirmed after a
  // failed toggle. A set, not a single slot: a second failure must not
  // silently release the first app's lock.
  const [statusErrorAppIds, setStatusErrorAppIds] = useState<string[]>([])
  const [togglingAppId, setTogglingAppId] = useState<string | null>(null)

  const { data, isLoading, error, refetch } = useQuery(
    clientAppsQueryOptions(realmId, {
      page: search.page,
      pageSize: search.pageSize,
    })
  )

  // Any successful list fetch is authoritative for every row's enabled
  // state, so stale "unconfirmed" locks must not outlive it (window-focus
  // refetch, another app's successful toggle, pagination).
  useEffect(() => {
    setStatusErrorAppIds((previous) => (previous.length === 0 ? previous : []))
  }, [data])

  const { mutate: deleteMutate } = useFormMutation({
    mutationFn: (app: ClientAppItem) =>
      deleteClientApp({
        path: { clientAppId: app.id },
      }).then((response) => {
        if (response.error) throw response.error
        return response.data
      }),
    getSuccessMessage: () => m['client_apps.deleted_success'](),
    invalidateQueries: [queryKeys.clientAppsList(realmId)],
  })

  const { mutate: toggleMutate } = useFormMutation({
    mutationFn: (app: ClientAppItem) =>
      updateClientApp({
        path: { clientAppId: app.id },
        body: { enabled: !app.enabled },
      }).then((response) => {
        if (response.error) throw response.error
        return response.data
      }),
    getSuccessMessage: (data) =>
      m['client_apps.toggled_status']({
        name: data.name,
        status: data.enabled
          ? m['client_apps.status_enabled']()
          : m['client_apps.status_disabled'](),
      }),
    invalidateQueries: [queryKeys.clientAppsList(realmId)],
    // Only the toggled app's detail cache is affected; invalidating the
    // whole detail prefix would needlessly stain every other app's cache.
    onSuccess: (data) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.clientApp(realmId, data.id) })
    },
  })

  // A failed toggle may still have persisted server-side (e.g. disable commits
  // before a Redis cleanup failure returns 500), so recovery must re-fetch the
  // real state instead of trusting the pre-toggle row.
  const refetchRealStatus = async (app: ClientAppItem) => {
    try {
      const result = await refetch()
      if (result.error) throw result.error
      await queryClient.invalidateQueries({ queryKey: queryKeys.clientApp(realmId, app.id) })
      setStatusErrorAppIds((previous) => previous.filter((id) => id !== app.id))
    } catch {
      setStatusErrorAppIds((previous) =>
        previous.includes(app.id) ? previous : [...previous, app.id]
      )
    }
  }

  const handleToggleEnabled = async (app: ClientAppItem) => {
    setTogglingAppId(app.id)
    try {
      await toggleMutate(app)
    } catch {
      await refetchRealStatus(app)
    } finally {
      setTogglingAppId(null)
    }
  }

  const handlePageChange = (newPage: number) => {
    navigate({
      to: realmPath(realmContext, '/manage/client-apps'),
      search: { ...search, page: newPage },
    })
  }

  return (
    <div className="space-y-6" data-testid="client-apps-page">
      <PageHeader
        title={m['client_apps.page_title']()}
        headingTestId="client-apps-heading"
        action={
          canCreate
            ? {
                label: m['client_apps.add_button'](),
                onClick: () => navigate({ to: realmPath(realmContext, '/manage/client-apps/new') }),
                testId: 'add-client-app-button',
                icon: <Plus className="h-4 w-4 mr-2" />,
              }
            : undefined
        }
      />

      <Card>
        <CardContent className="space-y-4 pt-6">
          <ClientAppTable
            data={data?.items ?? []}
            isLoading={isLoading}
            error={error}
            onEdit={(app) =>
              navigate({
                to: realmPath(realmContext, `/manage/client-apps/${app.id}/edit`),
              })
            }
            onDelete={(app) => deleteDialog.open(app)}
            onToggleEnabled={handleToggleEnabled}
            onRetryStatus={refetchRealStatus}
            canUpdate={canUpdate}
            canDelete={canDelete}
            togglingAppId={togglingAppId}
            statusErrorAppIds={statusErrorAppIds}
          />
        </CardContent>
      </Card>

      {data && (
        <ListPagination
          page={data.page}
          pageSize={data.pageSize}
          total={data.total}
          onPageChange={handlePageChange}
          testIdPrefix="client-app-pagination"
        />
      )}

      {deleteDialog.selectedItem && (
        <DeleteClientAppDialog
          open={deleteDialog.isOpen}
          onOpenChange={deleteDialog.onOpenChange}
          onConfirm={() => {
            if (deleteDialog.selectedItem) {
              deleteMutate(deleteDialog.selectedItem)
              deleteDialog.close()
            }
          }}
          clientAppName={deleteDialog.selectedItem.name}
        />
      )}
    </div>
  )
}
