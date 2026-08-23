/**
 * Unified Purchase Test Helpers
 *
 * Reusable helper functions for the user-domain purchase flow.
 *
 * The purchase-page rewrite replaced the entitlement-key-grouped
 * `mapping-card-*` cards with a price-card grid + period toggle
 * (`purchase-price-card-*`). The exported names importers rely on are
 * PRESERVED (`selectFirstMappingAndProceed`, `selectMappingAndProceed`,
 * `TEST_DATA`, `extractPaymentAttemptId`, `selectPaymentMethodAndProceed`,
 * `initiatePurchaseFlow`, `verifyRedirectPromptOrDegraded`); only the
 * card-selection internals switch to the price-card grid.
 *
 * Purchase flow (rewritten page):
 * 1. User sees price cards on /{realm}/user/purchase-points (Monthly pane by
 *    default; Annual pane via period toggle).
 * 2. Selects a price card (click sets `selectedMappingId = option.mappingId`).
 * 3. Clicks Next to proceed to the payment step.
 * 4. Selects payment method and clicks Complete Purchase.
 * 5. `createPaymentAttempt` POSTs `{targetType:'entitlement_mapping',
 *    targetId:<selectedMappingId>, paymentProvider:<derived from option>}`.
 *
 * Section IA (load-bearing — mirrors `multi-price-purchase.helpers.ts`):
 * The page splits options into two sections by billing type. Recurring options
 * live in the Subscriptions section under `purchase-price-grid-${period}`;
 * one_time options live in the Credit packs section under
 * `purchase-price-grid-credit-packs` (NOT a period-agnostic duplicate). The
 * price-card testid is period-invariant (`purchase-price-card-${priceId}`,
 * no `-annual` suffix). This helper searches BOTH grids so a single-price /
 * one-time product resolves regardless of the requested period.
 *
 * Boundary:
 * - Uses ONLY `SELECTORS.purchasePriceCard.*` + `SELECTORS.purchasePoints.*` +
 *   `SELECTORS.paymentMethodSelector.*` + `SELECTORS.paymentProviderUI.*`.
 * - Does NOT hardcode selector strings (every locator flows from selectors.ts).
 * - Re-exports the period-toggle helper from `multi-price-purchase.helpers.ts`
 *   to avoid duplicating the period-switch mechanics.
 */

import { expect, type Page } from '@playwright/test'
import { SELECTORS } from '../selectors'
import {
  selectPeriod,
  selectPriceCard,
  type PurchasePeriod,
} from './multi-price-purchase.helpers'

export type { PurchasePeriod }

// Re-export so callers that need pane-level mechanics can reach the canonical
// helpers without a second import line.
export { selectPeriod, selectPriceCard }

// Test constants
export const TEST_DATA = {
  REALMS: {
    REALM_001: 'realm-001',
  },
  USERS: {
    ADMIN_REALM_001: 'admin@realm-001.com',
    USER_REALM_001: 'user@realm-001.com',
  },
  CREDENTIALS: {
    DEFAULT_PASSWORD: 'password',
  },
  TIMEOUTS: {
    DEFAULT_NAVIGATION: 5000,
    PAYMENT_POLLING: 15000,
    ATTEMPT_CREATION: 2000,
    ELEMENT_VISIBLE: 10000,
  },
  PAYMENT_PROVIDERS: {
    STRIPE: 'stripe',
    CREEM: 'creem',
    WECHAT: 'wechat',
  } as const,
} as const

export type PaymentProvider =
  (typeof TEST_DATA.PAYMENT_PROVIDERS)[keyof typeof TEST_DATA.PAYMENT_PROVIDERS]

/**
 * Extracts payment attempt ID from localStorage
 */
export async function extractPaymentAttemptId(page: Page): Promise<string> {
  await page.waitForTimeout(TEST_DATA.TIMEOUTS.ATTEMPT_CREATION)

  const attemptId = await page.evaluate(() => {
    const state = localStorage.getItem('cas-purchase-flow')
    if (state) {
      const parsed = JSON.parse(state)
      return parsed?.state?.attemptId
    }
    return null
  })

  if (!attemptId) {
    throw new Error('Payment attempt ID not found in localStorage')
  }

  return attemptId
}

