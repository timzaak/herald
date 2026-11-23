/**
 * Outbound webhook signature verification and event envelope parsing for Herald.
 *
 * Mirrors `sign_delivery` and `OutboundEventEnvelope` in
 * `backend/domain/src/webhook_outbound.rs`:
 *   signedPayload = `${X-Herald-Timestamp}.${rawBodyBytes}`
 *   X-Herald-Signature = `v1=${hex(HMAC-SHA256(secret, signedPayload))}`
 *
 * Zero runtime dependencies — uses Node's built-in `node:crypto`.
 */

import { createHmac, timingSafeEqual } from 'node:crypto'

export const WEBHOOK_HEADER_EVENT_ID = 'x-herald-event-id'
export const WEBHOOK_HEADER_EVENT_TYPE = 'x-herald-event-type'
export const WEBHOOK_HEADER_DELIVERY_ID = 'x-herald-delivery-id'
export const WEBHOOK_HEADER_TIMESTAMP = 'x-herald-timestamp'
export const WEBHOOK_HEADER_SIGNATURE = 'x-herald-signature'

const DEFAULT_TOLERANCE_SECONDS = 300
const INTEGER_TIMESTAMP_RE = /^-?\d+$/

export type WebhookEventCategory =
  | 'user_lifecycle'
  | 'role_events'
  | 'subscription_lifecycle'

export type WebhookRoleEventSource = 'manual' | 'payment'

export interface HeraldWebhookEnvelope<
  TType extends string = string,
  TData = Record<string, unknown>,
> {
  /** Globally unique UUIDv7 idempotency key; identical across retries and fan-out. */
  id: string
  /** Event type string (e.g. `role.granted`, `user.disabled`). */
  type: TType
  /** Realm the event belongs to. */
  realmId: string
  /** RFC3339 timestamp when Herald recorded the event. */
  createdAt: string
  /** RFC3339 timestamp when the state change took effect; use for ordering arbitration. */
  occurredAt: string
  data: TData
}

export interface UserWebhookEventData {
  userId: string
}

export interface RoleWebhookEventData {
  userId: string
  roleId: string
  roleName: string | null
  source: WebhookRoleEventSource
  expiresAt: string | null
}

export interface SubscriptionWebhookEventData {
  userId: string
  subscriptionId: string | null
  entitlementKey: string | null
  currentPeriodStart: string | null
  currentPeriodEnd: string | null
}

/** Known outbound webhook event envelopes emitted by Herald. */
export type HeraldWebhookEvent =
  | HeraldWebhookEnvelope<'user.created', UserWebhookEventData>
  | HeraldWebhookEnvelope<'user.disabled', UserWebhookEventData>
  | HeraldWebhookEnvelope<'user.deleted', UserWebhookEventData>
  | HeraldWebhookEnvelope<'role.granted', RoleWebhookEventData>
  | HeraldWebhookEnvelope<'role.revoked', RoleWebhookEventData>
  | HeraldWebhookEnvelope<'subscription.activated', SubscriptionWebhookEventData>
  | HeraldWebhookEnvelope<'subscription.renewed', SubscriptionWebhookEventData>
  | HeraldWebhookEnvelope<'subscription.expired', SubscriptionWebhookEventData>
  | HeraldWebhookEnvelope<'subscription.refunded', SubscriptionWebhookEventData>

export type HeraldWebhookErrorCode =
  | 'missing-headers'
  | 'invalid-timestamp'
  | 'timestamp-out-of-tolerance'
  | 'invalid-signature'
  | 'invalid-payload'

export class HeraldWebhookError extends Error {
  /** Stable machine-readable category; prefer this over string-matching the message. */
  readonly code: HeraldWebhookErrorCode

  constructor(code: HeraldWebhookErrorCode, message: string) {
    super(`Herald webhook error (${code}): ${message}`)
    this.name = 'HeraldWebhookError'
    this.code = code
  }
}

