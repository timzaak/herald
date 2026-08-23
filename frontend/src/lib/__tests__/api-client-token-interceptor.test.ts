/**
 * Bearer API client interceptor state machine (design §4.4 — FE-D01).
 *
 * Exercises `initBearerClient()` end-to-end against MSW, using the REAL auth
 * store + Herald SDK client (NOT mocked) so the interceptor's reads
 * (`tokens.getAccessToken`) and the refresh delegation (`herald.refresh()`) go
 * through the same surfaces production uses. Network calls go through the
 * generated client (`status`) and the SDK transport (`refresh`) — no internal
 * API functions are mocked — and MSW controls every response, including
 * scripted 401 → 200 sequences for the loop guard.
 *
 * Coverage:
 * - Bearer `Authorization` injection from the SDK's in-memory token holder
 * - single 401 → refresh (SDK single-flight) → replay exactly once (new AT/RT swapped in)
 * - refresh-loop guard (retried request re-401s → no second refresh, 401 surfaces)
 * - refresh failure (401 reuse/absolute-expiry) → logout path, RT cleared, no retry
 * - refresh endpoint itself is never Bearer-injected and never auto-refreshed
 * - 401 racing a first-party client switch (root loader): recovery waits for
 *   the rotated family and replays with it — never refreshes with the
 *   superseded refresh token (which would revoke the family and log out)
 * - 403 racing the same switch (admin-console endpoints reject the superseded
 *   token with 403, not 401): replayed with the rotated token ONLY when the
 *   token rotated mid-flight; a genuine permission 403 surfaces untouched
 */

import { describe, it, expect, beforeEach, vi } from 'vitest'
import { http, HttpResponse } from 'msw'
import { server } from '@/test/mocks/server'
import { status, listRealmConfigs } from '@/lib/api-generated'
import { initBearerClient } from '@/lib/api-client'
import { switchFirstPartyClient } from '@/lib/auth-service'
import {
  ensureHeraldClient,
  getActiveHeraldClient,
  applyTokenSet,
  runTokenSwitch,
  HERALD_REFRESH_TOKEN_STORAGE_KEY,
} from '@/lib/herald-client'
import { useAuthStore } from '@/stores/auth-store'
import { ADMIN_WEB_CONSOLE_CLIENT_ID, AUTH_STORAGE_KEY } from '@/lib/constants/auth-constants'
import {
  REFRESH_URL,
  createRefreshSuccessHandler,
  REFRESH_ERRORS,
  type CapturedRefreshRequest,
} from '@/test/mocks/handlers/browser-token'
import { TOKEN_FIXTURE } from '@/test/fixtures/browser-token'

const API_BASE_URL = 'http://localhost:3000'
const STATUS_URL = `${API_BASE_URL}/api/auth/status`
const SWITCH_CLIENT_URL = `${API_BASE_URL}/api/auth/browser-token/switch-client`
/**
 * The admin-console configs endpoint (`listRealmConfigs` with realmId 'admin')
 * — the production surface where a superseded token draws a 403 instead of a
 * 401 ("Access denied: admin console credential required").
 */
const CONFIGS_ADMIN_URL = `${API_BASE_URL}/api/configs/admin`
/** Arbitrary realm for the SDK client in this file (refresh/status are realm-agnostic). */
const TEST_REALM = 'realm-1'

/** A representative authenticated /status 200 body. */
const STATUS_OK_BODY = { authenticated: true, realmId: TEST_REALM, userId: 'user-1' }

/** Install the Bearer interceptor once for the whole file (idempotent). */
initBearerClient()

/**
 * Reset ALL auth surfaces between tests: the SDK's token holder + storage, the
 * real Zustand store state, and any persisted localStorage. Without this the
 * single-flight / loop-guard module state could leak between cases.
 */
function resetAuthSurfaces() {
  getActiveHeraldClient()?.tokens.clear()
  useAuthStore.getState().reset()
  window.localStorage.removeItem(AUTH_STORAGE_KEY)
  window.localStorage.removeItem(HERALD_REFRESH_TOKEN_STORAGE_KEY)
}

/** Seed an authenticated session: AT + RT in the Herald SDK client. */
function seedSession(accessToken: string = TOKEN_FIXTURE.accessToken) {
  ensureHeraldClient(TEST_REALM)
  applyTokenSet({
    accessToken,
    refreshToken: TOKEN_FIXTURE.refreshToken,
    clientId: TOKEN_FIXTURE.clientId,
  })
}