/**
 * Discover the first purchasable price card on the page and return its
 * (priceId, period) tuple so the caller can target the exact DOM node via
 * `selectPriceCard`.
 *
 * Under the section IA the page renders two grids: the Subscriptions-section
 * grid (`purchase-price-grid-${period}`, recurring only) and the Credit-packs
 * grid (`purchase-price-grid-credit-packs`, one_time only). The card testid is
 * period-invariant (`purchase-price-card-${priceId}`, no `-annual` suffix).
 *
 * We search the Subscriptions grid for the requested period first; if it has
 * no purchasable card we fall back to the Credit-packs grid. This makes a
 * single-price / one-time product resolve under the default `'month'` without
 * callers pinning a section.
 *
 * Disabled cards (mapping disabled or no provider wired) render the SAME card
 * testid plus a child `-reason` row; they are skipped here so the helper never
 * attempts to click a card whose `onClick` is undefined.
 *
 * Default period is `'month'` (page boots in Monthly). Callers with a known
 * annual-recurring product should pass `'year'`.
 */
async function discoverFirstPurchasablePriceCard(
  page: Page,
  period: PurchasePeriod = 'month',
): Promise<{ priceId: string; period: PurchasePeriod }> {
  // The price grids render only after the purchase-options response lands
  // (the page shows a loading spinner until then), so a one-shot isVisible
  // below races a slow response and misreads a still-loading page as "no
  // cards". Wait for the data to have rendered — any grid or the empty
  // state — before scanning; the empty state is included so a genuinely
  // card-less realm fails fast on the descriptive error below instead of a
  // visibility timeout.
  const contentLoaded = page.locator(
    [
      SELECTORS.purchasePriceCard.subscriptionsGrid,
      SELECTORS.purchasePriceCard.creditPacksGrid,
      SELECTORS.purchasePriceCard.emptyState,
    ].join(','),
  )
  await expect(contentLoaded.first()).toBeVisible({
    timeout: TEST_DATA.TIMEOUTS.ELEMENT_VISIBLE,
  })

  // Candidate grids in priority order: the requested-period Subscriptions
  // grid, then the Credit-packs grid (one_time). A grid may be absent (e.g.
  // no recurring options → Subscriptions section not rendered).
  const candidateGrids = [
    SELECTORS.purchasePriceCard.priceGrid(period),
    SELECTORS.purchasePriceCard.creditPacksGrid,
  ]

  for (const gridSelector of candidateGrids) {
    const grid = page.locator(gridSelector)
    // Skip grids that are not rendered (section hidden) rather than asserting
    // visibility — the requested period's Subscriptions grid is legitimately
    // absent for a one-time-only product.
    if (!(await grid.isVisible().catch(() => false))) continue

    const cards = grid.locator('[data-testid^="purchase-price-card-"]')
    const count = await cards.count()
    for (let i = 0; i < count; i++) {
      const card = cards.nth(i)
      const testid = (await card.getAttribute('data-testid')) ?? ''
      // Skip reason rows (defensive — they live as children of cards, not the
      // grid).
      if (testid.endsWith('-reason')) continue
      const priceId = testid.replace(/^purchase-price-card-/, '')
      // Confirm the card is not disabled (no `-reason` child row) before
      // returning it.
      const reason = card.locator(`[data-testid="${testid}-reason"]`)
      if ((await reason.count()) > 0) continue
      return { priceId, period }
    }
  }

  throw new Error(
    `No purchasable price card found (Subscriptions ${period} + Credit packs; all disabled or seed empty?).`,
  )
}

/**
 * Selects the first available price card and proceeds to the payment step.
 *
 * Replaces the legacy `mappingCard.firstCard()` click. Defaults to the Monthly
 * period so both one-time (Credit packs section) and month-recurring
 * (Subscriptions section) products resolve. Pass `period: 'year'` for
 * annual-recurring products.
 *
 * Internals:
 * - card testid → `purchase-price-card-${priceId}` (period-invariant; no
 *   `-annual` suffix under the section IA).
 * - The page derives the checkout `mappingId` itself from the clicked option
 *   (`selectedMappingId = option.mappingId`), so this helper does NOT need to
 *   resolve mappingId — the click is the load-bearing act.
 */
