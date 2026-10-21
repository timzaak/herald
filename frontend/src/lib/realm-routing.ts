import { resolveCustomDomain, type PublicConfigResponse } from '@/lib/api-generated'
import { useLocation } from '@tanstack/react-router'

const SESSION_SCOPED_ROOT_SEGMENTS = new Set(['manage', 'user', 'subscription'])
const REALM_SCOPED_PUBLIC_ROOT_SEGMENTS = new Set(['auth', 'legal', 'device'])
// A custom domain serves the union: the prefix-less product tree plus the
// realm-less public entrances.
const CUSTOM_DOMAIN_ROOT_SEGMENTS = new Set([
  ...SESSION_SCOPED_ROOT_SEGMENTS,
  ...REALM_SCOPED_PUBLIC_ROOT_SEGMENTS,
])

export interface ResolvedRealmContext {
  realmId: string
  isCustomDomain: boolean
  publicConfig?: PublicConfigResponse
}

/**
 * Whether a path belongs to the prefix-less product tree used on custom domains.
 */
export function isSessionScopedPath(pathname: string): boolean {
  return SESSION_SCOPED_ROOT_SEGMENTS.has(firstPathSegment(pathname) ?? '')
}

/**
 * Whether a path is one of the public entrances that carry no realm in the
 * URL on the main domain (`/auth/...`, `/legal/...`, `/device/...`).
 */
export function isRealmScopedPublicPath(pathname: string): boolean {
  return REALM_SCOPED_PUBLIC_ROOT_SEGMENTS.has(firstPathSegment(pathname) ?? '')
}

let cachedCustomDomainContext: ResolvedRealmContext | null = null
let hostResolved = false
let hostResolution: Promise<void> | null = null

export class RealmRequiredError extends Error {
  constructor() {
    super('Use a realm-prefixed URL on the main domain')
    this.name = 'RealmRequiredError'
  }
}

function firstPathSegment(pathname: string): string | null {
  return pathname.split('/').filter(Boolean)[0] ?? null
}

export function isCustomDomainPath(pathname: string): boolean {
  const first = firstPathSegment(pathname)
  return first === null || CUSTOM_DOMAIN_ROOT_SEGMENTS.has(first)
}

export function getLegacyRealmId(pathname: string): string {
  const first = firstPathSegment(pathname)
  if (first && SESSION_SCOPED_ROOT_SEGMENTS.has(first)) throw new RealmRequiredError()
  // Only the main-domain root and public legacy entrances default to admin.
  // Protected pages must identify their realm in the URL, never in an auth store.
  if (!first || REALM_SCOPED_PUBLIC_ROOT_SEGMENTS.has(first)) return 'admin'
  return first
}

export async function resolveRealmContext(pathname: string): Promise<ResolvedRealmContext> {
  // Resolve Host even on a prefixed URL: a custom domain cannot be used to
  // select another realm by changing a path segment. Cache only host identity.
  if (!hostResolved) {
    hostResolution ??= (async () => {
      const response = await resolveCustomDomain()
      if (response.error) {
        // A network-level failure resolves with no response object at all —
        // that is an outage, not a 404 "this host is the main domain".
        if (response.response?.status !== 404)
          throw new Error('Unable to resolve custom-domain realm')
      } else {
        if (!response.data.realmId)
          throw new Error('Custom-domain resolve response did not include realmId')
        cachedCustomDomainContext = {
          realmId: response.data.realmId,
          isCustomDomain: true,
          publicConfig: response.data.publicConfig,
        }
      }
      hostResolved = true
    })().finally(() => {
      hostResolution = null
    })
    await hostResolution
  }
  return resolvedRealmFromPath(pathname)
}

export function resolvedRealmFromPath(pathname: string): ResolvedRealmContext {
  if (cachedCustomDomainContext) return cachedCustomDomainContext

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
  return resolvedRealmFromPath(useCurrentPathname())
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
    const location = useLocation()
    return location.search as TSearch
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
  return `/${context.realmId}${normalized === '/' ? '' : normalized}`
}

export function realmRelativePath(context: ResolvedRealmContext, pathname: string): string {
  const prefix = `/${context.realmId}`
  return pathname === prefix || pathname.startsWith(`${prefix}/`)
    ? pathname.slice(prefix.length) || '/'
    : pathname
}

export interface RealmRouteClassification {
  isRootPath: boolean
  /** Path relative to the realm prefix (identity on the custom-domain tree). */
  routePath: string
  isRealmRootPath: boolean
  isAuthRoute: boolean
  isLegalRoute: boolean
  isManageRoute: boolean
}

/**
 * Route-type classification shared by the root beforeLoad's guards and the
 * root component's consent gate — the flags must not drift between the two.
 */
export function classifyRealmRoute(
  context: ResolvedRealmContext,
  pathname: string
): RealmRouteClassification {
  const routePath = realmRelativePath(context, pathname)
  // A "realm root" is a single-segment realm-prefixed URL like `/admin` or
  // `/realm-001/` — authenticated users there are redirected to their home.
  // Session-scoped paths (`/manage`, `/user`, `/subscription`) carry no realm
  // in the URL and are valid landing targets, so they must be excluded:
  // otherwise `/manage` would match the single-segment pattern and the admin
  // redirect would target `/manage` again → infinite self-redirect loop.
  const isRealmRootPath =
    !context.isCustomDomain && !isSessionScopedPath(pathname) && /^\/[^/]+\/?$/.test(pathname)
  return {
    isRootPath: pathname === '/',
    routePath,
    isRealmRootPath,
    isAuthRoute: /^\/auth\//.test(routePath),
    isLegalRoute: /^\/legal\//.test(routePath),
    isManageRoute: /^\/manage/.test(routePath),
  }
}
