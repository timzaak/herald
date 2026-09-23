import { createFileRoute, redirect } from '@tanstack/react-router'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { useDialogManager } from '@/hooks/use-dialog-state'
import { toast } from 'sonner'
import { queryKeys, usersQueryOptions } from '@/data/query-options'
import { usersSearchSchema, type UsersSearchParams } from '@/lib/schemas/search-params'
import { UserSearch } from '@/components/users/user-search'
import { UserTable } from '@/components/users/user-table'
import { ListPagination } from '@/components/shared'
import { CreateUserDialog } from '@/components/users/create-user-dialog'
import { EditUserDialog } from '@/components/users/edit-user-dialog'
import { UserRolesDialog } from '@/components/users/user-roles-dialog'
import { deleteUser, resetUserPassword } from '@/lib/api-generated'
import { useRealmId, useAuthStore } from '@/stores/auth-store'
import type { UserResponse } from '@/lib/api-generated'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { ConfirmDialog, PageHeader } from '@/components/shared'
import { ResetPasswordResultDialog } from '@/components/users/reset-password-result-dialog'
import { UserSessionsDialog } from '@/components/users/user-sessions-dialog'
import { m } from '@/paraglide/messages'
import { useCurrentSearch } from '@/lib/realm-routing'
import { getErrorMessage } from '@/lib/error-utils'
import { usePermission } from '@/hooks/use-permission'
import { PERMISSION } from '@/lib/constants/auth-constants'

export const Route = createFileRoute('/$realmId/manage/users')({
  component: UsersPage,
  validateSearch: (search) => usersSearchSchema.parse(search),
  loader: ({ params }) => {
    const urlRealmId = params.realmId
    const authRealmId = useAuthStore.getState().realmId

    // Prevent cross-realm access: redirect user to their authenticated realm
    if (authRealmId && urlRealmId !== authRealmId) {
      console.warn(
        `[Users loader] Cross-realm access blocked - URL: ${urlRealmId}, Auth: ${authRealmId}`
      )
      throw redirect({
        to: '/$realmId/manage/users',
        params: { realmId: authRealmId },
      })
    }

    return { urlRealmId }
  },
})