beforeEach(() => {
  resetAuthSurfaces()
})

describe('Bearer request interceptor: Authorization header injection', () => {
  it('injects Authorization: Bearer {accessToken} from the SDK token holder onto protected requests', async () => {
    seedSession()
    let capturedAuth: string | null = null
    server.use(
      http.get(STATUS_URL, ({ request }) => {
        capturedAuth = request.headers.get('Authorization')
        return HttpResponse.json(STATUS_OK_BODY)
      })
    )

    await status()

    expect(capturedAuth).toBe(`Bearer ${TOKEN_FIXTURE.accessToken}`)
  })

  it('does NOT inject Authorization when no access token is held (e.g. pre-login)', async () => {
    // No seed → SDK holder empty.
    let capturedAuth: string | null = '__sentinel__'
    server.use(
      http.get(STATUS_URL, ({ request }) => {
        capturedAuth = request.headers.get('Authorization')
        return HttpResponse.json(STATUS_OK_BODY)
      })
    )

    await status()

    expect(capturedAuth).toBeNull()
  })
})

describe('single 401 → refresh → retry exactly once', () => {
  it('on a 401, refreshes once, swaps AT/RT, and replays the original request with the new AT', async () => {
    seedSession(TOKEN_FIXTURE.expiredAccessToken)

    // Protected endpoint: 401 once (expired AT), then 200 on the replay.
    let statusCallCount = 0
    let replayAuth: string | null = null
    server.use(
      http.get(STATUS_URL, ({ request }) => {
        statusCallCount += 1
        if (statusCallCount === 1) {
          return HttpResponse.json({ error: 'token_expired' }, { status: 401 })
        }
        replayAuth = request.headers.get('Authorization')
        return HttpResponse.json(STATUS_OK_BODY)
      })
    )

    // Refresh returns a rotated token set.
    const refreshCapture: CapturedRefreshRequest = { body: undefined, authorization: undefined }
    server.use(createRefreshSuccessHandler(refreshCapture))

    const result = await status()

    // The original request was issued twice (initial 401 + one replay).
    expect(statusCallCount).toBe(2)
    // The replay carried the NEW (rotated) access token.
    expect(replayAuth).toBe(`Bearer ${TOKEN_FIXTURE.rotatedAccessToken}`)
    // The caller received the business data, not an error.
    expect(result.data).toMatchObject({ authenticated: true })

    // Refresh was called with the persisted RT only (the server recovers the
    // bound Client App from the token family; the body carries no clientId).
    expect(refreshCapture.body).toEqual({
      refreshToken: TOKEN_FIXTURE.refreshToken,
    })
    // The refresh endpoint is skipped by the Bearer injector (RT in body, not header).
    expect(refreshCapture.authorization).toBeNull()

    // New AT/RT were swapped into the SDK holder / storage.
    expect(getActiveHeraldClient()?.tokens.getAccessToken()).toBe(TOKEN_FIXTURE.rotatedAccessToken)
    expect(getActiveHeraldClient()?.storage.getRefreshToken()).toBe(
      TOKEN_FIXTURE.rotatedRefreshToken
    )
  })
})

describe('refresh-loop guard: a retried request that 401s again is not refreshed twice', () => {
  it('surfaces the second 401 instead of refreshing a second time', async () => {
    seedSession(TOKEN_FIXTURE.expiredAccessToken)

    // Protected endpoint always 401s (even after the replay) → loop guard must
    // kick in: only ONE refresh, then the second 401 propagates.
    let statusCallCount = 0
    server.use(
      http.get(STATUS_URL, () => {
        statusCallCount += 1
        return HttpResponse.json({ error: 'invalid_token' }, { status: 401 })
      })
    )

    let refreshCallCount = 0
    server.use(
      http.post(REFRESH_URL, () => {
        refreshCallCount += 1
        return HttpResponse.json({
          access_token: TOKEN_FIXTURE.rotatedAccessToken,
          refresh_token: TOKEN_FIXTURE.rotatedRefreshToken,
          token_type: 'Bearer',
          expires_in: 900,
          refresh_expires_in: 2592000,
        })
      })
    )

    const result = await status()

    // The request fired twice (initial + one replay), refresh fired ONCE.
    expect(statusCallCount).toBe(2)
    expect(refreshCallCount).toBe(1)
    // No business data — the 401 surfaced to the caller as an error envelope.
    expect(result.error).toBeDefined()
    expect(result.response?.status).toBe(401)
  })
})