export async function selectFirstMappingAndProceed(
  page: Page,
  opts?: { period?: PurchasePeriod },
): Promise<void> {
  const period = opts?.period ?? 'month'
  const { priceId, period: resolvedPeriod } =
    await discoverFirstPurchasablePriceCard(page, period)
  await selectPriceCard(page, priceId, resolvedPeriod)

  // Verify the card shows selected state and Next is enabled.
  await expect(
    page.locator(SELECTORS.purchasePriceCard.nextButton),
  ).toBeEnabled()

  await page.locator(SELECTORS.purchasePriceCard.nextButton).click()
}

/**
 * Selects a specific price card by priceId and proceeds to the payment step.
 *
 * Used via `initiatePurchaseFlow`'s `priceId` opt by demos pinned to a seeded
 * card in realms that also carry other providers' cards. `priceId` is
 * `externalPriceId ?? mappingId` (Creem NULL-price rows use mappingId);
 * targets `purchase-price-card-${priceId}` (period-invariant under the
 * section IA).
 */
export async function selectMappingAndProceed(
  page: Page,
  priceId: string,
  period?: PurchasePeriod,
): Promise<void> {
  await selectPriceCard(page, priceId, period)

  await expect(
    page.locator(SELECTORS.purchasePriceCard.nextButton),
  ).toBeEnabled()
  await page.locator(SELECTORS.purchasePriceCard.nextButton).click()
}

/**
 * Selects payment method and proceeds to processing
 */
export async function selectPaymentMethodAndProceed(
  page: Page,
  provider: PaymentProvider
): Promise<void> {
  await page.getByTestId(`payment-method-select-${provider}`).click()
  await expect(page.getByTestId(`payment-method-selected-${provider}`)).toBeVisible()

  await expect(page.locator(SELECTORS.purchasePoints.nextButton)).toBeEnabled()
  await page.locator(SELECTORS.purchasePoints.nextButton).click()
}

/**
 * Full purchase flow: navigate -> select price card -> (payment step?) ->
 * create payment attempt. Returns the payment attempt ID.
 *
 * The card-selection step is price-card driven via
 * `selectFirstMappingAndProceed` (`priceId` pins a specific seeded card
 * instead of "first card" — realms with cards from multiple providers need
 * this). The checkout `mappingId` is resolved by the page from the clicked
 * option; this helper does not pin it.
 *
 * Two provider contracts (frontend 533ec22d + a71c72a4,
 * purchase-points.tsx `createPaymentAttempt` onSuccess):
 *
 * - Hosted checkout (`stripe`): when the attempt POST returns a checkout URL
 *   the page redirects the SAME TAB to the provider host
 *   (`window.location.href`) — no in-app `processing` step renders, and after
 *   the redirect the app's localStorage is unreadable. See
 *   `initiateHostedCheckoutPurchaseFlow`.
 * - WeChat (`wechat`): never redirects (no checkout URL) — the pending
 *   Native QR / JSAPI UI IS the in-app `processing` step, and the attempt id
 *   stays readable from localStorage. See
 *   `initiateInAppProcessingPurchaseFlow`.
 *
 * NOTE: `creem` shares stripe's same-tab redirect contract in the frontend
 * (`creemCheckoutUrl`) but has no adapted demo caller yet; it still takes the
 * in-app branch (known-broken under the current frontend — flagged here, not
 * silently forked).
 */
export async function initiatePurchaseFlow(
  page: Page,
  provider: PaymentProvider,
  realmId: string = TEST_DATA.REALMS.REALM_001,
  opts?: { period?: PurchasePeriod; priceId?: string }
): Promise<string> {
  if (provider === TEST_DATA.PAYMENT_PROVIDERS.STRIPE) {
    return initiateHostedCheckoutPurchaseFlow(page, provider, realmId, opts)
  }
  return initiateInAppProcessingPurchaseFlow(page, provider, opts)
}

/**
 * In-app purchase contract (WeChat): no checkout URL — the page falls back to
 * the in-app `processing` step (the pending Native QR / JSAPI UI), so the
 * attempt id remains readable from localStorage once the step renders.
 *
 * Auto-skip behavior: when the selected price's provider is auto-determined
 * (at most one matching provider), clicking Next on the packages step fires
 * `createPaymentMutation` immediately and the page advances straight to
 * `processing`; the `payment` step only renders when more than one provider
 * matches. This helper handles BOTH paths by racing the two.
 */