/** Incoming HTTP headers from Node `http.IncomingMessage`, Express `req.headers`,
 * or a Web Fetch `Headers` instance. */
export type WebhookHeadersInput =
  | { get(name: string): string | null }
  | Record<string, string | readonly string[] | undefined>

export interface VerifyWebhookSignatureOptions {
  /** Raw request body bytes (or unmodified UTF-8 string). Never pass re-serialized JSON. */
  payload: string | Uint8Array | Buffer
  /** Signing secret (`whsec_...`) or candidate secrets for zero-downtime key rotation. */
  secret: string | readonly string[]
  /** Incoming HTTP headers (Express `req.headers`, Web `Headers`, etc.). */
  headers?: WebhookHeadersInput
  /** Explicit `X-Herald-Timestamp` header value (takes precedence over `headers`). */
  timestamp?: string
  /** Explicit `X-Herald-Signature` header value (takes precedence over `headers`). */
  signature?: string
  /** Allowed clock skew in seconds (default: `300`). Pass `null` to disable replay checks. */
  toleranceSeconds?: number | null
  /** Override current Unix time in seconds (primarily for deterministic testing). */
  nowSeconds?: number
}

function readHeader(headers: WebhookHeadersInput | undefined, targetName: string): string | undefined {
  if (!headers) return undefined
  if (typeof (headers as { get?: unknown }).get === 'function') {
    const val = (headers as { get(name: string): string | null }).get(targetName)
    return val ?? undefined
  }
  const lower = targetName.toLowerCase()
  const record = headers as Record<string, string | readonly string[] | undefined>
  for (const [key, value] of Object.entries(record)) {
    if (key.toLowerCase() !== lower) continue
    if (typeof value === 'string') return value
    if (Array.isArray(value)) return value[0]
    return undefined
  }
  return undefined
}

function toRawBytes(payload: string | Uint8Array | Buffer): Buffer {
  if (typeof payload === 'string') {
    return Buffer.from(payload, 'utf8')
  }
  if (Buffer.isBuffer(payload)) {
    return payload
  }
  return Buffer.from(payload.buffer, payload.byteOffset, payload.byteLength)
}

function computeSignature(secret: string, timestamp: string, rawBytes: Buffer): Buffer {
  const hmac = createHmac('sha256', secret)
  hmac.update(timestamp, 'utf8')
  hmac.update('.')
  hmac.update(rawBytes)
  return Buffer.from(`v1=${hmac.digest('hex')}`, 'utf8')
}

