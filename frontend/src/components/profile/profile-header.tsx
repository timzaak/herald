import { useRealmId } from '@/stores/auth-store'
import { LogOut } from 'lucide-react'
import { useCallback } from 'react'
import { useQuery } from '@tanstack/react-query'
import { logoutFlow } from '@/lib/auth-utils'
import { m } from '@/paraglide/messages'
import { useLocation } from '@tanstack/react-router'
import { resolvedRealmFromPath } from '@/lib/realm-routing'
import { publicConfigQueryOptions } from '@/data/query-options'

export function ProfileHeader() {
  const storeRealmId = useRealmId()
  const location = useLocation()
  const realmContext = resolvedRealmFromPath(location.pathname)
  const realmId = realmContext.realmId || storeRealmId || 'admin'

  // The header shows the human-facing realm name, not the raw realm id. The
  // public config endpoint is the user-plane source (no admin permission
  // needed — GET /api/realms/{id} would 403 for a plain user).
  const { data: publicConfig } = useQuery(publicConfigQueryOptions(realmId))
  const realmLabel = publicConfig?.realmName ?? realmId

  const handleLogout = useCallback(async () => {
    await logoutFlow(realmId)
  }, [realmId])

  return (
    <header data-testid="profile-header" className="hidden border-b border-border md:block">
      <div className="mx-auto flex w-full max-w-2xl items-center justify-between px-4 py-3 md:px-8 md:py-4">
        <h2
          data-testid="profile-heading"
          className="min-w-0 truncate font-mono text-xs uppercase tracking-wide text-muted-foreground"
        >
          {realmLabel} / {m['nav_profile.profile']()}
        </h2>
        <button
          data-testid="profile-header-logout-button"
          onClick={handleLogout}
          className="flex items-center gap-2 px-3 py-2 text-sm text-muted-foreground hover:text-foreground transition-colors"
        >
          <LogOut className="h-4 w-4" />
          <span>{m['user_menu.logout']()}</span>
        </button>
      </div>
    </header>
  )
}
