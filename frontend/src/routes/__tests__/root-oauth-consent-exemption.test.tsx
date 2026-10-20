/**
 * @vitest-environment jsdom
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { isRedirect } from '@tanstack/react-router'
import { USER_ACCOUNT_CENTER_CLIENT_ID } from '@/lib/constants/auth-constants'
import { useAuthStore } from '@/stores/auth-store'

/**
 * The __root loader's OAuth-consent exemption, pinned at the loader level.
 *
 * /auth/consent is the one /auth route that mounts FOR authenticated users —
 * its whole job is completing a pending third-party authorize for the
 * existing session. Before the exemption, the authenticated-bounce redirected
 * session-holders to /manage or /user/profile and silently dropped
 * oauthClientId/redirectUri/state, so third-party authorization could never
 * finish for anyone with a console or user-center session. consent.test.tsx
 * mocks the router entirely and never executes the root loader; only this
 * file runs the real loader, so the exemption cannot regress silently: the
 * consent path must resolve without a redirect while the sibling login path
 * still bounces.
 */

const mockInitializeAuth = vi.fn()

vi.mock('@/lib/auth-utils', () => ({
  initializeAuth: (...args: unknown[]) => mockInitializeAuth(...args),
  checkAdminPermission: () => false,
  logoutFlow: vi.fn(),
}))

vi.mock('@/lib/realm-routing', async () => {
  const actual = await vi.importActual<typeof import('@/lib/realm-routing')>('@/lib/realm-routing')
  return {
    ...actual,
    resolveRealmContext: async (pathname: string) => ({
      realmId: pathname.split('/')[1] || '',
      isCustomDomain: false,
    }),
  }
})

// Return the raw route config so `loader` sits at the top level of the
// exported Route object (the real types nest it under `.options`).
vi.mock('@tanstack/react-router', async () => {
  const actual =
    await vi.importActual<typeof import('@tanstack/react-router')>('@tanstack/react-router')
  return {
    ...actual,
    createRootRouteWithContext: () => (config: Record<string, unknown>) => config,
  }
})

import { Route as RootRoute } from '../__root'

type RootLoaderFn = (ctx: { location: { pathname: string } }) => Promise<Record<string, never>>

function runRootLoader(pathname: string) {
  const loader = (RootRoute as unknown as { loader: RootLoaderFn }).loader
  return loader({ location: { pathname } })
}

describe('__root loader OAuth consent exemption', () => {
  beforeEach(() => {
    // initializeAuth is mocked here, so its store side effect — recording the
    // session's realm — must be simulated; the loader's cross-realm routing
    // reads exactly that. 'test-realm' matches every path used below.
    useAuthStore.setState({ realmId: 'test-realm' })
  })

  it('lets an authenticated user stay on /auth/consent (no redirect, continuation params survive)', async () => {
    mockInitializeAuth.mockResolvedValue({
      authenticated: true,
      clientId: USER_ACCOUNT_CENTER_CLIENT_ID,
    })

    await expect(runRootLoader('/test-realm/auth/consent')).resolves.toEqual({})
  })

  it('still bounces an authenticated user off a regular /auth page (the contrast that makes the exemption necessary)', async () => {
    mockInitializeAuth.mockResolvedValue({
      authenticated: true,
      clientId: USER_ACCOUNT_CENTER_CLIENT_ID,
    })

    const error = await runRootLoader('/test-realm/auth/login').catch((thrown) => thrown)
    expect(isRedirect(error)).toBe(true)
    expect(JSON.stringify(error)).toContain('user/profile')
  })

  it('lets an anonymous visitor reach /auth/consent (the page forwards to login itself, params preserved)', async () => {
    mockInitializeAuth.mockResolvedValue({
      authenticated: false,
      clientId: USER_ACCOUNT_CENTER_CLIENT_ID,
    })

    await expect(runRootLoader('/test-realm/auth/consent')).resolves.toEqual({})
  })

  it('lets a cross-realm session reach /auth/login (the consent page forwards it there to re-authenticate)', async () => {
    // The refresh-token storage is shared across realms, so an admin-console
    // session can be restored on a third-party realm's pages. Bouncing such
    // a user off /auth/login would strand the consent forward — this is the
    // loader half of the cross-realm consent fix, the page half lives in
    // consent.tsx.
    useAuthStore.setState({ realmId: 'admin' })
    mockInitializeAuth.mockResolvedValue({
      authenticated: true,
      clientId: USER_ACCOUNT_CENTER_CLIENT_ID,
    })

    await expect(runRootLoader('/test-realm/auth/login')).resolves.toEqual({})
  })
})
