/**
 * MSW handler factories for the user-sessions management endpoints
 * (`GET/DELETE /api/users/{userId}/sessions[/{familyId}]`).
 *
 * These factories are **NOT** registered in the global default `handlers` export
 * (see `src/test/mocks/handlers.ts`) — that would pollute every other suite with
 * an opinionated sessions response. Instead, each sessions dialog test imports
 * the factories it needs and installs them ad-hoc via `server.use(...)`, which
 * lets every test control the exact response (list contents, error status,
 * captured DELETE target, deferred resolution for pending-state assertions).
 *
 * The API client base URL is set globally to `http://localhost:3000` in
 * `src/test/setup.ts` (`client.setConfig({ baseUrl })`). MSW only intercepts
 * requests whose URL matches the handler URL exactly (scheme+host+path), so
 * every handler here uses the FULL URL — relative paths slip through unhandled.
 * This mirrors the convention in `browser-token.ts` and `credit-buckets.ts`.
 */

import { http, HttpResponse } from 'msw'
import type { UserSessionResponse } from '@/lib/api-generated'

const API_BASE_URL = 'http://localhost:3000'

/** Full list/revoke-all URL for a user's sessions collection. */
export function sessionsListUrl(realmId: string, userId: string): string {
  return `${API_BASE_URL}/api/users/${userId}/sessions`
}

/** Revoke-one URL template (path params interpolated by MSW). */
const REVOKE_ONE_URL = `${API_BASE_URL}/api/users/:userId/sessions/:familyId`

/**
 * Captured DELETE target. A test passes a `{ familyId: '' }` seed to the
 * revoke-one factory and asserts on `capture.familyId` after the confirm click —
 * this is the spy-on-the-real-URL pattern used by
 * `credit-bucket/__tests__/destructive-confirm.test.tsx`, proving the real
 * mutation fired with the right path param rather than a mocked function.
 */
export interface CapturedRevokeRequest {
  familyId?: string
}

/**
 * `GET /api/users/{userId}/sessions` → 200 with the given list.
 * Defaults to an empty 200 (the happy "no sessions" response) so a caller can
 * `server.use(createListUserSessionsHandler())` to settle the query.
 */
export function createListUserSessionsHandler(
  sessions: UserSessionResponse[] = [],
  status: 200 | number = 200
) {
  return http.get(sessionsListUrl('r1', 'u1'), () => HttpResponse.json(sessions, { status }))
}

/**
 * `GET .../sessions` → error status, so the dialog renders its error/retry
 * branch. The body shape is irrelevant — the query's `isError` flips on the
 * non-2xx status alone.
 */
export function createListUserSessionsErrorHandler(status: 500 | 403 | 404 | number = 500) {
  return http.get(sessionsListUrl('r1', 'u1'), () =>
    HttpResponse.json({ error: 'internal' }, { status })
  )
}

/**
 * `DELETE .../sessions/{familyId}` → 204. When a `capture` object is supplied,
 * the handler records `params.familyId` so the test can assert the real DELETE
 * targeted the row's family (not a stubbed call).
 */
export function createRevokeUserSessionHandler(capture?: CapturedRevokeRequest) {
  return http.delete(REVOKE_ONE_URL, ({ params }) => {
    if (capture) {
      capture.familyId = String(params.familyId)
    }
    return new HttpResponse(null, { status: 204 })
  })
}

/**
 * `DELETE .../sessions/{familyId}` whose response is gated on a caller-resolved
 * deferred. Used by the "revoke pending state" test to hold the mutation
 * in-flight (so `isPending` is observably true) until the test resolves.
 * `resolveCount` is incremented when the handler is actually invoked, letting
 * the test assert no double-fire from a second confirm click.
 */
export function createDeferredRevokeUserSessionHandler(opts: {
  capture?: CapturedRevokeRequest
  gate: Promise<void>
  onInvoked?: () => void
}) {
  return http.delete(REVOKE_ONE_URL, async ({ params }) => {
    opts.onInvoked?.()
    if (opts.capture) {
      opts.capture.familyId = String(params.familyId)
    }
    await opts.gate
    return new HttpResponse(null, { status: 204 })
  })
}

/**
 * `DELETE .../sessions` → 200 `{ revokedCount }`. The count is forwarded by the
 * mutation hook to the dialog's success toast, so a test asserts the rendered
 * toast carries this number.
 */
export function createRevokeAllUserSessionsHandler(revokedCount = 3, status: 200 | number = 200) {
  return http.delete(sessionsListUrl('r1', 'u1'), () =>
    HttpResponse.json({ revokedCount }, { status })
  )
}

/** `DELETE .../sessions` → error status, driving the revoke-all failure toast. */
export function createRevokeAllUserSessionsErrorHandler(status: 500 | 403 | 404 | number = 500) {
  return http.delete(sessionsListUrl('r1', 'u1'), () =>
    HttpResponse.json({ error: 'internal' }, { status })
  )
}

/**
 * Default bundle for tests that just need the dialog to load cleanly: an empty
 * sessions list (200) plus a success revoke-all. Registered ad-hoc via
 * `server.use(...userSessionsHandlers)`. Per-endpoint overrides installed AFTER
 * this spread win (MSW evaluates handlers in registration order).
 */
export const userSessionsHandlers = [
  createListUserSessionsHandler(),
  createRevokeAllUserSessionsHandler(0),
]
