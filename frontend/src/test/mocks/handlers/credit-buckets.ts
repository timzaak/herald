import { http, HttpResponse } from 'msw'

const API_BASE_URL = 'http://localhost:3000'
const BASE = `${API_BASE_URL}/api/bill/credit-buckets`

export interface BucketInUseErrorBody {
  code: 'bucket_in_use'
  activeSubscriptions: number
  holdersWithBalance: number
}

/**
 * Per-domain MSW handlers for Credit Buckets.
 *
 * Default handlers are intentionally minimal; tests cover error branches
 * ad-hoc via the exported factory helpers below + `server.use(...)`.
 */

export const creditBucketsHandlers = [
  // Empty success list so list/detail queries used by the editor resolve.
  http.get(`${BASE}`, () =>
    HttpResponse.json({
      items: [],
      page: 0,
      pageSize: 20,
      total: 0,
    })
  ),
  http.get(`${BASE}/overview`, () => HttpResponse.json({ rows: [], grandTotal: {} })),
  // Client apps pulled by the editor multiselect (`listClientApps` →
  // /api/client). Empty list is enough — the editor's multiselect
  // renders from it and we don't exercise it here.
  // The mapping-list query is served by the shared `entitlementMappingHandlers`
  // module (registered globally in handlers.ts); its default also returns an
  // empty list.
  http.get(`${API_BASE_URL}/api/client`, () =>
    HttpResponse.json({ items: [], page: 0, pageSize: 100, total: 0 })
  ),
]

// ===== Error-scenario factories (consumed via server.use) =====

/** 409 `bucket_in_use` body. */
export function deleteBucketInUseHandler(body: BucketInUseErrorBody) {
  return http.delete(`${BASE}/:bucketId`, () => HttpResponse.json(body, { status: 409 }))
}
