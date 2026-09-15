/**
 * Permission-check caching behaviour, ported from the Rust crate's tests
 * (`test_caching`, `test_invalidate_cache`) plus the 300s token-staleness
 * heuristic. WHY this matters: third-party servers call `checkPermission` on
 * every request, so the cache — and its invalidation guarantees — is the
 * SDK's core value; silently serving a stale `allowed: true` after a
 * permission revocation would be a security hole.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { HeraldClient } from '../src'
import { server } from './mocks/server'
import { API_KEY, BASE_URL, permissionHandler } from './helpers'

beforeEach(() => {
  vi.useFakeTimers()
})
afterEach(() => {
  vi.useRealTimers()
})

const makeClient = (cacheTtlSeconds?: number) => new HeraldClient(BASE_URL, API_KEY, cacheTtlSeconds)

describe('checkPermission cache', () => {
  it('sends the X-API-Key header and returns the parsed response', async () => {
    const { handler, requests } = permissionHandler('user-1')
    server.use(handler)

    const result = await makeClient().checkPermission({
      accessToken: 'tok',
      rules: [{ resource: 'doc', action: 'read' }],
    })

    expect(result).toEqual({ allowed: true, userId: 'user-1' })
    expect(requests).toHaveLength(1)
    expect(requests[0]?.headers.get('X-API-Key')).toBe(API_KEY)
  })

  it('serves a repeated identical request from cache (one HTTP call)', async () => {
    const { handler, requests } = permissionHandler('user-1')
    server.use(handler)
    const client = makeClient()

    const first = await client.checkPermission({
      accessToken: 'tok',
      rules: [{ resource: 'doc', action: 'read' }],
    })
    const second = await client.checkPermission({
      accessToken: 'tok',
      rules: [{ resource: 'doc', action: 'read' }],
    })

    expect(second).toEqual(first)
    expect(requests).toHaveLength(1)
  })

  it('distinguishes requests by rules ([] vs populated vs order)', async () => {
    const { handler, requests } = permissionHandler('user-1')
    server.use(handler)
    const client = makeClient()

    await client.checkPermission({ accessToken: 'tok', rules: [] })
    await client.checkPermission({
      accessToken: 'tok',
      rules: [{ resource: 'doc', action: 'read' }],
    })
    await client.checkPermission({
      accessToken: 'tok',
      rules: [{ resource: 'doc', action: 'read' }],
    })
    await client.checkPermission({
      accessToken: 'tok',
      rules: [{ resource: 'doc', action: 'read' }, { resource: 'doc', action: 'write' }],
    })
    await client.checkPermission({
      accessToken: 'tok',
      rules: [{ resource: 'doc', action: 'read' }, { resource: 'doc', action: 'write' }],
    })

    // 3 distinct requests + 2 cache hits = 3 HTTP calls.
    expect(requests).toHaveLength(3)
  })

  it('refetches after the cache TTL expires', async () => {
    const { handler, requests } = permissionHandler('user-1')
    server.use(handler)
    const client = makeClient(1 /* second */)

    await client.checkPermission({ accessToken: 'tok', rules: [] })
    vi.setSystemTime(vi.getMockedSystemTime()!.getTime() + 1500)
    await client.checkPermission({ accessToken: 'tok', rules: [] })

    expect(requests).toHaveLength(2)
  })

  it('invalidates all cached checks for a token, leaving other tokens cached', async () => {
    const { handler, requests } = permissionHandler('user-1')
    server.use(handler)
    const client = makeClient()

    const req1 = { accessToken: 'token1', rules: [] }
    const req2 = { accessToken: 'token2', rules: [] }
    await client.checkPermission(req1)
    await client.checkPermission(req2)

    client.invalidateCache('token1')

    await client.checkPermission(req1) // invalidated → refetch
    await client.checkPermission(req2) // still cached

    expect(requests).toHaveLength(3)
  })

  it('invalidates a token whose last check is older than 5 minutes (staleness heuristic)', async () => {
    const { handler, requests } = permissionHandler('user-1')
    server.use(handler)
    const client = makeClient(600) // TTL longer than the 5-minute heuristic

    await client.checkPermission({ accessToken: 'tok', rules: [] })
    vi.setSystemTime(vi.getMockedSystemTime()!.getTime() + 301_000) // token now "expired" per heuristic
    await client.checkPermission({ accessToken: 'tok', rules: [] })

    expect(requests).toHaveLength(2)
  })
})
