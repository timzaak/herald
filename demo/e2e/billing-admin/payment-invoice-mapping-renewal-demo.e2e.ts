/**
 * [Billing][Payment-Invoice-Mapping] Simulated renewal — US-PM-001/002/003
 *
 * Drives the subscription *renewal* write paths with signed synthetic webhooks
 * (Creem `subscription.paid` renewal branch + Stripe `invoice.payment_succeeded`
 * subscription renewal branch) and asserts on PERSISTENT business state only:
 * invoice rows, attempt attribution, the attribution-anomalies API response,
 * and admin UI badges / detail blocks. No assertion relies on ephemeral toasts.
 *
 * User stories (docs/user-stories/billing/payment-invoice-mapping.md):
 * - US-PM-001: every amount>0 renewal produces one succeeded payment_attempt.
 * - US-PM-002: every Creem renewal cycle produces one Paid invoice (tran_-keyed),
 *              aligned with Stripe renewal coverage.
 * - US-PM-003: external invoices carry non-null local attribution
 *              (subscription_id + payment_attempt_id); gaps are discoverable.
 *
 * Design: .ai/design/payment-invoice-mapping.md §5.1 (renewal attempt +
 * provider_reference idempotency keys), §5.2 (Creem renewal invoice), §5.3
 * (Stripe renewal attribution), §6.2 (demo acceptance).
 *
 * Backend assertion targets (verified):
 * - Renewal attempt: payment_attempts row status=succeeded,
 *   target_type=entitlement_mapping, provider_reference =
 *     `creem_renewal:{ext_sub_id}:{tran_}` / `stripe_renewal:{sub_}:{in_}`.
 *   Surfaced to the test via `invoice.paymentAttemptId` (the only API-exposed
 *   handle; there is no list endpoint for payment_attempts).
 * - Invoice: external_invoice_id = tran_ (Creem) / in_ (Stripe),
 *   subscription_id + payment_attempt_id non-null (camelCase in the API DTO:
 *   `externalInvoiceId` / `subscriptionId` / `paymentAttemptId`).
 * - Anomaly discovery:
 *     GET /api/bill/{realmId}/invoices?attribution=missing
 *       → provider<>'manual' AND subscription_id IS NULL AND payment_attempt_id IS NULL
 *     GET /api/bill/{realmId}/invoice-attribution/anomalies
 *       → { unattributed_invoices[], payments_without_invoice[] }
 *   (backend/api-billing/src/invoice_handlers.rs:449,498; invoice_types.rs:521-554)
 *
 * Frontend testids (verified in source):
 * - invoice-admin-page.tsx: page `invoice-admin-page` (:488);
 *   attribution Select `invoice-attribution-filter` (:390, options all/missing
 *   :314-318); row badge `invoice-unattributed-badge-{id}` (:138, rendered when
 *   provider!=='manual' && !subscriptionId && !paymentAttemptId).
 * - invoice-detail-dialog.tsx: section `invoice-attribution-section` (:350,
 *   hidden when both attribution fields null); subscription link
 *   `invoice-attribution-subscription-link` (:360, tanstack/router Link to
 *   /$realmId/manage/billing/subscriptions with search={paymentProvider});
 *   payment-attempt span `invoice-attribution-payment-attempt` (:370, font-mono).
 *
 * -------------------------------------------------------------------------
 * STEP 0 — Establishing a renewable external subscription (recoverability
 * hard-prerequisite). Method chosen: **(a) synthetic first-period chain**,
 * provider-specific (documented loudly below). The seed has NO subscription
 * rows, and the slot forbids subscription-creation.helpers.ts /
 * api-test-data.helpers.ts / db-test-data.helpers.ts.
 *
 *   - Creem: the renewal handler calls `sync_creem_subscription` (find-or-create)
 *     AFTER resolving the entitlement mapping. So the FIRST renewal event
 *     delivered with `metadata.herald_user_id = <realm admin uuid>` bootstraps
 *     the subscription row as a side effect; every later event (fresh tran_)
 *     renews it. No checkout.completed chain is needed for Creem because
 *     `sync_creem_subscription` creates the row when it is absent.
 *
 *   - Stripe: the renewal handler resolves the entitlement mapping from the
 *     EXISTING subscription's external_product_id/external_price_id BEFORE
 *     `sync_stripe_subscription_with_history_in_txn` runs, so a bare renewal
 *     event cannot bootstrap the row (resolver miss → BadRequest). To establish
 *     the Stripe subscription we first deliver a signed
 *     `checkout.session.completed` (mode=subscription, no attemptId) which
 *     resolves the mapping from `display_items[0].price.{product,id}` and
 *     creates the subscription via `sync_stripe_subscription_with_history_*`.
 *     Subsequent `invoice.payment_succeeded` events then renew it.
 *
 * Establishment runs in `beforeEach` (not `beforeAll`) because the unified
 * fixture model provides `page`/`request` per-test, not in `beforeAll`. The
 * establishment deliveries are IDEMPOTENT: the renewal webhook dedups by
 * `payment_event.external_event_id` and the renewal attempt is find-or-create by
 * `provider_reference`, so re-establishing each test is safe. The bootstrapped
 * subscription persists across tests (the renewable anchor); per-test renewal
 * events use FRESH tran_/in_/sub_ ids from the DE-D01 factories so they never
 * collide.
 *
 * -------------------------------------------------------------------------
 * Creem delivery note: `buildCreemSubscriptionPaidRenewalPayload`
 * (helpers/webhook-renewal-simulation.ts) already emits the top-level `id`,
 * `eventType`, and `type` that `handle_creem_webhook`
 * (backend/api-billing/src/webhook_handlers.rs) requires (it 400s without
 * `event.id` + `event.eventType`). `deliverCreemRenewal` therefore only
 * injects metadata — it must NOT overwrite `id`, or re-delivering the same
 * payload would bypass the payment_event-level idempotency dedup.
 * -------------------------------------------------------------------------
 *
 * ngrok: NOT needed. The synthetic webhooks are posted directly to the local
 * backend; no real provider callback is involved.
 */

import { randomUUID } from 'crypto'
import { test, cleanupTestData, expect } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { createBearerApiContext, DEMO_ADMIN, REALM_ADMINS } from '../helpers/auth'
import { seedCreemConfig, seedStripeConfig } from '../secrets/realm-seed'
import { secrets, hasStripePayment, hasCreemPayment } from '../secrets/env'
import { ensureMultiPriceCatalog } from '../helpers/resolve-mappings'
import {
  deliverCreemRenewalWebhook,
  deliverStripeRenewalWebhook,
  signStripeWebhook,
  WEBHOOK_ROUTES,
  type CreemSubscriptionPaidRenewalPayload,
  type StripeInvoicePaymentSucceededPayload,
} from '../helpers/webhook-renewal-simulation'
import {
  makeCreemRenewalScenario,
  makeStripeRenewalScenario,
} from '../fixtures/webhook-renewal-events'
import { navigateToInvoiceAdminPage } from './helpers/invoice-helpers'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'

