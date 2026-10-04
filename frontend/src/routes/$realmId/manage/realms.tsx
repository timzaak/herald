import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { useDialogManager } from '@/hooks/use-dialog-state'
import { realmsQueryOptions } from '@/data/query-options'
import { realmsSearchSchema, type RealmsSearchParams } from '@/lib/schemas/search-params'
import { RealmSearch } from '@/components/realms/realm-search'
import { RealmTable } from '@/components/realms/realm-table'
import { ListPagination } from '@/components/shared'
import { CreateRealmDialog } from '@/components/realms/create-realm-dialog'
import { RealmDetailDialog } from '@/components/realms/realm-detail-dialog'
import { usePermission } from '@/hooks/use-permission'
import { PERMISSION } from '@/lib/constants/auth-constants'
import { Plus } from 'lucide-react'
import { Card, CardContent } from '@/components/ui/card'
import { PageHeader } from '@/components/shared'
import { m } from '@/paraglide/messages'
import { useCurrentSearch } from '@/lib/realm-routing'

export const Route = createFileRoute('/$realmId/manage/realms')({
  component: RealmsPage,
  validateSearch: (search) => realmsSearchSchema.parse(search),
})

export function RealmsPage() {
  const search = useCurrentSearch<RealmsSearchParams>()
  const navigate = useNavigate()
  const [isCreateDialogOpen, setIsCreateDialogOpen] = useState(false)
  const detailDialog = useDialogManager<string>()

  const { hasPermission } = usePermission()
  const canCreateRealm = hasPermission(PERMISSION.REALM_MANAGE)

  const { data, isLoading, error } = useQuery(
    realmsQueryOptions({
      page: search.page,
      pageSize: search.pageSize,
      search: search.search,
      sortBy: search.sortBy,
      sortOrder: search.sortOrder,
    })
  )

  function handleCreateRealm() {
    setIsCreateDialogOpen(true)
  }

  function handleViewDetail(realm: { id: string }) {
    detailDialog.open(realm.id)
  }

  function handleSearchChange(query: string | undefined) {
    // `to: '.'` keeps the update on the CURRENT route match: this component
    // backs both /$realmId/manage/realms and the prefix-less /manage/realms,
    // and a navigate bound to the $realmId route would hop mirror visitors
    // across route matches (remounting the page and dropping search context).
    navigate({ to: '.', search: (prev) => ({ ...prev, search: query, page: 0 }) })
  }

  function handlePageChange(page: number) {
    navigate({ to: '.', search: (prev) => ({ ...prev, page }) })
  }

  return (
    <div data-testid="realms-page" className="space-y-6">
      <PageHeader
        title={m['realms.page_title']()}
        headingTestId="realms-heading"
        action={
          canCreateRealm
            ? {
                label: m['realms.create_button'](),
                onClick: handleCreateRealm,
                testId: 'create-realm-button',
                icon: <Plus className="mr-2 h-4 w-4" />,
              }
            : undefined
        }
      />

      <Card>
        <CardContent className="space-y-4 pt-6">
          <div className="flex items-center gap-4">
            <RealmSearch realmId={search.search} onSearchChange={handleSearchChange} />
          </div>

          {data && (
            <RealmTable
              data={data.items}
              isLoading={isLoading}
              error={error ?? undefined}
              onViewDetail={handleViewDetail}
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
          testIdPrefix="realm-pagination"
        />
      )}

      <CreateRealmDialog open={isCreateDialogOpen} onOpenChange={setIsCreateDialogOpen} />

      <RealmDetailDialog
        open={detailDialog.isOpen}
        onOpenChange={(open) => !open && detailDialog.close()}
        realmId={detailDialog.selectedItem}
      />
    </div>
  )
}
