/**
 * Points Quota Demo Helpers
 *
 * User stories covered:
 * - US-PU-010: 滚动窗口额度与充值余额的可用性体验
 * - US-PO-009: 配置多时间窗滚动配额
 * - US-FU-005: 免费周期积分改为滚动窗口配额
 */

import { Page, expect, type APIRequestContext, type Locator } from '@playwright/test'
import { SELECTORS } from '../selectors'
import { makeExtApiRequest } from './ext-api-helper'
import { loginAsAdmin, loginWithCredentials } from './auth'
import { registerUser, POINTS_ROUTES } from './points-helpers'
import type { QuotaWindowFixture } from '../fixtures/points-quota.fixtures'

export type { QuotaWindowFixture }

export interface ConsumePointsExtApiBody {
  userId: string
  amount: number
  clientAppId: string
  description?: string
  idempotencyKey?: string
}

export interface ConsumePointsResult {
  status: number
  body: unknown
}

/**
 * Clear all rows in a `MultiWindowQuotaEditor`.
 *
 * Repeatedly clicks the first delete button until no data rows remain.
 */
export async function clearQuotaEditorRows(
  page: Page,
  prefix: string,
): Promise<void> {
  const editor = page.locator(SELECTORS.pointsQuotaEditor.editor(prefix))
  await expect(editor).toBeVisible()

  // Defensive: cap iterations at the component MAX_WINDOWS (8) + margin.
  for (let attempts = 0; attempts < 12; attempts += 1) {
    const firstRow = editor.locator(SELECTORS.pointsQuotaEditor.row(prefix, 0))
    if ((await firstRow.count()) === 0) break

    const deleteButton = editor.locator(
      SELECTORS.pointsQuotaEditor.deleteRow(prefix, 0),
    )
    if ((await deleteButton.count()) === 0) break

    await deleteButton.click()
  }
}

/**
 * Fill a `MultiWindowQuotaEditor` with the supplied window configuration.
 *
 * The editor is normalized to `seconds` before each length input is filled,
 * avoiding surprises from the component's display-unit derivation.
 */
export async function fillQuotaEditorRows(
  page: Page,
  prefix: string,
  windows: QuotaWindowFixture[],
): Promise<void> {
  const editor = page.locator(SELECTORS.pointsQuotaEditor.editor(prefix))
  await expect(editor).toBeVisible()

  for (let index = 0; index < windows.length; index += 1) {
    const rowCount = await editor
      .locator(SELECTORS.pointsQuotaEditor.row(prefix, 0))
      .count()

    if (rowCount <= index) {
      await editor.locator(SELECTORS.pointsQuotaEditor.addButton(prefix)).click()
    }

    const lengthInput = editor.locator(
      SELECTORS.pointsQuotaEditor.lengthRow(prefix, index),
    )
    const unitTrigger = editor.locator(
      SELECTORS.pointsQuotaEditor.unitRow(prefix, index),
    )
    const limitInput = editor.locator(
      SELECTORS.pointsQuotaEditor.limitRow(prefix, index),
    )

    await expect(lengthInput).toBeVisible()
    await expect(unitTrigger).toBeVisible()
    await expect(limitInput).toBeVisible()

    // Normalize to seconds so callers can pass raw windowSeconds.
    await unitTrigger.click()
    await page.getByRole('option', { name: 'seconds' }).click()

    await lengthInput.fill(windows[index].windowSeconds.toString())
    await limitInput.fill(windows[index].limit.toString())
  }
}

