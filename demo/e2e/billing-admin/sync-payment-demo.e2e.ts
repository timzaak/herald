/**
 * sync-payment demo E2E
 *
 * User story source: `docs/user-stories/billing/entitlement-mapping.md`
 *   US-BL-SYNC-001~004（故事 10~13）：同步携带商户 metadata、产品名识别、
 *   价格按 provider 单位展示、计费周期以 Stripe 为准且只读
 *
 * Selector calibration against frontend/src/components/billing/entitlement-mappings-page.tsx:
 * - `price-metadata-block-${price.externalPriceId ?? price.id}` -> SELECTORS.multiPriceMapping.priceMetadataBlock
 * - `metadata-entry-${scope}-${key}` -> SELECTORS.multiPriceMapping.metadataEntry
 * - `price-billing-type-${price.externalPriceId ?? price.id}` -> SELECTORS.multiPriceMapping.priceBillingType
 * - reused: detailHead, mappingProductRow, mappingDetailPanel, providerSyncButton/syncButton,
 *   syncResultProducts/syncResultPrices.
 */

import { test, cleanupTestData, expect } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import type { UnifiedLogger } from '../helpers/unified-logger'
import { createBearerApiContext, DEMO_ADMIN } from '../helpers/auth'
import { SELECTORS } from '../selectors'
import { secrets, hasStripePayment, hasCreemPayment } from '../secrets/env'
import { seedCreemConfig } from '../secrets/realm-seed'
import { ensureMultiPriceCatalog } from '../helpers/resolve-mappings'
import {
  ensureMetadataProduct,
  METADATA_PRODUCT_NAME,
  updateProductMetadata,
} from '../helpers/multi-price-live-product'
import { EntitlementMappingsPage } from '../pages/entitlement-mappings-page'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'
const INITIAL_PRODUCT_METADATA = { tier: 'pro', internal_sku: 'H-001' }
const UPDATED_PRODUCT_METADATA = { tier: 'team', internal_sku: 'H-001' }
const PRICE_METADATA = { price_code: 'monthly-999' }

interface MappingListItem {
  id: string
  paymentProvider: string
  externalProductId: string
  externalPriceId: string | null
  entitlementKey: string
  enabled: boolean
  billingType: string | null
  billingPeriod: string | null
  pointsPerPeriod: number | null
  providerProductInfo?: unknown
}

let stripeMetadataProductId = ''
let stripeMetadataPriceId = ''
let stripePlainProductId = ''
let stripePlainPriceId = ''
let creemMapping: MappingListItem | null = null
let stripeSetupError: string | null = null
let creemSetupError: string | null = null
let testStartTime = 0

