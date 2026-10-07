import { createFileRoute } from '@tanstack/react-router'
import { z } from 'zod'
import { EntitlementMappingsPage } from '@/components/billing/entitlement-mappings-page'
import { useResolvedRealmId } from '@/lib/realm-routing'

// eslint-disable-next-line react-refresh/only-export-components
export const entitlementMappingsSearchSchema = z.object({
  page: z.number().int().min(0).optional(),
  pageSize: z.number().int().min(1).max(100).optional(),
})

export const Route = createFileRoute('/$realmId/manage/billing/entitlement-mappings/')({
  validateSearch: entitlementMappingsSearchSchema,
  component: EntitlementMappingsRoute,
})

export function EntitlementMappingsRoute() {
  const realmId = useResolvedRealmId()

  return <EntitlementMappingsPage realmId={realmId} />
}
