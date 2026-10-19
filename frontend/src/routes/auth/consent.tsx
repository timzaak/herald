import { createFileRoute } from '@tanstack/react-router'
import { OAuthConsentPage } from '@/routes/$realmId/auth/consent'
import { consentSearchSchema } from '@/lib/schemas/search-params'

export const Route = createFileRoute('/auth/consent')({
  component: OAuthConsentPage,
  validateSearch: (search) => consentSearchSchema.parse(search),
})
