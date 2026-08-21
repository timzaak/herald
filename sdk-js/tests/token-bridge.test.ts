/**
 * First-party token bridge (Herald's own frontend consumes the SDK):
 *   - `tokens.setTokens` injects externally obtained token sets and optionally
 *     rebinds the request-body clientId (PKCE exchange / switch-client);
 *   - `auth.refresh()` exposes the single-flight refresh core;
 *   - `login` passes OAuth context through untouched and never exchanges the
 *     code itself (DEC-js-sdk-008 — the exchange stays with the caller).
 */
import { describe, it, expect } from 'vitest'
import { http, HttpResponse } from 'msw'
import { server } from './mocks/server'
import { HeraldError } from '../src'
import { makeClient, makeTokens, urls } from './helpers'

function captureLoginBody(): { body: Record<string, unknown> } {
  const captured: { body: Record<string, unknown> } = { body: {} }
  server.use(
    http.post(urls.login, async ({ request }) => {
      captured.body = (await request.json()) as Record<string, unknown>
      return HttpResponse.json(makeTokens({ accessToken: 'at-after' }))
    }),
  )
  return captured
}

describe('tokens bridge', () => {
  it('setTokens_stores_tokens_and_rebinds_client_id', async () => {
    const captured = captureLoginBody()

    const { client } = makeClient()
    client.tokens.setTokens({ accessToken: 'at-1', refreshToken: 'rt-1', clientId: 'client-2' })

    expect(client.tokens.getAccessToken()).toBe('at-1')
    expect(client.storage.getRefreshToken()).toBe('rt-1')

    // The rebound clientId rides the next login-family request body.
    await client.login({ email: 'a@b.c', password: 'pw' })
    expect(captured.body['clientId']).toBe('client-2')
  })

  it('setTokens_without_client_id_keeps_binding', async () => {
    const captured = captureLoginBody()

    const { client } = makeClient()
    client.tokens.setTokens({ accessToken: 'at-1', refreshToken: 'rt-1' })

    await client.login({ email: 'a@b.c', password: 'pw' })
    expect(captured.body['clientId']).toBe('client-1')
  })

  it('setTokens_is_a_pure_state_update_no_events', () => {
    const { client, events } = makeClient()
    client.tokens.setTokens({ accessToken: 'at-1', refreshToken: 'rt-1' })
    expect(events).toEqual([])
  })

  it('clear_wipes_tokens_without_events', () => {
    const { client, events } = makeClient()
    client.tokens.setTokens({ accessToken: 'at-1', refreshToken: 'rt-1' })
    client.tokens.clear()
    expect(client.tokens.getAccessToken()).toBeNull()
    expect(client.storage.getRefreshToken()).toBeNull()
    expect(events).toEqual([])
  })

  it('bindClientId_rebinds_without_touching_tokens', async () => {
    const captured = captureLoginBody()

    const { client } = makeClient()
    client.tokens.setTokens({ accessToken: 'at-1', refreshToken: 'rt-1' })
    client.tokens.bindClientId('client-3')

    expect(client.tokens.getAccessToken()).toBe('at-1')
    expect(client.storage.getRefreshToken()).toBe('rt-1')

    await client.login({ email: 'a@b.c', password: 'pw' })
    expect(captured.body['clientId']).toBe('client-3')
  })
})

describe('public refresh', () => {
  it('rotates_tokens_single_flight_across_concurrent_calls', async () => {
    let refreshCalls = 0
    server.use(
      http.post(urls.refresh, () => {
        refreshCalls += 1
        return HttpResponse.json(makeTokens({ accessToken: 'at-rot', refreshToken: 'rt-rot' }))
      }),
    )

    const { client } = makeClient()
    client.tokens.setTokens({ accessToken: 'at-1', refreshToken: 'rt-1' })

    const [a, b] = await Promise.all([client.refresh(), client.refresh()])
    expect(refreshCalls).toBe(1)
    expect(a.accessToken).toBe('at-rot')
    expect(b.accessToken).toBe('at-rot')
    expect(client.tokens.getAccessToken()).toBe('at-rot')
    expect(client.storage.getRefreshToken()).toBe('rt-rot')
  })

  it('without_rt_throws_session_expired_and_emits', async () => {
    const { client, events } = makeClient()
    await expect(client.refresh()).rejects.toMatchObject({ kind: 'session-expired' })
    expect(events.some((e) => e.type === 'session-expired')).toBe(true)
  })

  it('reuse_failure_throws_herald_error_and_emits_family_revoked', async () => {
    server.use(
      http.post(
        urls.refresh,
        () =>
          HttpResponse.json(
            { status: 401, code: 'refresh_token_reuse', message: 'revoked' },
            { status: 401 },
          ),
        { once: true },
      ),
    )

    const { client, events } = makeClient()
    client.tokens.setTokens({ accessToken: 'at-1', refreshToken: 'rt-bad' })

    await expect(client.refresh()).rejects.toBeInstanceOf(HeraldError)
    expect(
      events.some((e) => e.type === 'session-expired' && e.reason === 'family-revoked'),
    ).toBe(true)
  })
})

describe('login OAuth passthrough', () => {
  it('passes_oauth_context_through_and_returns_redirect', async () => {
    let body: Record<string, unknown> = {}
    server.use(
      http.post(
        urls.login,
        async ({ request }) => {
          body = (await request.json()) as Record<string, unknown>
          return HttpResponse.json({ redirectTo: 'https://app/callback?code=c&state=s' })
        },
        { once: true },
      ),
    )

    const { client } = makeClient()
    const result = await client.login({
      email: 'a@b.c',
      password: 'pw',
      oauthClientId: 'oauth-cid',
      redirectUri: 'https://app/callback',
      state: 's',
    })

    expect(result.kind).toBe('oauth-redirect')
    expect(body['oauthClientId']).toBe('oauth-cid')
    expect(body['redirectUri']).toBe('https://app/callback')
    expect(body['state']).toBe('s')
    // The SDK never exchanges the code itself (DEC-js-sdk-008).
    expect(client.tokens.getAccessToken()).toBeNull()
  })

  it('passkey_login_begin_passes_oauth_context_through', async () => {
    let body: Record<string, unknown> = {}
    server.use(
      http.post(urls.passkeyOptions, async ({ request }) => {
        body = (await request.json()) as Record<string, unknown>
        return HttpResponse.json({ authToken: 'pk-auth', options: { challenge: 'Y2hhbGxlbmdl' } })
      }),
    )

    const { client } = makeClient()
    const begin = await client.passkey.loginBegin({
      oauth: { clientId: 'oauth-cid', redirectUri: 'https://app/callback', state: 's' },
    })

    expect(begin.authToken).toBe('pk-auth')
    expect(body['oauth']).toEqual({
      clientId: 'oauth-cid',
      redirectUri: 'https://app/callback',
      state: 's',
    })
  })
})