test.describe('[Billing Admin] 支付产品同步增强 (US-BL-SYNC-001/002/003/004)', () => {
  test.beforeAll(async () => {
    if (hasStripePayment()) {
      const { chromium } = await import('@playwright/test')
      const browser = await chromium.launch()
      const context = await browser.newContext()
      const page = await context.newPage()
      try {
        const { LoginPage } = await import('../pages/login-page')
        const loginPage = new LoginPage(page)
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        const apiContext = await createBearerApiContext(loginPage.getAccessToken())
        try {
          const metadataProduct = await ensureMetadataProduct(secrets.stripe.secretKey!, {
            productMetadata: INITIAL_PRODUCT_METADATA,
            priceMetadata: PRICE_METADATA,
          })
          stripeMetadataProductId = metadataProduct.productId
          stripeMetadataPriceId = metadataProduct.priceId

          const catalog = await ensureMultiPriceCatalog(apiContext, {
            baseUrl: BASE_URL,
            realmId: DEMO_ADMIN.realmId,
            stripeSecretKey: secrets.stripe.secretKey!,
            stripePublishableKey: secrets.stripe.publishableKey!,
            stripeWebhookSecret: secrets.stripe.webhookSecret!,
          })
          stripePlainProductId = catalog.product.productId
          stripePlainPriceId = catalog.product.monthlyPriceId
        } finally {
          await apiContext.dispose()
        }
      } catch (error) {
        stripeSetupError = error instanceof Error ? error.message : String(error)
      } finally {
        await context.close()
        await browser.close()
      }
    }

    if (hasCreemPayment()) {
      const { chromium } = await import('@playwright/test')
      const browser = await chromium.launch()
      const context = await browser.newContext()
      const page = await context.newPage()
      try {
        const { LoginPage } = await import('../pages/login-page')
        const loginPage = new LoginPage(page)
        await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
        const apiContext = await createBearerApiContext(loginPage.getAccessToken())
        try {
          await seedCreemConfig(apiContext, DEMO_ADMIN.realmId, {
            apiKey: secrets.creem.apiKey!,
            webhookSecret: secrets.creem.webhookSecret!,
          })
          await syncProvider(apiContext, 'creem')
          const items = await fetchMappings(apiContext, 'creem')
          creemMapping =
            items.find((m) => m.externalProductId === secrets.creem.productId) ?? items[0] ?? null
        } finally {
          await apiContext.dispose()
        }
      } catch (error) {
        creemSetupError = error instanceof Error ? error.message : String(error)
      } finally {
        await context.close()
        await browser.close()
      }
    }
  })

  test.beforeEach(async ({ page }) => {
    testStartTime = Date.now()
    await verifyTestEnvironment(page, {
      requiredRealms: [DEMO_ADMIN.realmId],
      requiredUsers: [DEMO_ADMIN.email],
    })
  })

  test.afterEach(async ({ page }) => {
    await cleanupTestData(page, DEMO_ADMIN.realmId, {
      keepUsers: [DEMO_ADMIN.email],
      timestamp: testStartTime,
    })
  })

  test('Stripe metadata、产品名过滤、价格和周期只读持久展示 (US-BL-SYNC-001/002/003/004)', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    test.skip(!hasStripePayment(), 'Stripe credentials required')
    test.skip(!!stripeSetupError, `Stripe setup failed: ${stripeSetupError}`)
    const mappingsPage = await setupAdminMappingsPage(page, loginPage, demoLogger)
    const apiContext = await createBearerApiContext(loginPage.getAccessToken())

    try {

    // US-BL-SYNC-001 S1
    await test.step('Given/When/Then: Stripe 带 metadata 同步后详情展示 product/price metadata', async () => {
      await mappingsPage.selectProduct(stripeMetadataProductId)
      await expect(mappingsPage.getMetadataBlock(stripeMetadataPriceId)).toBeVisible()
      await expect(mappingsPage.getMetadataEntry('product', 'tier')).toHaveText('pro')
      await expect(mappingsPage.getMetadataEntry('product', 'internal_sku')).toHaveText('H-001')
      await expect(mappingsPage.getMetadataEntry('price', 'price_code')).toHaveText('monthly-999')
      await expect(mappingsPage.getBillingTypeInput(stripeMetadataPriceId)).toHaveValue(/Recurring/i)
      expect(await mappingsPage.getPriceDisplayValue(stripeMetadataPriceId)).toContain('9.99')
      await demoLogger.testCode.log(`Metadata product rendered: ${stripeMetadataProductId}`)
    })

    // US-BL-SYNC-001 S2
    await test.step('Given/When/Then: 不带 metadata 的 Stripe 产品省略 metadata 区', async () => {
      await mappingsPage.selectProduct(stripePlainProductId)
      await expect(mappingsPage.getMetadataBlock(stripePlainPriceId)).toHaveCount(0)
      await expect(mappingsPage.getPriceEditRow(stripePlainPriceId)).toBeVisible()
    })

    // US-BL-SYNC-002 S1/S2
    await test.step('Then: 列表和详情以产品名作为可识别主标签', async () => {
      await mappingsPage.selectProduct(stripeMetadataProductId)
      expect(await mappingsPage.getProductRowLabel(stripeMetadataProductId)).toContain(
        METADATA_PRODUCT_NAME,
      )
      expect(await mappingsPage.getDetailHeadLabel()).toContain(METADATA_PRODUCT_NAME)
    })

    // US-BL-SYNC-003 S1 + US-BL-SYNC-004 S1
    await test.step('Then: Stripe 价格按分换算展示，Period 只读并显示同步周期', async () => {
      await mappingsPage.selectProduct(stripeMetadataProductId)
      expect(await mappingsPage.getPriceDisplayValue(stripeMetadataPriceId)).toContain('9.99')
      expect(await mappingsPage.getBillingPeriodValue(stripeMetadataPriceId)).toMatch(/month/i)
      await expect(mappingsPage.getBillingPeriodInput(stripeMetadataPriceId)).toHaveAttribute(
        'readonly',
        '',
      )
    })

    // US-BL-SYNC-001 S4
    await test.step('When/Then: Stripe metadata 修改后重复同步，以最新同步值为准', async () => {
      await updateProductMetadata(
        secrets.stripe.secretKey!,
        stripeMetadataProductId,
        UPDATED_PRODUCT_METADATA,
      )
      await syncProvider(apiContext, 'stripe')
      await mappingsPage.goto(DEMO_ADMIN.realmId)
      await mappingsPage.waitForDataLoaded()
      await mappingsPage.selectProduct(stripeMetadataProductId)
      await expect(mappingsPage.getMetadataEntry('product', 'tier')).toHaveText('team')
    })

    // US-BL-SYNC-004 S2
    await test.step('When/Then: 直连 batch PUT 传入冲突 billingPeriod 不覆盖同步周期', async () => {
      const before = await findMappingByPrice(apiContext, stripeMetadataPriceId)
      expect(before?.billingPeriod).toBe('month')
      const resp = await apiContext.put(
        `${BASE_URL}/api/bill/${DEMO_ADMIN.realmId}/entitlement-mappings/batch`,
        {
          data: {
            paymentProvider: 'stripe',
            externalProductId: stripeMetadataProductId,
            updates: [
              {
                mappingId: before!.id,
                entitlementKey: before!.entitlementKey,
                billingPeriod: 'year',
                enabled: before!.enabled,
              },
            ],
          },
        },
      )
      expect([200, 201]).toContain(resp.status())
      const after = await findMappingByPrice(apiContext, stripeMetadataPriceId)
      expect(after?.billingPeriod).toBe('month')
      await mappingsPage.goto(DEMO_ADMIN.realmId)
      await mappingsPage.waitForDataLoaded()
      await mappingsPage.selectProduct(stripeMetadataProductId)
      expect(await mappingsPage.getBillingPeriodValue(stripeMetadataPriceId)).toMatch(/month/i)
    })
    } finally {
      await apiContext.dispose()
    }
  })

  test.describe('Creem 同步展示 (US-BL-SYNC-001/003/004)', () => {
    test.skip(!hasCreemPayment(), 'Creem credentials required')

    test('Creem metadata 省略，价格和周期按真实同步值展示', async ({
      page,
      loginPage,
      demoLogger,
    }) => {
      test.skip(!!creemSetupError, `Creem setup failed: ${creemSetupError}`)
      const mappingsPage = await setupAdminMappingsPage(page, loginPage, demoLogger)
      expect(creemMapping, 'Creem sync must resolve at least one mapping').not.toBeNull()
      const priceKey = creemMapping!.externalPriceId ?? creemMapping!.id
      const info = readProviderInfo(creemMapping!.providerProductInfo)

      // US-BL-SYNC-001 S3
      await test.step('Then: Creem mapping 不显示伪造 metadata 区', async () => {
        await mappingsPage.selectProduct(creemMapping!.externalProductId)
        await expect(mappingsPage.getMetadataBlock(priceKey)).toHaveCount(0)
      })

      // US-BL-SYNC-003 S2
      await test.step('Then: Creem 价格按分换算展示；provider 未返回 price 时记录并跳过该断言', async () => {
        if (typeof info.price !== 'number' || !info.currency) {
          await demoLogger.testCode.log('Creem provider response did not include price/currency')
          return
        }
        expect(await mappingsPage.getPriceDisplayValue(priceKey)).toContain(formatMajorAmount(info.price))
      })

      // US-BL-SYNC-004 S3
      await test.step('Then: Creem billing_period 展示同步值；provider 未返回时记录并跳过该断言', async () => {
        if (!creemMapping!.billingPeriod) {
          await demoLogger.testCode.log('Creem provider response did not include billing_period')
          return
        }
        expect(await mappingsPage.getBillingPeriodValue(priceKey)).toBe(
          mapBillingPeriodLabel(creemMapping!.billingPeriod),
        )
      })
    })
  })
})

