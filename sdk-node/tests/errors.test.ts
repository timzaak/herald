/**
 * Error mapping, mirroring the Rust `Error` variants. WHY: integrators branch
 * on `error.code` (e.g. surface "upgrade required" on 403) — a mis-mapped
 * status silently changes application behaviour.
 */

import { describe, expect, it } from 'vitest'
import { http, HttpResponse } from 'msw'
import { HeraldClient, HeraldSdkError } from '../src'
import { server } from './mocks/server'
import { API_KEY, BASE_URL } from './helpers'

const client = () => new HeraldClient(BASE_URL, API_KEY)

describe('error mapping', () => {
  it.each([
    [401, 'unauthorized'],
    [403, 'forbidden'],
    [404, 'not-found'],
    [500, 'internal-server-error'],
    [409, 'api-error'],
    [422, 'api-error'],
  ])('maps HTTP %i to code %s', async (status, code) => {
    server.use(
      http.get(`${BASE_URL}/api/ext/realms`, () => new HttpResponse('boom', { status })),
    )

    const error = await client().listRealms().catch((e: unknown) => e)

    expect(error).toBeInstanceOf(HeraldSdkError)
    expect((error as HeraldSdkError).code).toBe(code)
    expect((error as HeraldSdkError).status).toBe(status)
    expect((error as HeraldSdkError).body).toBe('boom')
  })

  it('maps a transport failure to code network (no status)', async () => {
    server.use(
      http.get(`${BASE_URL}/api/ext/realms`, () => HttpResponse.error()),
    )

    const error = await client().listRealms().catch((e: unknown) => e)

    expect(error).toBeInstanceOf(HeraldSdkError)
    expect((error as HeraldSdkError).code).toBe('network')
    expect((error as HeraldSdkError).status).toBeUndefined()
  })

  it('maps a 2xx non-JSON body to code parse', async () => {
    server.use(
      http.get(`${BASE_URL}/api/ext/realms`, () => new HttpResponse('not json', { status: 200 })),
    )

    const error = await client().listRealms().catch((e: unknown) => e)

    expect((error as HeraldSdkError).code).toBe('parse')
    expect((error as HeraldSdkError).status).toBe(200)
  })
})
