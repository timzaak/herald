/**
 * Live Creem Renewal tran_ Stability Smoke Test
 *
 * Related User Stories: US-PM-001, US-PM-002
 * Coverage: partial live smoke; verifies that a Creem renewal invoice row carries
 *   a non-empty `last_transaction_id` (tran_) as its external_invoice_id, and that
 *   a renewal payment attempt exists with provider_reference
 *   `creem_renewal:{ext_sub_id}:{tran_}`. This is the production-readiness layer
 *   for design §7 P1 risk (tran_ stability not yet verified on real Creem traffic)
 *   and docs/user-stories/billing/payment-invoice-mapping.md (tech-research §5.5, working layer).
 *   Does NOT count as full user-story coverage for US-PM-001/002.
 *
 *   IMPORTANT: Creem test mode does NOT expose a documented, scriptable
 *   "trigger renewal" / "trigger retry" API. This test therefore cannot
 *   deterministically drive a renewal tran_ end-to-end from inside the test
 *   process. See the degradation note in "Not Covered" and the operator runbook
 *   in "Manual Step". The test waits for a real Creem renewal tran_ to appear
 *   (driven by the Creem billing cycle, OR by an operator action in the Creem
 *   Dashboard that advances the subscription), then asserts on it.
 *
 * Not Covered:
 *   - Stripe renewal tran_ stability (the simulated branch is covered in
 *     demo/e2e/billing/payment-invoice-mapping/ DE-D02 scenarios).
 *   - Full US-PM-001/002 user-story matrix (price-granularity mapping editor,
 *     period-pane purchase, multi-price checkout).
 *   - Frontend polling states on the renewal invoice list.
 *   - Webhook compensation / dead-letter / replay-on-500 matrix.
 *   - Idempotency matrix (duplicate-event, same-tran-different-event,
 *     tran_-collision) — those are deterministic and belong in simulated tests.
 *   - Deterministic re-delivery of the SAME Creem renewal event across retries
 *     (Creem does not expose a "redeliver last transaction" knob in test mode;
 *     if Creem itself re-delivers, the backend's provider_reference idempotency
 *     is the safety net, but we cannot drive the re-delivery here).
 *   - Cross-retry stability assertion (same tran_ on retry) is therefore NOT
 *     asserted live; it is asserted in DE-D02 simulated scenarios instead.
 *
 * Live Dependency:
 *   - Real Creem test credentials: CREEM_API_KEY, CREEM_WEBHOOK_SECRET,
 *     CREEM_PRODUCT_ID set in demo/.env.demo (fail loud via requireCreemPayment()).
 *   - Public webhook callback: backend/config.demo.toml [frontend].url MUST
 *     point to a publicly reachable address (ngrok) so Creem can deliver the
 *     real subscription.paid renewal webhook. Without this the renewal invoice
 *     row is never written and this test will time out.
 *   - Creem Dashboard webhook endpoint configured to
 *     https://{public-host}/api/third/pay/{realmId}/creem/webhooks with the
 *     subscription.* event set.
 *
 * Manual Step: yes
 *   - Operator must ensure the Creem subscription under test actually produces
 *     a renewal cycle during the test window. Two accepted paths:
 *       (a) Let the natural Creem billing cycle fire (test-mode cycle length is
 *           governed by the product's billing interval — typically daily for
 *           test products).
 *       (b) Use the Creem Dashboard's subscription controls (if available) to
 *           advance / trigger the next billing cycle for the test subscription.
 *   - Operator must confirm in the Creem Dashboard that a renewal
 *     subscription.paid event with a non-empty last_transaction_id (tran_) was
 *     delivered to the public webhook endpoint before the test's poll window
 *     expires.
 *   - This test does NOT re-deliver events itself; it relies on real Creem.
 *
 * Run Command:
 *   cd demo
 *   npx playwright test e2e/live/billing/payment-invoice-mapping/us-pm-002-creem-renewal-tran-stability-live.e2e.ts --project=demo-fast --headed
 *
 * Skip/Fail Policy:
 *   - Fails loud when required Creem credentials are absent
 *     (requireCreemPayment() throws in beforeEach).
 *   - Fails (timeout) when no renewal tran_ invoice appears within the poll
 *     window — this is the production-readiness signal: if no renewal invoice
 *     is written, the tran_ stability contract cannot be verified.
 *
 * Prerequisites:
 *   - CREEM_API_KEY, CREEM_WEBHOOK_SECRET, CREEM_PRODUCT_ID set in demo/.env.demo
 *   - Demo seed data loaded (admin realm, admin@cas.com user)
 *   - Public webhook callback reachable (ngrok) and registered in Creem Dashboard
 *   - backend/config.demo.toml [frontend].url pointing at the public host
 *
 * Fixed test identifiers:
 *   - Entitlement Key: herald-live-creem-renewal-entitlement
 *
 * Design references:
 *   - Design §7 (P1 risk: last_transaction_id production stability unverified)
 *   - Design §6.2 (Creem test mode renewal + retry)
 *   - Tech research §5.5 (working layer; story: docs/user-stories/billing/payment-invoice-mapping.md)
 *   - Backend renewal write path: backend/api-billing/src/webhook_handlers.rs:1203-1331
 *     (provider_reference = creem_renewal:{ext_sub_id}:{last_transaction_id};
 *      external_invoice_id = last_transaction_id)
 */