/**
 * Realm hosting BOTH Creem and Stripe entitlement mappings + provider configs
 * in the demo seed (`scripts/lib/demo_seed.py::_ensure_points_package_payment_demo_data`).
 * Renewal webhook resolution needs a matching (realm, provider, product) mapping;
 * only realm-001 qualifies for Creem, so the whole suite runs here.
 */
const REALM_ID = 'realm-001'

// Real Stripe product/price ids resolved lazily on first Stripe setup
// (replaces the removed placeholder ids prod_stripe_multi_pro /
// price_stripe_pro_monthly). Populated by ensureStripeRenewalCatalog().
let STRIPE_RENEWAL_PRODUCT_ID = ''
let STRIPE_RENEWAL_PRICE_ID = ''

/** Polling cap (ms) for backend write propagation after a webhook delivery. */
const WRITE_PROPAGATION_TIMEOUT = 12_000

// ---------------------------------------------------------------------------
// Local types (API DTO shapes — camelCase per backend serde rename_all)
// ---------------------------------------------------------------------------

interface InvoiceApiResponse {
  total: number
  page: number
  pageSize: number
  data: InvoiceRow[]
}

interface InvoiceRow {
  id: string
  invoiceNumber: string
  source: string
  subscriptionId: string | null
  paymentAttemptId: string | null
  status: string
  provider: string
  paymentProvider: string | null
  externalInvoiceId: string | null
  total: number
  currency: string
}

interface AttributionAnomaliesResponse {
  unattributedInvoices: InvoiceRow[]
  paymentsWithoutInvoice: Array<{
    paymentAttemptId: string
    provider: string
    targetType: string
    amount: number
    currency: string
  }>
}

// ---------------------------------------------------------------------------
// Id generators — keep event-level ids unique across the suite run so the
// backend `payment_event.external_event_id` idempotency dedup never collapses
// distinct logical deliveries.
// ---------------------------------------------------------------------------

const RUN_TAG = `${Date.now().toString(36)}-${process.pid.toString(36)}`
function nextCheckoutEventId(): string {
  return `evt_checkout_demo_${randomUUID()}`
}

// ===========================================================================
// Test suite
// ===========================================================================

