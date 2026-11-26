/**
 * Multiple-currency purchase demo — user-side main chain
 *
 * User stories: US-MC-003 (purchase page currency grouping with explicit
 * selection — no default currency), US-MC-004 S1 display side (explicit
 * price-row checkout in the manually-selected currency, no cross-currency
 * order), US-MC-006 degrade semantics (single-currency groups render no
 * switcher). The US-MC-00x stories extend the published multi-price baseline
 * in docs/user-stories/billing/entitlement-mapping.md.
 *
 * The multi-currency catalog is a REAL Stripe product ensured in beforeAll via
 * {@link ensureMultiCurrencyCatalog} (create-or-reuse + provider sync + batch
 * enable): one product, four recurring prices (USD/month, USD/year, EUR/month,
 * CNY/month) sharing one entitlement key — the grouping substrate.
 *
 * Assertion discipline: selection is asserted via the VISIBLE price-card set
 * (a switchable group renders only the active currency's cards — persistent
 * page state, not transient styling or toasts). Until the user picks a
 * currency, NO cards render (explicit-selection contract). The US-MC-004
 * load-bearing assertion retrieves the real Stripe checkout session and
 * verifies line_items[0].price (id + currency + unit_amount) matches the EUR
 * row the user selected — the "charged currency equals displayed currency"
 * invariant.
 *
 * Data lifecycle: catalog rows are disabled in afterAll, so this demo leaves
 * the realm's purchase page unpolluted for other demos.
 *
 * Run: uv run scripts/demo-test-runner.py demo/e2e/regular-user/multiple-currency-purchase-demo.e2e.ts
 */

import { test, expect } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { SELECTORS } from '../selectors'
import { hasStripePayment, secrets } from '../secrets/env'
import {
  initiateMultiPriceCheckout,
  selectPriceCard,
} from '../helpers/multi-price-purchase.helpers'
import {
  type MultiCurrencyCatalog,
  setupMultiCurrencyDemo,
  teardownMultiCurrencyDemo,
  slugifyEntitlementKey,
} from '../helpers/multiple-currency-helpers'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'
const REALM_ID = 'realm-001'
const USER_EMAIL = 'user@realm-001.com'
const REALM_ADMIN_EMAIL = 'admin@realm-001.com'

// Resolved in beforeAll (worker scope). Empty until the multi-currency product
// is ensured + synced + enabled.
let catalog: MultiCurrencyCatalog | null = null
/** kebab-case slug of the catalog's entitlement key (per-entitlement testids). */
let entitlementSlug = ''

/**
 * `[Regular User] 多货币显式选择购买 (US-MC-003/004)` — the describe
 * title the runner targets via `--grep`.
 */
