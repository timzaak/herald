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
import { useIsAuthenticated, useRealmId } from '@/stores/auth-store'
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
  isSessionScopedPath,
} from '@/lib/realm-routing'

const Devtools = import.meta.env.DEV
  ? lazy(() => import('@/components/devtools').then((m) => ({ default: m.Devtools })))
  : () => null

type RouterContext = {
  queryClient: QueryClient
}

export const Route = createRootRouteWithContext<RouterContext>()({
  // Unified loader - preloads auth data for all routes and handles authentication/permissions
  // Uses Zustand store instead of React Query cache for synchronous state updates
  loader: async ({ location }) => {
    const pathname = location.pathname

    const realmContext = await resolveRealmContext(pathname)
    const { realmId } = realmContext

    // Route type detection
    const isRootPath = pathname === '/'
    const routePath = realmContext.isCustomDomain
      ? pathname
      : pathname.replace(new RegExp(`^/${realmId}`), '') || '/'
    // A "realm root" is a single-segment realm-prefixed URL like `/admin` or
    // `/realm-001/` — authenticated users there are redirected to their home.
    // Session-scoped paths (`/manage`, `/user`, `/subscription`) carry no realm
    // in the URL and are valid landing targets, so they must be excluded:
    // otherwise `/manage` would match the single-segment pattern and the admin
    // redirect would target `/manage` again → infinite self-redirect loop.
    const isRealmRootPath =
      !realmContext.isCustomDomain &&
      !isSessionScopedPath(pathname) &&
      !!pathname.match(/^\/[^/]+\/?$/)
    const isAuthRoute = routePath.match(/^\/auth\//)
    // The OAuth consent page is an /auth route that REQUIRES an
    // authenticated user to mount (its whole job is completing a pending
    // third-party authorize for the existing session). It must be exempt
    // from the authenticated-bounce below or the continuation params
    // (oauthClientId/redirectUri/state) are dropped and the authorization
    // can never finish; it forwards anonymous users to /auth/login itself.
    const isOAuthConsentRoute = routePath.startsWith('/auth/consent')
    const isLegalRoute = routePath.match(/^\/legal\//)
    const isManageRoute = routePath.match(/^\/manage/)

    try {
      // Initialize auth and populate Zustand store
      const targetClientId = isManageRoute
        ? ADMIN_WEB_CONSOLE_CLIENT_ID
        : USER_ACCOUNT_CENTER_CLIENT_ID
      const result = await initializeAuth(realmId, targetClientId)
      const authenticated = result.authenticated

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

      // Public routes (auth + legal): redirect authenticated users away from auth pages,
      // but allow them to stay on legal agreement pages — and on the OAuth
      // consent page, which only exists for authenticated users.
      if (isAuthRoute && authenticated && !isOAuthConsentRoute) {
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
      return {}
    } catch (error) {
      // If it's a redirect, re-throw it
      if (isRedirect(error)) {
        throw error
      }

      // Other errors: redirect to login (unless it's a public route)
      if (!isAuthRoute && !isLegalRoute) {
        const relativePath = routePath || '/'
        throw redirect({
          to: realmPath(realmContext, '/auth/login'),
          search: { redirect: relativePath },
        })
      }

      // For auth routes, let them render (they handle their own errors)
      return {}
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
  const sessionRealmId = useRealmId()
  const realmId = /^\/(manage|user|subscription)(?:\/|$)/.test(pathname)
    ? sessionRealmId
    : realmContext.realmId

  const isRootPath = pathname === '/'
  const routePath = realmContext.isCustomDomain
    ? pathname
    : pathname.replace(new RegExp(`^/${realmId}`), '') || '/'
  // Mirror the loader's definition: session-scoped paths (`/manage`, `/user`,
  // `/subscription`) are valid landing routes, not realm roots.
  const isRealmRootPath =
    !realmContext.isCustomDomain && !isSessionScopedPath(pathname) && /^\/[^/]+\/?$/.test(pathname)
  const isAuthRoute = /^\/auth\//.test(routePath)
  const isLegalRoute = /^\/legal\//.test(routePath)

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
