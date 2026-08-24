import { describe, it, expect } from 'vitest'
import { http, HttpResponse } from 'msw'
import { server } from './mocks/server'
import { HeraldError } from '../src'
import { makeClient, urls } from './helpers'

describe('errors (US-JS-008)', () => {
  it('origin_not_allowed_distinguishable_from_network', async () => {
    // A fetch-level failure (network or CORS rejection) maps to `network`. The
    // browser cannot distinguish a CORS rejection, so the message guides toward
    // origin pre-registration — distinct from HTTP status errors.
    server.use(http.post(urls.login, () => HttpResponse.error()))

    const { client } = makeClient()
    const promise = client.login({ email: 'a@b.c', password: 'pw' })
    await expect(promise).rejects.toThrowError(HeraldError)
    try {
      await client.login({ email: 'a@b.c', password: 'pw' })
    } catch (e) {
      const err = e as HeraldError
      expect(err.kind).toBe('network')
      expect(err.message.toLowerCase()).toContain('origin')
    }
  })

  it('kind_stable_branchable', async () => {
    const cases: Array<[number, HeraldError['kind']]> = [
      [400, 'validation'],
      [401, 'unauthorized'],
      [403, 'forbidden'],
      [404, 'not-found'],
      [429, 'rate-limited'],
      [500, 'api'],
    ]
    for (const [status, expectedKind] of cases) {
      server.use(
        http.post(urls.login, () =>
          HttpResponse.json({ status, code: 'x', message: `http ${status}` }, { status }),
        ),
      )
      const { client } = makeClient()
      try {
        await client.login({ email: 'a@b.c', password: 'pw' })
        throw new Error(`expected ${status} to reject`)
      } catch (e) {
        expect((e as HeraldError).kind).toBe(expectedKind)
        expect((e as HeraldError).status).toBe(status)
      }
    }
  })

  it('api_error_carries_code_requestId', async () => {
    server.use(
      http.post(
        urls.login,
        () =>
          HttpResponse.json(
            {
              status: 500,
              code: 'internal_error',
              message: 'boom',
              requestId: 'req-123',
              details: { foo: 'bar' },
            },
            { status: 500 },
          ),
        { once: true },
      ),
    )

    const { client } = makeClient()
    try {
      await client.login({ email: 'a@b.c', password: 'pw' })
      throw new Error('expected reject')
    } catch (e) {
      const err = e as HeraldError
      expect(err.kind).toBe('api')
      expect(err.code).toBe('internal_error')
      expect(err.requestId).toBe('req-123')
      expect(err.details).toEqual({ foo: 'bar' })
    }
  })
})
