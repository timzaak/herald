/**
 * Credit Bucket Demo Helpers
 *
 * Shared helper functions for the Credit Bucket feature demo tests.
 * User stories covered (across DE-D02..D05):
 * - US-CB-001: admin CRUD on the Credit Bucket directory
 * - US-CB-002: bind a Client App coverage set to a bucket (≥1 required)
 * - US-CB-003: entitlement mappings target a bucket via distribution rules;
 *   the bucket editor surfaces these references read-only.
 *
 * Scope:
 * - UI helpers drive the Master-Detail editor at
 *   `/{realmId}/manage/billing/credit-buckets`. They perform NO business
 *   assertions — they only orchestrate clicks/fills and surface the resulting
 *   state (created id, error code). Assertion responsibility lives in the
 *   `.e2e.ts` flow tests (DE-D02 et al.).
 * - API helpers mirror `_http_json` in `scripts/lib/demo_seed.py` so other
 *   items can provision buckets in `beforeAll` without exercising the UI.
 *
 * Selectors are imported from `../selectors` (single source of truth). Bucket
 * keys/names come from `./bucket-seed-ids`.
 *
 * @see docs/user-stories/billing/credit-bucket.md
 */

import { Page, expect, type APIRequestContext, type APIResponse } from '@playwright/test'
import { SELECTORS } from '../selectors'
import {
  createBearerApiContext,
  clearSessionData,
  DEMO_ADMIN,
  REALM_ADMINS,
} from './auth'
import { LoginPage } from '../pages/login-page'

// ============================================================================
// Types
// ============================================================================

/** Options accepted by `createBucketViaUI` (matches POST /credit-buckets). */
export interface CreateBucketOptions {
  bucketKey: string
  name: string
  description?: string
  displayOrder?: number
  enabled?: boolean
  /** Client App UUIDs to bind as the coverage set (≥1 required by the API). */
  clientAppIds: string[]
}

/** Partial fields for `editBucketViaUI`. `undefined` fields are left as-is. */
export interface EditBucketFields {
  name?: string
  description?: string
  displayOrder?: number
  enabled?: boolean
}

/** Result of attempting a destructive bucket delete via the confirm dialog. */
export interface DeleteBucketResult {
  /** Whether the dialog closed after confirm (delete accepted). */
  success: boolean
  /**
   * Error code surfaced in `delete-bucket-error-message` when the backend
   * refused the delete. Expected values: `bucket_in_use` (active subscriptions
   * or holders with balance remain). `undefined` on success.
   */
  errorCode?: string
  /** Raw error message text (for diagnostics / soft assertions). */
  errorMessage?: string
}

/**
 * API payload for `POST /api/realms/{realmId}/billing/credit-buckets`.
 * Mirrors `_http_json` in `scripts/lib/demo_seed.py`.
 */
export interface CreateBucketApiPayload {
  bucketKey: string
  name: string
  description?: string
  displayOrder?: number
  enabled?: boolean
  clientAppIds: string[]
}

/**
 * Bucket list item shape returned by `GET /api/realms/{realmId}/billing/credit-buckets`.
 *
 * Matches the backend `BucketResponse` (`credit_bucket_handlers.rs`): identity +
 * display + enabled + covered client-app count + the aggregate count of
 * distribution rules targeting the bucket. There is NO registration-pool flag
 * and NO entitlement-mapping count — mapping→bucket binding is expressed
 * through distribution rules (surfaced read-only as `ruleReferences` on the
 * detail response).
 */
export interface CreditBucketListItem {
  id: string
  bucketKey: string
  name: string
  displayOrder: number
  enabled: boolean
  coveredClientAppCount: number
  /** Number of distribution rules (both owners) targeting this bucket. */
  ruleReferenceCount: number
}

// ============================================================================
// UI Helpers — directory navigation / selection / editor
// ============================================================================

/**
 * Parse a bucket id from a `credit-bucket-list-item-${id}` testid attribute.
 *
 * Used by `createBucketViaUI` to return the created id after the directory
 * reloads. The list item wrapper carries the full testid on its root element;
 * Playwright exposes it via `getAttribute('data-testid')`.
 */