test.describe('[Billing][Payment-Invoice-Mapping] Simulated renewal — US-PM-001/002/003', () => {
  let testStartTime: number

  test.beforeEach(async ({ page, demoLogger }) => {
    // Live dependency: at least one provider's credentials must be configured.
    test.skip(
      !hasStripePayment() && !hasCreemPayment(),
      'Stripe or Creem credentials required (live test)',
    )
    testStartTime = Date.now()
    await verifyTestEnvironment(page, {
      requiredRealms: [DEMO_ADMIN.realmId, REALM_ID],
      requiredUsers: [DEMO_ADMIN.email, REALM_ADMINS[REALM_ID].email],
    })
    await demoLogger.testCode.log(
      `Environment verified (target realm=${REALM_ID} hosts Creem+Stripe mappings)`,
    )
  })

  test.afterEach(async ({ page, demoLogger }) => {
    await cleanupTestData(page, REALM_ID, {
      keepUsers: [DEMO_ADMIN.email, REALM_ADMINS[REALM_ID].email],
      timestamp: testStartTime,
    })
    await demoLogger.testCode.log('Test data cleanup requested (webhook secret not deleted)')
  })

  // =========================================================================
  // US-PM-001/002 — Creem multi-period renewal: each tran_ → 1 Paid invoice +
  // 1 succeeded attempt, attribution non-null.
  // =========================================================================
  test('Creem multi-period renewal', async ({ loginPage, demoLogger }) => {
    const { adminUserId, creemProductId, apiContext } =
      await setupRealmAndEstablishCreemSubscription(loginPage, demoLogger)
    try {
      // 3 distinct renewal cycles (fresh tran_/sub_ per factory call). All three
      // must produce exactly one Paid invoice + one attributed attempt each.
      const cycles = [
        makeCreemRenewalScenario(creemProductId, { amount: 1000 }),
        makeCreemRenewalScenario(creemProductId, { amount: 1500 }),
        makeCreemRenewalScenario(creemProductId, { amount: 2000 }),
      ]

      await test.step('When: deliver 3 distinct Creem renewal events (different tran_)', async () => {
        for (const c of cycles) {
          const res = await deliverCreemRenewal(apiContext, REALM_ID, c.payload, adminUserId)
          expect(res.ok, `Creem renewal delivery failed: ${res.status} ${res.body}`).toBe(true)
        }
      })

      await test.step('Then: each tran_ has exactly one Paid Creem invoice with non-null attribution', async () => {
        for (const c of cycles) {
          const invoice = await waitForInvoiceByExternalId(
            apiContext,
            REALM_ID,
            c.expectedExternalInvoiceId,
            'creem',
          )
          expect(
            invoice,
            `invoice for tran_=${c.expectedExternalInvoiceId} must exist`,
          ).not.toBeNull()
          expect(invoice!.status).toBe('paid')
          expect(invoice!.provider).toBe('creem')
          expect(invoice!.externalInvoiceId).toBe(c.expectedExternalInvoiceId)
          // US-PM-003: attribution must be back-filled (subscription + attempt).
          expect(invoice!.subscriptionId, 'subscriptionId must be non-null').not.toBeNull()
          expect(invoice!.paymentAttemptId, 'paymentAttemptId must be non-null').not.toBeNull()
        }
      })

      await test.step('And: each renewal produces a distinct succeeded attempt id (no collapse)', async () => {
        const attemptIds = new Set<string>()
        for (const c of cycles) {
          const inv = await waitForInvoiceByExternalId(
            apiContext,
            REALM_ID,
            c.expectedExternalInvoiceId,
            'creem',
          )
          attemptIds.add(inv!.paymentAttemptId!)
        }
        expect(attemptIds.size, '3 distinct tran_ must yield 3 distinct payment_attempt ids').toBe(
          cycles.length,
        )
      })

      await demoLogger.testCode.log(
        `Creem multi-period renewal verified: ${cycles.length} cycles → ${cycles.length} attempts + invoices`,
      )
    } finally {
      await apiContext.dispose()
    }
  })

  // =========================================================================
  // US-PM-001/002 — Creem same tran_ idempotent: re-delivery does not duplicate
  // the invoice row and does not create a second attempt.
  // =========================================================================
  test('Creem same tran_ idempotent', async ({ loginPage, demoLogger }) => {
    const { adminUserId, creemProductId, apiContext } =
      await setupRealmAndEstablishCreemSubscription(loginPage, demoLogger)
    try {
      const scenario = makeCreemRenewalScenario(creemProductId, {
        amount: 1200,
      })

      await test.step('When: deliver the same Creem renewal event twice', async () => {
        const r1 = await deliverCreemRenewal(apiContext, REALM_ID, scenario.payload, adminUserId)
        expect(r1.ok, `first delivery failed: ${r1.status}`).toBe(true)
        const r2 = await deliverCreemRenewal(apiContext, REALM_ID, scenario.payload, adminUserId)
        // Second delivery of the SAME event id is deduped at the payment_event
        // layer → backend returns 200 (idempotent), no duplicate write.
        expect(r2.status, 'repeat delivery must be accepted (idempotent)').toBe(200)
      })

      let attemptIdAfterFirst: string | null = null
      await test.step('Then: exactly one invoice row exists for the tran_', async () => {
        const invoice = await waitForInvoiceByExternalId(
          apiContext,
          REALM_ID,
          scenario.expectedExternalInvoiceId,
          'creem',
        )
        expect(invoice, 'invoice must exist after renewal').not.toBeNull()
        attemptIdAfterFirst = invoice!.paymentAttemptId
        expect(attemptIdAfterFirst, 'attempt id must be present').not.toBeNull()

        // Count invoice rows matching this external_invoice_id — must be exactly 1.
        const count = await countInvoicesByExternalId(
          apiContext,
          REALM_ID,
          scenario.expectedExternalInvoiceId,
        )
        expect(count, 'idempotent re-delivery must not duplicate the invoice row').toBe(1)
      })

      await test.step('And: the attributed payment_attempt id is unchanged after re-delivery', async () => {
        const invoice = await getInvoiceByExternalId(
          apiContext,
          REALM_ID,
          scenario.expectedExternalInvoiceId,
          'creem',
        )
        expect(invoice!.paymentAttemptId).toBe(attemptIdAfterFirst)
      })

      await demoLogger.testCode.log(
        'Creem same-tran_ idempotency verified (1 invoice, stable attempt)',
      )
    } finally {
      await apiContext.dispose()
    }
  })

  // =========================================================================
  // US-PM-002/003 — Stripe renewal aligned with Creem: invoice.payment_succeeded
  // (with subscription) → 1 Paid Stripe invoice, attribution non-null, same
  // coverage shape as Creem.
  // =========================================================================
  test('Stripe renewal aligned with Creem', async ({ loginPage, demoLogger }) => {
    const { adminUserId, clientAppId, apiContext } = await setupRealmAndEstablishStripeSubscription(
      loginPage,
      demoLogger,
    )
    try {
      // Establish the Stripe subscription first via a synthetic checkout, then
      // deliver a renewal against it.
      const stripeSubId = `sub_stripe_demo_${RUN_TAG}_${randomUUID().slice(0, 8)}`
      await test.step('Given: a renewable Stripe subscription exists (checkout.session.completed)', async () => {
        const result = await establishStripeSubscription(
          apiContext,
          REALM_ID,
          adminUserId,
          stripeSubId,
          clientAppId,
        )
        expect(
          result.ok,
          `Stripe subscription establishment failed: ${result.status} ${result.body}`,
        ).toBe(true)
      })

      const scenario = makeStripeRenewalScenario({ amount: 2500 })
      // Pin the scenario to the established subscription so the renewal resolves
      // the existing row (Stripe renewal requires an existing subscription).
      ;(scenario.payload.data.object as Record<string, unknown>).subscription = stripeSubId
      // Keep the scenario's expected identifiers consistent with the pinned
      // subscription (provider_reference = `stripe_renewal:{sub}:{in_}`). Without
      // this they still point at the factory's discarded random sub_ and would
      // mislead any future assertion against the recorded subscription/reference.
      scenario.expectedSubscriptionId = stripeSubId
      scenario.expectedProviderReference = `stripe_renewal:${stripeSubId}:${scenario.expectedExternalInvoiceId}`

      await test.step('When: deliver a Stripe invoice.payment_succeeded renewal', async () => {
        const res = await deliverStripeRenewal(apiContext, REALM_ID, scenario.payload, adminUserId)
        expect(res.ok, `Stripe renewal delivery failed: ${res.status} ${res.body}`).toBe(true)
      })

      await test.step('Then: one Paid Stripe invoice exists for the in_ with non-null attribution', async () => {
        const invoice = await waitForInvoiceByExternalId(
          apiContext,
          REALM_ID,
          scenario.expectedExternalInvoiceId,
          'stripe',
        )
        expect(
          invoice,
          `invoice for in_=${scenario.expectedExternalInvoiceId} must exist`,
        ).not.toBeNull()
        expect(invoice!.status).toBe('paid')
        expect(invoice!.provider).toBe('stripe')
        expect(invoice!.externalInvoiceId).toBe(scenario.expectedExternalInvoiceId)
        expect(invoice!.subscriptionId, 'subscriptionId must be non-null').not.toBeNull()
        expect(invoice!.paymentAttemptId, 'paymentAttemptId must be non-null').not.toBeNull()
      })

      await demoLogger.testCode.log('Stripe renewal coverage aligned with Creem verified')
    } finally {
      await apiContext.dispose()
    }
  })

  // =========================================================================
  // US-PM-003 — Stripe same in_ idempotent: re-delivery does not duplicate the
  // invoice and does NOT null-out attribution (COALESCE on ON CONFLICT UPDATE).
  // =========================================================================
  test('Stripe same in_ idempotent', async ({ loginPage, demoLogger }) => {
    const { adminUserId, clientAppId, apiContext } = await setupRealmAndEstablishStripeSubscription(
      loginPage,
      demoLogger,
    )
    try {
      const stripeSubId = `sub_stripe_demo_${RUN_TAG}_${randomUUID().slice(0, 8)}`
      const establishment = await establishStripeSubscription(
        apiContext,
        REALM_ID,
        adminUserId,
        stripeSubId,
        clientAppId,
      )
      expect(
        establishment.ok,
        `Stripe subscription establishment failed: ${establishment.status} ${establishment.body}`,
      ).toBe(true)

      const scenario = makeStripeRenewalScenario({ amount: 1800 })
      ;(scenario.payload.data.object as Record<string, unknown>).subscription = stripeSubId

      let attemptIdAfterFirst: string | null = null
      await test.step('When: deliver the same Stripe renewal event twice', async () => {
        const r1 = await deliverStripeRenewal(apiContext, REALM_ID, scenario.payload, adminUserId)
        expect(r1.ok, `first delivery failed: ${r1.status}`).toBe(true)
        const r2 = await deliverStripeRenewal(apiContext, REALM_ID, scenario.payload, adminUserId)
        expect(r2.status, 'repeat delivery must be accepted (idempotent)').toBe(200)
      })

      await test.step('Then: exactly one invoice row exists for the in_', async () => {
        const invoice = await waitForInvoiceByExternalId(
          apiContext,
          REALM_ID,
          scenario.expectedExternalInvoiceId,
          'stripe',
        )
        expect(invoice, 'invoice must exist after renewal').not.toBeNull()
        attemptIdAfterFirst = invoice!.paymentAttemptId
        expect(attemptIdAfterFirst, 'attempt id must be present').not.toBeNull()
        const count = await countInvoicesByExternalId(
          apiContext,
          REALM_ID,
          scenario.expectedExternalInvoiceId,
        )
        expect(count, 'idempotent re-delivery must not duplicate the invoice row').toBe(1)
      })

      await test.step('And: attribution is NOT nullified by the re-upsert (COALESCE verified)', async () => {
        const invoice = await getInvoiceByExternalId(
          apiContext,
          REALM_ID,
          scenario.expectedExternalInvoiceId,
          'stripe',
        )
        expect(invoice!.paymentAttemptId, 'COALESCE must preserve paymentAttemptId').toBe(
          attemptIdAfterFirst,
        )
        expect(invoice!.subscriptionId, 'COALESCE must preserve subscriptionId').not.toBeNull()
      })

      await demoLogger.testCode.log('Stripe same-in_ idempotency + COALESCE preservation verified')
    } finally {
      await apiContext.dispose()
    }
  })

  // =========================================================================
  // US-PM-001 §5.1 — Zero-amount period skipped: amount=0 renewal produces no
  // new attempt and no invoice (CHECK(amount>0) at the write side).
  // =========================================================================
  test('Zero-amount period skipped', async ({ loginPage, demoLogger }) => {
    const { adminUserId, creemProductId, apiContext } =
      await setupRealmAndEstablishCreemSubscription(loginPage, demoLogger)
    try {
      // Build the renewal payload manually because the DE-D01 builder throws on
      // amount<=0 by design (it guards the caller). The backend skip path is
      // what we are asserting, so we must bypass the guard here.
      const zeroScenario = makeCreemRenewalScenario(creemProductId, {
        amount: 100,
      })
      // Force the amount to 0 on the already-built payload to exercise the
      // backend's amount==0 skip branch (design §5.2 / §5.1).
      ;(zeroScenario.payload.object as Record<string, unknown>).amount = 0
      const zeroTran = zeroScenario.expectedExternalInvoiceId

      await test.step('When: deliver a Creem renewal with amount=0', async () => {
        const res = await deliverCreemRenewal(
          apiContext,
          REALM_ID,
          zeroScenario.payload,
          adminUserId,
        )
        // Backend accepts the event (200) but skips the renewal attempt + invoice
        // write because amount is not > 0.
        expect(res.ok, `zero-amount delivery unexpectedly failed: ${res.status} ${res.body}`).toBe(
          true,
        )
      })

      await test.step('Then: NO invoice row exists for the zero-amount tran_', async () => {
        const invoice = await getInvoiceByExternalId(apiContext, REALM_ID, zeroTran, 'creem')
        expect(invoice, 'zero-amount cycle must NOT produce an invoice').toBeNull()
      })

      await demoLogger.testCode.log('Zero-amount period skip verified (no invoice written)')
    } finally {
      await apiContext.dispose()
    }
  })

  // =========================================================================
  // US-PM-002 §5.2 — First-period attempt_id not duplicated: a Creem
  // subscription.paid carrying an attempt_id takes the first-period branch and
  // must NOT enter the renewal invoice logic (no tran_ invoice, no renewal
  // attempt). This is the P0 first/renewal de-dup guard.
  // =========================================================================
  test('First-period attempt_id not duplicated', async ({ loginPage, demoLogger }) => {
    const { adminUserId, creemProductId, apiContext } =
      await setupRealmAndEstablishCreemSubscription(loginPage, demoLogger)
    try {
      // Build a renewal-shaped payload, then attach a (non-nil) attempt_id in the
      // object metadata so the backend routes it through the FIRST-PERIOD branch
      // (early return), not the renewal invoice write. The backend reads the
      // camelCase key `attemptId` (metadata_keys::ATTEMPT_ID,
      // backend/domain/src/purchase/services.rs:17) — snake_case variants are
      // ignored and the renewal invoice write would fire.
      const scenario = makeCreemRenewalScenario(creemProductId, {
        amount: 3000,
      })
      const fakeAttemptId = randomUUID()
      ;(scenario.payload.object as Record<string, unknown>).metadata = {
        attemptId: fakeAttemptId,
        herald_attempt_id: fakeAttemptId,
        attempt_id: fakeAttemptId,
      }

      await test.step('When: deliver a Creem subscription.paid WITH attempt_id (first-period)', async () => {
        const res = await deliverCreemRenewal(apiContext, REALM_ID, scenario.payload, adminUserId)
        // The first-period branch fires whenever `attemptId` is present in
        // metadata (webhook_handlers.rs:1021). It calls complete_succeeded_payment_attempt,
        // which 404s because the synthetic attemptId has no payment_attempt row
        // (this test deliberately does not create one — its purpose is purely to
        // prove the renewal invoice write is NOT reached). Accept 404 as proof
        // the first-period branch was selected; the load-bearing assertion is the
        // absence of a renewal invoice below.
        expect(
          res.status === 200 || res.status === 404,
          `first-period delivery should hit the attempt branch (200 ok, or 404 for ` +
            `a synthetic attemptId); got ${res.status} ${res.body}`,
        ).toBe(true)
      })

      await test.step('Then: NO tran_-keyed renewal invoice was written', async () => {
        const invoice = await getInvoiceByExternalId(
          apiContext,
          REALM_ID,
          scenario.expectedExternalInvoiceId,
          'creem',
        )
        expect(
          invoice,
          'first-period (attempt_id) event must NOT produce a tran_-keyed renewal invoice',
        ).toBeNull()
      })

      await demoLogger.testCode.log('First-period attempt_id de-dup verified (no renewal invoice)')
    } finally {
      await apiContext.dispose()
    }
  })

  // =========================================================================
  // US-PM-003 — Unattributed invoice discoverable: when an externally-synced
  // invoice lacks both subscription_id and payment_attempt_id, it is surfaced
  // by the `attribution=missing` filter AND the anomalies endpoint, and the
  // admin UI renders the per-row unattributed badge.
  // =========================================================================
  test('Unattributed invoice discoverable', async ({ page, loginPage, demoLogger }) => {
    // A Stripe one-time invoice (no subscription, no renewal) arrives via
    // invoice.payment_succeeded WITHOUT a subscription field → the backend
    // delegates to handle_stripe_invoice_event which upserts the invoice with
    // NULL attribution. That row is the unattributed fixture for this scenario.
    await loginPage.loginAsAdmin(
      REALM_ADMINS[REALM_ID].email,
      REALM_ADMINS[REALM_ID].password,
      REALM_ID,
    )
    const apiContext = await createBearerApiContext(loginPage.getAccessToken())
    try {
      await seedStripeWebhookConfig(apiContext)

      // One-time Stripe invoice: omit `subscription` so the handler takes the
      // one-time branch (no renewal attempt, no attribution back-fill).
      const oneTime = makeStripeRenewalScenario({ amount: 900 })
      delete (oneTime.payload.data.object as Record<string, unknown>).subscription

      let unattributedInvoiceId: string | null = null
      await test.step('Given: an externally-synced invoice with NULL attribution exists', async () => {
        const res = await deliverStripeRenewalWebhook(apiContext, REALM_ID, oneTime.payload)
        expect(res.ok, `one-time invoice delivery failed: ${res.status} ${res.body}`).toBe(true)
        const inv = await waitForInvoiceByExternalId(
          apiContext,
          REALM_ID,
          oneTime.expectedExternalInvoiceId,
          'stripe',
        )
        expect(inv, 'one-time invoice must be written').not.toBeNull()
        // Sanity: this row should indeed be unattributed.
        expect(inv!.subscriptionId).toBeNull()
        expect(inv!.paymentAttemptId).toBeNull()
        unattributedInvoiceId = inv!.id
      })

      await test.step('Then: GET /invoices?attribution=missing returns the invoice', async () => {
        const missing = await listInvoices(apiContext, REALM_ID, {
          attribution: 'missing',
        })
        const ids = missing.data.map((i) => i.id)
        expect(ids, 'attribution=missing filter must include the unattributed invoice').toContain(
          unattributedInvoiceId,
        )
      })

      await test.step('And: GET /invoice-attribution/anomalies lists it under unattributed_invoices', async () => {
        const anomalies = await getAttributionAnomalies(apiContext, REALM_ID)
        const ids = anomalies.unattributedInvoices.map((i) => i.id)
        expect(ids, 'anomalies.unattributed_invoices must include the target invoice').toContain(
          unattributedInvoiceId,
        )
      })

      await test.step('And: admin UI renders the unattributed badge + missing filter narrows to it', async () => {
        // Login fresh as the realm admin and open the invoice admin page.
        await loginPage.loginAsAdmin(
          REALM_ADMINS[REALM_ID].email,
          REALM_ADMINS[REALM_ID].password,
          REALM_ID,
        )
        await navigateToInvoiceAdminPage(page, REALM_ID)

        // Select the "missing" attribution filter. The option label is the
        // localized "Unattributed" (frontend/messages/en.json:944
        // invoice_attribution_missing = "Unattributed"); the value sent to the
        // server is `missing`.
        await page.getByTestId('invoice-attribution-filter').click()
        await page.getByRole('option', { name: /unattributed/i }).click()
        await page.waitForLoadState('networkidle')

        // The unattributed badge for our invoice must be visible in the filtered list.
        const badge = page.getByTestId(`invoice-unattributed-badge-${unattributedInvoiceId}`)
        await expect(
          badge,
          'unattributed badge must render for the missing-filtered row',
        ).toBeVisible({
          timeout: 10_000,
        })
      })

      await demoLogger.testCode.log(
        `Unattributed invoice ${unattributedInvoiceId} discoverable via API + admin UI`,
      )
    } finally {
      await apiContext.dispose()
    }
  })

  // =========================================================================
  // US-PM-003 — Invoice detail attribution block: opening a RENEWAL invoice in
  // the admin detail dialog renders the attribution section + subscription
  // link + payment-attempt span (font-mono UUID).
  // =========================================================================
  test('Invoice detail attribution block', async ({ page, loginPage, demoLogger }) => {
    const { adminUserId, creemProductId, apiContext } =
      await setupRealmAndEstablishCreemSubscription(loginPage, demoLogger)
    try {
      const scenario = makeCreemRenewalScenario(creemProductId, {
        amount: 2200,
      })
      let attributedInvoiceId: string | null = null

      await test.step('Given: a Creem renewal invoice with non-null attribution exists', async () => {
        const res = await deliverCreemRenewal(apiContext, REALM_ID, scenario.payload, adminUserId)
        expect(res.ok, `renewal delivery failed: ${res.status}`).toBe(true)
        const inv = await waitForInvoiceByExternalId(
          apiContext,
          REALM_ID,
          scenario.expectedExternalInvoiceId,
          'creem',
        )
        expect(inv, 'renewal invoice must be written').not.toBeNull()
        expect(
          inv!.subscriptionId,
          'renewal invoice must carry subscription attribution',
        ).not.toBeNull()
        expect(
          inv!.paymentAttemptId,
          'renewal invoice must carry attempt attribution',
        ).not.toBeNull()
        attributedInvoiceId = inv!.id
      })

      await test.step('When: admin opens the invoice detail dialog', async () => {
        await loginPage.loginAsAdmin(
          REALM_ADMINS[REALM_ID].email,
          REALM_ADMINS[REALM_ID].password,
          REALM_ID,
        )
        await navigateToInvoiceAdminPage(page, REALM_ID)

        // The invoice row renders `invoiceNumber` (NOT the raw UUID) as visible
        // text, so hasText-matching on the id never finds it. The row's actions
        // menu carries the id in its data-testid (invoice-admin-page.tsx:189),
        // which is the stable per-row anchor.
        const actionsMenu = page.getByTestId(`invoice-actions-menu-${attributedInvoiceId}`)
        await expect(actionsMenu, 'attributed invoice row must be visible in the list').toBeVisible(
          {
            timeout: 15_000,
          },
        )
        await actionsMenu.click()
        await page
          .getByRole('menuitem', { name: /^View$/ })
          .first()
          .click()
        await expect(page.getByTestId('invoice-detail-dialog')).toBeVisible({
          timeout: 10_000,
        })
      })

      await test.step('Then: the attribution section + subscription link + attempt span render', async () => {
        await expect(
          page.getByTestId('invoice-attribution-section'),
          'attribution section must render for an attributed invoice',
        ).toBeVisible({ timeout: 10_000 })

        const subLink = page.getByTestId('invoice-attribution-subscription-link')
        await expect(subLink, 'subscription link must render').toBeVisible()
        // The link targets the subscriptions route with the invoice's provider.
        await expect(subLink).toHaveAttribute('href', new RegExp(`/manage/billing/subscriptions`))

        const attemptSpan = page.getByTestId('invoice-attribution-payment-attempt')
        await expect(attemptSpan, 'payment-attempt span must render').toBeVisible()
        const attemptText = (await attemptSpan.textContent())?.trim() ?? ''
        expect(
          attemptText.length,
          'payment-attempt span must show a non-empty UUID',
        ).toBeGreaterThan(8)
      })

      await test.step('Cleanup: close the detail dialog', async () => {
        await page.keyboard.press('Escape')
        await expect(page.getByTestId('invoice-detail-dialog')).toBeHidden({
          timeout: 5_000,
        })
      })

      await demoLogger.testCode.log(
        `Invoice detail attribution block verified for invoice ${attributedInvoiceId}`,
      )
    } finally {
      await apiContext.dispose()
    }
  })
})

