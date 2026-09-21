import { resolveCustomDomain, type PublicConfigResponse } from '@/lib/api-generated'
import { useAuthStore } from '@/stores/auth-store'
import { useLocation, useRouter } from '@tanstack/react-router'

const CUSTOM_DOMAIN_ROOT_SEGMENTS = new Set(['auth', 'manage', 'user', 'legal', 'device'])
const SESSION_SCOPED_ROOT_SEGMENTS = new Set(['manage', 'user', 'subscription'])
const REALM_SCOPED_PUBLIC_ROOT_SEGMENTS = new Set(['auth', 'legal', 'device'])

export interface ResolvedRealmContext {
  realmId: string
  isCustomDomain: boolean
  publicConfig?: PublicConfigResponse
}

/**
 * Whether `pathname` is a session-scoped root (e.g. `/manage`, `/user/profile`).
 *
 * Session-scoped routes carry no realm in the URL — their realm is read from
 * the session store. The root loader must NOT treat these as "realm root"
 * paths (which would redirect an authenticated admin back to `/manage`,
 * creating a self-redirect loop). See `.ai/future/f1.md`.
 */
export function isSessionScopedPath(pathname: string): boolean {
  return SESSION_SCOPED_ROOT_SEGMENTS.has(firstPathSegment(pathname) ?? '')
}

let cachedCustomDomainContext: ResolvedRealmContext | null = null

function firstPathSegment(pathname: string): string | null {
  return pathname.split('/').filter(Boolean)[0] ?? null
}

export function isCustomDomainPath(pathname: string): boolean {
  const first = firstPathSegment(pathname)
  return first === null || CUSTOM_DOMAIN_ROOT_SEGMENTS.has(first)
}

export function getLegacyRealmId(pathname: string): string {
  const first = firstPathSegment(pathname)
  if (!first) return 'admin'
  // Session-scoped roots (/manage, /user, /subscription) carry no realm in the
  // URL — their realm comes from the session store.
  if (SESSION_SCOPED_ROOT_SEGMENTS.has(first)) {
    return useAuthStore.getState().realmId || 'admin'
  }
  // Public realm-scoped roots (/auth, /legal, /device) are prefix-less on the
  // main domain and resolve to the default 'admin' realm. Returning the segment
  // name itself (e.g. 'auth') would yield an invalid realm id.
  if (REALM_SCOPED_PUBLIC_ROOT_SEGMENTS.has(first)) {
    return 'admin'
  }
  // Legacy realm-prefixed URLs (e.g. /admin/auth/login) — first segment is the realm.
  return first
}

export function getCachedCustomDomainRealm(): ResolvedRealmContext | null {
  return cachedCustomDomainContext
}

export async function resolveRealmContext(pathname: string): Promise<ResolvedRealmContext> {
  if (!isCustomDomainPath(pathname)) {
    return {
      realmId: getLegacyRealmId(pathname),
      isCustomDomain: false,
    }
  }

  if (cachedCustomDomainContext) {
    return cachedCustomDomainContext
  }

  // The resolve endpoint has no `?host=` override (removed as a host-to-realm
  // oracle); it reads the request Host header, which the browser fetch carries
  // as window.location.host automatically.
  const response = await resolveCustomDomain()
  if (response.error) {
    // On a non-custom-domain host the resolve endpoint deterministically 404s.
    // For main-domain paths that carry no realm prefix ('/', session-scoped
    // /manage /user /subscription, public /auth /legal /device), fall back to
    // the legacy realm model via getLegacyRealmId so the app still renders
    // instead of throwing. getLegacyRealmId is the single source of truth for
    // the fallback realm id across both entry points.
    const firstSeg = firstPathSegment(pathname) ?? ''
    const isUnprefixedMainDomainPath =
      pathname === '/' ||
      SESSION_SCOPED_ROOT_SEGMENTS.has(firstSeg) ||
      REALM_SCOPED_PUBLIC_ROOT_SEGMENTS.has(firstSeg)
    if (isUnprefixedMainDomainPath) {
      // Cache the fallback so subsequent renders/loads don't re-fetch the same
      // 404ing resolve endpoint on every navigation (the host is stable within
      // a page session; a non-custom-domain host deterministically 404s). Without
      // this cache, `resolveRealmContext` would re-fire on every render and drive
      // the root loader + session-scoped routes into a fetch loop.
      cachedCustomDomainContext = {
        realmId: getLegacyRealmId(pathname),
        isCustomDomain: false,
      }
      return cachedCustomDomainContext
    }
    throw new Error('Unable to resolve custom-domain realm')
  }

  const payload = response.data
  if (!payload.realmId) {
    throw new Error('Custom-domain resolve response did not include realmId')
  }

  cachedCustomDomainContext = {
    realmId: payload.realmId,
    isCustomDomain: true,
    publicConfig: payload.publicConfig,
  }
  return cachedCustomDomainContext
}