export async function parseBucketIdFromListItem(
  page: Page,
  bucketKey: string,
): Promise<string> {
  // The directory list renders one `[data-testid^="credit-bucket-list-item-"]`
  // wrapper per bucket. The bucket_key is not in the testid, so locate the
  // matching item by its visible bucket-key text and read the testid suffix.
  const item = page.locator('[data-testid^="credit-bucket-list-item-"]').filter({
    hasText: bucketKey,
  })
  await expect(item.first()).toBeVisible({ timeout: 10000 })
  const testid = await item.first().getAttribute('data-testid')
  if (!testid) {
    throw new Error(`Could not read data-testid from list item for bucket ${bucketKey}`)
  }
  // Strip the static prefix; the remainder is the bucket id.
  return testid.replace('credit-bucket-list-item-', '')
}

/**
 * Create a Credit Bucket via the directory UI editor.
 *
 * Assumes the caller is already authenticated as a realm admin and on (or able
 * to navigate to) the Credit Bucket directory. Returns the created bucket's id
 * by reading the list-item testid after the directory re-renders.
 *
 * NOTE: this helper performs no assertions on success semantics — it only
 * orchestrates the create flow and resolves the id. The calling test asserts
 * the persistent state (list item visible, registration badge, etc.).
 */
export async function createBucketViaUI(
  page: Page,
  realmId: string,
  options: CreateBucketOptions,
): Promise<string> {
  const cb = SELECTORS.creditBucket

  await page.goto(`/manage/billing/credit-buckets`)
  await expect(page.locator(cb.directoryPage)).toBeVisible()

  await page.locator(cb.newButton).click()
  await expect(page.locator(cb.editor)).toBeVisible()

  await page.locator(cb.editorBucketKey).fill(options.bucketKey)
  await page.locator(cb.editorName).fill(options.name)
  if (options.description !== undefined) {
    await page.locator(cb.editorDescription).fill(options.description)
  }

  // Coverage set — bind client apps via the multiselect. Each click on
  // `${prefix}-item-${id}` toggles selection; the dropdown opens on input focus.
  await bindCoverageSetViaUI(page, options.clientAppIds)

  // Submit and wait for the editor to close (directory re-renders the row).
  await page.locator(cb.editorSubmit).click()
  await expect(page.locator(cb.editor)).toBeHidden({ timeout: 15000 })

  return parseBucketIdFromListItem(page, options.bucketKey)
}

/**
 * Open the editor for an existing bucket (Master-Detail selection).
 *
 * Clicks the list item, which loads the editor on the right pane.
 */
export async function openBucketEditor(
  page: Page,
  bucketId: string,
): Promise<void> {
  const cb = SELECTORS.creditBucket
  await page.locator(cb.listItem(bucketId)).click()
  await expect(page.locator(cb.editor)).toBeVisible({ timeout: 10000 })
}

/**
 * Edit editable fields of an existing bucket and submit.
 *
 * Only fields present in `fields` are touched. Returns nothing — the caller
 * asserts the resulting list-item / detail state.
 */
export async function editBucketViaUI(
  page: Page,
  bucketId: string,
  fields: EditBucketFields,
): Promise<void> {
  const cb = SELECTORS.creditBucket
  await openBucketEditor(page, bucketId)

  if (fields.name !== undefined) {
    await page.locator(cb.editorName).fill(fields.name)
  }
  if (fields.description !== undefined) {
    await page.locator(cb.editorDescription).fill(fields.description)
  }
  if (fields.enabled !== undefined) {
    const enabledSwitch = page.locator(cb.editorEnabled)
    const wantChecked = fields.enabled ? 'checked' : 'unchecked'
    // Read the current state; if it doesn't match the target, click and wait
    // for Radix Switch's `data-state` to transition before continuing. The
    // transition is what makes the form's `field.state.value` flip; without
    // waiting, a fast submit can race past the React state update and persist
    // the OLD value (the badge / list refresh then reflects no change).
    let state = await enabledSwitch.getAttribute('data-state')
    if (state !== wantChecked) {
      await enabledSwitch.click()
      await expect(async () => {
        state = await enabledSwitch.getAttribute('data-state')
        expect(state).toBe(wantChecked)
      }).toPass({ timeout: 3000 })
    }
  }

  await page.locator(cb.editorSubmit).click()
  // Wait for the PUT to settle so the list/detail refetch completes before the
  // caller reads back state. The editor stays open on update (the directory
  // refetches list/detail via `onSaved`); a stable persistent signal is the
  // editor remaining visible with no submission error, so just confirm the
  // editor is still mounted after the save resolves.
  await expect(page.locator(cb.editor)).toBeVisible({ timeout: 5000 })
}