export function UsersPage() {
  const search = useCurrentSearch<UsersSearchParams>()
  const navigate = Route.useNavigate()
  const queryClient = useQueryClient()
  const realmId = useRealmId()
  const [isCreateDialogOpen, setIsCreateDialogOpen] = useState(false)
  const editDialog = useDialogManager<UserResponse>()
  const deleteDialog = useDialogManager<UserResponse>()
  const rolesDialog = useDialogManager<UserResponse>()
  const resetPasswordDialog = useDialogManager<UserResponse>()
  const sessionsDialog = useDialogManager<UserResponse>()
  const [resetPasswordResult, setResetPasswordResult] = useState<string | null>(null)
  const { hasPermission } = usePermission()
  const canManage = hasPermission(PERMISSION.USERS_MANAGE)

  const { data, isLoading, error } = useQuery(
    usersQueryOptions(realmId, {
      page: search.page,
      pageSize: search.pageSize,
      email: search.email,
      status: search.status,
    })
  )

  const deleteMutation = useMutation({
    mutationFn: async (userId: string) => {
      const result = await deleteUser({ path: { userId } })
      if (result.error) throw result.error
      return result.data
    },
    onSuccess: () => {
      deleteDialog.close()
      queryClient.invalidateQueries({ queryKey: queryKeys.usersList(realmId) })
      toast.success(m['users.user_deleted']())
    },
    onError: (error: unknown) => {
      toast.error(getErrorMessage(error) ?? m['users.delete_failed']())
    },
  })

  const resetPasswordMutation = useMutation({
    mutationFn: async (userId: string) => {
      const result = await resetUserPassword({
        path: { userId },
      })
      if (result.error) throw result.error
      return result.data
    },
    onSuccess: (data) => {
      resetPasswordDialog.close()
      setResetPasswordResult(data.newPassword)
    },
    onError: (error: unknown) => {
      toast.error(getErrorMessage(error) ?? m['users.reset_password_failed']())
    },
  })

  function handleEdit(user: UserResponse) {
    editDialog.open(user)
  }

  function handleDelete(user: UserResponse) {
    deleteDialog.open(user)
  }

  function handleCreateUser() {
    setIsCreateDialogOpen(true)
  }

  function handleManageRoles(user: UserResponse) {
    rolesDialog.open(user)
  }

  function handleResetPassword(user: UserResponse) {
    resetPasswordDialog.open(user)
  }

  function handleManageSessions(user: UserResponse) {
    sessionsDialog.open(user)
  }

  function handleSearchChange(email: string | undefined) {
    navigate({ search: (prev) => ({ ...prev, email, page: 0 }) })
  }

  function handleStatusChange(status: string | undefined) {
    navigate({ search: (prev) => ({ ...prev, status, page: 0 }) })
  }

  function handlePageChange(page: number) {
    navigate({ search: (prev) => ({ ...prev, page }) })
  }

  return (
    <div data-testid="users-page" className="space-y-6">
      <PageHeader
        title={m['users.page_title']()}
        headingTestId="users-heading"
        action={{
          label: m['users.add_button'](),
          onClick: handleCreateUser,
          testId: 'create-user-button',
        }}
      />

      <Card>
        <CardHeader>
          <CardTitle>{m['users.card_title']()}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center gap-4">
            <UserSearch
              email={search.email}
              status={search.status}
              onSearchChange={handleSearchChange}
              onStatusChange={handleStatusChange}
            />
          </div>

          {data && (
            <UserTable
              data={data.items}
              isLoading={isLoading}
              error={error ?? undefined}
              onEdit={handleEdit}
              onDelete={handleDelete}
              onManageRoles={handleManageRoles}
              onResetPassword={handleResetPassword}
              onManageSessions={canManage ? handleManageSessions : undefined}
            />
          )}
        </CardContent>
      </Card>

      {data && (
        <ListPagination
          page={data.page}
          pageSize={data.pageSize}
          total={data.total}
          onPageChange={handlePageChange}
          testIdPrefix="user-pagination"
        />
      )}

      <CreateUserDialog
        open={isCreateDialogOpen}
        onOpenChange={setIsCreateDialogOpen}
        realmId={realmId}
      />

      {editDialog.selectedItem && (
        <EditUserDialog
          open={editDialog.isOpen}
          onOpenChange={editDialog.onOpenChange}
          realmId={realmId}
          user={editDialog.selectedItem}
        />
      )}

      {rolesDialog.selectedItem && (
        <UserRolesDialog
          open={rolesDialog.isOpen}
          onOpenChange={(v) => {
            if (!v) rolesDialog.close()
          }}
          userId={rolesDialog.selectedItem.id}
          userEmail={rolesDialog.selectedItem.email}
        />
      )}

      {deleteDialog.selectedItem && (
        <ConfirmDialog
          open={deleteDialog.isOpen}
          onOpenChange={(v) => {
            if (!v) deleteDialog.close()
          }}
          title={m['users.delete_title']()}
          description={m['users.delete_description']({ email: deleteDialog.selectedItem.email })}
          onConfirm={() => deleteMutation.mutate(deleteDialog.selectedItem!.id)}
          isPending={deleteMutation.isPending}
          contentTestId="delete-user-dialog"
          confirmTestId="confirm-delete-user-button"
          cancelTestId="cancel-delete-user-button"
        />
      )}

      {resetPasswordDialog.selectedItem && (
        <ConfirmDialog
          open={resetPasswordDialog.isOpen}
          onOpenChange={(v) => {
            if (!v) resetPasswordDialog.close()
          }}
          title={m['users.reset_password_title']()}
          description={m['users.reset_password_description']({
            email: resetPasswordDialog.selectedItem.email,
          })}
          onConfirm={() => resetPasswordMutation.mutate(resetPasswordDialog.selectedItem!.id)}
          confirmLabel={m['users.reset_password_confirm']()}
          confirmClassName="bg-primary text-primary-foreground hover:bg-primary/90"
          isPending={resetPasswordMutation.isPending}
          contentTestId="reset-password-dialog"
          confirmTestId="confirm-reset-password-button"
        />
      )}

      <ResetPasswordResultDialog
        open={!!resetPasswordResult}
        onOpenChange={(v) => {
          if (!v) setResetPasswordResult(null)
        }}
        newPassword={resetPasswordResult ?? ''}
      />

      {sessionsDialog.selectedItem && (
        <UserSessionsDialog
          open={sessionsDialog.isOpen}
          onOpenChange={sessionsDialog.onOpenChange}
          realmId={realmId}
          user={sessionsDialog.selectedItem}
        />
      )}
    </div>
  )
}