async function initiateInAppProcessingPurchaseFlow(
  page: Page,
  provider: PaymentProvider,
  opts?: { period?: PurchasePeriod; priceId?: string }
): Promise<string> {
  await page.evaluate(() => localStorage.removeItem('cas-purchase-flow'))
  await page.goto(`/user/purchase-points`)
  await expect(page.locator(SELECTORS.purchasePoints.page)).toBeVisible()

  // Precondition: realm must have at least one purchasable price card in the
  // target period pane. If none exist, the page shows the empty state and this
  // helper will fail. Callers in conditional test contexts should check for
  // price cards first. `priceId` pins a specific seeded card instead of
  // "first card" (realms with cards from multiple providers need this).
  if (opts?.priceId) {
    await selectMappingAndProceed(page, opts.priceId)
  } else {
    await selectFirstMappingAndProceed(page, opts)
  }

  const reachedPayment = await page
    .locator(SELECTORS.purchasePoints.stepPayment)
    .waitFor({ state: 'visible', timeout: 4000 })
    .then(() => true)
    .catch(() => false)

  if (reachedPayment) {
    await selectPaymentMethodAndProceed(page, provider)
  }
  await expect(page.locator(SELECTORS.purchasePoints.stepProcessing)).toBeVisible({
    timeout: TEST_DATA.TIMEOUTS.ELEMENT_VISIBLE,
  })

  return extractPaymentAttemptId(page)
}

/**
 * Interrupt-guarded `page.goto` for the hosted-checkout branch.
 *
 * A previous `initiatePurchaseFlow` call in the same test leaves the tab on
 * the aborted stripe redirect's `chrome-error://` error document, and a goto
 * fired while that error document's load is still settling either rejects
 * with "interrupted by another navigation" or resolves ON the error
 * document — both leave the purchase page shell never rendering (observed in
 * the final run: `purchase-points-page` 10s invisible from this branch's
 * goto). Settle with `waitForLoadState`, detect the race (thrown message OR
 * the post-goto URL still on chrome-error), and retry after 200ms, at most 3
 * attempts. Same pattern as support-paywall-purchase-grant-demo.e2e.ts's
 * gotoWithInterruptRetry.
 */
async function gotoAppPageWithInterruptRetry(page: Page, url: string): Promise<void> {
  for (let attempt = 1; attempt <= 3; attempt++) {
    try {
      await page.goto(url)
      await page.waitForLoadState('domcontentloaded')
      // Non-throwing variant of the race: the goto resolved against the
      // still-settling error document instead of the app URL.
      if (/chrome-error/i.test(page.url())) {
        throw new Error(`goto landed on chrome-error document instead of ${url}`)
      }
      return
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      const isNavigationRace =
        /interrupted by another navigation/i.test(message) ||
        /ERR_ABORTED|chrome-error/i.test(message)
      if (attempt < 3 && isNavigationRace) {
        await page.waitForTimeout(200)
        continue
      }
      throw error
    }
  }
}

/**
 * Hosted-checkout purchase contract (stripe): the attempt POST's response
 * carries `paymentContext.stripeCheckoutUrl` and the page redirects the SAME
 * TAB to `checkout.stripe.com` — `purchase-step-processing` never renders and
 * localStorage is unreadable after the redirect. Verified pattern from
 * credit-bucket-purchase-consume-demo.e2e.ts:
 *
 * 1. Abort the provider-host navigation (registered before any navigation) so
 *    the browser never actually loads the external checkout. The abort leaves
 *    an error document that callers replace with their own navigation.
 * 2. Capture the attempt id NODE-side via a route handler on the POST: the
 *    handler proxies the real request (`route.fetch`) and fulfills the page
 *    with the untouched response, so frontend behavior — including the
 *    subsequent redirect — is unchanged. This is the only capture immune to
 *    the same-tab redirect (the page's buffered response body is evicted once
 *    the redirect starts, and localStorage is unreadable after it). The glob
 *    matches only the POST path, not the per-attempt status polls.
 * 3. Click through the packages/(payment?) steps and wait for the captured id.
 *    Single-provider prices submit directly from the packages-step Next click;
 *    multi-provider prices render the `payment` step first — race the payment
 *    step against the attempt POST so neither path blocks on the other.
 *
 * The caller drives the attempt to completion itself (e.g. `fulfillPayment`)
 * and may resume the page with
 * `/{realm}/user/purchase-points?attemptId={id}` the way the provider bounce
 * would — that bounce re-enters processing, polls, and renders the complete
 * step once the attempt succeeds.
 */