/**
 * Toggle the `enabled` flag on a bucket via the editor switch.
 */
export async function toggleBucketEnabledViaUI(
  page: Page,
  bucketId: string,
): Promise<void> {
  const cb = SELECTORS.creditBucket
  await openBucketEditor(page, bucketId)
  await page.locator(cb.editorEnabled).click()
  await page.locator(cb.editorSubmit).click()
}

// ============================================================================
// UI Helpers — coverage set multiselect
// ============================================================================

/**
 * Toggle client apps in the coverage multiselect so the given set is bound.
 *
 * The multiselect is a command-palette: focus the search input, then click each
 * `${prefix}-item-${id}` row to toggle membership. Idempotent in the sense that
 * re-clicking an already-selected item deselects it — callers should pass the
 * full desired set each time.
 *
 * Exposed separately so DE-D02 can drive coverage-set edits independently of
 * full bucket creation.
 */
export async function bindCoverageSetViaUI(
  page: Page,
  clientAppIds: string[],
): Promise<void> {
  const cb = SELECTORS.creditBucket
  if (clientAppIds.length === 0) {
    // Coverage set must be non-empty on create (400 from backend). Nothing to
    // toggle here — the caller is responsible for the validation assertion.
    return
  }

  // Open the command palette by clicking the combobox TRIGGER (the search
  // input lives inside Radix `PopoverContent`, which only renders after the
  // `PopoverTrigger` button is activated). Clicking the search input directly
  // times out because the input does not exist in the DOM until the popover
  // opens. The trigger carries the prefix testid `bucket-coverage-multiselect`.
  await page.locator(cb.coverageMultiselect).click()
  const search = page.locator(cb.coverageMultiselectSearch)
  await expect(search).toBeVisible({ timeout: 5000 })
  await search.click()

  for (const appId of clientAppIds) {
    const item = page.locator(cb.coverageMultiselectItem(appId))
    await expect(item).toBeVisible({ timeout: 5000 })
    await item.click()
  }

  // Close the popover via Escape. `search.blur()` does NOT close a Radix
  // Popover — without this the popover stays open and obscures / intercepts
  // the editor submit click, leading to "element detached" retries.
  await page.keyboard.press('Escape')
  await expect(search).toBeHidden({ timeout: 2000 })
}

// ============================================================================
// UI Helpers — destructive delete flow
// ============================================================================

/**
 * Open the delete confirm dialog for a bucket (does NOT confirm).
 *
 * The `credit-bucket-delete-button` is rendered in the right-pane editor area
 * (credit-bucket-directory-page.tsx) ONLY when a bucket is selected and its
 * editor is visible — NOT inside the left-column list item. This helper opens
 * the editor for the bucket first, then clicks the delete button.
 */
export async function requestDeleteBucketViaUI(
  page: Page,
  bucketId: string,
): Promise<void> {
  const cb = SELECTORS.creditBucket
  await openBucketEditor(page, bucketId)
  await page.locator(cb.deleteButton).click()
  await expect(page.locator(cb.deleteConfirmDialog)).toBeVisible({ timeout: 5000 })
}

/**
 * Confirm the bucket delete dialog and classify the outcome.
 *
 * On success the dialog closes. On 409 `bucket_in_use` the dialog stays open
 * and renders `delete-bucket-error-message`. This helper does not assert; it
 * returns the outcome so the caller can assert either branch.
 *
 * @param page Playwright page (must have `requestDeleteBucketViaUI` called first)
 * @returns `{ success: true }` or `{ success: false, errorCode, errorMessage }`
 */