// ===========================================================================
// Helpers — establishment + delivery + API queries
// ===========================================================================

/**
 * Realm setup shared by every Creem scenario:
 *   1. login as realm-001 admin and create a Bearer API context.
 *   2. seed Creem webhook config from demo/.env.demo (must match the signing
 *      secret used by deliverCreemRenewal, else backend returns 400).
 *   3. discover the seeded Creem product id (runtime lookup — robust against
 *      CREEM_PRODUCT_ID drift between seed-time and .env.demo).
 *
 * Step-0 establishment itself is folded into the first renewal delivery of each
 * test (see the STEP-0 comment atop this file): `sync_creem_subscription`
 * bootstraps the subscription when the first renewal event lands. We do not
 * pre-deliver here; each scenario's first `deliverCreemRenewal` with a fresh
 * sub_ id establishes + renews in one shot. This keeps the subscription anchor
 * unique per scenario (no cross-test coupling) while staying idempotent.
 */
async function setupRealmAndEstablishCreemSubscription(
  loginPage: import('../pages/login-page').LoginPage,
  demoLogger: import('../helpers/unified-logger').UnifiedLogger,
): Promise<{
  adminUserId: string
  creemProductId: string
  apiContext: import('@playwright/test').APIRequestContext
}> {
  // LoginPage.loginAsAdmin returns the userId from the login API response —
  // the authoritative admin persona id (no separate /me lookup needed).
  const adminUserId = await loginPage.loginAsAdmin(
    REALM_ADMINS[REALM_ID].email,
    REALM_ADMINS[REALM_ID].password,
    REALM_ID,
  )
  const apiContext = await createBearerApiContext(loginPage.getAccessToken())
  try {
    await seedCreemWebhookConfig(apiContext)
    // Pull the real Creem catalog into Herald (replaces the removed placeholder
    // seed). discoverCreemProductId then resolves the first synced product.
    await syncCreemCatalog(apiContext)
    const creemProductId = await discoverCreemProductId(apiContext)
    await demoLogger.testCode.log(
      `Creem realm ready (admin=${adminUserId}, product=${creemProductId})`,
    )
    return { adminUserId, creemProductId, apiContext }
  } catch (error) {
    await apiContext.dispose()
    throw error
  }
}

