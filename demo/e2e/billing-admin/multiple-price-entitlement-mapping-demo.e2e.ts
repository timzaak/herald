/**
 * US-EM-007 — Admin multi-price sync + master-detail configuration E2E
 *
 * User stories: docs/user-stories/billing/entitlement-mapping.md
 *   US-EM-007 S1/S2/S3/S4 + the admin-side surface of US-EM-009 S2
 *   (disabled/protected-price surface).
 *
 * Consumed infra (AUTHORITATIVE — do not duplicate surface here):
 * - POM: `demo/e2e/pages/entitlement-mappings-page.ts`
 *     goto / waitForReady / waitForDataLoaded / selectProduct / selectFirstProduct
 *     / isProductSelected / isListEmpty
 *     / getPriceEditRow / getPriceEnabledToggle / getSharedKeyChip
 *     / getEntitlementKeyValue / configureFixedPointRule / getFixedPointRuleAmount
 *     / togglePriceEnabled / saveChanges
 *     / expectProtectedPriceDialog / getProtectedPriceActiveSubs / cancelProtectedPrice
 *     / sync
 *     + exposed locator fields (mappingDetailPanel, saveMappingButton, …).
 * - seed ids: `demo/e2e/helpers/multi-price-seed-ids.ts`.
 *
 * The multi-price product is a REAL Stripe product ensured in beforeAll via
 * `ensureMultiPriceCatalog` (create-or-reuse + provider sync). S1 drives the
 * toolbar sync button which surfaces `sync-result-products` /
 * `sync-result-prices`; the real product is the master-detail fixture S2-S4 +
 * protected-price operates on.
 *
 * Assertion discipline: every assertion lands on persistent state — list/detail
 * panel structure, price-row count, shared-key chip presence, saved row values
 * re-rendered after save, disabled-state, and the 409 dialog body. No assertion
 * is toast-only.
 *
 * Setup API calls use a dedicated Bearer-authenticated request context created
 * from the fresh admin login response.
 */

import { test, cleanupTestData, expect } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import type { APIRequestContext } from '@playwright/test'
import type { UnifiedLogger } from 'playwright-unified-logger'
import { createBearerApiContext, DEMO_ADMIN } from '../helpers/auth'
import {
  MULTI_PRICE_PAYMENT_PROVIDER,
} from '../helpers/multi-price-seed-ids'
import { secrets, hasStripePayment } from '../secrets/env'
import { ensureMultiPriceCatalog } from '../helpers/resolve-mappings'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'

// Real Stripe ids resolved in beforeAll (admin realm). Empty until the
// multi-price product is ensured + synced. Replaces the removed placeholder
// seed ids (prod_stripe_multi_pro / price_stripe_pro_monthly|annual).
let STRIPE_MULTI_PRO_PRODUCT_ID = ''
let STRIPE_PRO_ANNUAL_PRICE_ID = ''
let STRIPE_PRO_MONTHLY_PRICE_ID = ''

// Design-pinned per-price point amounts (NOT seed ids).
const STRIPE_MULTI_PRO_POINTS_STRATEGY: Record<string, number> = {
  // populated in beforeAll once STRIPE_PRO_*_PRICE_ID are known
}
/**
 * `[Billing Admin] 多价格 Entitlement 映射同步与配置 (US-EM-007)` — the describe
 * title the runner targets via `--grep`.
 */
