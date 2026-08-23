/**
 * Bearer API client wiring (design §4.4 — Herald own frontend token model).
 *
 * This is the single hand-maintained module that adapts the generated
 * `@hey-api` client to the Bearer access/refresh token family owned by the
 * `@herald/web` SDK client (`lib/herald-client.ts`, DEC-js-sdk-013):
 *
 *   - a **request interceptor** injects `Authorization: Bearer {accessToken}`
 *     from the SDK's in-memory token holder onto every generated-client
 *     request;
 *   - a **response interceptor** catches a single 401, delegates to the SDK's
 *     single-flight refresh (which rotates the tokens in place), and replays
 *     the original request exactly once (loop-guarded);
 *   - a 403 is replayed once ONLY when the request went out with an access
 *     token that a first-party client switch has since superseded (admin-
 *     console-scoped endpoints reject that stale token with 403, not 401);
 *     any other 403 is a genuine permission denial and passes through.
 *
 * It is deliberately thin: it imports only the generated `client` and the
 * Herald client bridge. It hardcodes no API paths — all network calls go
 * through generated SDK functions (the refresh itself runs inside the SDK
 * transport).
 *
 * `initBearerClient()` is called from `main.tsx` before `createRouter`/render.
 */

import { client } from '@/lib/api-generated/client.gen'
import type { ResolvedRequestOptions } from '@/lib/api-generated/client/types.gen'
import {
  getActiveHeraldClient,
  getHeraldAccessToken,
  waitForTokenSwitch,
} from '@/lib/herald-client'

/** Interceptor argument types (the generated client is a fetch client, so the
 *  request/response generic params are the DOM `Request`/`Response`). */
type ReqOptions = ResolvedRequestOptions

/** Marker header so the response interceptor only retries each request once. */
const RETRY_HEADER = 'X-Herald-Refresh-Retried'
/** The path of the refresh endpoint, to avoid refreshing the refresh call. */
const REFRESH_PATH = '/api/auth/browser-token/refresh'
/**
 * The path of the switch-client endpoint. Its own 401s must not wait on the
 * token-switch gate: the gate is held by the very flow that issued this
 * request, so waiting here would deadlock instead of recovering.
 */
const SWITCH_CLIENT_PATH = '/api/auth/browser-token/switch-client'

/**
 * Run the refresh exactly once for a batch of concurrent 401s. The SDK's
 * transport core provides the single-flight promise (and the rotation); this
 * wrapper only decides whether a retry is possible.
 *
 * On failure the SDK emits `session-expired`, which the herald-client bridge
 * turns into a token clear + store logout — mirroring the pre-SDK behaviour —
 * and the 401 propagates to the caller.
 */
function refreshOnce(): Promise<boolean> {
  const herald = getActiveHeraldClient()
  if (!herald || !herald.storage.getRefreshToken()) {
    return Promise.resolve(false)
  }
  return herald.refresh().then(
    () => true,
    () => false
  )
}

/**
 * Replay the original request after a successful refresh. The generated client
 * builds a fresh `Request` for each call via the SDK function that produced it,
 * so we cannot naively re-`fetch` the old `Request` (its body stream is
 * already consumed). Instead we re-issue through `client.request` with the same
 * options, which rebuilds the request with the now-updated Authorization.
 */
async function replayRequest(options: Record<string, unknown>): Promise<Response> {
  // `client.request` returns the `{ data, error, request, response }` envelope
  // (responseStyle 'fields', the default). It also fully consumes + parses the
  // response body internally (success → `data`, error → `error`). We cannot
  // return `result.response` directly: the OUTER request pipeline that invoked
  // this interceptor will try to read the body AGAIN, and a `Response` body can
  // only be read once ("Body is unusable: Body has already been read"). So
  // reconstruct a fresh Response from the already-parsed payload.
  const result = (await client.request(options as never)) as {
    response?: Response
    data?: unknown
    error?: unknown
  }
  if (!result.response) {
    // No response object (e.g. network error path) — synthesize a 401 so the
    // caller surfaces re-login rather than masking the failure.
    return new Response(null, { status: 401 })
  }
  const status = result.response.status
  const headers = new Headers(result.response.headers)
  const payload = result.data !== undefined ? result.data : result.error
  const body = payload !== undefined && payload !== null ? JSON.stringify(payload) : null
  return new Response(body, { status, headers })
}