/**
 * Trigger a Creem provider sync for realm-001 so the real Creem product(s)
 * materialize as entitlement mappings. No-op safe to call per-test (idempotent
 * server-side). Throws if sync fails.
 */
async function syncCreemCatalog(
  request: import('@playwright/test').APIRequestContext,
): Promise<void> {
  const resp = await request.post(`${BASE_URL}/api/bill/${REALM_ID}/entitlement-mappings/sync`, {
    data: { paymentProvider: 'creem' },
  })
  if (!resp.ok()) {
    throw new Error(
      `Creem provider sync failed: ${resp.status()} ${await resp.text().catch(() => '')}`,
    )
  }
}

/**
 * Realm setup shared by every Stripe scenario:
 *   1. login as realm-001 admin.
 *   2. seed Stripe webhook config from demo/.env.demo.
 * The actual Stripe subscription is established per-test via
 * `establishStripeSubscription` because each scenario needs its own sub_ id
 * (and the Stripe renewal handler REQUIRES a pre-existing row).
 */
async function setupRealmAndEstablishStripeSubscription(
  loginPage: import('../pages/login-page').LoginPage,
  demoLogger: import('../helpers/unified-logger').UnifiedLogger,
): Promise<{
  adminUserId: string
  clientAppId: string
  apiContext: import('@playwright/test').APIRequestContext
}> {
  const adminUserId = await loginPage.loginAsAdmin(
    REALM_ADMINS[REALM_ID].email,
    REALM_ADMINS[REALM_ID].password,
    REALM_ID,
  )
  const apiContext = await createBearerApiContext(loginPage.getAccessToken())
  try {
    await seedStripeWebhookConfig(apiContext)
    // Resolve the real multi-price Stripe catalog (replaces the removed
    // placeholder seed). Sync pulls the product into Herald so the renewal
    // resolver can match (realm, provider, product, price).
    if (!STRIPE_RENEWAL_PRODUCT_ID) {
      const catalog = await ensureMultiPriceCatalog(apiContext, {
        baseUrl: BASE_URL,
        realmId: REALM_ID,
        stripeSecretKey: secrets.stripe.secretKey!,
        stripePublishableKey: secrets.stripe.publishableKey!,
        stripeWebhookSecret: secrets.stripe.webhookSecret!,
      })
      STRIPE_RENEWAL_PRODUCT_ID = catalog.product.productId
      STRIPE_RENEWAL_PRICE_ID = catalog.product.monthlyPriceId
    }
    const clientAppId = await discoverClientAppId(apiContext)
    await demoLogger.testCode.log(
      `Stripe realm ready (admin=${adminUserId}, clientApp=${clientAppId}, ` +
        `product=${STRIPE_RENEWAL_PRODUCT_ID}, price=${STRIPE_RENEWAL_PRICE_ID})`,
    )
    return { adminUserId, clientAppId, apiContext }
  } catch (error) {
    await apiContext.dispose()
    throw error
  }
}