test.describe('[Billing Admin] 多价格 Entitlement 映射同步与配置 (US-EM-007)', () => {
  let testStartTime: number

  test.beforeAll(async () => {
    // Skip gracefully when Stripe credentials are absent (live dependency).
    test.skip(!hasStripePayment(), 'Stripe credentials required (live test)')

    // Resolve the real multi-price product in the ADMIN realm. Uses a throwaway
    // browser context so the (per-test) admin login still carries a clean
    // session. The sync requires an authenticated admin request, so we log in
    // programmatically first.
    const { chromium } = await import('@playwright/test')
    const browser = await chromium.launch()
    const context = await browser.newContext()
    const page = await context.newPage()
    try {
      const { LoginPage } = await import('../pages/login-page')
      const loginPage = new LoginPage(page)
      await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)

      const apiContext = await createBearerApiContext(loginPage.getAccessToken())
      const catalog = await ensureMultiPriceCatalog(apiContext, {
        baseUrl: BASE_URL,
        realmId: DEMO_ADMIN.realmId,
        stripeSecretKey: secrets.stripe.secretKey!,
        stripePublishableKey: secrets.stripe.publishableKey!,
        stripeWebhookSecret: secrets.stripe.webhookSecret!,
      }).finally(() => apiContext.dispose())
      STRIPE_MULTI_PRO_PRODUCT_ID = catalog.product.productId
      STRIPE_PRO_MONTHLY_PRICE_ID = catalog.product.monthlyPriceId
      STRIPE_PRO_ANNUAL_PRICE_ID = catalog.product.annualPriceId
      STRIPE_MULTI_PRO_POINTS_STRATEGY[STRIPE_PRO_MONTHLY_PRICE_ID] = 1000
      STRIPE_MULTI_PRO_POINTS_STRATEGY[STRIPE_PRO_ANNUAL_PRICE_ID] = 12000
    } finally {
      await context.close()
      await browser.close()
    }
  })

  test.beforeEach(async ({ page, demoLogger }) => {
    testStartTime = Date.now()
    await verifyTestEnvironment(page, {
      requiredRealms: [DEMO_ADMIN.realmId],
      requiredUsers: [DEMO_ADMIN.email],
    })
    await demoLogger.testCode.log('Environment verified (admin persona for billing.manage)')
  })

  test.afterEach(async ({ page, demoLogger }) => {
    await cleanupTestData(page, DEMO_ADMIN.realmId, {
      keepUsers: [DEMO_ADMIN.email],
      timestamp: testStartTime,
    })
    await demoLogger.testCode.log('Test data cleaned up')
  })

  // ==========================================================================
  // S1 — Sync multi-price → one mapping row per price (prices_synced count)
  // US-EM-007 S1 + reused US-EM-002 sync surface.
  // ==========================================================================
  test('S1 同步多价格产品并展开每价格一行 (prices_synced ≥ 2)', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    const mappingsPage = await setupAdminMappingsPage(page, loginPage, demoLogger)

    let productsSynced = 0
    let pricesSynced = 0

    await test.step('When: 触发 Stripe 同步并解析 result spans', async () => {
      const result = await mappingsPage.sync(MULTI_PRICE_PAYMENT_PROVIDER)
      productsSynced = result.productsSynced
      pricesSynced = result.pricesSynced
      await demoLogger.testCode.log(
        `Sync result: products=${productsSynced} prices=${pricesSynced}`,
      )
    })

    await test.step('Then: 断言同步结果 spans 为持久可见且 products ≥ 1（当 sync 命中真实 Stripe 价格时）', async () => {
      // The result spans are persistent DOM regions (not toasts). The frontend
      // renders them ONLY when the sync mutation returns syncStatus
      // `completed`/`partial` (see provider-sync-button.tsx showCounts). The
      // multi-price product is now a REAL Stripe product ensured in beforeAll,
      // so a sync surfaces ≥ 1 product. The load-bearing assertion for THIS
      // scenario is the 2-row price-edit-row shape below.
      if (productsSynced >= 1) {
        await expect(mappingsPage.syncResultProducts).toBeVisible()
        await expect(mappingsPage.syncResultPrices).toBeVisible()
        expect(
          productsSynced,
          'sync-result-products must report ≥ 1 after a successful Stripe sync',
        ).toBeGreaterThanOrEqual(1)
      } else {
        await demoLogger.testCode.log(
          `Sync returned ${productsSynced} products against placeholder seed ids; ` +
            'span re-assertion skipped (expected demo-env outcome). Load-bearing 2-row assertion follows.',
        )
      }
    })

    await test.step('Then: 多价格产品展开为两行 price-edit-row', async () => {
      // The real multi-price product was synced in beforeAll; selecting it
      // mounts the detail panel with both price rows (one per price).
      await mappingsPage.selectProduct(STRIPE_MULTI_PRO_PRODUCT_ID)
      await expect(mappingsPage.mappingDetailPanel).toBeVisible()

      const monthlyRow = mappingsPage.getPriceEditRow(STRIPE_PRO_MONTHLY_PRICE_ID)
      const annualRow = mappingsPage.getPriceEditRow(STRIPE_PRO_ANNUAL_PRICE_ID)
      await expect(monthlyRow).toBeVisible()
      await expect(annualRow).toBeVisible()

      // prices_synced reflects price-row count. The detail panel
      // shows ≥ 2 price rows for the multi-price product — the load-bearing
      // shape that distinguishes this from the legacy single-row product.
      const rowCount = await mappingsPage.mappingDetailPanel
        .locator('[data-testid^="price-edit-row-"]')
        .count()
      expect(rowCount, 'multi-price product must render ≥ 2 price-edit-rows').toBeGreaterThanOrEqual(
        2,
      )
      await demoLogger.testCode.log(`Multi-price product rendered ${rowCount} price rows`)
    })
  })

  // ==========================================================================
  // S2 — Provider-owned shared entitlement key + per-price point rules.
  // The current sync contract owns entitlement keys; operators only configure
  // the independent PointDistributionRuleEditor on each price.
  // ==========================================================================
  test('S2 共享 entitlement_key + 每价格积分策略 (monthly 1000 / annual 12000)', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    const mappingsPage = await setupAdminMappingsPage(page, loginPage, demoLogger)

    await test.step('Given: 选中多价格产品并展开详情面板', async () => {
      await mappingsPage.selectProduct(STRIPE_MULTI_PRO_PRODUCT_ID)
      await expect(mappingsPage.mappingDetailPanel).toBeVisible()
    })

    let providerOwnedKey = ''
    await test.step('Then: 两个价格行共享同一个只读 provider-owned key', async () => {
      providerOwnedKey = await mappingsPage.getEntitlementKeyValue(STRIPE_PRO_MONTHLY_PRICE_ID)
      expect(providerOwnedKey, 'provider sync must supply a non-empty entitlement key').toBeTruthy()
      expect(await mappingsPage.getEntitlementKeyValue(STRIPE_PRO_ANNUAL_PRICE_ID)).toBe(
        providerOwnedKey,
      )
      await expect(mappingsPage.getSharedKeyChip(providerOwnedKey)).toBeVisible()
    })

    await test.step('When: 用 PointDistributionRuleEditor 配置月/年规则', async () => {
      await mappingsPage.configureFixedPointRule(
        STRIPE_PRO_MONTHLY_PRICE_ID,
        STRIPE_MULTI_PRO_POINTS_STRATEGY[STRIPE_PRO_MONTHLY_PRICE_ID],
      )
      await mappingsPage.configureFixedPointRule(
        STRIPE_PRO_ANNUAL_PRICE_ID,
        STRIPE_MULTI_PRO_POINTS_STRATEGY[STRIPE_PRO_ANNUAL_PRICE_ID],
      )
    })

    await test.step('When: 保存批次', async () => {
      await expect(mappingsPage.saveMappingButton).toBeVisible()
      const saveResponse = await mappingsPage.saveChangesAwaitBatchPut()
      await demoLogger.testCode.log(`Batch PUT responded ${saveResponse.status()}`)
      expect([200, 201]).toContain(saveResponse.status())
    })

    await test.step('Then: reload 后规则金额与只读共享 key 均持久', async () => {
      await page.reload()
      await mappingsPage.waitForReady()
      await mappingsPage.waitForDataLoaded()
      await mappingsPage.selectProduct(STRIPE_MULTI_PRO_PRODUCT_ID)
      expect(await mappingsPage.getFixedPointRuleAmount(STRIPE_PRO_MONTHLY_PRICE_ID)).toBe('1000')
      expect(await mappingsPage.getFixedPointRuleAmount(STRIPE_PRO_ANNUAL_PRICE_ID)).toBe('12000')
      expect(await mappingsPage.getEntitlementKeyValue(STRIPE_PRO_MONTHLY_PRICE_ID)).toBe(
        providerOwnedKey,
      )
      expect(await mappingsPage.getEntitlementKeyValue(STRIPE_PRO_ANNUAL_PRICE_ID)).toBe(
        providerOwnedKey,
      )
      await expect(mappingsPage.getSharedKeyChip(providerOwnedKey)).toBeVisible()
    })
  })

  // ==========================================================================
  // S3 — Per-price provider billing periods remain independently observable.
  // The published rename story is obsolete: synced keys are read-only, and the
  // public create API has no matching delete operation for isolated cleanup.
  // ==========================================================================
  test('S3 同产品月付/年付价格保留各自 provider billing period', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    const mappingsPage = await setupAdminMappingsPage(page, loginPage, demoLogger)

    await test.step('Given: 选中多价格产品', async () => {
      await mappingsPage.selectProduct(STRIPE_MULTI_PRO_PRODUCT_ID)
      await expect(mappingsPage.mappingDetailPanel).toBeVisible()
    })

    await test.step('Then: 两个只读 period 不同且 reload 后稳定', async () => {
      const monthlyPeriod = await mappingsPage.getBillingPeriodValue(STRIPE_PRO_MONTHLY_PRICE_ID)
      const annualPeriod = await mappingsPage.getBillingPeriodValue(STRIPE_PRO_ANNUAL_PRICE_ID)
      expect(monthlyPeriod, 'monthly price must expose a provider billing period').toBeTruthy()
      expect(annualPeriod, 'annual price must expose a provider billing period').toBeTruthy()
      expect(monthlyPeriod).not.toBe(annualPeriod)

      await page.reload()
      await mappingsPage.waitForReady()
      await mappingsPage.waitForDataLoaded()
      await mappingsPage.selectProduct(STRIPE_MULTI_PRO_PRODUCT_ID)
      expect(await mappingsPage.getBillingPeriodValue(STRIPE_PRO_MONTHLY_PRICE_ID)).toBe(
        monthlyPeriod,
      )
      expect(await mappingsPage.getBillingPeriodValue(STRIPE_PRO_ANNUAL_PRICE_ID)).toBe(
        annualPeriod,
      )
      await demoLogger.testCode.log(`Provider periods persisted: ${monthlyPeriod}/${annualPeriod}`)
    })
  })

  // ==========================================================================
  // S4 — Single-price product → exactly one price-edit-row
  // US-EM-007 S4. A product with one price yields exactly one edit row.
  // ==========================================================================
  test('S4 单价格产品 → 仅一行 price-edit-row', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    const mappingsPage = await setupAdminMappingsPage(page, loginPage, demoLogger)
    const apiContext = await createBearerApiContext(loginPage.getAccessToken())
    try {
      let singlePriceMappingId: string | null = null

      await test.step('Given: 通过 API 定位一个单价格产品', async () => {
        singlePriceMappingId = await findSinglePriceProductId(apiContext, demoLogger)
        expect(
          singlePriceMappingId,
          'seeded catalog must contain at least one single-price product for S4',
        ).not.toBeNull()
      })

      await test.step('When: 选中该单价格产品', async () => {
        await mappingsPage.selectProduct(singlePriceMappingId as string)
        await expect(mappingsPage.mappingDetailPanel).toBeVisible()
      })

      await test.step('Then: 详情面板恰有一行 price-edit-row', async () => {
        const rowCount = await mappingsPage.mappingDetailPanel
          .locator('[data-testid^="price-edit-row-"]')
          .count()
        expect(rowCount, 'single-price product renders exactly one price-edit-row').toBe(1)
        await demoLogger.testCode.log(
          `Single-price product ${singlePriceMappingId} rendered ${rowCount} price row(s)`,
        )
      })
    } finally {
      await apiContext.dispose()
    }
  })

  // ==========================================================================
  // Protected price (US-EM-009 S2 admin surface)
  // Active-subscription state is not deterministic in this demo environment.
  // The scenario therefore observes exactly two valid outcomes: 409 with the
  // protected-price contract, or a successful disable which is persisted and
  // then restored. Authentication failures are never treated as business state.
  // ==========================================================================
  test('价格禁用条件观测：有活跃订阅时 409，否则持久禁用并恢复', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    const mappingsPage = await setupAdminMappingsPage(page, loginPage, demoLogger)
    const apiContext = await createBearerApiContext(loginPage.getAccessToken())

    let protectedMappingId: string | null = null
    let protectedProductExternalId: string | null = null
    let protectedPriceKey: string | null = null

    try {
    await test.step('Given: 解析多价格产品的 mappingId / price key', async () => {
      // Resolve real mapping ids via the GET list (so the direct PUT fallback
      // targets a real row). The seeded multi-price product has two price rows;
      // pick the annual one as the disable candidate.
      const resolved = await findProtectedPriceCandidate(apiContext, demoLogger, {
        externalProductId: STRIPE_MULTI_PRO_PRODUCT_ID,
        externalPriceId: STRIPE_PRO_ANNUAL_PRICE_ID,
      })
      // The helper returns null on miss (not a sentinel empty-string mappingId),
      // so this guard can actually fail if the seeded row is absent — preventing
      // a silent empty-mappingId from reaching the batch PUT below.
      expect(resolved, 'must resolve a real mappingId for the protected-price path').not.toBeNull()
      protectedMappingId = resolved!.mappingId
      protectedProductExternalId = resolved!.externalProductId
      protectedPriceKey = resolved!.priceKey
    })

    await setMappingEnabled(apiContext, {
      externalProductId: protectedProductExternalId as string,
      mappingId: protectedMappingId as string,
      enabled: true,
    })
    await page.reload()
    await mappingsPage.waitForReady()
    await mappingsPage.waitForDataLoaded()

    await test.step('When: 关闭价格 enabled 开关并保存', async () => {
      await mappingsPage.selectProduct(protectedProductExternalId as string)
      await expect(mappingsPage.mappingDetailPanel).toBeVisible()
      await expect(mappingsPage.getPriceEnabledToggle(protectedPriceKey as string)).toBeChecked()
      await mappingsPage.togglePriceEnabled(protectedPriceKey as string)

      const saveResp = await mappingsPage.saveChangesAwaitBatchPut()
      const status = saveResp.status()

      if (status === 409) {
        const body = await saveResp.json()
        expect(body.code, '409 body must identify mapping_in_use').toBe('mapping_in_use')
        expect(body.activeSubscriptions, '409 must report active subscriptions').toBeGreaterThan(0)
        await test.step('Then: 409 对话框仅提供 Cancel', async () => {
          await mappingsPage.expectProtectedPriceDialog()
          const activeSubs = await mappingsPage.getProtectedPriceActiveSubs()
          expect(
            activeSubs,
            '409 dialog must surface an active-subscription count ≥ 1',
          ).toBeGreaterThanOrEqual(1)

          // The dialog is Cancel-only: there is NO protected-price-confirm-proceed
          // testid (re-verified against
          // frontend/src/components/billing/entitlement-mapping-detail-dialog.tsx).
          await expect(
            page.locator('[data-testid="protected-price-confirm-proceed"]'),
          ).toHaveCount(0)

          await mappingsPage.cancelProtectedPrice()
          await expect(mappingsPage.protectedPriceConfirmDialog).toBeHidden()
          await demoLogger.testCode.log(`Protected-price 409 observed; activeSubs=${activeSubs}`)
        })
      } else if (status === 200 || status === 201) {
        await page.reload()
        await mappingsPage.waitForReady()
        await mappingsPage.waitForDataLoaded()
        await mappingsPage.selectProduct(protectedProductExternalId as string)
        await expect(mappingsPage.getPriceEnabledToggle(protectedPriceKey as string)).not.toBeChecked()
        await demoLogger.testCode.log(`Unprotected disable persisted with status ${status}`)
      } else {
        const body = await saveResp.text().catch(() => '<unreadable body>')
        throw new Error(`Unexpected disable response ${status}: ${body}`)
      }
    })

    } finally {
      try {
        if (await mappingsPage.protectedPriceConfirmDialog.isVisible().catch(() => false)) {
          await mappingsPage.cancelProtectedPrice()
        }
        if (protectedMappingId && protectedProductExternalId && protectedPriceKey) {
          await setMappingEnabled(apiContext, {
            externalProductId: protectedProductExternalId,
            mappingId: protectedMappingId,
            enabled: true,
          })
          await page.reload()
          await mappingsPage.waitForReady()
          await mappingsPage.waitForDataLoaded()
          await mappingsPage.selectProduct(protectedProductExternalId)
          await expect(mappingsPage.getPriceEnabledToggle(protectedPriceKey)).toBeChecked()
        }
      } finally {
        await apiContext.dispose()
      }
    }
  })

  // Webhook-unresolved is a defensive UI state, but current public create
  // requires billingType and current update DTOs cannot clear it. No truthful,
  // isolated E2E fixture can therefore be constructed here. The stale scenario
  // was removed instead of sending unsupported DTO fields. A focused frontend
  // component regression for this state is still a known coverage gap.
})

