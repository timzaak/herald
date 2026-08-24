import { describe, it, expect } from 'vitest'
import { http, HttpResponse } from 'msw'
import { server } from './mocks/server'
import { makeClient, makeStatus, makeTokens, urls } from './helpers'

const RETRY = 'X-Herald-Refresh-Retried'

/** Status handler: 401 on the initial call, 200 (authenticated) on the replay
 *  (recognized via the loop-guard header). */
function status401Then200() {
  return http.get(urls.status, ({ request }) =>
    request.headers.get(RETRY)
      ? HttpResponse.json(makeStatus())
      : new HttpResponse(null, { status: 401 }),
  )
}

describe('transport (US-JS-005 / DEC-007)', () => {
  it('single_flight_concurrent_401_shares_one_refresh', async () => {
    let refreshCalls = 0
    server.use(
      status401Then200(),
      http.post(urls.refresh, async () => {
        refreshCalls += 1
        // Delay so the concurrent 401s all arrive while the first refresh is in flight.
        await new Promise((r) => setTimeout(r, 20))
        return HttpResponse.json(makeTokens({ refreshToken: 'rt-new' }))
      }),
    )

    const { client } = makeClient()
    client.storage.setRefreshToken('rt-old')

    const results = await Promise.all([client.getStatus(), client.getStatus(), client.getStatus()])
    expect(results.every((r) => r.authenticated)).toBe(true)
    expect(refreshCalls).toBe(1)
    expect(client.storage.getRefreshToken()).toBe('rt-new')
  })

  it('rotates_both_tokens_and_replays_once', async () => {
    let statusCalls = 0
    server.use(
      http.get(urls.status, ({ request }) => {
        statusCalls += 1
        return request.headers.get(RETRY)
          ? HttpResponse.json(makeStatus())
          : new HttpResponse(null, { status: 401 })
      }),
      http.post(urls.refresh, () =>
        HttpResponse.json(makeTokens({ accessToken: 'at-new', refreshToken: 'rt-new' })),
      ),
    )

    const { client } = makeClient()
    client.storage.setRefreshToken('rt-old')

    const data = await client.getStatus()
    expect(data.authenticated).toBe(true)
    expect(client.storage.getRefreshToken()).toBe('rt-new') // RT rotated
    expect(statusCalls).toBe(2) // initial 401 + single replay 200
  })

  it('replay_401_loop_guard_terminates', async () => {
    let refreshCalls = 0
    server.use(
      // Always 401 — even the replayed (retried) request must not loop.
      http.get(urls.status, () => new HttpResponse(null, { status: 401 })),
      http.post(urls.refresh, () => {
        refreshCalls += 1
        return HttpResponse.json(makeTokens({ refreshToken: 'rt-new' }))
      }),
    )

    const { client } = makeClient()
    client.storage.setRefreshToken('rt-old')

    await expect(client.getStatus()).rejects.toMatchObject({ kind: 'unauthorized' })
    expect(refreshCalls).toBe(1) // refreshed once, then the loop guard terminated
  })

  it('refresh_reuse_401_clears_session_emits_expired', async () => {
    server.use(
      http.get(urls.status, () => new HttpResponse(null, { status: 401 })),
      // Refresh itself 401s (reuse detected → family revoked server-side).
      http.post(urls.refresh, () => new HttpResponse(null, { status: 401 })),
    )

    const { client, events } = makeClient()
    client.storage.setRefreshToken('rt-old')

    await expect(client.getStatus()).rejects.toMatchObject({ kind: 'unauthorized' })
    expect(events.some((e) => e.type === 'session-expired')).toBe(true)
    expect(client.session.getSession().authenticated).toBe(false)
    // Refresh failed, so the stored RT was NOT rotated.
    expect(client.storage.getRefreshToken()).toBe('rt-old')
  })

  it('refresh_endpoint_not_bearer_injected', async () => {
    let refreshHeaders: Headers | null = null
    server.use(
      http.post(urls.login, () => HttpResponse.json(makeTokens({ accessToken: 'at-present' }))),
      status401Then200(),
      http.post(urls.refresh, ({ request }) => {
        refreshHeaders = request.headers
        return HttpResponse.json(makeTokens({ accessToken: 'at-2', refreshToken: 'rt-2' }))
      }),
    )

    const { client } = makeClient()
    await client.login({ email: 'a@b.c', password: 'pw' }) // puts 'at-present' in the holder

    await client.getStatus() // 401 -> refresh (must skip Bearer) -> replay 200

    expect(refreshHeaders).not.toBeNull()
    // The refresh request carries the refresh token in the body, never a Bearer AT.
    expect(refreshHeaders!.get('authorization')).toBeNull()
  })

  it('no_refresh_token_emits_session_expired', async () => {
    let refreshCalls = 0
    server.use(
      http.get(urls.status, () => new HttpResponse(null, { status: 401 })),
      http.post(urls.refresh, () => {
        refreshCalls += 1
        return HttpResponse.json(makeTokens())
      }),
    )

    const { client, events } = makeClient()
    // No refresh token stored.
    await expect(client.getStatus()).rejects.toMatchObject({ kind: 'unauthorized' })
    expect(events.some((e) => e.type === 'session-expired')).toBe(true)
    expect(refreshCalls).toBe(0) // not attempted without an RT
  })
})
