/**
 * Outbound webhook signature verification & typed event parsing tests.
 *
 * WHY: integrators gate paywall unlocks (`role.granted`) and account takedowns
 * (`user.disabled`) on this verifier. Any drift from Herald's Rust
 * `sign_delivery` contract (`backend/domain/src/webhook_outbound.rs`) either
 * silently drops legitimate deliveries or accepts forged/replayed webhooks.
 */

import { createHmac } from 'node:crypto'
import { describe, expect, it } from 'vitest'
import {
  HeraldWebhookError,
  WEBHOOK_HEADER_SIGNATURE,
  WEBHOOK_HEADER_TIMESTAMP,
  verifyAndParseWebhookEvent,
  verifyWebhookSignature,
} from '../src'

function sign(secret: string, timestamp: number | string, body: string | Buffer): string {
  const raw = typeof body === 'string' ? Buffer.from(body, 'utf8') : body
  const hex = createHmac('sha256', secret)
    .update(String(timestamp), 'utf8')
    .update('.')
    .update(raw)
    .digest('hex')
  return `v1=${hex}`
}

const FIXED_NOW = 1763640000
const SECRET = 'whsec_test_key'

describe('verifyWebhookSignature', () => {
  it('matches the fixed test vectors from backend/domain/src/webhook_outbound.rs', () => {
    // Vectors pinned in `sign_delivery_matches_fixed_vectors` (Rust domain test):
    // hex(HMAC-SHA256("whsec_test_key", "1763640000." + body))
    expect(
      verifyWebhookSignature({
        payload: '{"type":"user.created"}',
        secret: 'whsec_test_key',
        timestamp: '1763640000',
        signature: 'v1=03d317fbd891f5d25a1b48149824ef6a4cb981d249b5c029fd792720d418d0c0',
        nowSeconds: 1763640000,
      }),
    ).toBe(true)

    // Empty-body edge: the "." separator is still signed.
    expect(
      verifyWebhookSignature({
        payload: Buffer.alloc(0),
        secret: 'whsec_test_key',
        timestamp: '1763640000',
        signature: 'v1=5175027c75fed714286b89d9b4cbf090d701a102b61128d91adf3886103099cf',
        nowSeconds: 1763640000,
      }),
    ).toBe(true)
  })

  it('accepts Buffer, Uint8Array, UTF-8 string, Fetch Headers, and case-insensitive Express headers', () => {
    const bodyStr = '{"id":"evt_1","note":"中文载荷"}'
    const bodyBuf = Buffer.from(bodyStr, 'utf8')
    const bodyU8 = new Uint8Array(bodyBuf.buffer, bodyBuf.byteOffset, bodyBuf.byteLength)
    const sig = sign(SECRET, FIXED_NOW, bodyBuf)

    expect(
      verifyWebhookSignature({
        payload: bodyBuf,
        secret: SECRET,
        headers: {
          'X-Herald-Timestamp': String(FIXED_NOW),
          'X-Herald-Signature': sig,
        },
        nowSeconds: FIXED_NOW,
      }),
    ).toBe(true)

    expect(
      verifyWebhookSignature({
        payload: bodyU8,
        secret: SECRET,
        headers: new Headers({
          [WEBHOOK_HEADER_TIMESTAMP]: String(FIXED_NOW),
          [WEBHOOK_HEADER_SIGNATURE]: sig,
        }),
        nowSeconds: FIXED_NOW,
      }),
    ).toBe(true)

    expect(
      verifyWebhookSignature({
        payload: bodyStr,
        secret: SECRET,
        headers: {
          'x-herald-timestamp': [String(FIXED_NOW)],
          'x-herald-signature': [sig],
        },
        nowSeconds: FIXED_NOW,
      }),
    ).toBe(true)
  })

  it('supports zero-downtime key rotation with multiple candidate secrets', () => {
    const oldSecret = 'whsec_old_secret'
    const newSecret = 'whsec_new_secret'
    const payload = '{"id":"evt_rot"}'

    expect(
      verifyWebhookSignature({
        payload,
        secret: [oldSecret, newSecret],
        timestamp: String(FIXED_NOW),
        signature: sign(oldSecret, FIXED_NOW, payload),
        nowSeconds: FIXED_NOW,
      }),
    ).toBe(true)

    expect(
      verifyWebhookSignature({
        payload,
        secret: [oldSecret, newSecret],
        timestamp: String(FIXED_NOW),
        signature: sign(newSecret, FIXED_NOW, payload),
        nowSeconds: FIXED_NOW,
      }),
    ).toBe(true)

    expect(
      verifyWebhookSignature({
        payload,
        secret: [oldSecret, newSecret],
        timestamp: String(FIXED_NOW),
        signature: sign('whsec_unrelated', FIXED_NOW, payload),
        nowSeconds: FIXED_NOW,
      }),
    ).toBe(false)

    expect(
      verifyWebhookSignature({
        payload,
        secret: ['', ''],
        timestamp: String(FIXED_NOW),
        signature: sign('', FIXED_NOW, payload),
        nowSeconds: FIXED_NOW,
      }),
    ).toBe(false)
  })

  it('rejects tampered payloads, timestamps, or signatures', () => {
    const payload = '{"id":"evt_1","type":"user.disabled"}'
    const sig = sign(SECRET, FIXED_NOW, payload)

    expect(
      verifyWebhookSignature({
        payload: '{"id":"evt_2","type":"user.disabled"}',
        secret: SECRET,
        timestamp: String(FIXED_NOW),
        signature: sig,
        nowSeconds: FIXED_NOW,
      }),
    ).toBe(false)

    expect(
      verifyWebhookSignature({
        payload,
        secret: SECRET,
        timestamp: String(FIXED_NOW + 1),
        signature: sig,
        nowSeconds: FIXED_NOW,
      }),
    ).toBe(false)

    expect(
      verifyWebhookSignature({
        payload,
        secret: SECRET,
        timestamp: String(FIXED_NOW),
        signature: sig.slice(3),
        nowSeconds: FIXED_NOW,
      }),
    ).toBe(false)

    expect(
      verifyWebhookSignature({
        payload,
        secret: SECRET,
        timestamp: String(FIXED_NOW),
        signature: sig.slice(0, -2),
        nowSeconds: FIXED_NOW,
      }),
    ).toBe(false)
  })

  it('enforces the 300s replay tolerance window and allows disabling it with null', () => {
    const payload = '{"id":"evt_replay"}'
    const sig = sign(SECRET, FIXED_NOW, payload)

    expect(
      verifyWebhookSignature({
        payload,
        secret: SECRET,
        timestamp: String(FIXED_NOW),
        signature: sig,
        nowSeconds: FIXED_NOW + 300,
      }),
    ).toBe(true)
    expect(
      verifyWebhookSignature({
        payload,
        secret: SECRET,
        timestamp: String(FIXED_NOW),
        signature: sig,
        nowSeconds: FIXED_NOW - 300,
      }),
    ).toBe(true)

    expect(
      verifyWebhookSignature({
        payload,
        secret: SECRET,
        timestamp: String(FIXED_NOW),
        signature: sig,
        nowSeconds: FIXED_NOW + 301,
      }),
    ).toBe(false)

    expect(
      verifyWebhookSignature({
        payload,
        secret: SECRET,
        timestamp: String(FIXED_NOW),
        signature: sig,
        toleranceSeconds: null,
        nowSeconds: FIXED_NOW + 86_400,
      }),
    ).toBe(true)
  })

  it('rejects malformed or missing timestamp/signature headers', () => {
    const payload = '{}'
    for (const badTs of ['', '   ', '1763640000.5', '1e9', 'not-a-number']) {
      expect(
        verifyWebhookSignature({
          payload,
          secret: SECRET,
          timestamp: badTs,
          signature: sign(SECRET, badTs, payload),
          toleranceSeconds: null,
        }),
      ).toBe(false)
    }

    expect(
      verifyWebhookSignature({
        payload,
        secret: SECRET,
        headers: {},
        nowSeconds: FIXED_NOW,
      }),
    ).toBe(false)
  })

  it('returns false for unset secrets instead of throwing a native TypeError', () => {
    // README's rotation snippet reads env vars with `!`; outside a rotation
    // window the array carries undefined. That must stay a structured
    // rejection (false), never a TypeError that escapes as an HTTP 500 and
    // drives the backend's auto-pause after consecutive failures.
    const headers = {
      'X-Herald-Timestamp': String(FIXED_NOW),
      'X-Herald-Signature': 'v1=deadbeef',
    }
    expect(
      verifyWebhookSignature({
        payload: '{}',
        secret: [SECRET, undefined as unknown as string],
        headers,
        nowSeconds: FIXED_NOW,
      }),
    ).toBe(false)
    expect(
      verifyWebhookSignature({
        payload: '{}',
        secret: undefined as unknown as string,
        headers,
        nowSeconds: FIXED_NOW,
      }),
    ).toBe(false)
  })

  it('returns false for non-raw payloads instead of throwing a native TypeError', () => {
    // A route misconfigured with express.json() hands a parsed object to the
    // verifier; the bool contract is "false on any failure without throwing".
    expect(
      verifyWebhookSignature({
        payload: { type: 'user.created' } as unknown as string,
        secret: SECRET,
        timestamp: String(FIXED_NOW),
        signature: 'v1=deadbeef',
        nowSeconds: FIXED_NOW,
      }),
    ).toBe(false)
  })

  it('throws RangeError for invalid toleranceSeconds instead of reporting a replay-window failure', () => {
    // A negative tolerance is a caller config bug; reporting it as
    // 'timestamp-out-of-tolerance' (or silently false) points operators at
    // clock skew/replays instead of their own configuration.
    expect(() =>
      verifyWebhookSignature({
        payload: '{}',
        secret: SECRET,
        timestamp: String(FIXED_NOW),
        signature: 'v1=deadbeef',
        nowSeconds: FIXED_NOW,
        toleranceSeconds: -1,
      }),
    ).toThrow(RangeError)
  })
})

