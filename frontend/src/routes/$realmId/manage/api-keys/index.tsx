import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { apiKeysQueryOptions, queryKeys } from '@/data/query-options'
import { apiKeysSearchSchema } from '@/lib/schemas/search-params'
import { DeleteApiKeyDialog } from '@/components/api-keys/delete-api-key-dialog'
import { ApiKeyRolesDialog } from '@/components/api-keys/api-key-roles-dialog'
import { ApiKeyTable } from '@/components/api-keys/api-key-table'
import { ListPagination, AccessDenied } from '@/components/shared'
import { Plus } from 'lucide-react'
import { useDialogManager } from '@/hooks/use-dialog-state'
import { deleteApiKey, updateApiKey } from '@/lib/api-generated'
import { useFormMutation } from '@/hooks/use-form-mutation'
import { usePermission } from '@/hooks/use-permission'
import { PERMISSION } from '@/lib/constants/auth-constants'
import type { ApiKeyListItem } from '@/lib/api-generated'
import type { ApiKeysSearchParams } from '@/lib/schemas/search-params'
import { Card, CardContent } from '@/components/ui/card'
import { PageHeader } from '@/components/shared'
import { m } from '@/paraglide/messages'
import { realmPath, useCurrentSearch, useResolvedRealmContext } from '@/lib/realm-routing'

export const Route = createFileRoute('/$realmId/manage/api-keys/')({
  component: ApiKeysPage,
  validateSearch: (search): ApiKeysSearchParams => {
    const parsed = apiKeysSearchSchema.parse(search)
    return {
      page: parsed.page,
      pageSize: parsed.pageSize,
    }
  },
})

export function ApiKeysPage() {
  const realmContext = useResolvedRealmContext()
  const realmId = realmContext.realmId
  const navigate = useNavigate()
  const search = useCurrentSearch<ApiKeysSearchParams>()
  const { hasPermission } = usePermission()

  const canManage = hasPermission(PERMISSION.API_KEYS_MANAGE)
  const canView = hasPermission(PERMISSION.API_KEYS_VIEW)
  const canManageRoles = hasPermission(PERMISSION.ROLES_MANAGE)

  const deleteDialog = useDialogManager<ApiKeyListItem>()
  const rolesDialog = useDialogManager<ApiKeyListItem>()

  const { data, isLoading, error } = useQuery(
    apiKeysQueryOptions(realmId, {
      page: search.page,
      pageSize: search.pageSize,
    })
  )

  const { mutate: deleteMutate } = useFormMutation({
    mutationFn: (key: ApiKeyListItem) =>
      deleteApiKey({
        path: { apiKeyId: key.id },
      }).then((response) => {
        if (response.error) throw response.error
        return response.data
      }),
    getSuccessMessage: () => m['api_keys.deleted_success'](),
    invalidateQueries: [queryKeys.apiKeysList(realmId)],
  })

  const { mutate: toggleMutate } = useFormMutation({
    mutationFn: (key: ApiKeyListItem) =>
      updateApiKey({
        path: { apiKeyId: key.id },
        body: { enabled: !key.enabled },
      }).then((response) => {
        if (response.error) throw response.error
        return { ...key, enabled: !key.enabled }
      }),
    getSuccessMessage: (data) =>
      m['api_keys.toggled_status']({
        name: data.name,
        status: data.enabled ? m['api_keys.status_enabled']() : m['api_keys.status_disabled'](),
      }),
    invalidateQueries: [queryKeys.apiKeysList(realmId)],
  })

  const handlePageChange = (newPage: number) => {
    navigate({
      to: realmPath(realmContext, '/manage/api-keys'),
      search: { ...search, page: newPage },
    })
  }

  if (!canView) {
    return <AccessDenied message={m['api_keys.access_denied']()} />
  }

  return (
    <div className="space-y-6" data-testid="api-keys-page">
      <PageHeader
        title={m['api_keys.page_title']()}
        headingTestId="api-keys-heading"
        action={
          canManage
            ? {
                label: m['api_keys.add_button'](),
                onClick: () => navigate({ to: realmPath(realmContext, '/manage/api-keys/new') }),
                testId: 'add-api-key-button',
                icon: <Plus className="h-4 w-4 mr-2" />,
              }
            : undefined
        }
      />

      <Card>
        <CardContent className="space-y-4 pt-6">
          <ApiKeyTable
            data={data?.items ?? []}
            isLoading={isLoading}
            error={error}
            onEdit={(key) =>
              navigate({
                to: realmPath(realmContext, `/manage/api-keys/${key.id}/edit`),
              })
            }
            onDelete={(key) => deleteDialog.open(key)}
            onToggleEnabled={(key) => toggleMutate(key)}
            canUpdate={canManage}
            canDelete={canManage}
            onManageRoles={(key) => rolesDialog.open(key)}
            canManageRoles={canManageRoles}
          />
        </CardContent>
      </Card>

      {data && (
        <ListPagination
          page={data.page}
          pageSize={data.pageSize}
          total={data.total}
          onPageChange={handlePageChange}
          testIdPrefix="api-key-pagination"
        />
      )}

      {deleteDialog.selectedItem && (
        <DeleteApiKeyDialog
          open={deleteDialog.isOpen}
          onOpenChange={deleteDialog.onOpenChange}
          onConfirm={() => {
            if (deleteDialog.selectedItem) {
              deleteMutate(deleteDialog.selectedItem)
              deleteDialog.close()
            }
          }}
          apiKeyName={deleteDialog.selectedItem.name}
        />
      )}

      {rolesDialog.selectedItem && (
        <ApiKeyRolesDialog
          open={rolesDialog.isOpen}
          onOpenChange={rolesDialog.onOpenChange}
          apiKeyId={rolesDialog.selectedItem.id}
          apiKeyName={rolesDialog.selectedItem.name}
        />
      )}
    </div>
  )
}