import { type Page } from '@playwright/test'
import { test, expect } from '../../../fixtures/demo-auth.fixtures'
import { secrets, requireCreemPayment } from '../../../secrets/env'
import { seedCreemConfig } from '../../../secrets/realm-seed'
import { loginAsAdmin } from '../../../helpers/auth'
import { verifyTestEnvironment } from '../../../helpers/environment-setup'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'
const REALM_ID = 'admin'
const ENTITLEMENT_KEY = 'herald-live-creem-renewal-entitlement'

/**
 * How long to wait for a real Creem renewal tran_ to land. Creem test-mode
 * renewal cadence depends on the product's billing interval; we poll
 * conservatively. Operator may raise via env override for long-cycle products.
 */
const RENEWAL_POLL_TIMEOUT_MS = Number(process.env.CREEM_RENEWAL_POLL_MS ?? 180_000)

// ---------------------------------------------------------------------------
// Invoice / attempt typing (camelCase as serialized by the backend)
// ---------------------------------------------------------------------------

interface CreemInvoice {
  id: string
  provider: string
  paymentProvider?: string
  externalInvoiceId?: string
  status: string
  total: number
  source?: string
  paymentAttemptId?: string
  subscriptionId?: string
  [key: string]: unknown
}

interface PaymentAttempt {
  id: string
  paymentProvider?: string
  provider?: string
  providerReference?: string
  status?: string
  amount?: number
  currency?: string
  [key: string]: unknown
}

// ---------------------------------------------------------------------------
// Setup helpers (mirror us-pa-001-creem-checkout-live.e2e.ts conventions)
// ---------------------------------------------------------------------------

/** Find or create a client app and return its UUID. */
async function ensureClientApp(
  request: import('@playwright/test').APIRequestContext,
): Promise<string> {
  const listResp = await request.get(`${BASE_URL}/api/client/${REALM_ID}`)
  if (listResp.ok()) {
    const body = await listResp.json()
    const apps = body.items ?? body
    if (Array.isArray(apps) && apps.length > 0) {
      return apps[0].id
    }
  }

  const createResp = await request.post(`${BASE_URL}/api/client/${REALM_ID}`, {
    data: {
      clientId: `live-creem-renewal-${Date.now()}`,
      name: 'Live Creem Renewal Test App',
      redirectUris: ['http://localhost:3000/callback'],
      enabled: true,
    },
  })
  expect(createResp.ok()).toBeTruthy()
  const created = await createResp.json()
  return created.id
}

/**
 * Snapshot existing Creem invoice external ids so the renewal assertion can
 * only be satisfied by a NEW invoice written during this run's renewal window
 * (prevents a stale first-checkout invoice from satisfying the tran_ check).
 */
async function snapshotCreemInvoiceExternalIds(page: Page): Promise<Set<string>> {
  const ids = new Set<string>()
  const resp = await page.request.get(
    `${BASE_URL}/api/bill/${REALM_ID}/invoices?provider=creem`,
  )
  if (!resp.ok()) return ids
  const body = await resp.json()
  const items = body.data ?? body.items ?? body
  if (!Array.isArray(items)) return ids
  for (const inv of items as Array<{ externalInvoiceId?: string }>) {
    const id = inv.externalInvoiceId
    if (typeof id === 'string' && id.length > 0) {
      ids.add(id)
    }
  }
  return ids
}

/**
 * Poll the Creem invoice list until a NEW invoice appears that was NOT in the
 * baseline snapshot. Returns the first such invoice. Throws on timeout.
 *
 * The renewal invoice's external_invoice_id is the Creem `last_transaction_id`
 * (tran_) per backend/api-billing/src/webhook_handlers.rs:1283. We accept any
 * non-empty external_invoice_id; we do NOT hardcode a tran_ prefix because
 * Creem's tran_ format is not part of the contract — only its presence and
 * stability across the idempotency key matter.
 */