describe('refresh failure → force re-login (no infinite retry)', () => {
  it.each([
    {
      label: 'refresh reuse detected (family revoked)',
      error: REFRESH_ERRORS.reuseDetected,
    },
    {
      label: 'refresh absolute expiry reached',
      error: REFRESH_ERRORS.absoluteExpiry,
    },
    {
      label: 'refresh token invalid/revoked',
      error: REFRESH_ERRORS.invalid,
    },
  ])(
    '$label: refresh 401 clears RT + logs out and surfaces 401 (no retry loop)',
    async ({ error }) => {
      seedSession(TOKEN_FIXTURE.expiredAccessToken)

      // Protected endpoint 401s once.
      let statusCallCount = 0
      server.use(
        http.get(STATUS_URL, () => {
          statusCallCount += 1
          return HttpResponse.json({ error: 'token_expired' }, { status: 401 })
        })
      )
      // Refresh also fails with a distinguishable 401. The handler counts
      // invocations so the test can assert there is exactly ONE refresh attempt
      // (no retry loop) when the refresh endpoint itself rejects.
      let refreshCallCount = 0
      server.use(
        http.post(REFRESH_URL, () => {
          refreshCallCount += 1
          return HttpResponse.json({ error, error_description: error }, { status: 401 })
        })
      )

      const result = await status()

      // The original request fired once; it was NOT replayed (refresh failed).
      expect(statusCallCount).toBe(1)
      // Refresh attempted exactly once — no retry loop.
      expect(refreshCallCount).toBe(1)
      // The 401 surfaced to the caller.
      expect(result.error).toBeDefined()
      expect(result.response?.status).toBe(401)

      // Force-re-login path: the SDK session-expired bridge cleared the token
      // family (in-memory AT + persisted RT) and logged the store out.
      expect(getActiveHeraldClient()?.storage.getRefreshToken()).toBeNull()
      expect(getActiveHeraldClient()?.tokens.getAccessToken()).toBeNull()
      expect(useAuthStore.getState().refreshClientId).toBeNull()
      expect(useAuthStore.getState().isAuthenticated).toBe(false)
    }
  )
})

describe('refresh endpoint isolation', () => {
  it('the refresh call itself is never auto-refreshed on its own 401', async () => {
    seedSession(TOKEN_FIXTURE.expiredAccessToken)

    // Protected endpoint 401s once → triggers refresh.
    let statusCallCount = 0
    server.use(
      http.get(STATUS_URL, () => {
        statusCallCount += 1
        if (statusCallCount === 1) {
          return HttpResponse.json({ error: 'token_expired' }, { status: 401 })
        }
        return HttpResponse.json(STATUS_OK_BODY)
      })
    )

    // Refresh returns 401. Because the response interceptor skips the refresh
    // path entirely, this must NOT recurse into another refresh call.
    let refreshCallCount = 0
    server.use(
      http.post(REFRESH_URL, () => {
        refreshCallCount += 1
        return HttpResponse.json(
          { error: REFRESH_ERRORS.invalid, error_description: REFRESH_ERRORS.invalid },
          { status: 401 }
        )
      })
    )

    await status()

    // Exactly one refresh attempt — the refresh 401 did not trigger recursion.
    expect(refreshCallCount).toBe(1)
  })
})

