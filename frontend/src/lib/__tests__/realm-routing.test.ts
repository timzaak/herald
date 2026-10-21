import { describe, it, expect, beforeEach, vi } from 'vitest'

const resolveHost = vi.hoisted(() => vi.fn())
vi.mock('@/lib/api-generated', () => ({ resolveCustomDomain: resolveHost }))

let routing: typeof import('@/lib/realm-routing')
let authStore: typeof import('@/stores/auth-store')

beforeEach(async () => {
  vi.resetModules()
  resolveHost.mockReset()
  resolveHost.mockResolvedValue({ error: {}, response: { status: 404 } })
  routing = await import('@/lib/realm-routing')
  // Import the store from the SAME post-resetModules graph as the module
  // under test: if realm-routing ever goes back to reading login memory from
  // the store, these writes are what it must observe. A statically imported
  // instance would be isolated by resetModules and the "URL realm ignores
  // login memory" contract would pass vacuously.
  authStore = await import('@/stores/auth-store')
  authStore.useAuthStore.getState().reset()
})

describe('main-domain realm identity belongs to the URL', () => {
  it('keeps two realm URLs independent of the last login or logout', async () => {
    authStore.useAuthStore.getState().setAuthStatus(true, 'admin')
    expect((await routing.resolveRealmContext('/oncewise/manage/settings')).realmId).toBe(
      'oncewise'
    )
    authStore.useAuthStore.getState().setAuthStatus(false, 'oncewise')
    expect((await routing.resolveRealmContext('/admin/manage/settings')).realmId).toBe('admin')
    expect((await routing.resolveRealmContext('/oncewise/manage/settings')).realmId).toBe(
      'oncewise'
    )
    expect(resolveHost).toHaveBeenCalledTimes(1)
  })

  it.each(['/manage/settings', '/user/profile', '/subscription'])(
    'refuses to guess the realm for %s',
    async (pathname) => {
      authStore.useAuthStore.getState().setAuthStatus(true, 'oncewise')
      await expect(routing.resolveRealmContext(pathname)).rejects.toBeInstanceOf(
        routing.RealmRequiredError
      )
    }
  )

  it('uses a deterministic admin root and public legacy entry instead of shared login memory', () => {
    authStore.useAuthStore.getState().setAuthStatus(true, 'oncewise')
    expect(routing.getLegacyRealmId('/')).toBe('admin')
    expect(routing.getLegacyRealmId('/auth/login')).toBe('admin')
  })

  it.each([
    '/manage/settings',
    '/user/profile',
    '/subscription',
    '/auth/login',
    '/legal/privacy',
    '/device',
  ])('retains the realm when generating %s', (path) => {
    expect(routing.realmPath({ realmId: 'oncewise', isCustomDomain: false }, path)).toBe(
      `/oncewise${path}`
    )
  })
})

describe('a bound custom domain owns all realm entrances', () => {
  beforeEach(() => {
    resolveHost.mockResolvedValue({ data: { realmId: 'oncewise', publicConfig: {} } })
  })

  it.each(['/manage/settings', '/user/profile', '/subscription', '/auth/login', '/'])(
    'uses the Host mapping for %s even with an admin store',
    async (pathname) => {
      authStore.useAuthStore.getState().setAuthStatus(true, 'admin')
      expect(await routing.resolveRealmContext(pathname)).toMatchObject({
        realmId: 'oncewise',
        isCustomDomain: true,
      })
      expect(routing.resolvedRealmFromPath(pathname).realmId).toBe('oncewise')
      expect(
        routing.realmPath({ realmId: 'oncewise', isCustomDomain: true }, '/manage/settings')
      ).toBe('/manage/settings')
    }
  )

  it('resolves the Host even when the path carries another realm', async () => {
    expect(await routing.resolveRealmContext('/admin/manage/settings')).toMatchObject({
      realmId: 'oncewise',
      isCustomDomain: true,
    })
    expect(resolveHost).toHaveBeenCalledTimes(1)
  })

  it('shares one in-flight host lookup between concurrent guards', async () => {
    await Promise.all([
      routing.resolveRealmContext('/auth/login'),
      routing.resolveRealmContext('/manage'),
    ])
    expect(resolveHost).toHaveBeenCalledTimes(1)
  })
})

it('fails closed on a resolver outage and allows retry, rather than selecting admin', async () => {
  resolveHost.mockResolvedValueOnce({ error: {}, response: { status: 503 } })
  await expect(routing.resolveRealmContext('/oncewise/manage')).rejects.toThrow('Unable to resolve')
  resolveHost.mockResolvedValueOnce({ data: { realmId: 'oncewise' } })
  expect(await routing.resolveRealmContext('/manage')).toMatchObject({
    realmId: 'oncewise',
    isCustomDomain: true,
  })
})

it('fails closed with the sentinel error when the resolver fails without a response (network outage)', async () => {
  // The generated client resolves network-level failures (DNS / offline) as
  // `{ error, response: undefined }` — the sentinel must surface, not a
  // TypeError from dereferencing the missing response.
  resolveHost.mockResolvedValueOnce({ error: new Error('fetch failed'), response: undefined })
  await expect(routing.resolveRealmContext('/oncewise/manage')).rejects.toThrow(
    'Unable to resolve custom-domain realm'
  )
})

it('removes only a complete realm segment when preserving a return path', () => {
  const context = { realmId: 'once', isCustomDomain: false }
  expect(routing.realmRelativePath(context, '/once/manage/settings')).toBe('/manage/settings')
  expect(routing.realmRelativePath(context, '/oncewise/manage/settings')).toBe(
    '/oncewise/manage/settings'
  )
})
