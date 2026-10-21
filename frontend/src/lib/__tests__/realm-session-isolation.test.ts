import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { webcrypto } from 'node:crypto'
import { http, HttpResponse } from 'msw'
import { QueryClient } from '@tanstack/react-query'
import { server } from '@/test/mocks/server'
import { useAuthStore } from '@/stores/auth-store'
import {
  ensureHeraldClient,
  heraldRefreshTokenStorageKey,
  setHeraldBaseUrlOverride,
} from '@/lib/herald-client'
import {
  beginFirstPartyPkceFlow,
  completeLoginAfterOneTap,
  completeLoginAfterTotp,
  initializeAuth,
  logoutFlow,
} from '@/lib/auth-utils'
import { initBearerClient } from '@/lib/api-client'
import { getProfile } from '@/lib/api-generated'
import { profileQueryOptions } from '@/data/query-options'
import { ADMIN_WEB_CONSOLE_CLIENT_ID } from '@/lib/constants/auth-constants'

vi.mock('@/lib/api-generated', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/lib/api-generated')>()),
  resolveCustomDomain: vi.fn(async () => ({ error: {}, response: { status: 404 } })),
}))

vi.mock('@tanstack/react-router', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@tanstack/react-router')>()),
  createRootRouteWithContext: () => (config: Record<string, unknown>) => config,
}))

import { Route } from '@/routes/__root'
import { runRootBeforeLoad } from '@/test/utils/root-route'

initBearerClient()
const refreshRequests: string[] = []
const logoutRequests: Array<string | null> = []
let profileAccount = 'user'

function realmFromRequest(request: Request) {
  return request.headers.get('Authorization')?.includes('oncewise') ? 'oncewise' : 'admin'
}

const enter = (pathname: string, queryClient = new QueryClient()) =>
  runRootBeforeLoad(Route, pathname, queryClient)

beforeEach(() => {
  setHeraldBaseUrlOverride('http://localhost:3000')
  useAuthStore.getState().reset()
  localStorage.clear()
  sessionStorage.clear()
  refreshRequests.length = 0
  logoutRequests.length = 0
  profileAccount = 'user'
  // Navigation is captured without navigating JSDOM away from the test.
  vi.stubGlobal('window', {
    localStorage,
    sessionStorage,
    location: { origin: 'http://localhost:3000', pathname: '/admin/manage/settings', href: '' },
  })
  vi.stubGlobal('crypto', webcrypto)
  server.use(
    http.post('*/api/auth/browser-token/refresh', async ({ request }) => {
      const { refreshToken } = (await request.json()) as { refreshToken: string }
      refreshRequests.push(refreshToken)
      const realmId = refreshToken.includes('oncewise') ? 'oncewise' : 'admin'
      return HttpResponse.json({
        accessToken: `at-${realmId}`,
        refreshToken: `rt-${realmId}-rotated`,
        tokenType: 'Bearer',
        expiresIn: 900,
        refreshExpiresIn: 3600,
      })
    }),
    http.get('*/api/auth/status', ({ request }) =>
      HttpResponse.json({
        authenticated: true,
        realmId: realmFromRequest(request),
        clientId: ADMIN_WEB_CONSOLE_CLIENT_ID,
        clientAppId: 'console',
        userId: profileAccount,
        credentialClass: 'first_party',
        scopes: [],
      })
    ),
    http.get('*/api/user/profile', ({ request }) =>
      HttpResponse.json({
        id: `${realmFromRequest(request)}-${profileAccount}`,
        email: 'user@example.com',
        nickname: null,
        status: 1,
      })
    ),
    http.get('*/api/user/permissions', () => HttpResponse.json({ permissions: ['settings.view'] })),
    http.get('*/api/user/roles', () => HttpResponse.json({ roles: [] })),
    http.post('*/api/auth/logout', ({ request }) => {
      logoutRequests.push(request.headers.get('Authorization'))
      return HttpResponse.json({ message: 'ok' })
    }),
    http.get('*/api/oauth/:realmId/authorize', () => HttpResponse.json({}))
  )
})