describe('401 racing a first-party client switch (root-loader token rotation)', () => {
  /**
   * Reproduces the /manage entry race: the root loader switches the token
   * family (switch-client + applyTokenSet) while layout-level queries are
   * still in flight with the OLD access token. Their 401 recovery must wait
   * for the switch and replay with the NEW token — refreshing with the
   * superseded refresh token gets a family-revocation 401 and logs the just-
   * rotated session out (the historical "bounced back to login" flake).
   */
  it('waits out the switch, replays with the rotated access token, and never refreshes or logs out', async () => {
    // Pre-switch session (e.g. the user-account-center family after login).
    seedSession(TOKEN_FIXTURE.accessToken)

    // Hold the switch-client response so the query's 401 recovery runs INSIDE
    // the rotation window (local storage still holds the old refresh token).
    let releaseSwitch!: (body: Record<string, unknown>) => void
    const switchHold = new Promise<Record<string, unknown>>((resolve) => {
      releaseSwitch = resolve
    })
    server.use(
      http.post(SWITCH_CLIENT_URL, () => switchHold.then((body) => HttpResponse.json(body)))
    )

    // Any refresh attempt during the window is the bug: the old refresh token
    // is superseded the moment the backend rotates the family.
    let refreshCallCount = 0
    server.use(
      http.post(REFRESH_URL, () => {
        refreshCallCount += 1
        return HttpResponse.json(
          { error: REFRESH_ERRORS.reuseDetected, error_description: REFRESH_ERRORS.reuseDetected },
          { status: 401 }
        )
      })
    )

    // The protected endpoint answers 401 for the pre-switch access token.
    let statusCallCount = 0
    let replayAuth: string | null = null
    server.use(
      http.get(STATUS_URL, ({ request }) => {
        statusCallCount += 1
        if (statusCallCount === 1) {
          return HttpResponse.json({ error: 'invalid_token' }, { status: 401 })
        }
        replayAuth = request.headers.get('Authorization')
        return HttpResponse.json(STATUS_OK_BODY)
      })
    )

    // Mirror the root loader: the switch HTTP call + applyTokenSet run as one
    // critical section (`initializeAuth` wraps them in `runTokenSwitch`).
    const switchDone = runTokenSwitch(async () => {
      const tokenSet = await switchFirstPartyClient(ADMIN_WEB_CONSOLE_CLIENT_ID)
      applyTokenSet({
        accessToken: tokenSet.accessToken,
        refreshToken: tokenSet.refreshToken,
        clientId: tokenSet.clientId,
      })
    })

    const queryDone = status()

    // The 401 arrived (and its recovery parked on the switch gate) before the
    // rotation is released.
    await vi.waitFor(() => expect(statusCallCount).toBe(1))
    await new Promise((resolve) => setTimeout(resolve, 0))

    releaseSwitch({
      accessToken: TOKEN_FIXTURE.rotatedAccessToken,
      refreshToken: TOKEN_FIXTURE.rotatedRefreshToken,
      tokenType: 'Bearer',
      expiresIn: 900,
      refreshExpiresIn: 2592000,
      clientId: ADMIN_WEB_CONSOLE_CLIENT_ID,
    })

    const result = await queryDone
    await switchDone

    // The query recovered on the switched family: one 401 + one replay, and
    // the replay carried the NEW access token (not the superseded one).
    expect(statusCallCount).toBe(2)
    expect(replayAuth).toBe(`Bearer ${TOKEN_FIXTURE.rotatedAccessToken}`)
    expect(result.data).toMatchObject({ authenticated: true })

    // No refresh was ever attempted → no family revocation, no logout.
    expect(refreshCallCount).toBe(0)
    // The freshly-switched family survived intact (a session-expired teardown
    // would have cleared both tokens and the bound client).
    expect(getActiveHeraldClient()?.tokens.getAccessToken()).toBe(TOKEN_FIXTURE.rotatedAccessToken)
    expect(getActiveHeraldClient()?.storage.getRefreshToken()).toBe(
      TOKEN_FIXTURE.rotatedRefreshToken
    )
    expect(useAuthStore.getState().refreshClientId).toBe(ADMIN_WEB_CONSOLE_CLIENT_ID)
  })
})