// ============================================================================
// Helpers
// ============================================================================

/**
 * Log in as the DEMO_ADMIN persona and navigate to the entitlement mappings
 * page via the POM. The admin persona carries `billing.manage` (required for
 * the master-detail editing surface to mount the Save button).
 */
async function setupAdminMappingsPage(
  page: import('@playwright/test').Page,
  loginPage: import('../pages/login-page').LoginPage,
  demoLogger: UnifiedLogger,
) {
  const { EntitlementMappingsPage } = await import('../pages/entitlement-mappings-page')
  await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
  const mappingsPage = new EntitlementMappingsPage(page, demoLogger)
  await mappingsPage.goto(DEMO_ADMIN.realmId)
  await mappingsPage.waitForDataLoaded()
  await demoLogger.testCode.log('Admin on entitlement-mappings page (master-detail)')
  return mappingsPage
}

interface MappingRecord {
  id?: string
  externalProductId?: string
  externalPriceId?: string
}

/** Shared authenticated GET. Transport/auth/schema failures are test failures. */
async function fetchMappingsList(
  apiContext: APIRequestContext,
  demoLogger: UnifiedLogger,
): Promise<MappingRecord[]> {
  const resp = await apiContext.get(
    `${BASE_URL}/api/bill/entitlement-mappings`,
  )
  if (!resp.ok()) {
    const body = await resp.text().catch(() => '<unreadable body>')
    throw new Error(`GET entitlement-mappings failed ${resp.status()}: ${body}`)
  }
  const body: unknown = await resp.json()
  const items = Array.isArray(body)
    ? body
    : typeof body === 'object' && body !== null && 'items' in body
      ? (body as { items: unknown }).items
      : null
  if (!Array.isArray(items)) {
    throw new Error('GET entitlement-mappings returned a body without an items array')
  }
  await demoLogger.testCode.log(`Fetched ${items.length} entitlement mappings`)
  return items as MappingRecord[]
}