afterEach(() => {
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('coexisting realm sessions on the main domain', () => {
  it('restores oncewise after admin logout, regardless of the old shared auth-storage snapshot', async () => {
    localStorage.setItem(heraldRefreshTokenStorageKey('oncewise'), 'rt-oncewise')
    ensureHeraldClient('admin').tokens.setTokens({
      accessToken: 'at-admin',
      refreshToken: 'rt-admin',
    })
    await logoutFlow('admin')
    expect(logoutRequests).toEqual(['Bearer at-admin'])
    expect(window.location.href).toBe('/admin/auth/login')
    expect(localStorage.getItem(heraldRefreshTokenStorageKey('oncewise'))).toBe('rt-oncewise')
    expect(localStorage.getItem(heraldRefreshTokenStorageKey('admin'))).toBeNull()

    // A page reload resets memory, and an old tab/version may still write the
    // former shared key. Neither can change the explicit oncewise URL.
    setHeraldBaseUrlOverride('http://localhost:3000')
    useAuthStore.getState().reset()
    localStorage.setItem(
      'auth-storage',
      JSON.stringify({ state: { realmId: 'admin', isAuthenticated: false } })
    )
    await expect(enter('/oncewise/manage/settings')).resolves.toMatchObject({
      realmContext: { realmId: 'oncewise' },
    })
    expect(refreshRequests).toEqual(['rt-oncewise'])
    expect(useAuthStore.getState()).toMatchObject({
      isAuthenticated: true,
      realmId: 'oncewise',
      user: { id: 'oncewise-user' },
    })
    expect(localStorage.getItem('auth-storage')).toBeNull()
  })

  it('queries status with the target realm token instead of the previous stores realm', async () => {
    useAuthStore.getState().setAuthStatus(true, 'admin')
    ensureHeraldClient('oncewise').tokens.setTokens({
      accessToken: 'at-oncewise',
      refreshToken: 'rt-oncewise',
    })
    await expect(
      initializeAuth('oncewise', ADMIN_WEB_CONSOLE_CLIENT_ID, true)
    ).resolves.toMatchObject({ authenticated: true })
    expect(useAuthStore.getState()).toMatchObject({
      realmId: 'oncewise',
      user: { id: 'oncewise-user' },
    })
  })

  it('clears static user-query caches before entering another realm or another account', async () => {
    const queryClient = new QueryClient()
    try {
      ensureHeraldClient('admin').tokens.setTokens({
        accessToken: 'at-admin',
        refreshToken: 'rt-admin',
      })
      await enter('/admin/manage/settings', queryClient)
      expect((await queryClient.fetchQuery(profileQueryOptions))?.id).toBe('admin-user')
      ensureHeraldClient('oncewise').tokens.setTokens({
        accessToken: 'at-oncewise',
        refreshToken: 'rt-oncewise',
      })
      await enter('/oncewise/manage/settings', queryClient)
      expect((await queryClient.fetchQuery(profileQueryOptions))?.id).toBe('oncewise-user')
      profileAccount = 'another-user'
      await initializeAuth('oncewise', ADMIN_WEB_CONSOLE_CLIENT_ID, true)
      await enter('/oncewise/manage/settings', queryClient)
      expect((await queryClient.fetchQuery(profileQueryOptions))?.id).toBe('oncewise-another-user')
    } finally {
      queryClient.clear()
    }
  })

  it('does not reuse a pending PKCE transaction from another realm with the same clientId', async () => {
    ensureHeraldClient('admin')
    useAuthStore.getState().setPkceState({
      realmId: 'admin',
      expiresAt: Date.now() + 600000,
      clientId: ADMIN_WEB_CONSOLE_CLIENT_ID,
      codeVerifier: 'admin-verifier',
      redirectUri: 'http://localhost:3000/callback',
      state: 'admin-flow',
    })
    const startedAt = Date.now()
    const result = await beginFirstPartyPkceFlow('oncewise', ADMIN_WEB_CONSOLE_CLIENT_ID)
    expect(result?.state).toBeTruthy()
    expect(result?.state).not.toBe('admin-flow')
    expect(useAuthStore.getState().pkceState).toMatchObject({ realmId: 'oncewise' })
    expect(useAuthStore.getState().pkceState?.expiresAt).toBeGreaterThanOrEqual(startedAt + 300000)
    expect(useAuthStore.getState().pkceState?.expiresAt).toBeLessThanOrEqual(Date.now() + 300000)
    expect(localStorage.getItem('auth-storage')).toBeNull()
  })

  it('preserves an existing console family while visiting public consent instead of switching products', async () => {
    const herald = ensureHeraldClient('oncewise')
    herald.tokens.setTokens({ accessToken: 'at-oncewise', refreshToken: 'rt-oncewise' })
    await enter('/oncewise/auth/consent')
    expect(useAuthStore.getState()).toMatchObject({
      isAuthenticated: true,
      refreshClientId: ADMIN_WEB_CONSOLE_CLIENT_ID,
    })
    expect(herald.tokens.getAccessToken()).toBe('at-oncewise')
    expect(herald.storage.getRefreshToken()).toBe('rt-oncewise')
  })

  it('rejects a status identity that does not match the selected realm before hydrating user data', async () => {
    ensureHeraldClient('oncewise').tokens.setTokens({
      accessToken: 'wrong-realm-token',
      refreshToken: 'wrong-refresh',
    })
    const result = await initializeAuth('oncewise', ADMIN_WEB_CONSOLE_CLIENT_ID, true)
    expect(result.authenticated).toBe(false)
    expect(useAuthStore.getState().user).toBeNull()
    expect(localStorage.getItem(heraldRefreshTokenStorageKey('oncewise'))).toBeNull()
  })

  it('does not let a late status response overwrite a newly selected realm', async () => {
    let release!: () => void
    let started!: () => void
    const waiting = new Promise<void>((resolve) => {
      started = resolve
    })
    const gate = new Promise<void>((resolve) => {
      release = resolve
    })
    server.use(
      http.get('*/api/auth/status', async () => {
        started()
        await gate
        return HttpResponse.json({
          authenticated: true,
          realmId: 'admin',
          clientId: ADMIN_WEB_CONSOLE_CLIENT_ID,
        })
      })
    )
    ensureHeraldClient('admin').tokens.setTokens({
      accessToken: 'at-admin',
      refreshToken: 'rt-admin',
    })
    const restoring = initializeAuth('admin', ADMIN_WEB_CONSOLE_CLIENT_ID, true)
    const rejected = expect(restoring).rejects.toMatchObject({ name: 'AbortError' })
    await waiting
    ensureHeraldClient('oncewise').tokens.setTokens({
      accessToken: 'at-oncewise',
      refreshToken: 'rt-oncewise',
    })
    useAuthStore.getState().setAuthStatus(true, 'oncewise')
    release()
    await rejected
    expect(useAuthStore.getState()).toMatchObject({ realmId: 'oncewise', isAuthenticated: true })
  })

  it('does not replay an old realms 401 with the new realms credential', async () => {
    let release!: () => void
    let started!: () => void
    const waiting = new Promise<void>((resolve) => {
      started = resolve
    })
    const gate = new Promise<void>((resolve) => {
      release = resolve
    })
    let requests = 0
    server.use(
      http.get('*/api/user/profile', async () => {
        requests += 1
        started()
        await gate
        return HttpResponse.json({ error: 'expired' }, { status: 401 })
      })
    )
    ensureHeraldClient('admin').tokens.setTokens({
      accessToken: 'at-admin',
      refreshToken: 'rt-admin',
    })
    const pending = getProfile()
    await waiting
    ensureHeraldClient('oncewise').tokens.setTokens({
      accessToken: 'at-oncewise',
      refreshToken: 'rt-oncewise',
    })
    release()
    expect((await pending).response.status).toBe(401)
    expect(requests).toBe(1)
    expect(refreshRequests).toEqual([])
  })

  it('does not reactivate an old realm when its direct-login completion arrives after navigation', async () => {
    ensureHeraldClient('oncewise').tokens.setTokens({
      accessToken: 'at-oncewise',
      refreshToken: 'rt-oncewise',
    })
    await expect(
      completeLoginAfterOneTap(
        'admin',
        {
          accessToken: 'late-at-admin',
          refreshToken: 'late-rt-admin',
          tokenType: 'Bearer',
          expiresIn: 900,
          refreshExpiresIn: 3600,
          userId: 'admin-user',
          message: 'ok',
        },
        ADMIN_WEB_CONSOLE_CLIENT_ID
      )
    ).rejects.toMatchObject({ name: 'AbortError' })
    expect(useAuthStore.getState().realmId).toBe('oncewise')
    expect(localStorage.getItem(heraldRefreshTokenStorageKey('oncewise'))).toBe('rt-oncewise')
  })

  it('fails loudly when a second factor completes after the PKCE flow is gone, instead of navigating to /callback', async () => {
    // WHY: /callback is not a route — handing the first-party redirect back
    // for "external" navigation after the PKCE state expired lands a
    // successfully-verified user on a dead page with no tokens applied.
    ensureHeraldClient('oncewise')
    useAuthStore.getState().setAuthStatus(true, 'oncewise')
    await expect(
      completeLoginAfterTotp('oncewise', {
        redirectTo: 'http://localhost:3000/callback?code=late-code&state=late-state',
      } as Parameters<typeof completeLoginAfterTotp>[1])
    ).rejects.toThrow('PKCE token exchange failed')

    // A genuinely third-party redirect still passes through untouched.
    await expect(
      completeLoginAfterTotp('oncewise', {
        redirectTo: 'https://app.example.com/cb?code=x&state=y',
      } as Parameters<typeof completeLoginAfterTotp>[1])
    ).resolves.toEqual({ redirectTo: 'https://app.example.com/cb?code=x&state=y' })
  })
})
