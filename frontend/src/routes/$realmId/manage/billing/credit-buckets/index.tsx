import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { z } from 'zod'
import { CreditBucketsDirectoryPage } from '@/components/billing/credit-bucket/credit-bucket-directory-page'
import { realmPath, useCurrentSearch, useResolvedRealmContext } from '@/lib/realm-routing'

// eslint-disable-next-line react-refresh/only-export-components
export const creditBucketsSearchSchema = z.object({
  selected: z.string().optional(),
})

export const Route = createFileRoute('/$realmId/manage/billing/credit-buckets/')({
  validateSearch: creditBucketsSearchSchema,
  component: CreditBucketsIndexRoute,
})

export function CreditBucketsIndexRoute() {
  const realmContext = useResolvedRealmContext()
  const realmId = realmContext.realmId
  const search = useCurrentSearch<{ selected?: string }>()
  const navigate = useNavigate()

  function handleSelect(bucketId: string | undefined) {
    navigate({
      to: realmPath(realmContext, '/manage/billing/credit-buckets'),
      search: (prev: { selected?: string }) =>
        bucketId ? { selected: bucketId } : { ...prev, selected: undefined },
      replace: true,
    })
  }

  return (
    <CreditBucketsDirectoryPage
      realmId={realmId}
      selectedId={search.selected}
      onSelect={handleSelect}
    />
  )
}