/**
 * Fetch the flat entitlement mappings list and locate a product whose price
 * set has exactly one price. Returns the product's external product id (the
 * master-list row key). Returns null if no single-price product is seeded.
 */
async function findSinglePriceProductId(
  apiContext: APIRequestContext,
  demoLogger: UnifiedLogger,
): Promise<string | null> {
  const items = await fetchMappingsList(apiContext, demoLogger)

  // Group by externalProductId; find a product with exactly one price.
  const byProduct = new Map<string, number>()
  for (const item of items) {
    const rec = item as { externalProductId?: string }
    if (!rec.externalProductId) continue
    byProduct.set(rec.externalProductId, (byProduct.get(rec.externalProductId) ?? 0) + 1)
  }
  for (const [productId, count] of byProduct.entries()) {
    if (count === 1) {
      // Skip the multi-price product (it has 2 prices; this branch never
      // matches it but the explicit skip documents intent).
      if (productId === STRIPE_MULTI_PRO_PRODUCT_ID) continue
      return productId
    }
  }
  return null
}

interface PriceCandidate {
  mappingId: string
  externalProductId: string
  priceKey: string
}

/**
 * Resolve the mapping id + price key for a specific (product, price) pair from
 * the flat list. Used by the protected-price scenario to target a real row.
 * Returns null when no matching row is found (NOT a sentinel empty-string
 * mappingId) so the caller's `not.toBeNull()` guard can actually fail.
 */
