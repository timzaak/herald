import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { useSuspenseQuery } from '@tanstack/react-query'
import { lazy, Suspense } from 'react'
import { ArrowLeft, Loader2 } from 'lucide-react'
import { clientAppQueryOptions } from '@/data/query-options'
import {
  realmPath,
  useLastPathSegment,
  useResolvedRealmContext,
  useResolvedRealmId,
} from '@/lib/realm-routing'
import { BuiltinBadge } from '@/components/shared/builtin-badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { MCP_CLIENT_ID } from '@/lib/constants'
import { m } from '@/paraglide/messages'

const ClientAppFormPage = lazy(() =>
  import('@/components/client-apps/client-app-form-page').then((m) => ({
    default: m.ClientAppFormPage,
  }))
)

export const Route = createFileRoute('/$realmId/manage/client-apps/$clientAppId/edit')({
  component: EditClientAppPage,
})

export function EditClientAppPage() {
  const realmContext = useResolvedRealmContext()
  const realmId = useResolvedRealmId()
  const clientAppId = useLastPathSegment(1)
  const navigate = useNavigate()
  const { data: clientApp } = useSuspenseQuery(clientAppQueryOptions(realmId, clientAppId))

  // The server rejects every field except `enabled` for the built-in MCP
  // client, so the generic form must not mount for it.
  if (clientApp.clientId === MCP_CLIENT_ID) {
    return (
      <div className="container max-w-4xl mx-auto py-6 px-6">
        <div className="space-y-6" data-testid="client-app-mcp-protected">
          <div className="flex items-center gap-4">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => navigate({ to: realmPath(realmContext, '/manage/client-apps') })}
              data-testid="client-app-mcp-back-button"
            >
              <ArrowLeft className="h-4 w-4" />
            </Button>
            <div>
              <h1 className="flex items-center gap-2 text-2xl font-bold" data-testid="page-title">
                {clientApp.name}
                <BuiltinBadge isBuiltin={clientApp.isSystemBuiltin} />
              </h1>
              <p className="text-sm text-muted-foreground">
                {m['client_apps.mcp_protected_description']()}
              </p>
            </div>
          </div>
          <Card>
            <CardContent className="space-y-2 pt-6 text-sm text-muted-foreground">
              <p>{m['client_apps.mcp_protection_only_toggle']()}</p>
              <p>{m['client_apps.mcp_disable_hint']()}</p>
            </CardContent>
          </Card>
        </div>
      </div>
    )
  }

  return (
    <div className="container max-w-4xl mx-auto py-6 px-6">
      <Suspense
        fallback={
          <div
            className="flex items-center justify-center py-12"
            data-testid="client-app-form-loading"
          >
            <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
          </div>
        }
      >
        <ClientAppFormPage mode="edit" realmId={realmId} clientApp={clientApp} />
      </Suspense>
    </div>
  )
}
