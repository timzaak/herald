import { http, HttpResponse } from 'msw'
import type {
  BatchUpdateEntitlementMappingsResponse,
  EntitlementMappingResponse,
  PurchaseOptionView,
  SyncProviderResponse,
} from '@/lib/api-generated'
import {
  batchUpdateOkBody,
  batch400Body,
  multiPriceMappingList,
  protectedPrice409Body,
  purchaseOptionsList,
  syncResult,
} from '../../fixtures/entitlement-mappings'

/**
 * Shared MSW handlers + per-status factory handlers for the price-granularity
 * billing endpoints. Paths are copied verbatim from
 * the generated client `frontend/src/lib/api-generated/sdk.gen.ts`.
 *
 * Consolidation rule (load-bearing): the DEFAULT `GET .../entitlement-mappings`
 * handler returns an EMPTY list. This preserves the semantics of the inline
 * handler it replaced (`credit-buckets.ts:37`) so unrelated consumers — most
 * notably the credit-bucket editor multiselect — still get an empty list and
 * do not receive surprise multi-price data. Feature tests that need multi-price
 * data override via `server.use(...)` with a handler built from
 * `multiPriceMappingList()`.
 */

const API_BASE_URL = 'http://localhost:3000'
const MAPPING_BASE = `${API_BASE_URL}/api/bill/entitlement-mappings`

// ---------------------------------------------------------------------------
// Default success handlers (registered globally via handlers.ts)
// ---------------------------------------------------------------------------

/**
 * Default handlers. The list endpoint returns an EMPTY list on purpose —
 * see the consolidation rule in the file header.
 */
export const entitlementMappingHandlers = [
  // GET .../entitlement-mappings → empty list (consumers override via server.use).
  http.get(MAPPING_BASE, () => HttpResponse.json({ items: [], total: 0 })),
]

// ---------------------------------------------------------------------------
// Per-status factory handlers for `server.use(...)` overrides
// ---------------------------------------------------------------------------

/**
 * Override: `GET .../entitlement-mappings` returning the supplied rows.
 * Use `multiPriceMappingList()` or `unresolvedMappingList()` from the fixtures
 * module for the common cases.
 */
export function entitlementMappingListHandler(items: EntitlementMappingResponse[]) {
  return http.get(MAPPING_BASE, () => HttpResponse.json({ items, total: items.length }))
}

/**
 * Override: `PUT .../entitlement-mappings/batch` → 200 with the product's full
 * latest price set and a `saved` count.
 */
export function batchUpdateOkHandler(body?: BatchUpdateEntitlementMappingsResponse) {
  return http.put(`${MAPPING_BASE}/batch`, () => HttpResponse.json(body ?? batchUpdateOkBody()))
}

/**
 * Override: `PUT .../entitlement-mappings/batch` → 200 with the product's full
 * latest price set, CAPTURING the request body into `capture` so tests assert
 * the `updates[*].quotaWindows` payload by observing the MSW request (per the
 * testing guide — do NOT mock the internal API function).
 */
export function batchUpdateOkCaptureHandler(
  capture: { body: unknown },
  body?: BatchUpdateEntitlementMappingsResponse
) {
  return http.put(`${MAPPING_BASE}/batch`, async ({ request }) => {
    capture.body = await request.json()
    return HttpResponse.json(body ?? batchUpdateOkBody())
  })
}

/**
 * Override: `PUT .../entitlement-mappings/batch` → 400 validation error
 * (entitlement-key regex / cross-product shared-key rename).
 */
export function batchUpdate400Handler(message?: string) {
  return http.put(`${MAPPING_BASE}/batch`, () =>
    HttpResponse.json(batch400Body(message), { status: 400 })
  )
}

/**
 * Override: `PUT .../entitlement-mappings/batch` → 409 active-subscription
 * lock. The whole batch transaction is rolled back.
 */
export function batchUpdate409Handler(activeSubscriptions: number) {
  return http.put(`${MAPPING_BASE}/batch`, () =>
    HttpResponse.json(protectedPrice409Body(activeSubscriptions), { status: 409 })
  )
}

/**
 * Override: `POST .../entitlement-mappings/sync` → 200 sync result.
 */
export function syncHandler(result?: Partial<SyncProviderResponse>) {
  return http.post(`${MAPPING_BASE}/sync`, () => HttpResponse.json(syncResult(result)))
}

/**
 * Override: `GET .../client/:clientAppId/purchase-options` → 200 with the
 * supplied items (defaults to the multi-price fixture set).
 */
export function purchaseOptionsHandler(items?: PurchaseOptionView[]) {
  return http.get(`${API_BASE_URL}/api/bill/client/:clientAppId/purchase-options`, () =>
    HttpResponse.json({ items: items ?? purchaseOptionsList() })
  )
}
