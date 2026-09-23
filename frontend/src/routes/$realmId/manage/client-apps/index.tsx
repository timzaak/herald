import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
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

  const { data, isLoading, error } = useQuery(
    clientAppsQueryOptions(realmId, {
      page: search.page,
      pageSize: search.pageSize,
    })
  )

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
        return { ...app, enabled: !app.enabled }
      }),
    getSuccessMessage: (data) =>
      m['client_apps.toggled_status']({
        name: data.name,
        status: data.enabled
          ? m['client_apps.status_enabled']()
          : m['client_apps.status_disabled'](),
      }),
    invalidateQueries: [queryKeys.clientAppsList(realmId)],
  })

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
            onToggleEnabled={(app) => toggleMutate(app)}
            canUpdate={canUpdate}
            canDelete={canDelete}
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
