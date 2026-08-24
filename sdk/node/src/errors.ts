/**
 * Error type for the Herald Node SDK, mirroring the Rust `herald_sdk::Error`
 * variants (`sdk/rust/src/lib.rs`): every non-2xx or transport failure is
 * thrown as a `HeraldSdkError` carrying a stable machine-readable `code`.
 */

export type HeraldSdkErrorCode =
  /** Fetch-level transport failure (Rust: `Error::Reqwest`). */
  | 'network'
  /** 401 — invalid API key (Rust: `Error::Unauthorized`). */
  | 'unauthorized'
  /** 403 — e.g. cross-realm access or insufficient permission (Rust: `Error::Forbidden`). */
  | 'forbidden'
  /** 404 (Rust: `Error::NotFound`). */
  | 'not-found'
  /** 500 (Rust: `Error::InternalServerError`). */
  | 'internal-server-error'
  /** Any other non-2xx status (Rust: `Error::ApiError`). */
  | 'api-error'
  /** 2xx with a non-JSON body (Rust: `Error::SerdeJson`). */
  | 'parse'

export class HeraldSdkError extends Error {
  /** Stable machine-readable category; prefer this over string-matching the message. */
  readonly code: HeraldSdkErrorCode
  /** HTTP status when the error came from a response; undefined for network errors. */
  readonly status?: number
  /** Raw response body text when the error came from a response. */
  readonly body?: string

  constructor(code: HeraldSdkErrorCode, status: number | undefined, body: string | undefined) {
    const label = status !== undefined ? `${code} (${status})` : code
    super(body ? `Herald SDK error: ${label}: ${body}` : `Herald SDK error: ${label}`)
    this.name = 'HeraldSdkError'
    this.code = code
    this.status = status
    this.body = body
  }
}
