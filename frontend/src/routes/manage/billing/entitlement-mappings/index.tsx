import { createFileRoute } from '@tanstack/react-router'
import {
  EntitlementMappingsRoute,
  entitlementMappingsSearchSchema,
} from '@/routes/$realmId/manage/billing/entitlement-mappings/index'

export const Route = createFileRoute('/manage/billing/entitlement-mappings/')({
  validateSearch: entitlementMappingsSearchSchema,
  component: EntitlementMappingsRoute,
})