async function waitForNewCreemInvoice(
  page: Page,
  baselineExternalIds: Set<string>,
  timeout = RENEWAL_POLL_TIMEOUT_MS,
): Promise<CreemInvoice> {
  const startTime = Date.now()
  let delay = 2000
  const maxDelay = 5000

  while (Date.now() - startTime < timeout) {
    const resp = await page.request.get(
      `${BASE_URL}/api/bill/${REALM_ID}/invoices?provider=creem`,
    )
    if (resp.ok()) {
      const body = await resp.json()
      const items = body.data ?? body.items ?? body
      if (Array.isArray(items)) {
        // API returns newest-first (ORDER BY created_at DESC).
        const fresh = items.find((inv: any) => {
          const provider = inv.provider ?? inv.paymentProvider
          const externalId = inv.externalInvoiceId
          return (
            provider === 'creem' &&
            typeof externalId === 'string' &&
            externalId.length > 0 &&
            !baselineExternalIds.has(externalId)
          )
        })
        if (fresh) {
          return fresh as CreemInvoice
        }
      }
    }
    await new Promise((resolve) => setTimeout(resolve, delay))
    delay = Math.min(delay * 1.3, maxDelay)
  }

  throw new Error(
    `Timed out waiting for a NEW Creem renewal invoice after ${timeout}ms. ` +
      'Ensure Creem test mode actually fired a renewal subscription.paid event ' +
      'with a non-empty last_transaction_id (tran_) AND that the public webhook ' +
      'callback delivered it to the backend (see Manual Step in the file header).',
  )
}

/** Fetch the admin realm's most recent creem payment attempts (raw list). */
async function listCreemPaymentAttempts(page: Page): Promise<PaymentAttempt[]> {
  const resp = await page.request.get(
    `${BASE_URL}/api/bill/${REALM_ID}/purchase/payment-attempts`,
  )
  if (!resp.ok()) return []
  const body = await resp.json()
  const items = body.items ?? body.attempts ?? body
  if (!Array.isArray(items)) return []
  return items.filter((a: any) => {
    const provider = a.paymentProvider ?? a.provider
    return provider === 'creem'
  })
}

/**
 * Look up the renewal payment attempt whose provider_reference matches
 * `creem_renewal:{ext_sub_id}:{tran_}` for the given external_invoice_id.
 *
 * The backend derives provider_reference from the event's ext_sub_id and
 * last_transaction_id (webhook_handlers.rs:1219-1231). The external_invoice_id
 * stored on the invoice IS the last_transaction_id, so we can reconstruct the
 * expected provider_reference as `creem_renewal:*:{externalInvoiceId}` and
 * match by suffix.
 */