/**
 * Create (or overwrite) multi-window quota configuration on an entitlement
 * mapping by driving the NEW `PointDistributionRuleEditor` flow.
 *
 * The entitlement-mappings refactor removed the per-price "Advanced"
 * collapsible + bare `MultiWindowQuotaEditor`. The detail panel now embeds a
 * `PointDistributionRuleEditor` directly inside each `PriceEditRow`; a rule's
 * grant-mode select (`point-rule-mode`) switches between `fixed` and `quota`,
 * and only `quota` mode reveals an embedded `MultiWindowQuotaEditor` whose
 * testid prefix is `point-rule-quota-${key}` (key = `rule.id ?? 'new-${index}'`).
 *
 * Flow: open the product detail panel, add a fresh rule on the FIRST price
 * row's editor (so we own its key), target the seeded `primary-pool` bucket,
 * switch the rule to quota mode, then fill the embedded quota editor and save.
 */
export async function createEntitlementMappingWithQuotaWindows(
  page: Page,
  realmId: string,
  productHint: string,
  windows: QuotaWindowFixture[],
): Promise<void> {
  await loginAsAdmin(page, { realmId, waitNavigation: true })
  await page.goto(`/manage/billing/entitlement-mappings`)
  await expect(page.locator(SELECTORS.multiPriceMapping.page)).toBeVisible()

  const productRow = page.locator(
    SELECTORS.multiPriceMapping.mappingProductRow(productHint),
  )
  await expect(productRow).toBeVisible()
  await productRow.click()
  await expect(page.locator(SELECTORS.multiPriceMapping.mappingDetailPanel)).toBeVisible()

  // Scope to the FIRST price-edit-row's PointDistributionRuleEditor. A product
  // may carry multiple prices, each with its own editor + add/bucket/mode
  // controls, so unscoped lookups would be ambiguous.
  const priceRow = page.locator('[data-testid^="price-edit-row-"]').first()
  await expect(priceRow).toBeVisible()

  // Add a brand-new rule so we own its key. The editor assigns unsaved rules
  // the React `key` `new-${index}` (index = array position); a freshly-added
  // rule is appended, so its index === the pre-add rule count. NOTE the card
  // testid uses the bare index (`point-rule-${index}`), while per-rule child
  // testids and the embedded quota-editor prefix use the `new-${index}` key.
  const ruleList = priceRow.locator('[data-testid="point-rule-list"]')
  await expect(ruleList).toBeVisible()
  const ruleCountBefore = await ruleList.locator(':scope > div').count()
  await priceRow.locator('[data-testid="point-rule-add"]').click()

  // The new rule card is the LAST direct child div of the list (the add
  // button is a sibling outside the list container).
  const newRuleCard = ruleList.locator(':scope > div').last()
  await expect(newRuleCard).toBeVisible()
  const ruleKey = `new-${ruleCountBefore}`

  // Target the seeded primary pool. The demo seed (`scripts/lib/demo_seed.py`)
  // creates the bucket with name "Primary Pool" (key `primary-pool`); the
  // select option text is the bucket `name`, so match it case-insensitively.
  await newRuleCard.locator('[data-testid="point-rule-bucket"]').click()
  await page.getByRole('option', { name: /primary pool/i }).click()

  // Switch grant-mode to quota. The mode select lists `fixed` first and
  // `quota` second (only rendered when allowQuota — true for non-one-time
  // prices). To avoid localization coupling we pick by ORDER: the second
  // option in the open list is `quota`.
  await newRuleCard.locator('[data-testid="point-rule-mode"]').click()
  const quotaOption = page.locator('[role="option"]').nth(1)
  await expect(quotaOption).toBeVisible()
  await quotaOption.click()

  // The embedded MultiWindowQuotaEditor is now visible. Its testid prefix is
  // `point-rule-quota-${key}` (the React key, not the card index). Reuse the
  // unchanged clear/fill helpers with this prefix.
  const prefix = `point-rule-quota-${ruleKey}`
  await expect(page.locator(SELECTORS.pointsQuotaEditor.editor(prefix))).toBeVisible()

  await clearQuotaEditorRows(page, prefix)
  await fillQuotaEditorRows(page, prefix, windows)

  await page.locator(SELECTORS.multiPriceMapping.saveMappingButton).click()
  await page.waitForLoadState('networkidle')
}