export async function confirmDeleteBucket(
  page: Page,
): Promise<DeleteBucketResult> {
  const cb = SELECTORS.creditBucket
  const dialog = page.locator(cb.deleteConfirmDialog)
  await expect(dialog).toBeVisible()

  await page.locator(cb.deleteConfirmButton).click()

  // Two terminal states, raced: dialog hidden (success) or error message
  // visible (409 bucket_in_use). Resolve whichever fires within the timeout.
  const errorLocator = page.locator(cb.deleteErrorMessage)
  let outcome: DeleteBucketResult
  try {
    await Promise.race([
      expect(dialog).toBeHidden({ timeout: 10000 }),
      expect(errorLocator).toBeVisible({ timeout: 10000 }),
    ])
  } catch {
    // Neither terminal state rendered — treat as failure with no code.
    return { success: false }
  }

  const errorVisible = await errorLocator.isVisible().catch(() => false)
  if (errorVisible) {
    const rawMessage = (await errorLocator.textContent()) || ''
    return {
      success: false,
      errorCode: classifyDeleteErrorCode(rawMessage),
      errorMessage: rawMessage.trim(),
    }
  }
  return { success: true }
}

/**
 * Map a human-readable error message to its backend code.
 *
 * The frontend renders a localized message for `bucket_in_use`; the helper
 * recognizes the design-documented code substring if present, and otherwise
 * returns `undefined` (caller should assert on the raw message for stability).
 */
function classifyDeleteErrorCode(message: string): string | undefined {
  const lower = message.toLowerCase()
  if (lower.includes('bucket_in_use')) return 'bucket_in_use'
  if (lower.includes('in use') || lower.includes('active subscription')) {
    return 'bucket_in_use'
  }
  return undefined
}

// ============================================================================
// API Helpers — admin-authenticated bucket provisioning
// ============================================================================

/**
 * Resolve the backend base URL the same way other helpers do.
 *
 * Mirrors the resolution in `grant-points-helpers.ts::createTestApiKeyWithPermission`.
 */
function backendBaseUrl(): string {
  return (
    process.env.API_BASE_URL ||
    process.env.BASE_URL?.replace(/:\d+/, ':8080') ||
    'http://localhost:8080'
  )
}

/**
 * Build a Bearer-authenticated `APIRequestContext` for the realm admin.
 *
 * ⚠️ Auth model (corrected): the `/api/realms/{realmId}/billing/credit-buckets`
 * admin endpoints are mounted under `inject_token_identity`
 * (`backend/api-base/.../identity_middleware.rs`), which ONLY reads the
 * `Authorization: Bearer` header — it ignores cookies entirely. This project's
 * access token is held purely in-memory in the frontend
 * (`frontend/src/stores/auth-store.ts`) and injected per-request by the SPA
 * fetch interceptor. `page.context().request` shares only the browser
 * context's cookies and does NOT run the SPA interceptor, so it carries no
 * Bearer → 401 `"missing bearer token"` (the failure captured in run
 * `demo-run-all-20260801-103017`).
 *
 * This helper logs the realm admin in inside an ISOLATED disposable browser
 * context (spawned from `page`'s browser) so it does NOT clobber the caller's
 * `page` session (cookies, in-memory token, current route). The captured
 * admin-web-console access token (`LoginPage.loginAsAdmin` awaits
 * `/api/auth/browser-token/switch-client` and stores the post-switch token) is
 * used to mint an independent `APIRequestContext`. The browser context is
 * closed inside this helper; the returned context keeps a copy of the token in
 * its `extraHTTPHeaders` and remains usable until the caller disposes it.
 *
 * `realmId` selects credentials via `REALM_ADMINS`, falling back to
 * `DEMO_ADMIN`. Mirrors `createAdminBearerContext(browser, realmId)` in
 * `realm-admin/user-sessions-management-demo.e2e.ts`.
 *
 * @returns A disposable Bearer-authenticated API request context. The caller
 *          MUST `await context.dispose()` (typically in a try/finally) once
 *          it is no longer needed.
 */
export async function createAdminBearerContext(
  page: Page,
  realmId: string,
): Promise<APIRequestContext> {
  const browser = page.context().browser()
  if (!browser) {
    throw new Error(
      'createAdminBearerContext: page.context().browser() returned null — ' +
        'a Browser is required to spawn an isolated admin login context.',
    )
  }
  const context = await browser.newContext()
  try {
    const adminPage = await context.newPage()
    await clearSessionData(adminPage)
    const loginPage = new LoginPage(adminPage)
    const creds = REALM_ADMINS[realmId] || DEMO_ADMIN
    await loginPage.loginAsAdmin(creds.email, creds.password, realmId)
    return createBearerApiContext(loginPage.getAccessToken())
  } finally {
    // Closing the page context does NOT invalidate the captured access token
    // (the token family lives server-side in Redis and is independent of any
    // browser context). The returned APIRequestContext keeps a copy of the
    // token in its extraHTTPHeaders and remains usable.
    await context.close().catch(() => {})
  }
}