describe('403 racing a first-party client switch', () => {
  /**
   * Same rotation window as the 401 case above, but on admin-console-scoped
   * endpoints (`GET /api/configs/admin`), which answer a superseded token with
   * 403 "admin console credential required" instead of 401. Two 403 kinds must
   * stay distinguishable: the race (token rotated mid-flight → recover by
   * replaying with the rotated token) and a genuine permission denial (token
   * still current → surface untouched; swallowing it would hide real authz
   * errors, refreshing on it would rotate the family for nothing).
   */
  it('waits out the switch and replays the 403-ed request with the rotated access token', async () => {
    // Pre-switch session (e.g. the user-account-center family right after login).
    seedSession(TOKEN_FIXTURE.accessToken)

    // Hold the switch-client response so the query's 403 recovery runs INSIDE
    // the rotation window.
    let releaseSwitch!: (body: Record<string, unknown>) => void
    const switchHold = new Promise<Record<string, unknown>>((resolve) => {
      releaseSwitch = resolve
    })
    server.use(
      http.post(SWITCH_CLIENT_URL, () => switchHold.then((body) => HttpResponse.json(body)))
    )

    // A 403 must recover via replay only — never via refresh.
    let refreshCallCount = 0
    server.use(
      http.post(REFRESH_URL, () => {
        refreshCallCount += 1
        return HttpResponse.json(
          { error: REFRESH_ERRORS.reuseDetected, error_description: REFRESH_ERRORS.reuseDetected },
          { status: 401 }
        )
      })
    )

    // The admin-console endpoint answers the pre-switch token with the
    // production 403 body, then accepts the replay.
    let configsCallCount = 0
    let replayAuth: string | null = null
    server.use(
      http.get(CONFIGS_ADMIN_URL, ({ request }) => {
        configsCallCount += 1
        if (configsCallCount === 1) {
          return HttpResponse.json(
            { code: 'forbidden', message: 'Access denied: admin console credential required' },
            { status: 403 }
          )
        }
        replayAuth = request.headers.get('Authorization')
        return HttpResponse.json([])
      })
    )

    // Mirror the root loader: switch HTTP call + applyTokenSet as one critical
    // section, racing the configs query issued with the OLD access token.
    const switchDone = runTokenSwitch(async () => {
      const tokenSet = await switchFirstPartyClient(ADMIN_WEB_CONSOLE_CLIENT_ID)
      applyTokenSet({
        accessToken: tokenSet.accessToken,
        refreshToken: tokenSet.refreshToken,
        clientId: tokenSet.clientId,
      })
    })

    const queryDone = listRealmConfigs({ path: { realmId: 'admin' } })

    // The 403 arrived (and its recovery parked on the switch gate) before the
    // rotation is released.
    await vi.waitFor(() => expect(configsCallCount).toBe(1))
    await new Promise((resolve) => setTimeout(resolve, 0))

    releaseSwitch({
      accessToken: TOKEN_FIXTURE.rotatedAccessToken,
      refreshToken: TOKEN_FIXTURE.rotatedRefreshToken,
      tokenType: 'Bearer',
      expiresIn: 900,
      refreshExpiresIn: 2592000,
      clientId: ADMIN_WEB_CONSOLE_CLIENT_ID,
    })

    const result = await queryDone
    await switchDone

    // The query recovered on the switched family: one 403 + one replay, and
    // the replay carried the NEW access token (not the superseded one).
    expect(configsCallCount).toBe(2)
    expect(replayAuth).toBe(`Bearer ${TOKEN_FIXTURE.rotatedAccessToken}`)
    expect(result.data).toEqual([])
    // No refresh, no logout — the switched family survived intact.
    expect(refreshCallCount).toBe(0)
    expect(getActiveHeraldClient()?.tokens.getAccessToken()).toBe(TOKEN_FIXTURE.rotatedAccessToken)
    expect(useAuthStore.getState().refreshClientId).toBe(ADMIN_WEB_CONSOLE_CLIENT_ID)
  })

  it('a genuine permission 403 (token still current) is not replayed and surfaces untouched', async () => {
    seedSession(TOKEN_FIXTURE.accessToken)

    // The endpoint denies this token's permissions outright — no switch in
    // flight, the token is the current one.
    let configsCallCount = 0
    server.use(
      http.get(CONFIGS_ADMIN_URL, () => {
        configsCallCount += 1
        return HttpResponse.json(
          { code: 'forbidden', message: 'Access denied: admin console credential required' },
          { status: 403 }
        )
      })
    )

    // A permission denial must not trigger a refresh (family rotation) any
    // more than a replay.
    let refreshCallCount = 0
    server.use(
      http.post(REFRESH_URL, () => {
        refreshCallCount += 1
        return HttpResponse.json({}, { status: 401 })
      })
    )

    const result = await listRealmConfigs({ path: { realmId: 'admin' } })

    // Fired exactly once, no replay; the 403 (not a masked 401 or empty data)
    // reached the caller.
    expect(configsCallCount).toBe(1)
    expect(result.error).toBeDefined()
    expect(result.response?.status).toBe(403)
    // No session side effects: tokens untouched, no refresh attempted.
    expect(refreshCallCount).toBe(0)
    expect(getActiveHeraldClient()?.tokens.getAccessToken()).toBe(TOKEN_FIXTURE.accessToken)
    expect(getActiveHeraldClient()?.storage.getRefreshToken()).toBe(TOKEN_FIXTURE.refreshToken)
  })
})