/**
 * Replay `options` with the loop-guard marker set, so a 401 on the replay
 * surfaces to the caller instead of triggering another recovery round.
 */
async function replayMarkedRequest(options: ReqOptions): Promise<Response> {
  // Mark the replay so a subsequent 401 is not refreshed again (loop guard).
  // Preserve the original per-request headers (a `Headers` instance by this
  // stage — spreading it as a plain object would drop them) and add the marker.
  const replayOptions: Record<string, unknown> = { ...options }
  const headers = new Headers(
    (replayOptions.headers as Headers | Record<string, string> | undefined) ?? undefined
  )
  headers.set(RETRY_HEADER, '1')
  replayOptions.headers = headers
  return replayRequest(replayOptions)
}

/**
 * Wait for any in-flight first-party client switch to settle, then report
 * whether `request` went out with an access token that has since been
 * superseded. This is the ONLY condition under which a rejection is treated
 * as "the switch raced me" (safe to replay with the current token); with the
 * token still current, the rejection was genuinely earned and must surface.
 */
async function usedSupersededAccessToken(request: Request): Promise<boolean> {
  await waitForTokenSwitch()
  const accessToken = getHeraldAccessToken()
  const usedAuthorization = request.headers.get('Authorization')
  return Boolean(accessToken && usedAuthorization && usedAuthorization !== `Bearer ${accessToken}`)
}

/**
 * Install the Bearer request interceptor and the 401 silent-refresh retry
 * interceptor on the generated client. Idempotent — safe to call once from
 * `main.tsx`.
 */
export function initBearerClient(): void {
  // --- Request interceptor: inject Authorization: Bearer ---
  client.interceptors.request.use((request: Request, options: ReqOptions) => {
    // Skip the refresh endpoint itself — it authenticates with the refresh
    // token in the body, not a Bearer access token.
    const url = options.url
    if (url === REFRESH_PATH) {
      return request
    }
    const accessToken = getHeraldAccessToken()
    if (!accessToken) {
      return request
    }
    // Reuse the same Request object but add the header. `Request` is immutable
    // for headers, so we clone with the authorization set.
    const headers = new Headers(request.headers)
    // Only set if not already present (e.g. a request that pre-set its own).
    if (!headers.has('Authorization')) {
      headers.set('Authorization', `Bearer ${accessToken}`)
    }
    return new Request(request, { headers })
  })

  // --- Response interceptor: 401 → refresh → retry once; mid-switch 403 →
  // replay with the rotated token (genuine 403s pass through) ---
  client.interceptors.response.use(
    async (response: Response, request: Request, options: ReqOptions) => {
      // Admin-console-scoped endpoints reject a mid-switch (superseded) access
      // token with 403 instead of 401. Replay only when the token actually
      // rotated while the request was in flight; a genuine permission 403 must
      // reach the caller untouched — no replay (would loop on a denial the new
      // token earns too) and no refresh (a permission denial is not a session
      // problem; refreshing on it needlessly rotates the family).
      if (response.status === 403) {
        if (
          options.url !== SWITCH_CLIENT_PATH &&
          !request.headers.get(RETRY_HEADER) &&
          (await usedSupersededAccessToken(request))
        ) {
          return replayMarkedRequest(options)
        }
        return response
      }
      if (response.status !== 401) {
        return response
      }
      const url = options.url
      // Never auto-refresh the refresh endpoint, and never retry a request twice.
      if (url === REFRESH_PATH) {
        return response
      }
      const retried = request.headers.get(RETRY_HEADER)
      if (retried) {
        return response
      }

      // A first-party client switch (root loader) may be rotating the token
      // family under this request: wait for it to settle before deciding the
      // session expired, and replay directly with the current token when the
      // access token rotated while the request was in flight — refreshing with
      // the superseded refresh token would revoke the family and log the
      // freshly-rotated session out.
      if (url !== SWITCH_CLIENT_PATH && (await usedSupersededAccessToken(request))) {
        return replayMarkedRequest(options)
      }

      const refreshed = await refreshOnce()
      if (!refreshed) {
        // Refresh failed; let the 401 propagate so the caller routes to re-login.
        return response
      }
      return replayMarkedRequest(options)
    }
  )
}