test.describe('[Regular User] 多货币显式选择购买 (US-MC-003/004)', () => {
  test.beforeAll(async () => {
    // Skip gracefully when Stripe credentials are absent (live dependency).
    test.skip(!hasStripePayment(), 'Stripe credentials required (live catalog dependency)')

    catalog = await setupMultiCurrencyDemo({
      baseUrl: BASE_URL,
      realmId: REALM_ID,
      adminEmail: REALM_ADMIN_EMAIL,
      stripeSecretKey: secrets.stripe.secretKey!,
      stripePublishableKey: secrets.stripe.publishableKey!,
      stripeWebhookSecret: secrets.stripe.webhookSecret ?? '',
    })
    entitlementSlug = slugifyEntitlementKey(catalog.entitlementKey)
  })

  test.afterAll(async () => {
    if (!catalog || !hasStripePayment()) return
    try {
      await teardownMultiCurrencyDemo({
        baseUrl: BASE_URL,
        realmId: REALM_ID,
        adminEmail: REALM_ADMIN_EMAIL,
        catalog,
      })
    } catch (error) {
      // Cleanup failure must be loud but must not mask test failures.
      console.error('[multiple-currency-purchase] afterAll cleanup failed:', error)
    }
  })

  test.beforeEach(async ({ page }) => {
    await verifyTestEnvironment(page, {
      requiredRealms: [REALM_ID],
      requiredUsers: [USER_EMAIL],
    })
  })

  // ==========================================================================
  // US-MC-003 — purchase page groups by currency, shows every available
  // currency, and renders NO price rows until the user explicitly picks one.
  // Also covers the degrade-display invariant (US-MC-006 semantics): a
  // single-currency entitlement group renders NO currency switcher.
  // ==========================================================================
  test('US-MC-003: 按货币分组全展示，无默认货币，显式选择后才显示价格行', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    requireCatalog()

    await test.step('Given: 用户登录并打开购买页', async () => {
      await loginPage.loginAsUser(USER_EMAIL, 'password', REALM_ID)
      await page.goto(`/${REALM_ID}/user/purchase-points`)
      await expect(page.locator(SELECTORS.purchasePoints.page)).toBeVisible({ timeout: 15000 })
    })

    await test.step('Then: CNY/USD/EUR 全部展示但无任何价格卡（无默认货币）', async () => {
      const group = page.locator(SELECTORS.purchaseCurrencyGroup.entitlement(entitlementSlug))
      await expect(group).toBeVisible()
      // All available currencies stay presented (展示侧全展示, DEC-006).
      for (const code of ['cny', 'usd', 'eur']) {
        await expect(
          group.locator(SELECTORS.purchaseCurrencyGroup.option(entitlementSlug, code)),
        ).toBeVisible()
      }
      // Base-currency annotation is part of the switcher (Adaptive Pricing note).
      await expect(
        group.locator(SELECTORS.purchaseCurrencyGroup.adaptivePricingNote(entitlementSlug)),
      ).toBeVisible()

      // No currency is pre-selected: no price cards render, only the prompt.
      for (const key of ['cny-month', 'usd-month', 'usd-year', 'eur-month']) {
        await expect(
          group.locator(SELECTORS.purchasePriceCard.priceCard(catalog!.product.priceIds[key])),
        ).toHaveCount(0)
      }
      await demoLogger.testCode.log('No default currency: prompt shown until explicit pick')
    })

    await test.step('When: 显式选择 USD 组 (同货币多计费周期并列, DEC-005)', async () => {
      await page
        .locator(SELECTORS.purchaseCurrencyGroup.option(entitlementSlug, 'usd'))
        .click()
    })

    await test.step('Then: USD 月付与年付两行同时可见，其他货币行不渲染', async () => {
      const group = page.locator(SELECTORS.purchaseCurrencyGroup.entitlement(entitlementSlug))
      await expect(
        group.locator(SELECTORS.purchasePriceCard.priceCard(catalog!.product.priceIds['usd-month'])),
      ).toBeVisible()
      await expect(
        group.locator(SELECTORS.purchasePriceCard.priceCard(catalog!.product.priceIds['usd-year'])),
      ).toBeVisible()
      await expect(
        group.locator(SELECTORS.purchasePriceCard.priceCard(catalog!.product.priceIds['cny-month'])),
      ).toHaveCount(0)
      await demoLogger.testCode.log('USD group lists month + year side by side after explicit pick')
    })

    await test.step('And: 单货币权益组降级展示（无货币切换器, US-MC-006 语义）', async () => {
      // The demo seed provisions single-currency one-time Stripe mappings in
      // realm-001 (same substrate the unified-purchase demos rely on). Such a
      // group must degrade to a flat price list — no switch container.
      const creditPacks = page.locator(SELECTORS.purchasePriceCard.creditPacksGrid)
      await expect(creditPacks).toBeVisible()
      const entitlementBlocks = creditPacks.locator('[data-testid^="purchase-entitlement-"]')
      const blockCount = await entitlementBlocks.count()
      expect(blockCount, 'expected at least one seeded one-time entitlement group').toBeGreaterThan(
        0,
      )
      let degradeVerified = false
      for (let i = 0; i < blockCount; i++) {
        const block = entitlementBlocks.nth(i)
        const hasSwitch = await block
          .locator('[data-testid^="purchase-currency-switch-"]')
          .count()
        if (hasSwitch === 0) {
          degradeVerified = true
          break
        }
      }
      expect(
        degradeVerified,
        'a single-currency entitlement group must render without a currency switcher',
      ).toBeTruthy()
      await demoLogger.testCode.log('Single-currency group degrades without switcher')
    })
  })

  // ==========================================================================
  // US-MC-004 S1 (display side) — manual switch to EUR, select the EUR price
  // row, and verify the real Stripe checkout session charges the EUR row
  // (id + currency + amount). Browser checkout always submits an explicit
  // mapping id (DEC-008), so the charged currency must equal the displayed
  // one — the no-cross-currency invariant.
  // ==========================================================================
  test('US-MC-004 S1: 手动选择 EUR 并按 EUR 价格行发起购买（不串货币）', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    requireCatalog()

    await test.step('Given: 用户登录并打开购买页', async () => {
      await loginPage.loginAsUser(USER_EMAIL, 'password', REALM_ID)
      await page.goto(`/${REALM_ID}/user/purchase-points`)
      await expect(page.locator(SELECTORS.purchasePoints.page)).toBeVisible({ timeout: 15000 })
      const group = page.locator(SELECTORS.purchaseCurrencyGroup.entitlement(entitlementSlug))
      await expect(group).toBeVisible()
    })

    await test.step('When: 手动选择 EUR 组并选中 EUR 价格行', async () => {
      await page
        .locator(SELECTORS.purchaseCurrencyGroup.option(entitlementSlug, 'eur'))
        .click()
      const group = page.locator(SELECTORS.purchaseCurrencyGroup.entitlement(entitlementSlug))
      await expect(
        group.locator(SELECTORS.purchasePriceCard.priceCard(catalog!.product.priceIds['eur-month'])),
      ).toBeVisible()
      await selectPriceCard(page, catalog!.product.priceIds['eur-month'])
    })

    let checkoutUrl = ''
    await test.step('And: 发起购买（显式 target_id → createPaymentAttempt）', async () => {
      // The purchase flow auto-redirects the page to the Stripe checkout URL
      // right after the POST, which evicts Playwright's buffered response
      // body — reading the Response's json() after the redirect flakes with
      // "Network.getResponseBody: No resource with given identifier". The
      // checkout URL is therefore taken from the final address bar.
      const checkoutResponse = await initiateMultiPriceCheckout(page, {
        mappingId: catalog!.mappingIds['eur-month'],
        paymentProvider: 'stripe',
      })
      // The status is readable even when the body has been evicted.
      expect(checkoutResponse.status(), 'create-payment-attempt must succeed').toBe(201)

      // The auto-redirect to the real Stripe checkout page is the persistent
      // post-initiation state (not a toast): wait for the navigation and take
      // the checkout URL from the address bar. domcontentloaded is the right
      // gate — the external page's `load` can hang past the timeout on a slow
      // route to checkout.stripe.com, and only the URL is consumed here.
      await page.waitForURL(/checkout\.stripe\.com/, {
        timeout: 15000,
        waitUntil: 'domcontentloaded',
      })
      checkoutUrl = page.url()

      // The no-cross-currency invariant is asserted on the NEXT step against
      // the real Stripe checkout session (the charge artifact): its line item
      // must reference the EUR price row selected on the purchase page. The
      // Herald purchase-history API only lists SUCCEEDED attempts (webhook
      // completion never fires in the demo env), so it cannot witness a just
      // created Pending attempt.
      await demoLogger.testCode.log(`EUR checkout initiated: ${checkoutUrl.slice(0, 60)}...`)
    })

    await test.step('Then: Stripe 结算会话引用 EUR 价格行（货币与金额一致，US-MC-004 S1）', async () => {
      const match = /(cs_(?:live|test)_[A-Za-z0-9]+)/.exec(checkoutUrl)
      expect(match, 'checkout URL must expose a parseable Stripe session id').toBeTruthy()

      const sessionResp = await fetch(
        `https://api.stripe.com/v1/checkout/sessions/${match![1]}?expand[]=line_items`,
        { headers: { Authorization: `Bearer ${secrets.stripe.secretKey}` } },
      )
      // Read the body exactly once before asserting.
      const sessionText = await sessionResp.text()
      expect(
        sessionResp.ok,
        `Stripe retrieve session failed: ${sessionResp.status} ${sessionText}`,
      ).toBeTruthy()
      const session = JSON.parse(sessionText) as {
        line_items?: {
          data: Array<{
            price?: { id: string; currency: string; unit_amount: number | null } | null
          }>
        }
      }
      const lineItemPrice = session.line_items?.data?.[0]?.price
      expect(lineItemPrice, 'expected line_items[0].price to be populated').toBeTruthy()
      // THE load-bearing assertion: the charged row is the EUR/month price the
      // user picked on the purchase page — no cross-currency substitution.
      expect(lineItemPrice!.id).toBe(catalog!.product.priceIds['eur-month'])
      expect(lineItemPrice!.currency).toBe('eur')
      expect(lineItemPrice!.unit_amount).toBe(1000)
      await demoLogger.testCode.log(
        `Checkout line item: price=${lineItemPrice!.id} currency=eur amount=1000`,
      )
    })
  })
})

// ============================================================================
// Helpers
// ============================================================================

function requireCatalog(): void {
  if (!catalog || !entitlementSlug) {
    throw new Error('beforeAll did not populate the multi-currency catalog')
  }
}