export function resolvedRealmFromPath(pathname: string): ResolvedRealmContext {
  if (isCustomDomainPath(pathname)) {
    const resolved = getCachedCustomDomainRealm()
    if (resolved) return resolved
  }

  return {
    realmId: getLegacyRealmId(pathname),
    isCustomDomain: false,
  }
}

function getWindowPathname(): string {
  return typeof window === 'undefined' ? '/' : window.location.pathname
}

function getWindowSearch(): Record<string, string> {
  if (typeof window === 'undefined') return {}
  return Object.fromEntries(new URLSearchParams(window.location.search))
}

export function useCurrentPathname(): string {
  try {
    return useLocation().pathname
  } catch {
    return getWindowPathname()
  }
}

export function useResolvedRealmContext(): ResolvedRealmContext {
  const pathname = useCurrentPathname()
  const sessionRealmId = useAuthStore((state) => state.realmId)
  const context = resolvedRealmFromPath(pathname)
  return SESSION_SCOPED_ROOT_SEGMENTS.has(firstPathSegment(pathname) ?? '')
    ? { ...context, realmId: sessionRealmId || context.realmId }
    : context
}

export function useResolvedRealmId(fallback: string = 'admin'): string {
  return useResolvedRealmContext().realmId || fallback
}

export function usePathSegments(): string[] {
  return useCurrentPathname().split('/').filter(Boolean)
}

export function useLastPathSegment(offsetFromEnd: number = 0): string {
  const segments = usePathSegments()
  return segments[segments.length - 1 - offsetFromEnd] ?? ''
}

export function useCurrentSearch<TSearch>(): TSearch {
  try {
    const router = useRouter()
    return router.state.location.search as TSearch
  } catch {
    return getWindowSearch() as TSearch
  }
}

/**
 * Read the params of a `createFileRoute` route without throwing when the
 * component is mounted under a different route tree.
 *
 * `$realmId/...` route components are reused by the prefix-less custom-domain
 * routes (e.g. `/auth/login` reuses `LoginPage`). On the custom-domain tree
 * there is no active match for `/$realmId/auth/login`, so `Route.useParams()`
 * throws. TanStack's own hooks dereference an undefined match when
 * `shouldThrow:false`, so the params cannot be recovered any other way; we
 * invoke `useParams` through the passed-in route object and catch, falling back
 * to an empty object so callers can use the resolved realm context. Calling it
 * indirectly (rather than as a static `Route.useParams()`) also keeps the
 * `react-hooks/rules-of-hooks` lint rule quiet.
 */
export function useOptionalRouteParams<TParams extends Record<string, string | undefined>>(route: {
  useParams: () => TParams
}): TParams {
  try {
    return route.useParams()
  } catch {
    return {} as TParams
  }
}

export function realmPath(context: ResolvedRealmContext, path: string): string {
  const normalized = path.startsWith('/') ? path : `/${path}`
  if (context.isCustomDomain) return normalized
  if (!REALM_SCOPED_PUBLIC_ROOT_SEGMENTS.has(firstPathSegment(normalized) ?? '')) {
    return normalized
  }
  return `/${context.realmId}${normalized}`
}
