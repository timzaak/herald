/**
 * Error mapping, mirroring the Rust `Error` variants. WHY: integrators branch
 * on `error.code` (e.g. surface "upgrade required" on 403) — a mis-mapped
 * status silently changes application behaviour.
 */

import { describe, expect, it } from 'vitest'
import { delay, http, HttpResponse } from 'msw'
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

  it('aborts slow requests when withHttpTimeout is configured', async () => {
    server.use(
      http.get(`${BASE_URL}/api/ext/realms`, async () => {
        await delay(500)
        return HttpResponse.json({ realms: [] })
      }),
    )

    const error = await client()
      .withHttpTimeout(40)
      .listRealms()
      .catch((e: unknown) => e)

    expect(error).toBeInstanceOf(HeraldSdkError)
    expect((error as HeraldSdkError).code).toBe('network')
    expect((error as HeraldSdkError).status).toBeUndefined()
  })

  it('maps body-phase transport failures to code network', async () => {
    // A response whose body dies mid-read (connection reset, or the
    // AbortSignal.timeout firing after headers arrived) rejects inside
    // response.text() — outside sendRequest's catch. It must surface as the
    // documented 'network' HeraldSdkError (the Rust reqwest timeout spans
    // the whole request), not as a bare DOMException/TypeError.
    server.use(
      http.get(`${BASE_URL}/api/ext/realms`, () => {
        const stream = new ReadableStream<Uint8Array>({
          start(controller) {
            controller.enqueue(new TextEncoder().encode('{"realms":'))
            controller.error(new TypeError('terminated'))
          },
        })
        return new HttpResponse(stream, { headers: { 'Content-Type': 'application/json' } })
      }),
    )

    const error = await client().listRealms().catch((e: unknown) => e)

    expect(error).toBeInstanceOf(HeraldSdkError)
    expect((error as HeraldSdkError).code).toBe('network')
    expect((error as HeraldSdkError).status).toBeUndefined()
  })

  it('rejects non-positive or non-finite timeout values', () => {
    expect(() => client().withHttpTimeout(0)).toThrow(RangeError)
    expect(() => client().withHttpTimeout(-10)).toThrow(RangeError)
    expect(() => client().withHttpTimeout(Number.NaN)).toThrow(RangeError)
    expect(() => client().withHttpTimeout(Number.POSITIVE_INFINITY)).toThrow(RangeError)
    // AbortSignal.timeout's 2^31-1 ms ceiling must fail at configuration
    // time, not surface later as a misclassified 'network' error per request.
    expect(() => client().withHttpTimeout(2 ** 31)).toThrow(RangeError)
  })
})
