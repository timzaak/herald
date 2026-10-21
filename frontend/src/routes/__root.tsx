import {
  createRootRouteWithContext,
  Outlet,
  redirect,
  isRedirect,
  useRouter,
} from '@tanstack/react-router'
import { Toaster } from '@/components/ui/sonner'
import { useQuery, useMutation, useQueryClient, type QueryClient } from '@tanstack/react-query'
import { initializeAuth, checkAdminPermission, logoutFlow } from '@/lib/auth-utils'
import {
  ADMIN_WEB_CONSOLE_CLIENT_ID,
  USER_ACCOUNT_CENTER_CLIENT_ID,
} from '@/lib/constants/auth-constants'
import { useAuthStore, useIsAuthenticated } from '@/stores/auth-store'
import { ReconsentDialog } from '@/components/legal/ReconsentDialog'
import { RootRouteError } from '@/components/route-error'
import {
  consentStatusQueryOptions,
  recordConsentMutation,
  toRecordConsentRequestFromStatus,
  queryKeys,
} from '@/data/query-options'
import { lazy, Suspense } from 'react'
import {
  realmPath,
  resolveRealmContext,
  resolvedRealmFromPath,
  classifyRealmRoute,
  isCustomDomainPath,
  isRealmScopedPublicPath,
  realmRelativePath,
} from '@/lib/realm-routing'
import { realmSessionChangedError } from '@/lib/herald-client'

const Devtools = import.meta.env.DEV
  ? lazy(() => import('@/components/devtools').then((m) => ({ default: m.Devtools })))
  : () => null

type RouterContext = {
  queryClient: QueryClient
}

const queryScopes = new WeakMap<QueryClient, string>()