/**
 * Consume points through the external API.
 *
 * Thin wrapper around `makeExtApiRequest` for
 * `POST /api/ext/points/{realmId}/consume`.
 */
export async function consumePointsViaExtApi(
  apiKey: string,
  realmId: string,
  body: ConsumePointsExtApiBody,
): Promise<ConsumePointsResult> {
  const { status, body: responseBody } = await makeExtApiRequest({
    apiKey,
    method: 'POST',
    path: `/points/${realmId}/consume`,
    body,
  })

  return { status, body: responseBody }
}

/** Register a new user and assert the realm-default quota windows render. */
export async function registerNewUserWithRealmDefaultQuota(
  page: Page,
  realmId: string,
  email: string,
  password: string = 'password123',
): Promise<void> {
  await page.context().clearCookies()
  await page.evaluate(() => {
    localStorage.clear()
    sessionStorage.clear()
  })
  await registerUser(page, realmId, email, password)
  await loginWithCredentials(page, { realmId, email, password })

  await page.goto(POINTS_ROUTES.USER_POINTS(realmId))
  await expect(page.locator(SELECTORS.pointsUser.page)).toBeVisible()
  await expect(
    page.locator(SELECTORS.pointsUsageDashboard.page),
  ).toBeVisible({ timeout: 15000 })

  // Assert at least one window row exists for each configured window without
  // requiring callers to know the bucket UUID up front.
  const windowRows = page.locator('[data-testid^="points-window-row-"]')
  await expect(windowRows).toHaveCount(2)
}

export function getWindowRow(
  page: Page,
  bucketId: string,
  winKey: string,
): Locator {
  return page.locator(SELECTORS.pointsUsageDashboard.windowRow(bucketId, winKey))
}

/**
 * Read the "remaining" value from a window row.
 *
 * The row renders `remaining / limit · used`; this returns the first integer.
 */