async function findRenewalAttemptForInvoice(
  page: Page,
  invoice: CreemInvoice,
): Promise<PaymentAttempt | undefined> {
  const tran = invoice.externalInvoiceId
  if (!tran) return undefined
  const attempts = await listCreemPaymentAttempts(page)
  return attempts.find((a) => {
    const ref = a.providerReference
    if (!ref) return false
    if (!ref.startsWith('creem_renewal:')) return false
    // Suffix match on :{tran}. Using suffix (not equality) because the
    // ext_sub_id segment is not knowable from the invoice row alone.
    return ref.endsWith(`:${tran}`)
  })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

test.describe('[Live][Billing Payment Invoice Mapping] US-PM-002: Creem renewal tran_ stability', () => {

  test.beforeEach(async ({ page, demoLogger }, testInfo) => {
    // Fail loud on missing real credentials — this is a live integration smoke.
    requireCreemPayment()

    demoLogger.testCode.log(`[${testInfo.title}] verifying demo environment`)
    await verifyTestEnvironment(page, {
      requiredRealms: [REALM_ID],
      requiredUsers: ['admin@cas.com'],
    })

    await loginAsAdmin(page, { realmId: REALM_ID })

    // Seed real Creem credentials into the admin realm.
    await seedCreemConfig(page.request, REALM_ID, {
      apiKey: secrets.creem.apiKey!,
      webhookSecret: secrets.creem.webhookSecret!,
    })
    demoLogger.testCode.log('[Live] ✓ Creem config seeded + admin login')

    // Cleanup stale entitlement mappings from previous runs so the mapping is
    // deterministically configured for THIS run.
    try {
      const mappingsResp = await page.request.get(
        `${BASE_URL}/api/bill/${REALM_ID}/entitlement-mappings`,
      )
      if (mappingsResp.ok()) {
        const body = await mappingsResp.json()
        const items = body.items ?? body
        if (Array.isArray(items)) {
          for (const m of items) {
            if (m.entitlementKey === ENTITLEMENT_KEY) {
              await page.request.patch(
                `${BASE_URL}/api/bill/${REALM_ID}/entitlement-mappings/${m.id}`,
                {
                  data: {
                    entitlementKey: `stale-${ENTITLEMENT_KEY}-${Date.now()}`,
                    enabled: false,
                  },
                },
              )
              console.log(`[cleanup] Reset stale mapping ${m.id}`)
            }
          }
        }
      }
    } catch (error) {
      console.error('[cleanup] Error during stale data cleanup (non-fatal):', error)
    }
  })

  test.afterEach(async ({ page, demoLogger }) => {
    // Mirror the reference live Creem test's config cleanup pattern.
    try {
      for (const key of ['api_key', 'webhook_secret']) {
        const resp = await page.request.delete(
          `${BASE_URL}/api/configs/${REALM_ID}/creem/${key}`,
        )
        demoLogger.testCode.log(`[Live] ✓ Creem ${key} cleanup delete: ${resp.status()}`)
      }
    } catch (error) {
      demoLogger.testCode.log(`[Live] ✗ Creem config cleanup error: ${error}`)
      console.error('[cleanup] Error during Creem config cleanup:', error)
    }
  })

  test('US-PM-002 Scenario 1: Creem renewal invoice carries a stable tran_ and a renewal attempt with creem_renewal provider_reference', async ({
    page,
    demoLogger,
  }, testInfo) => {
    test.setTimeout(Math.max(testInfo.timeout ?? 0, RENEWAL_POLL_TIMEOUT_MS + 120_000))

    let clientAppId: string
    let mappingId: string

    await test.step('Given a Creem entitlement mapping is configured for renewal', async () => {
      // Sync provider products to ensure the real Creem product is present.
      const syncResp = await page.request.post(
        `${BASE_URL}/api/bill/${REALM_ID}/entitlement-mappings/sync`,
        { data: { paymentProvider: 'creem' } },
      )
      expect(syncResp.ok(), `sync failed: ${await syncResp.text().catch(() => '')}`).toBeTruthy()

      const mappingsResp = await page.request.get(
        `${BASE_URL}/api/bill/${REALM_ID}/entitlement-mappings?paymentProvider=creem`,
      )
      expect(mappingsResp.ok()).toBeTruthy()
      const body = await mappingsResp.json()
      const items = body.items ?? body
      const targetMapping = items.find(
        (m: any) => m.externalProductId === secrets.creem.productId,
      )
      expect(targetMapping, 'Creem product mapping not found after sync').toBeTruthy()
      mappingId = targetMapping.id

      if (targetMapping.entitlementKey !== ENTITLEMENT_KEY || !targetMapping.enabled) {
        const patchResp = await page.request.patch(
          `${BASE_URL}/api/bill/${REALM_ID}/entitlement-mappings/${targetMapping.id}`,
          {
            data: {
              entitlementKey: ENTITLEMENT_KEY,
              enabled: true,
              pointsPerPeriod: 1000,
              grantPeriodType: 'monthly',
              validityDays: 30,
              grantOnSubscribe: true,
            },
          },
        )
        expect(patchResp.ok()).toBeTruthy()
      }

      clientAppId = await ensureClientApp(page.request)
      console.log(`[setup] clientAppId=${clientAppId} mappingId=${mappingId}`)
    })

    // Snapshot baseline AFTER first-checkout so the renewal assertion can only
    // be satisfied by a genuine NEW renewal invoice. We snapshot here (before
    // any checkout) to also exclude first-checkout invoices from prior runs.
    const invoiceBaseline = await snapshotCreemInvoiceExternalIds(page)
    demoLogger.testCode.log(`[Live] baseline creem invoices: ${invoiceBaseline.size}`)

    await test.step('When a Creem subscription is created via real checkout OR an existing subscription is reused', async () => {
      // This live test assumes an active Creem subscription exists for the
      // admin realm (created in a prior run, OR created by this step). We do
      // NOT drive the full hosted checkout here because:
      //   1. The hosted checkout flow is already smoke-covered by
      //      us-pa-001-creem-checkout-live.e2e.ts.
      //   2. This test's purpose is the RENEWAL tran_, not checkout.
      // Instead we confirm a subscription exists for the client app; if none
      // exists yet, the operator must create one (via the hosted checkout
      // referenced above) before this test can observe a renewal.
      const subResp = await page.request.get(
        `${BASE_URL}/api/bill/${REALM_ID}/client/${clientAppId}/subscription`,
      )
      if (subResp.ok()) {
        const sub = await subResp.json()
        console.log(
          `[subscription] existing sub id=${sub.id} status=${sub.status} ` +
            `extSubId=${sub.externalSubscriptionId ?? sub.external_subscription_id ?? '<none>'}`,
        )
      } else {
        // No active subscription. Surface clearly — without an active sub,
        // Creem will never fire a renewal event for this client app.
        console.warn(
          '[live] No active subscription found for client app. ' +
            'Create one first (run us-pa-001-creem-checkout-live.e2e.ts), then ' +
            're-run this test once the subscription is due for renewal.',
        )
      }
    })

    let renewalInvoice: CreemInvoice

    await test.step('Then a NEW Creem renewal invoice appears with a non-empty tran_ external_invoice_id', async () => {
      // The renewal invoice is written by the backend's renewal branch
      // (webhook_handlers.rs:1203-1331) when Creem delivers a real
      // subscription.paid renewal event carrying last_transaction_id.
      renewalInvoice = await waitForNewCreemInvoice(page, invoiceBaseline)
      demoLogger.testCode.log(
        `[Live] new renewal invoice id=${renewalInvoice.id} ` +
          `externalInvoiceId=${renewalInvoice.externalInvoiceId} ` +
          `status=${renewalInvoice.status} total=${renewalInvoice.total}`,
      )

      // tran_ stability contract (design §7 P1): the renewal invoice MUST carry
      // a non-empty external_invoice_id (the last_transaction_id). We do NOT
      // hardcode the tran_ prefix format — Creem owns that — but its presence
      // and its use as the idempotency anchor is the load-bearing assertion.
      expect(
        renewalInvoice.externalInvoiceId,
        'Renewal invoice must have a non-empty external_invoice_id (Creem last_transaction_id / tran_)',
      ).toBeTruthy()
      expect(renewalInvoice.provider).toBe('creem')
      expect(
        renewalInvoice.total,
        'Renewal invoice total must be > 0 (zero-amount cycles must not produce invoices)',
      ).toBeGreaterThan(0)
      demoLogger.testCode.log('[Live] ✓ renewal invoice carries non-empty tran_ + provider=creem + total>0')
    })

    await test.step('And the renewal invoice detail carries provider=creem and a paid status', async () => {
      const detailResp = await page.request.get(
        `${BASE_URL}/api/bill/${REALM_ID}/invoices/${renewalInvoice!.id}`,
      )
      expect(detailResp.ok(), `detail fetch failed: ${await detailResp.text().catch(() => '')}`).toBeTruthy()
      const detail = await detailResp.json()
      console.log(`[renewal] invoice detail: ${JSON.stringify(detail)}`)
      expect(detail.provider).toBe('creem')
      expect(detail.externalInvoiceId).toBeTruthy()
      // Renewal invoices are written as Paid (webhook_handlers.rs:1306 InvoiceStatus::Paid).
      expect(detail.status, 'Renewal invoice must be paid').toBe('paid')
    })

    await test.step('And a renewal payment attempt exists with provider_reference creem_renewal:{ext_sub_id}:{tran_}', async () => {
      const attempt = await findRenewalAttemptForInvoice(page, renewalInvoice!)
      expect(
        attempt,
        `Expected a creem renewal payment attempt whose provider_reference ends with ` +
          `:${renewalInvoice!.externalInvoiceId} (creem_renewal:{{sub}}:{tran_})`,
      ).toBeTruthy()
      console.log(
        `[renewal] matched attempt id=${attempt!.id} ` +
          `providerReference=${attempt!.providerReference} status=${attempt!.status}`,
      )

      expect(attempt!.providerReference, 'provider_reference must be the renewal idempotency key').toMatch(
        /^creem_renewal:.+:/,
      )
      expect(
        attempt!.providerReference!.endsWith(`:${renewalInvoice!.externalInvoiceId}`),
        'provider_reference must end with the tran_ (last_transaction_id) of the renewal invoice',
      ).toBeTruthy()

      // The backend writes the renewal attempt as Succeeded via
      // record_subscription_renewal_attempt (webhook_handlers.rs:1239).
      const status = (attempt!.status ?? '').toLowerCase()
      expect(
        status === 'succeeded' || status === 'success',
        `Renewal attempt status should be succeeded, got "${attempt!.status}"`,
      ).toBeTruthy()
      demoLogger.testCode.log('[Live] ✓ renewal attempt provider_reference + status verified')
    })
  })
})