describe('verifyAndParseWebhookEvent', () => {
  const capture = (fn: () => unknown): HeraldWebhookError => {
    try {
      fn()
    } catch (e) {
      expect(e).toBeInstanceOf(HeraldWebhookError)
      return e as HeraldWebhookError
    }
    throw new Error('expected HeraldWebhookError')
  }

  it('verifies and returns strongly-typed event envelopes', () => {
    const raw = JSON.stringify({
      id: '0193d0c0-0000-7000-8000-000000000001',
      type: 'role.granted',
      realmId: 'my-realm',
      createdAt: '2026-11-20T12:00:00.100Z',
      occurredAt: '2026-11-20T11:59:58.000Z',
      data: {
        userId: '0193d0bf-0000-7000-8000-000000000042',
        roleId: '0193d0b0-0000-7000-8000-000000000009',
        roleName: 'pro',
        source: 'payment',
        expiresAt: '2026-12-20T11:59:58.000Z',
      },
    })

    const event = verifyAndParseWebhookEvent({
      payload: Buffer.from(raw, 'utf8'),
      secret: SECRET,
      headers: {
        'X-Herald-Timestamp': String(FIXED_NOW),
        'X-Herald-Signature': sign(SECRET, FIXED_NOW, raw),
      },
      nowSeconds: FIXED_NOW,
    })

    expect(event.type).toBe('role.granted')
    if (event.type === 'role.granted') {
      expect(event.data.roleName).toBe('pro')
      expect(event.data.source).toBe('payment')
    }
  })

  it('rejects invalid signatures before attempting JSON.parse', () => {
    // Body is intentionally invalid JSON; verifier must fail with invalid-signature,
    // never invalid-payload, proving unauthenticated input is not parsed.
    const err = capture(() =>
      verifyAndParseWebhookEvent({
        payload: '{not valid json',
        secret: SECRET,
        timestamp: String(FIXED_NOW),
        signature: 'v1=0000000000000000000000000000000000000000000000000000000000000000',
        nowSeconds: FIXED_NOW,
      }),
    )

    expect(err).toBeInstanceOf(HeraldWebhookError)
    expect(err.code).toBe('invalid-signature')
  })

  it('throws specific HeraldWebhookError codes for missing headers, timestamp errors, and invalid payloads', () => {
    expect(
      capture(() =>
        verifyAndParseWebhookEvent({ payload: '{}', secret: SECRET, headers: {} }),
      ).code,
    ).toBe('missing-headers')

    // Unset secrets (rotation var absent / env var missing) must surface as
    // the typed invalid-signature code, not a native TypeError.
    expect(
      capture(() =>
        verifyAndParseWebhookEvent({
          payload: '{}',
          secret: [SECRET, undefined as unknown as string],
          timestamp: String(FIXED_NOW),
          signature: 'v1=deadbeef',
          nowSeconds: FIXED_NOW,
        }),
      ).code,
    ).toBe('invalid-signature')

    // Non-raw payloads (express.json()-parsed objects) fail as invalid-payload
    // before any signature work, never as Buffer.from's TypeError.
    expect(
      capture(() =>
        verifyAndParseWebhookEvent({
          payload: { type: 'user.created' } as unknown as string,
          secret: SECRET,
          timestamp: String(FIXED_NOW),
          signature: 'v1=deadbeef',
          nowSeconds: FIXED_NOW,
        }),
      ).code,
    ).toBe('invalid-payload')

    expect(
      capture(() =>
        verifyAndParseWebhookEvent({
          payload: '{}',
          secret: SECRET,
          timestamp: 'abc',
          signature: 'v1=deadbeef',
        }),
      ).code,
    ).toBe('invalid-timestamp')

    expect(
      capture(() =>
        verifyAndParseWebhookEvent({
          payload: '{}',
          secret: SECRET,
          timestamp: String(FIXED_NOW),
          signature: sign(SECRET, FIXED_NOW, '{}'),
          nowSeconds: FIXED_NOW + 500,
        }),
      ).code,
    ).toBe('timestamp-out-of-tolerance')

    expect(
      capture(() =>
        verifyAndParseWebhookEvent({
          payload: '{broken',
          secret: SECRET,
          timestamp: String(FIXED_NOW),
          signature: sign(SECRET, FIXED_NOW, '{broken'),
          nowSeconds: FIXED_NOW,
        }),
      ).code,
    ).toBe('invalid-payload')

    const incomplete = JSON.stringify({ id: 'evt_1', type: 'user.created' })
    expect(
      capture(() =>
        verifyAndParseWebhookEvent({
          payload: incomplete,
          secret: SECRET,
          timestamp: String(FIXED_NOW),
          signature: sign(SECRET, FIXED_NOW, incomplete),
          nowSeconds: FIXED_NOW,
        }),
      ).code,
    ).toBe('invalid-payload')
  })
})