/**
 * Create a Credit Bucket via the admin HTTP API.
 *
 * Issues the request through a Bearer-authenticated `APIRequestContext`. If
 * the caller supplies `apiContext`, it is used directly (and owned/disposed by
 * the caller). When omitted, a one-shot admin Bearer context is built from
 * `page`'s browser (see {@link createAdminBearerContext}) and disposed inside
 * this helper — see that helper's docstring for why cookie-only auth does not
 * work here. Mirrors the `_http_json` pattern in `scripts/lib/demo_seed.py`
 * (status check + parsed body).
 *
 * Route: `POST /api/realms/{realmId}/billing/credit-buckets`.
 * Auth: Realm Admin with `points.manage`.
 *
 * @throws on network failure or unexpected (non-2xx, non-409) status.
 */
export async function createBucketViaApi(
  page: Page,
  realmId: string,
  payload: CreateBucketApiPayload,
  apiContext?: APIRequestContext,
): Promise<{ status: number; body: CreditBucketListItem | Record<string, unknown> }> {
  const url = `${backendBaseUrl()}/api/bill/credit-buckets`
  const ownsContext = !apiContext
  const requestContext = apiContext ?? (await createAdminBearerContext(page, realmId))
  try {
    const response = await requestContext.post(url, { data: payload })

    const body = await parseJsonBody(response)
    if (!response.ok() && response.status() !== 409) {
      throw new Error(
        `createBucketViaApi failed: status=${response.status()} body=${JSON.stringify(body)}`,
      )
    }
    return { status: response.status(), body: body as CreditBucketListItem | Record<string, unknown> }
  } finally {
    if (ownsContext) {
      await requestContext.dispose().catch(() => {})
    }
  }
}

/**
 * List Credit Buckets via the admin HTTP API.
 *
 * Issues the request through a Bearer-authenticated `APIRequestContext`. If
 * the caller supplies `requestContext` (e.g. built from a `loginPage` it
 * already holds), it is used directly and owned/disposed by the caller. When
 * omitted, a one-shot admin Bearer context is built from `page`'s browser (see
 * {@link createAdminBearerContext}) and disposed inside this helper — see that
 * helper's docstring for why cookie-only auth does not work here (the default
 * previously was `page.context().request`, which carries no Bearer → 401).
 *
 * Route: `GET /api/realms/{realmId}/billing/credit-buckets`.
 * Auth: Realm Admin with `points.manage` (view).
 */
export async function listBucketsViaApi(
  page: Page,
  realmId: string,
  requestContext?: APIRequestContext,
): Promise<CreditBucketListItem[]> {
  const url = `${backendBaseUrl()}/api/bill/credit-buckets`
  const ownsContext = !requestContext
  const apiContext = requestContext ?? (await createAdminBearerContext(page, realmId))
  try {
    const response = await apiContext.get(url)
    const body = await parseJsonBody(response)
    if (!response.ok()) {
      throw new Error(
        `listBucketsViaApi failed: status=${response.status()} body=${JSON.stringify(body)}`,
      )
    }
    // Backend wraps list responses either as a bare array or as { items: [...] }.
    if (Array.isArray(body)) {
      return body as CreditBucketListItem[]
    }
    if (body && Array.isArray((body as { items?: unknown }).items)) {
      return (body as { items: CreditBucketListItem[] }).items
    }
    if (body && Array.isArray((body as { data?: unknown }).data)) {
      return (body as { data: CreditBucketListItem[] }).data
    }
    throw new Error(
      `listBucketsViaApi returned unexpected body shape: ${JSON.stringify(body)}`,
    )
  } finally {
    if (ownsContext) {
      await apiContext.dispose().catch(() => {})
    }
  }
}

async function parseJsonBody(
  response: APIResponse,
): Promise<Record<string, unknown> | unknown[]> {
  const contentType = response.headers()['content-type'] || ''
  if (contentType.includes('application/json')) {
    try {
      return (await response.json()) as Record<string, unknown> | unknown[]
    } catch {
      return { raw: await response.text() }
    }
  }
  return { raw: await response.text() }
}