/**
 * Deliver a Creem renewal. The factory payload already carries the top-level
 * `id` + `eventType` the backend requires, so this wrapper only injects
 * `adminUserId` into `object.metadata` as `herald_user_id`/`userId` for the
 * renewal resolver to route the grant + bootstrap the subscription on the
 * first delivery of a given sub_. The event `id` is left untouched so that
 * re-delivering the same payload exercises payment_event-level idempotency.
 */
async function deliverCreemRenewal(
  request: import('@playwright/test').APIRequestContext,
  realmId: string,
  payload: CreemSubscriptionPaidRenewalPayload,
  adminUserId: string,
): Promise<{ ok: boolean; status: number; body: string }> {
  const augmented = {
    ...payload,
    object: {
      ...payload.object,
      metadata: {
        ...(payload.object.metadata as Record<string, unknown> | undefined),
        herald_user_id: adminUserId,
        userId: adminUserId,
      },
    },
  } as unknown as CreemSubscriptionPaidRenewalPayload
  const result = await deliverCreemRenewalWebhook(request, realmId, augmented)
  return { ok: result.ok, status: result.status, body: result.body }
}

/**
 * Deliver a Stripe renewal (invoice.payment_succeeded WITH subscription),
 * augmenting the DE-D01 factory payload with the metadata the backend renewal
 * parser requires. `parse_invoice_paid_payload`
 * (stripe_webhook_handlers.rs:643-650) reads `herald_user_id`/`userId` from
 * `data.object.metadata` and 400s with "Missing or invalid userId" if absent.
 * `adminUserId` is injected so the renewal resolver routes the grant to the
 * realm admin persona. (The one-time branch used by the unattributed-invoice
 * test omits `subscription` and does NOT call this wrapper — it needs no user.)
 */