async function initiateHostedCheckoutPurchaseFlow(
  page: Page,
  provider: PaymentProvider,
  realmId: string,
  opts?: { period?: PurchasePeriod; priceId?: string }
): Promise<string> {
  // Kept registered for the page's lifetime: the redirect fires only after
  // the POST response reaches the page, which can be after this function
  // returns.
  await page.route('https://checkout.stripe.com/**', (route) => route.abort())

  await page.evaluate(() => localStorage.removeItem('cas-purchase-flow'))

  let createdAttemptId = ''
  let resolveAttemptCreated: () => void = () => {}
  const attemptCreated = new Promise<void>((resolve) => {
    resolveAttemptCreated = resolve
  })
  await page.route('**/purchase/payment-attempts', async (route) => {
    const resp = await route.fetch()
    if (resp.status() === 201) {
      try {
        createdAttemptId = ((await resp.json()) as { id: string }).id
      } catch {
        // Leave empty — the expect.poll below fails loudly if the id never
        // arrives.
      }
    }
    await route.fulfill({ response: resp })
    resolveAttemptCreated()
  })

  await gotoAppPageWithInterruptRetry(page, `/${realmId}/user/purchase-points`)
  await expect(page.locator(SELECTORS.purchasePoints.page)).toBeVisible()

  if (opts?.priceId) {
    await selectMappingAndProceed(page, opts.priceId)
  } else {
    await selectFirstMappingAndProceed(page, opts)
  }

  try {
    // The POST fires either from the packages-step Next click (single
    // provider) or from the payment step's Complete Purchase (multi
    // provider). Whichever happens first decides whether the
    // provider-select click is still needed.
    const reachedPayment = await Promise.race([
      page
        .locator(SELECTORS.purchasePoints.stepPayment)
        .waitFor({ state: 'visible', timeout: TEST_DATA.TIMEOUTS.PAYMENT_POLLING })
        .then(() => true),
      attemptCreated.then(() => false),
    ])

    if (reachedPayment) {
      await selectPaymentMethodAndProceed(page, provider)
    }

    // The route handler resolves the id the moment the POST completes; the
    // (aborted) redirect that follows cannot disturb it.
    await expect
      .poll(() => createdAttemptId, {
        timeout: TEST_DATA.TIMEOUTS.PAYMENT_POLLING,
      })
      .toBeTruthy()
  } finally {
    await page.unroute('**/purchase/payment-attempts')
  }

  return createdAttemptId
}

/**
 * Verifies redirect prompt or degraded UI for a payment provider.
 * Handles two cases: checkout URL present (redirect prompt) or absent (degraded UI).
 * Verifies the redirect prompt (with checkout URL) or degraded UI is shown.
 */
export async function verifyRedirectPromptOrDegraded(
  page: Page,
  providerName: string
): Promise<void> {
  await expect(
    page
      .locator(SELECTORS.paymentProviderUI.redirectPrompt)
      .or(page.locator(SELECTORS.paymentProviderUI.contextDegraded))
  ).toBeVisible({ timeout: 5000 })

  const redirectPrompt = page.locator(SELECTORS.paymentProviderUI.redirectPrompt)
  const isRedirectPromptVisible = await redirectPrompt.isVisible()

  if (isRedirectPromptVisible) {
    const manualLink = page.locator(SELECTORS.paymentProviderUI.redirectManualLink)
    await expect(manualLink).toBeVisible()

    const href = await manualLink.getAttribute('href')
    expect(href).toBeTruthy()

    const promptText = await redirectPrompt.textContent()
    expect(promptText).toMatch(new RegExp(`${providerName}|Redirecting`, 'i'))
    // Do not cancel here: the redirect is user-initiated (no auto-redirect), and
    // cancelling would clearPurchaseState() and wipe the persisted attemptId that
    // the caller verifies afterwards. The pending attempt is cleaned up in afterEach.
  } else {
    const degraded = page.locator(SELECTORS.paymentProviderUI.contextDegraded)
    await expect(degraded).toBeVisible()
    await expect(degraded).toContainText('Payment Information Unavailable')

    const cancelButton = page.locator(SELECTORS.paymentProviderUI.cancelButton)
    await expect(cancelButton).toBeVisible()
  }
}