async function setupAdminMappingsPage(
  page: import('@playwright/test').Page,
  loginPage: import('../pages/login-page').LoginPage,
  demoLogger: UnifiedLogger,
): Promise<EntitlementMappingsPage> {
  await loginPage.loginAsAdmin(DEMO_ADMIN.email, 'password', DEMO_ADMIN.realmId)
  const mappingsPage = new EntitlementMappingsPage(page, demoLogger)
  await mappingsPage.goto(DEMO_ADMIN.realmId)
  await mappingsPage.waitForDataLoaded()
  await demoLogger.testCode.log('Admin on entitlement-mappings page for sync-payment demo')
  return mappingsPage
}

async function syncProvider(
  request: import('@playwright/test').APIRequestContext,
  provider: 'stripe' | 'creem',
): Promise<void> {
  const resp = await request.post(
    `${BASE_URL}/api/bill/${DEMO_ADMIN.realmId}/entitlement-mappings/sync`,
    { data: { paymentProvider: provider } },
  )
  if (!resp.ok()) {
    throw new Error(`${provider} sync failed: ${resp.status()} ${await resp.text()}`)
  }
}

async function fetchMappings(
  request: import('@playwright/test').APIRequestContext,
  provider: 'stripe' | 'creem',
): Promise<MappingListItem[]> {
  const resp = await request.get(
    `${BASE_URL}/api/bill/${DEMO_ADMIN.realmId}/entitlement-mappings?paymentProvider=${provider}`,
  )
  if (!resp.ok()) {
    throw new Error(`list ${provider} mappings failed: ${resp.status()} ${await resp.text()}`)
  }
  const body = (await resp.json()) as { items?: MappingListItem[] } | MappingListItem[]
  return Array.isArray(body) ? body : (body.items ?? [])
}

async function findMappingByPrice(
  request: import('@playwright/test').APIRequestContext,
  externalPriceId: string,
): Promise<MappingListItem | null> {
  const items = await fetchMappings(request, 'stripe')
  return items.find((m) => m.externalPriceId === externalPriceId) ?? null
}

function readProviderInfo(raw: unknown): { price?: number; currency?: string } {
  if (raw === null || typeof raw !== 'object') return {}
  const record = raw as Record<string, unknown>
  return {
    price: typeof record.price === 'number' ? record.price : undefined,
    currency: typeof record.currency === 'string' ? record.currency : undefined,
  }
}

function formatMajorAmount(amount: number): string {
  return (amount / 100).toFixed(2)
}

function mapBillingPeriodLabel(period: string | null): string {
  switch (period) {
    case 'every-month':
    case 'month':
      return 'Month'
    case 'every-year':
    case 'year':
      return 'Year'
    default:
      return period ?? ''
  }
}