async function deliverStripeRenewal(
  request: import('@playwright/test').APIRequestContext,
  realmId: string,
  payload: StripeInvoicePaymentSucceededPayload,
  adminUserId: string,
): Promise<{ ok: boolean; status: number; body: string }> {
  const augmented = {
    ...payload,
    data: {
      ...payload.data,
      object: {
        ...payload.data.object,
        metadata: {
          ...(payload.data.object.metadata as Record<string, unknown> | undefined),
          herald_user_id: adminUserId,
          userId: adminUserId,
        },
      },
    },
  } as unknown as StripeInvoicePaymentSucceededPayload
  const result = await deliverStripeRenewalWebhook(request, realmId, augmented)
  return { ok: result.ok, status: result.status, body: result.body }
}

/**
 * Establish a renewable Stripe subscription by delivering a signed
 * checkout.session.completed (mode=subscription, no attemptId). The backend
 * resolves the entitlement from display_items[0].price.{product,id} and creates
 * the subscription via `sync_stripe_subscription_with_history_*`.
 *
 * This is the Stripe half of step-0 method (a) — see the STEP-0 comment.
 */
async function establishStripeSubscription(
  request: import('@playwright/test').APIRequestContext,
  realmId: string,
  adminUserId: string,
  stripeSubscriptionId: string,
  clientAppId: string,
): Promise<{ ok: boolean; status: number; body: string }> {
  const checkoutEvent = {
    id: nextCheckoutEventId(),
    type: 'checkout.session.completed',
    data: {
      object: {
        id: `cs_test_${RUN_TAG}_${randomUUID().slice(0, 8)}`,
        object: 'checkout.session',
        mode: 'subscription',
        payment_status: 'paid',
        status: 'complete',
        subscription: stripeSubscriptionId,
        // No attemptId metadata → backend skips fulfill-via-attempt and creates
        // the subscription via the subscription checkout branch.
        // herald_client_app_id is REQUIRED by parse_checkout_completed_payload
        // (stripe_webhook_handlers.rs:456-459) — without it the handler 400s
        // with "Missing or invalid clientAppId".
        metadata: {
          herald_user_id: adminUserId,
          userId: adminUserId,
          herald_client_app_id: clientAppId,
          clientAppId,
        },
        display_items: [
          {
            price: {
              id: STRIPE_RENEWAL_PRICE_ID,
              product: STRIPE_RENEWAL_PRODUCT_ID,
            },
          },
        ],
        created: Math.floor(Date.now() / 1000),
      },
    },
  }
  const rawBody = Buffer.from(JSON.stringify(checkoutEvent), 'utf8')
  const signature = signStripeWebhook(rawBody)
  const response = await request.post(`${BASE_URL}${WEBHOOK_ROUTES.stripe(realmId)}`, {
    headers: {
      'content-type': 'application/json',
      'stripe-signature': signature,
    },
    // Pass a Buffer so Playwright does not re-serialize the body (signature
    // is over these exact bytes — see DE-D01 BYTE-CONSISTENCY CAVEAT).
    data: rawBody,
    timeout: 10_000,
  })
  const body = await response.text().catch(() => '')
  return { ok: response.ok(), status: response.status(), body }
}

// ---------------------------------------------------------------------------
// Auth + config seeding
// ---------------------------------------------------------------------------

async function seedCreemWebhookConfig(
  request: import('@playwright/test').APIRequestContext,
): Promise<void> {
  const webhookSecret = secrets.creem.webhookSecret
  const apiKey = secrets.creem.apiKey
  if (!webhookSecret || !apiKey) {
    throw new Error(
      'CREEM_API_KEY / CREEM_WEBHOOK_SECRET must be set in demo/.env.demo for the simulated renewal suite.',
    )
  }
  await seedCreemConfig(request, REALM_ID, { apiKey, webhookSecret })
}

async function seedStripeWebhookConfig(
  request: import('@playwright/test').APIRequestContext,
): Promise<void> {
  const webhookSecret = secrets.stripe.webhookSecret
  const publishableKey = secrets.stripe.publishableKey
  const secretKey = secrets.stripe.secretKey
  if (!webhookSecret || !publishableKey || !secretKey) {
    throw new Error(
      'STRIPE_PUBLISHABLE_KEY / STRIPE_SECRET_KEY / STRIPE_WEBHOOK_SECRET must be set in demo/.env.demo for the simulated renewal suite.',
    )
  }
  await seedStripeConfig(request, REALM_ID, {
    publishableKey,
    secretKey,
    webhookSecret,
  })
}

/**
 * Discover the realm-001 client app id at runtime. The Stripe checkout
 * establishment webhook REQUIRES metadata.herald_client_app_id (a UUID) — see
 * parse_checkout_completed_payload (stripe_webhook_handlers.rs:456-459). The
 * seeded `points-demo-app` client app gets a fresh uuidv7 DB id on every seed
 * run, so we resolve it via the admin client-apps list rather than hardcoding.
 * Route: GET /api/client/{realmId} (client_apps/list.rs:17).
 */