export const Route = createRootRouteWithContext<RouterContext>()({
  // Establish realm and authentication before any child guard or loader runs.
  beforeLoad: async ({ location, context, abortController }) => {
    const pathname = location.pathname
    const checkNavigation = () => {
      if (abortController?.signal.aborted)
        throw new DOMException('Navigation changed', 'AbortError')
    }

    const realmContext = await resolveRealmContext(pathname)
    checkNavigation()
    const { realmId } = realmContext

    if (realmContext.isCustomDomain && !isCustomDomainPath(pathname)) {
      const relativePath = realmRelativePath(realmContext, pathname)
      if (relativePath === pathname) throw new Error('URL realm does not match the custom domain')
      throw redirect({
        to: relativePath,
        search: location.search,
        hash: location.hash || true,
        replace: true,
      })
    }
    // Canonicalize the legacy public entrances on the main domain.
    if (!realmContext.isCustomDomain && isRealmScopedPublicPath(pathname)) {
      throw redirect({
        to: realmPath(realmContext, pathname),
        search: location.search,
        hash: location.hash || true,
        replace: true,
      })
    }

    if (useAuthStore.getState().realmId !== realmId) {
      // Stop the previous realm's in-flight queries before auth init runs.
      // Clearing the cache stays with the scope check below — its scope tuple
      // leads with realmId, so it fires whenever this does.
      await context.queryClient.cancelQueries()
      checkNavigation()
    }

    // Route type detection (shared with the root component's consent gate)
    const { isRootPath, routePath, isRealmRootPath, isAuthRoute, isLegalRoute, isManageRoute } =
      classifyRealmRoute(realmContext, pathname)
    // The OAuth consent page is an /auth route that REQUIRES an
    // authenticated user to mount (its whole job is completing a pending
    // third-party authorize for the existing session). It must be exempt
    // from the authenticated-bounce below or the continuation params
    // (oauthClientId/redirectUri/state) are dropped and the authorization
    // can never finish; it forwards anonymous users to /auth/login itself.
    const isOAuthConsentRoute = routePath.startsWith('/auth/consent')

    try {
      // Initialize auth and populate Zustand store
      const targetClientId =
        isAuthRoute || isLegalRoute
          ? null
          : isManageRoute
            ? ADMIN_WEB_CONSOLE_CLIENT_ID
            : USER_ACCOUNT_CENTER_CLIENT_ID
      const result = await initializeAuth(realmId, targetClientId)
      checkNavigation()
      // A session minted for another realm cannot serve this realm's pages
      // (its Bearer token 403s on realm-scoped endpoints), so for routing it
      // is no session: without this, the bounce below would exile a
      // cross-realm user from /auth/login — the very page the OAuth consent
      // route forwards them to — and a third-party authorize could never
      // complete for them.
      const sessionMatchesRealm = useAuthStore.getState().realmId === realmId
      const authenticated = result.authenticated && sessionMatchesRealm
      // A QueryClient belongs to one authenticated identity at a time. This
      // covers legacy static user-query keys as well as realm-scoped queries.
      const session = useAuthStore.getState()
      const scope = JSON.stringify([
        realmId,
        authenticated,
        session.user?.id,
        result.clientId,
        // The fingerprint tracks the permission SET, not the array order —
        // the backend's order is not contractual (no ORDER BY; role edits
        // rewrite row order), and an order-only change must not flush the
        // whole cache.
        [...session.permissions].sort(),
        [...session.roles].sort(),
      ])
      if (queryScopes.get(context.queryClient) !== scope) {
        await context.queryClient.cancelQueries()
        checkNavigation()
        if (useAuthStore.getState().realmId !== session.realmId) throw realmSessionChangedError()
        context.queryClient.clear()
        queryScopes.set(context.queryClient, scope)
      }

      // Route redirect logic using Zustand state

      // Root path: redirect based on auth status and permissions
      if (isRootPath) {
        if (!authenticated) {
          // Redirect to login page with return URL
          throw redirect({
            to: realmPath(realmContext, '/auth/login'),
            search: { redirect: '/' },
          })
        }

        throw redirect({
          to: realmPath(realmContext, '/user/profile'),
        })
      }

      // Realm root is the personal product entry point.
      if (isRealmRootPath && authenticated) {
        throw redirect({
          to: realmPath(realmContext, '/user/profile'),
        })
      }

      // Public routes (auth + legal): redirect authenticated users away from
      // auth pages, but allow them to stay on legal agreement pages, on the
      // OAuth consent page (which only exists for authenticated users), and
      // whenever the session belongs to another realm (such a user must be
      // able to reach this realm's login page to re-authenticate).
      if (isAuthRoute && authenticated && sessionMatchesRealm && !isOAuthConsentRoute) {
        const targetPath = checkAdminPermission() ? '/manage' : '/user/profile'
        throw redirect({
          to: realmPath(realmContext, targetPath),
        })
      }

      // Protected routes: require authentication (auth and legal are public)
      if (!isAuthRoute && !isLegalRoute && !authenticated) {
        // Extract the relative path (without realm prefix)
        const relativePath = routePath || '/'
        throw redirect({
          to: realmPath(realmContext, '/auth/login'),
          search: { redirect: relativePath },
        })
      }

      // Manage route: require admin permission (only for authenticated users)
      if (
        isManageRoute &&
        (result.clientId !== ADMIN_WEB_CONSOLE_CLIENT_ID || !checkAdminPermission())
      ) {
        throw redirect({
          to: realmPath(realmContext, '/user/profile'),
        })
      }

      // Return empty object (auth data is now in Zustand store)
      return { realmContext }
    } catch (error) {
      // If it's a redirect, re-throw it
      if (isRedirect(error)) {
        throw error
      }
      if (error instanceof DOMException && error.name === 'AbortError') throw error

      // Other errors: redirect to login (unless it's a public route)
      if (!isAuthRoute && !isLegalRoute) {
        const relativePath = routePath || '/'
        throw redirect({
          to: realmPath(realmContext, '/auth/login'),
          search: { redirect: relativePath },
        })
      }

      // For auth routes, let them render (they handle their own errors)
      return { realmContext }
    }
  },

  component: RootComponent,
  errorComponent: RootRouteError,
})

function RootComponent() {
  const router = useRouter()
  const queryClient = useQueryClient()
  const isAuthenticated = useIsAuthenticated()
  const pathname = router.state.location.pathname

  const realmContext = resolvedRealmFromPath(pathname)
  const realmId = realmContext.realmId

  const { isRootPath, isRealmRootPath, isAuthRoute, isLegalRoute } = classifyRealmRoute(
    realmContext,
    pathname
  )

  const isCoreRoute =
    isAuthenticated && !isAuthRoute && !isLegalRoute && !isRootPath && !isRealmRootPath

  const { data: consentStatus } = useQuery({
    ...consentStatusQueryOptions(realmId),
    enabled: isCoreRoute,
  })

  const pendingItems = consentStatus?.items?.filter((item) => item.needs_reconsent) ?? []
  const needsReconsent = isCoreRoute && pendingItems.length > 0

  const consentMutation = useMutation({
    mutationFn: async () => {
      await recordConsentMutation(realmId, toRecordConsentRequestFromStatus(pendingItems))
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.consentStatus(realmId) })
    },
  })

  return (
    <>
      <div className="min-h-screen bg-background font-sans">
        <Outlet />
      </div>
      <Toaster />
      <Suspense>
        <Devtools />
      </Suspense>
      {needsReconsent && (
        <ReconsentDialog
          realmId={realmId}
          open={true}
          items={pendingItems}
          isPending={consentMutation.isPending}
          onAgree={() => consentMutation.mutate()}
          onLogout={() => logoutFlow(realmId)}
        />
      )}
    </>
  )
}
