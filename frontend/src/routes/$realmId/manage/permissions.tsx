import { createFileRoute } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useRealmId } from '@/stores/auth-store'
import { adminPermissionsQueryOptions } from '@/data/query-options'
import { PermissionTable } from '@/components/permissions/permission-table'
import { CreatePermissionDialog } from '@/components/permissions/create-permission-dialog'
import { Card, CardContent } from '@/components/ui/card'
import { PageHeader } from '@/components/shared'
import { usePermission } from '@/hooks/use-permission'
import { PERMISSION } from '@/lib/constants/auth-constants'
import { useState } from 'react'
import { m } from '@/paraglide/messages'

export const Route = createFileRoute('/$realmId/manage/permissions')({
  component: PermissionsPage,
})

export function PermissionsPage() {
  const realmId = useRealmId()
  const [createDialogOpen, setCreateDialogOpen] = useState(false)
  // permissions.md §6 按钮级控制：仅有 permissions.view 时创建按钮不出现、
  // 表格写操作不可用（后端仍按 permissions.manage 兜底）。
  const { hasPermission } = usePermission()
  const canManage = hasPermission(PERMISSION.PERMISSIONS_MANAGE)

  const {
    data: permissions,
    isLoading,
    error,
  } = useQuery({
    ...adminPermissionsQueryOptions(realmId),
  })

  return (
    <div className="space-y-6" data-testid="permissions-page">
      <PageHeader
        title={m['permissions.page_title']()}
        action={{
          label: m['permissions.add_button'](),
          onClick: () => setCreateDialogOpen(true),
          testId: 'permission-create-button',
          show: canManage,
        }}
      />

      <Card>
        <CardContent className="pt-6">
          <PermissionTable
            permissions={permissions ?? []}
            isLoading={isLoading}
            error={error}
            canManage={canManage}
          />
        </CardContent>
      </Card>

      <CreatePermissionDialog
        open={createDialogOpen}
        onOpenChange={setCreateDialogOpen}
        realmId={realmId}
      />
    </div>
  )
}