async function discoverClientAppId(
  request: import('@playwright/test').APIRequestContext,
): Promise<string> {
  const resp = await request.get(`${BASE_URL}/api/client/${REALM_ID}?page=0&pageSize=100`)
  if (!resp.ok()) {
    throw new Error(`Failed to list client apps in realm ${REALM_ID}: ${resp.status()}`)
  }
  const body = await resp.json()
  const rows: unknown[] = Array.isArray(body?.items)
    ? body.items
    : Array.isArray(body?.data)
      ? body.data
      : Array.isArray(body)
        ? body
        : []
  // Prefer the seeded points-demo-app; fall back to the first enabled app.
  const seedApp = rows.find((r) => {
    const row = r as Record<string, unknown>
    return row?.clientId === 'points-demo-app' || row?.client_id === 'points-demo-app'
  }) as Record<string, unknown> | undefined
  const chosen =
    (seedApp as Record<string, unknown> | undefined) ??
    (rows.find((r) => (r as Record<string, unknown>)?.enabled !== false) as
      Record<string, unknown> | undefined) ??
    (rows[0] as Record<string, unknown> | undefined)
  const id = (chosen?.id as string | undefined) ?? (chosen?.clientAppId as string | undefined)
  if (typeof id === 'string' && id.length > 0) {
    return id
  }

  // No client app exists in this realm yet (the points-demo seed is not always
  // present). Create one — the Stripe checkout handler only needs A valid
  // client_app_id to attach to the subscription; it does not have to be the
  // seeded points-demo-app. Mirrors stripe-payment-comprehensive-demo.e2e.ts.
  const clientId = `renewal-demo-app-${RUN_TAG}`
  const createResp = await request.post(`${BASE_URL}/api/client/${REALM_ID}`, {
    data: {
      clientId,
      name: 'Renewal Demo App',
      redirectUris: ['http://localhost:3000/callback'],
      enabled: true,
    },
    timeout: 10_000,
  })
  if (!createResp.ok()) {
    throw new Error(
      `No client app in realm ${REALM_ID} and create failed: ${createResp.status()} ${await createResp.text().catch(() => '')}`,
    )
  }
  const created = (await createResp.json()) as Record<string, unknown>
  const createdId =
    (created.id as string | undefined) ?? (created.clientAppId as string | undefined)
  if (typeof createdId !== 'string' || createdId.length === 0) {
    throw new Error(`Created client app response missing id: ${JSON.stringify(created)}`)
  }
  return createdId
}

/**
 * Discover the seeded Creem product id in realm-001 at runtime. The seed
 * inserts the Creem mapping with whatever CREEM_PRODUCT_ID was at seed-time;
 * hardcoding the current .env.demo value would silently break if the seed ran
 * under a different value. Querying the mappings API is the robust contract.
 */
async function discoverCreemProductId(
  request: import('@playwright/test').APIRequestContext,
): Promise<string> {
  const resp = await request.get(
    `${BASE_URL}/api/bill/${REALM_ID}/entitlement-mappings?paymentProvider=creem`,
  )
  if (!resp.ok()) {
    throw new Error(
      `Failed to list Creem entitlement mappings in realm ${REALM_ID}: ${resp.status()}`,
    )
  }
  const body = await resp.json()
  // EntitlementMappingListResponse shape: { items: EntitlementMappingResponse[], total }
  // (backend/api-billing/src/entitlement_mapping_handlers.rs:104). Each item
  // exposes externalProductId (camelCase via serde rename_all).
  const rows: unknown[] = Array.isArray(body?.items)
    ? body.items
    : Array.isArray(body?.data)
      ? body.data
      : Array.isArray(body)
        ? body
        : []
  const first = rows[0] as Record<string, unknown> | undefined
  const productId =
    (first?.externalProductId as string | undefined) ??
    (first?.external_product_id as string | undefined)
  if (typeof productId !== 'string' || productId.length === 0) {
    throw new Error(
      `No Creem entitlement mapping found in realm ${REALM_ID}. ` +
        'Ensure CREEM_PRODUCT_ID is set in demo/.env.demo and a Creem provider sync has been triggered.',
    )
  }
  return productId
}

// ---------------------------------------------------------------------------
// Invoice API queries
// ---------------------------------------------------------------------------

async function listInvoices(
  request: import('@playwright/test').APIRequestContext,
  realmId: string,
  filters: Record<string, string>,
): Promise<InvoiceApiResponse> {
  // Page through the FULL result set. Webhook-written renewal invoices
  // accumulate in the shared demo realm across runs and are not removed by
  // cleanupTestData, so capping at a single page=1&pageSize=100 would miss a
  // target invoice once the table grows past 100 rows and cause a spurious
  // waitForInvoiceByExternalId timeout.
  const pageSize = 100
  const base = { pageSize: String(pageSize), ...filters }
  const merged: InvoiceRow[] = []
  let total = 0
  for (let page = 1; ; page += 1) {
    const qs = new URLSearchParams({ ...base, page: String(page) }).toString()
    const resp = await request.get(`${BASE_URL}/api/bill/invoices?${qs}`)
    if (!resp.ok()) {
      throw new Error(`listInvoices failed: ${resp.status()} ${await resp.text().catch(() => '')}`)
    }
    const body = (await resp.json()) as InvoiceApiResponse
    total = body.total
    merged.push(...body.data)
    if (body.data.length < pageSize || merged.length >= total) break
  }
  return { total, page: 1, pageSize, data: merged }
}

async function getInvoiceByExternalId(
  request: import('@playwright/test').APIRequestContext,
  realmId: string,
  externalInvoiceId: string,
  provider: 'creem' | 'stripe',
): Promise<InvoiceRow | null> {
  const all = await listInvoices(request, realmId, { provider })
  return (
    all.data.find((i) => i.externalInvoiceId === externalInvoiceId && i.provider === provider) ??
    null
  )
}

/**
 * Poll until the invoice for `externalInvoiceId` appears (write propagation
 * after a webhook delivery can take a moment), then return it.
 */
async function waitForInvoiceByExternalId(
  request: import('@playwright/test').APIRequestContext,
  realmId: string,
  externalInvoiceId: string,
  provider: 'creem' | 'stripe',
): Promise<InvoiceRow | null> {
  const deadline = Date.now() + WRITE_PROPAGATION_TIMEOUT
  while (Date.now() < deadline) {
    const inv = await getInvoiceByExternalId(request, realmId, externalInvoiceId, provider)
    if (inv) return inv
    await new Promise((r) => setTimeout(r, 300))
  }
  return null
}

async function countInvoicesByExternalId(
  request: import('@playwright/test').APIRequestContext,
  realmId: string,
  externalInvoiceId: string,
): Promise<number> {
  const all = await listInvoices(request, realmId, {})
  return all.data.filter((i) => i.externalInvoiceId === externalInvoiceId).length
}

async function getAttributionAnomalies(
  request: import('@playwright/test').APIRequestContext,
  realmId: string,
): Promise<AttributionAnomaliesResponse> {
  const resp = await request.get(`${BASE_URL}/api/bill/invoice-attribution/anomalies`)
  if (!resp.ok()) {
    throw new Error(
      `getAttributionAnomalies failed: ${resp.status()} ${await resp.text().catch(() => '')}`,
    )
  }
  return (await resp.json()) as AttributionAnomaliesResponse
}
