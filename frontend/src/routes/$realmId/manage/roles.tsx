import { createFileRoute } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useRealmId } from '@/stores/auth-store'
import { adminRolesQueryOptions } from '@/data/query-options'
import { RoleTable } from '@/components/roles/role-table'
import { CreateRoleDialog } from '@/components/roles/create-role-dialog'
import { Card, CardContent } from '@/components/ui/card'
import { PageHeader } from '@/components/shared'
import { usePermission } from '@/hooks/use-permission'
import { PERMISSION } from '@/lib/constants/auth-constants'
import { useState } from 'react'
import { m } from '@/paraglide/messages'

export const Route = createFileRoute('/$realmId/manage/roles')({
  component: RolesPage,
})

export function RolesPage() {
  const realmId = useRealmId()
  const [createDialogOpen, setCreateDialogOpen] = useState(false)
  // permissions.md §6 按钮级控制：仅有 roles.view 时创建按钮不出现、表格写操作
  // 不可用；角色权限关联对话框按 §4.2 由 policies.manage 单独门控。
  const { hasPermission } = usePermission()
  const canManage = hasPermission(PERMISSION.ROLES_MANAGE)
  const canManagePermissions = hasPermission(PERMISSION.POLICIES_MANAGE)

  const {
    data: roles,
    isLoading,
    error,
  } = useQuery({
    ...adminRolesQueryOptions(realmId),
  })

  return (
    <div className="space-y-6" data-testid="roles-page">
      <PageHeader
        title={m['roles.page_title']()}
        headingTestId="roles-heading"
        action={{
          label: m['roles.add_button'](),
          onClick: () => setCreateDialogOpen(true),
          testId: 'role-create-button',
          show: canManage,
        }}
      />

      <Card>
        <CardContent className="pt-6">
          <RoleTable
            roles={roles ?? []}
            isLoading={isLoading}
            error={error}
            canManage={canManage}
            canManagePermissions={canManagePermissions}
          />
        </CardContent>
      </Card>

      <CreateRoleDialog
        open={createDialogOpen}
        onOpenChange={setCreateDialogOpen}
        realmId={realmId}
      />
    </div>
  )
}