function assertValidWebhookSignature(options: VerifyWebhookSignatureOptions): Buffer {
  const timestamp = options.timestamp ?? readHeader(options.headers, WEBHOOK_HEADER_TIMESTAMP)
  const signature = options.signature ?? readHeader(options.headers, WEBHOOK_HEADER_SIGNATURE)

  if (!timestamp || !signature) {
    throw new HeraldWebhookError(
      'missing-headers',
      'X-Herald-Timestamp and X-Herald-Signature headers are required',
    )
  }

  if (!INTEGER_TIMESTAMP_RE.test(timestamp)) {
    throw new HeraldWebhookError('invalid-timestamp', 'X-Herald-Timestamp must be a Unix integer in seconds')
  }
  const parsedTimestamp = Number(timestamp)
  if (!Number.isSafeInteger(parsedTimestamp)) {
    throw new HeraldWebhookError('invalid-timestamp', 'X-Herald-Timestamp is out of safe integer range')
  }

  const tolerance = options.toleranceSeconds === undefined ? DEFAULT_TOLERANCE_SECONDS : options.toleranceSeconds
  if (tolerance !== null) {
    if (!Number.isFinite(tolerance) || tolerance < 0) {
      // A caller-side config bug — throw loudly (mirrors withHttpTimeout's
      // RangeError) instead of masquerading as a replay-window rejection.
      throw new RangeError('toleranceSeconds must be a non-negative finite number (or null to disable replay checks)')
    }
    const now = options.nowSeconds ?? Date.now() / 1000
    if (Math.abs(now - parsedTimestamp) > tolerance) {
      throw new HeraldWebhookError(
        'timestamp-out-of-tolerance',
        `timestamp drift exceeds ${tolerance} seconds`,
      )
    }
  }

  // Degrade unset/invalid secrets (README's rotation snippet reads env vars
  // with `!`, yielding undefined elements outside a rotation window) to the
  // typed rejection below instead of a native TypeError from `.length`.
  const secretCandidates: readonly unknown[] = typeof options.secret === 'string'
    ? [options.secret]
    : Array.isArray(options.secret)
      ? options.secret
      : []
  const secrets = secretCandidates.filter(
    (candidate): candidate is string => typeof candidate === 'string' && candidate.length > 0,
  )
  if (secrets.length === 0) {
    throw new HeraldWebhookError('invalid-signature', 'at least one non-empty signing secret is required')
  }

  // Reject non-raw payloads (e.g. a route misconfigured with express.json()
  // hands a parsed object) with the typed code instead of Buffer.from's
  // native TypeError.
  if (typeof options.payload !== 'string' && !(options.payload instanceof Uint8Array)) {
    throw new HeraldWebhookError(
      'invalid-payload',
      'payload must be the raw request body (string, Uint8Array, or Buffer), never a re-serialized or parsed JSON object',
    )
  }
  const rawBytes = toRawBytes(options.payload)
  const received = Buffer.from(signature, 'utf8')

  let matched = false
  for (const secret of secrets) {
    const expected = computeSignature(secret, timestamp, rawBytes)
    if (expected.length === received.length && timingSafeEqual(expected, received)) {
      matched = true
    }
  }

  if (!matched) {
    throw new HeraldWebhookError('invalid-signature', 'HMAC-SHA256 signature mismatch')
  }

  return rawBytes
}

/**
 * Verify a Herald outbound webhook delivery's HMAC-SHA256 signature and replay
 * timestamp window in constant time.
 *
 * Supports passing a single secret or an array of secrets for zero-downtime
 * key rotation. Returns `false` on any failure without throwing.
 */
export function verifyWebhookSignature(options: VerifyWebhookSignatureOptions): boolean {
  try {
    assertValidWebhookSignature(options)
    return true
  } catch (error) {
    if (error instanceof HeraldWebhookError) {
      return false
    }
    throw error
  }
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === 'string' && value.length > 0
}

/**
 * Verify a Herald outbound webhook signature and parse the raw request body
 * into a typed {@link HeraldWebhookEvent} envelope.
 *
 * Signature verification always runs before `JSON.parse`; invalid signatures,
 * missing headers, expired timestamps, or malformed envelopes throw
 * {@link HeraldWebhookError} with a machine-readable `code`.
 */
export function verifyAndParseWebhookEvent(options: VerifyWebhookSignatureOptions): HeraldWebhookEvent {
  const rawBytes = assertValidWebhookSignature(options)

  let parsed: unknown
  try {
    parsed = JSON.parse(rawBytes.toString('utf8'))
  } catch (cause) {
    throw new HeraldWebhookError('invalid-payload', `invalid JSON body: ${String(cause)}`)
  }

  if (
    !isPlainObject(parsed) ||
    !isNonEmptyString(parsed.id) ||
    !isNonEmptyString(parsed.type) ||
    !isNonEmptyString(parsed.realmId) ||
    !isNonEmptyString(parsed.createdAt) ||
    !isNonEmptyString(parsed.occurredAt) ||
    !isPlainObject(parsed.data)
  ) {
    throw new HeraldWebhookError(
      'invalid-payload',
      'webhook payload must be a JSON object with id, type, realmId, createdAt, occurredAt, and data',
    )
  }

  return parsed as unknown as HeraldWebhookEvent
}