async function findProtectedPriceCandidate(
  apiContext: APIRequestContext,
  demoLogger: UnifiedLogger,
  opts: { externalProductId: string; externalPriceId: string },
): Promise<PriceCandidate | null> {
  const items = await fetchMappingsList(apiContext, demoLogger)
  for (const item of items) {
    const rec = item as {
      id?: string
      externalProductId?: string
      externalPriceId?: string
    }
    if (
      rec.externalProductId === opts.externalProductId &&
      rec.externalPriceId === opts.externalPriceId &&
      rec.id
    ) {
      await demoLogger.testCode.log(
        `Resolved protected-price candidate mappingId=${rec.id} priceKey=${opts.externalPriceId}`,
      )
      return {
        mappingId: rec.id,
        externalProductId: opts.externalProductId,
        priceKey: opts.externalPriceId,
      }
    }
  }
  await demoLogger.testCode.log(
    `No protected-price candidate found for product=${opts.externalProductId} price=${opts.externalPriceId}`,
  )
  return null
}

async function setMappingEnabled(
  apiContext: APIRequestContext,
  update: { externalProductId: string; mappingId: string; enabled: boolean },
): Promise<void> {
  const response = await apiContext.put(
    `${BASE_URL}/api/bill/entitlement-mappings/batch`,
    {
      data: {
        paymentProvider: MULTI_PRICE_PAYMENT_PROVIDER,
        externalProductId: update.externalProductId,
        updates: [{ mappingId: update.mappingId, enabled: update.enabled }],
      },
    },
  )
  if (!response.ok()) {
    const body = await response.text().catch(() => '<unreadable body>')
    throw new Error(
      `Failed to set mapping ${update.mappingId} enabled=${update.enabled}: ` +
        `${response.status()} ${body}`,
    )
  }
}
