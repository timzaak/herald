import { describe, it, expect } from 'vitest'
import { HeraldError, localStorageStorage, memoryStorage } from '../src'
import type { TokenStorage } from '../src'
import { disableLocalStorage, makeClient } from './helpers'

describe('storage (US-JS-007 / DEC-006)', () => {
  it('localStorage_default_round_trip', () => {
    localStorage.clear()
    const storage = localStorageStorage('herald.rt')
    expect(storage.getRefreshToken()).toBeNull()
    storage.setRefreshToken('rt-abc')
    expect(storage.getRefreshToken()).toBe('rt-abc')
    expect(localStorage.getItem('herald.rt')).toBe('rt-abc')
    storage.setRefreshToken(null)
    expect(storage.getRefreshToken()).toBeNull()
    expect(localStorage.getItem('herald.rt')).toBeNull()
  })

  it('memory_storage_not_persisted', () => {
    const a = memoryStorage()
    const b = memoryStorage()
    a.setRefreshToken('rt-1')
    // Same instance round-trips; a separate instance does NOT see it (no shared
    // persistence, unlike localStorage).
    expect(a.getRefreshToken()).toBe('rt-1')
    expect(b.getRefreshToken()).toBeNull()
    a.setRefreshToken(null)
    expect(a.getRefreshToken()).toBeNull()
  })

  it('custom_storage_used_when_injected', () => {
    let captured: string | null = null
    const custom: TokenStorage = {
      getRefreshToken: () => captured,
      setRefreshToken: (t) => {
        captured = t
      },
    }
    const { client } = makeClient({ storage: custom })
    client.storage.setRefreshToken('rt-custom')
    expect(captured).toBe('rt-custom')
    expect(client.storage.getRefreshToken()).toBe('rt-custom')
  })

  it('ssr_no_window_throws_ssr_no_storage', () => {
    const restore = disableLocalStorage()
    try {
      expect(() => localStorageStorage('herald.rt')).toThrowError(HeraldError)
      try {
        localStorageStorage('herald.rt')
        throw new Error('expected throw')
      } catch (e) {
        expect((e as HeraldError).kind).toBe('ssr-no-storage')
      }
    } finally {
      restore()
    }
  })
})