export async function getWindowRemaining(
  page: Page,
  bucketId: string,
  winKey: string,
): Promise<number> {
  const row = getWindowRow(page, bucketId, winKey)
  await expect(row).toBeVisible()
  const text = (await row.textContent()) || ''
  const match = text.match(/([\d,]+)\s*\//)
  return parseAmount(match?.[1])
}

/**
 * Read the resets-in copy from a window row.
 *
 * The dedicated testid is not emitted; the copy is the last text span inside the row.
 */
export async function getWindowResetsIn(
  page: Page,
  bucketId: string,
  winKey: string,
): Promise<string> {
  const row = getWindowRow(page, bucketId, winKey)
  await expect(row).toBeVisible()
  const spans = row.locator('span')
  const count = await spans.count()
  if (count === 0) return ''
  const text = (await spans.last().textContent()) || ''
  return text.trim()
}

/**
 * Resolve the backend base URL for direct wallets-API calls. The frontend
 * proxies through `:3000`, but the API lives on `:8080`; mirrors the other
 * helpers that read backend state out-of-band.
 */
function walletsApiBaseUrl(): string {
  return (
    process.env.API_BASE_URL ||
    process.env.BASE_URL?.replace(/:\d+/, ':8080') ||
    'http://localhost:8080'
  )
}

/**
 * Fetch the wallets-by-bucket response for a realm.
 *
 * Auth note: Herald's frontend uses a Bearer-token auth model — it injects
 * `Authorization: Bearer {accessToken}` from an in-memory store
 * (`frontend/src/lib/api-client.ts`). `page.context().request` carries only
 * cookies, NOT the Bearer header, so it 401s with `"missing bearer token"`
 * (see `createAdminBearerContext` in `bucket-helpers.ts` for the same finding).
 *
 * Endpoint selection: the admin-facing `GET /api/points/{realmId}/wallets`
 * requires `points.manage` (regular users 403), whereas the user-facing
 * `GET /api/user/wallets` requires only the `points.view` (PointsRead) scope
 * every regular user holds and returns the SAME `ListWalletsByBucketResponse`
 * shape (server-side scoped to the caller). So when a Bearer `apiContext` is
 * supplied we hit `/api/user/wallets` (correct for regular users AND admins);
 * the legacy no-context fallback keeps the admin path for backward compat
 * (it 401s on cookies and returns 0, preserving prior behavior).
 *
 * Callers that need real values MUST pass a Bearer-authenticated `apiContext`
 * (e.g. built via `createBearerApiContext(loginPage.getAccessToken())`).
 */
async function fetchWalletsByBucket(
  page: Page,
  realmId: string,
  apiContext?: APIRequestContext,
): Promise<{ bucketId?: string }[] | null> {
  const requestContext = apiContext ?? page.context().request
  // Bearer context → user-scoped endpoint (points.view). Cookie fallback →
  // legacy admin endpoint path (kept for backward compat; 401s → null → 0).
  const url = apiContext
    ? `${walletsApiBaseUrl()}/api/user/wallets`
    : `${walletsApiBaseUrl()}/api/points/${realmId}/wallets`
  const resp = await requestContext.get(url)
  if (!resp.ok()) return null
  const body = await resp.json()
  return (body?.items ?? []) as { bucketId?: string }[]
}

/**
 * Read the demo user's `spendable_from_pool` (topup + registration + granted
 * balances) for a bucket directly from the wallets API.
 *
 * Used by the total-formula test to assert `spendableNow === smallestRemaining
 * + pool` without hard-coding the pool value, which accumulates across demo
 * runs because the ext grant API has no idempotency key.
 *
 * Pass `apiContext` (a Bearer-authenticated request context built from the
 * logged-in user's access token) for correct values — see `fetchWalletsByBucket`.
 */
export async function getSpendableFromPool(
  page: Page,
  realmId: string,
  bucketId: string,
  apiContext?: APIRequestContext,
): Promise<number> {
  const items = await fetchWalletsByBucket(page, realmId, apiContext)
  if (!items) return 0
  const match = (items as {
    bucketId?: string
    spendableFromPool?: number | null
  }[]).find((i) => i.bucketId === bucketId)
  return match?.spendableFromPool ?? 0
}

/**
 * Read the backend-computed effective consumable total for a bucket directly
 * from the wallets API `bucketTotal` field.
 *
 * The backend derives this as `min(window remaining) + pool balance`
 * (see `backend/api-points/src/wallets.rs` — `spendable_from_quota` is the min
 * window remaining, and `bucket_total = spendable_from_quota + pool_sum`), so
 * it is the authoritative "spendable now" total.
 *
 * The dashboard refactor (`PointsUsageDashboard.tsx`) no longer renders an
 * explicit "spendable now" headline with a stable testid — it folds
 * `card.bucketTotal` into internal alert logic only. The total-formula test
 * therefore verifies the invariant server-side via the same wallets response
 * the dashboard derives from, using this helper for the per-bucket total.
 *
 * Pass `apiContext` (a Bearer-authenticated request context built from the
 * logged-in user's access token) for correct values — see `fetchWalletsByBucket`.
 */
export async function getBucketTotal(
  page: Page,
  realmId: string,
  bucketId: string,
  apiContext?: APIRequestContext,
): Promise<number> {
  const items = await fetchWalletsByBucket(page, realmId, apiContext)
  if (!items) return 0
  const match = (items as {
    bucketId?: string
    bucketTotal?: number | null
  }[]).find((i) => i.bucketId === bucketId)
  return match?.bucketTotal ?? 0
}

function parseAmount(text: string | undefined | null): number {
  if (!text) return 0
  const cleaned = text.replace(/[^\d-]/g, '')
  const n = parseInt(cleaned, 10)
  return Number.isNaN(n) ? 0 : n
}
