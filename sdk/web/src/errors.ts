/**
 * Typed, programmatically-discriminable error union (US-JS-008).
 *
 * Every public SDK method rejects with a `HeraldError`. Branch on the stable
 * `kind` field instead of parsing messages.
 */

export type HeraldErrorKind =
  /** Non-browser/SSR use without an injected `TokenStorage` adapter. */
  | 'ssr-no-storage'
  /** Cross-origin blocked because the page origin is not on the Client App allow-list. */
  | 'origin-not-allowed'
  /** Network failure or undifferentiated fetch error. */
  | 'network'
  /** 401 — credentials invalid (non-refresh context). */
  | 'unauthorized'
  /** 403. */
  | 'forbidden'
  /** 404. */
  | 'not-found'
  /** Login returned a second-factor challenge where a direct token was expected. */
  | 'requires-second-factor'
  /** Login returned a consent-required gate where a direct token was expected. */
  | 'consent-required'
  /** Refresh failed / token family revoked / Client App disabled. */
  | 'session-expired'
  /** 429. */
  | 'rate-limited'
  /** 400 field validation. */
  | 'validation'
  /** Any other backend error (carries `code`/`requestId`/`details`). */
  | 'api'

export interface HeraldErrorInit {
  kind: HeraldErrorKind
  message?: string
  status?: number
  /** Backend `ApiError.code` (snake_case slug) when available. */
  code?: string
  /** Backend `ApiError.requestId` when available. */
  requestId?: string
  /** Backend `ApiError.details` when available. */
  details?: unknown
}

export class HeraldError extends Error {
  readonly kind: HeraldErrorKind
  readonly status?: number
  readonly code?: string
  readonly requestId?: string
  readonly details?: unknown

  constructor(init: HeraldErrorInit) {
    super(init.message ?? init.kind)
    this.name = 'HeraldError'
    this.kind = init.kind
    this.status = init.status
    this.code = init.code
    this.requestId = init.requestId
    this.details = init.details
  }
}

/** Backend `ApiError` JSON body shape (`backend/api-base/.../api_error.rs`). */
interface ApiErrorBody {
  status?: number
  code?: string
  message?: string
  error?: string
  details?: unknown
  requestId?: string | null
}

function kindForStatus(status: number): HeraldErrorKind {
  switch (status) {
    case 400:
      return 'validation'
    case 401:
      return 'unauthorized'
    case 403:
      return 'forbidden'
    case 404:
      return 'not-found'
    case 429:
      return 'rate-limited'
    default:
      return 'api'
  }
}

/**
 * Map a generated-client op result error to a `HeraldError`.
 *
 * - `response === undefined` ⇒ the underlying `fetch` threw. The browser cannot
 *   reliably distinguish a CORS rejection (origin not pre-registered on the
 *   Client App) from a generic network failure, so we report `network` and hint
 *   at origin pre-registration.
 * - otherwise map by HTTP status, preserving backend `code`/`requestId`/`details`.
 */
export function toHeraldError(error: unknown, response: Response | undefined): HeraldError {
  if (!response) {
    return new HeraldError({
      kind: 'network',
      message:
        'Network request failed. For cross-origin integrations, ensure the page origin is pre-registered on the Client App (allowed_origins).',
      details: error,
    })
  }
  const body = (error ?? {}) as ApiErrorBody
  return new HeraldError({
    kind: kindForStatus(response.status),
    status: response.status,
    message: body.message ?? body.error ?? `HTTP ${response.status}`,
    code: body.code,
    requestId: body.requestId ?? undefined,
    details: body.details,
  })
}
