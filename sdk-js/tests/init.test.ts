import { describe, it, expect } from 'vitest'
import pkg from '../package.json'
import { createHeraldClient, HeraldError, memoryStorage } from '../src'
import { disableLocalStorage, makeClient } from './helpers'

describe('init (US-JS-001)', () => {
  it('create_client_success — exposes the full auth lifecycle surface', () => {
    const { client } = makeClient()
    expect(typeof client.register).toBe('function')
    expect(typeof client.triggerVerifyEmail).toBe('function')
    expect(typeof client.requestPasswordReset).toBe('function')
    expect(typeof client.login).toBe('function')
    expect(typeof client.verifyTotp).toBe('function')
    expect(typeof client.passkey.loginBegin).toBe('function')
    expect(typeof client.passkey.loginFinish).toBe('function')
    expect(typeof client.loginWithEmailOtp.send).toBe('function')
    expect(typeof client.loginWithEmailOtp.verify).toBe('function')
    expect(typeof client.getStatus).toBe('function')
    expect(typeof client.logout).toBe('function')
    expect(typeof client.refresh).toBe('function')
    expect(typeof client.session.getSession).toBe('function')
    expect(typeof client.session.subscribe).toBe('function')
    expect(typeof client.tokens.getAccessToken).toBe('function')
    expect(typeof client.tokens.setTokens).toBe('function')
    expect(typeof client.tokens.clear).toBe('function')
    expect(typeof client.tokens.bindClientId).toBe('function')
  })

  it('framework_agnostic_no_react_dep — zero runtime dependencies', () => {
    // Zero runtime deps => framework-agnostic by construction. (package.json has
    // no `dependencies` field — only devDependencies — which is the assertion.)
    const dependencies = (pkg as { dependencies?: Record<string, unknown> }).dependencies
    expect(Object.keys(dependencies ?? {}).length).toBe(0)
  })

  it('ssr_guard — throws ssr-no-storage when localStorage is unavailable', () => {
    const restore = disableLocalStorage()
    try {
      expect(() =>
        createHeraldClient({
          baseUrl: 'http://localhost:3000',
          realmId: 'r',
          clientId: 'c',
          // no storage injected
        }),
      ).toThrowError(HeraldError)
      try {
        createHeraldClient({ baseUrl: 'http://localhost:3000', realmId: 'r', clientId: 'c' })
        throw new Error('expected throw')
      } catch (e) {
        expect((e as HeraldError).kind).toBe('ssr-no-storage')
      }
    } finally {
      restore()
    }
  })

  it('ssr_guard — an injected storage adapter bypasses the SSR guard', () => {
    const restore = disableLocalStorage()
    try {
      const client = createHeraldClient({
        baseUrl: 'http://localhost:3000',
        realmId: 'r',
        clientId: 'c',
        storage: memoryStorage(),
      })
      expect(typeof client.login).toBe('function')
    } finally {
      restore()
    }
  })
})
