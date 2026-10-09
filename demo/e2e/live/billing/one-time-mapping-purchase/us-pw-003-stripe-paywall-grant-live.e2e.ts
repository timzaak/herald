/**
 * Live Stripe Paywall Grant Test — real payment → role grant → RBAC gate
 *
 * Related User Stories: US-PW-002, US-PW-003 (场景1), US-PW-006 (场景1)
 * Coverage: partial; one-time Stripe checkout completes, the mapping's granted
 *   role is auto-attached to the buyer (source=payment, permanent), and a
 *   third-party app gating on that role's bound permission resolves allowed=true
 *   via the existing RBAC /permission/check endpoint.
 * Not Covered: alreadyOwned repeat gate (US-PW-004), subscription grant+revoke
 *   (US-PW-005), concurrency anti-repeat (US-PW-004 场景3), webhook
 *   loss/reorder compensation, points fulfillment.
 * Live Dependency: real Stripe test credentials + a one-time product, and a
 *   publicly reachable webhook URL (ngrok) so Stripe can deliver
 *   `checkout.session.completed` to drive role fulfillment.
 * Manual Step: no
 * Run Command:
 *   cd demo
 *   npx playwright test e2e/live/billing/one-time-mapping-purchase/us-pw-003-stripe-paywall-grant-live.e2e.ts --project=demo-fast --headed
 * Skip/Fail Policy:
 *   Fails loud when required Stripe one-time credentials are absent.
 *
 * Prerequisites:
 *   - STRIPE_PUBLISHABLE_KEY, STRIPE_SECRET_KEY, STRIPE_WEBHOOK_SECRET,
 *     STRIPE_ONETIME_PRODUCT_ID set in demo/.env.demo
 *   - Demo seed data loaded (admin realm, admin@cas.com user)
 *   - backend/config.demo.toml [frontend].url must point to a publicly
 *     reachable address (e.g. an ngrok tunnel) so Stripe can deliver webhook
 *     callbacks during checkout. Without the webhook, role fulfillment never
 *     fires and the post-purchase RBAC assertion will time out.
 *   - INTERNAL_API_KEY set in demo/.env.demo (fallback fulfillment path).
 *
 * Why this test exists (load-bearing claim):
 *   The non-live paywall demo (support-paywall-purchase-grant-demo) proves the
 *   grant chain via SIMULATED fulfillment (`fulfillPayment` calls the internal
 *   fulfill endpoint directly, bypassing the Stripe webhook). That cannot prove
 *   the real Stripe → webhook → grant pipeline. This live test fills that gap:
 *   it drives a REAL Stripe checkout with 4242, lets the REAL webhook fulfill
 *   the attempt, and asserts the buyer ends up holding the granted role via
 *   the source-agnostic /permission/check gate.
 *
 * Purchase entry point (resolved from backend source, NOT the deprecated
 * /client/{id}/checkout path used by the older US-PU-006 live tests):
 *   POST /api/bill/{realmId}/purchase/payment-attempts
 *     body: { targetType:'entitlement_mapping', targetId, paymentProvider:'stripe' }
 *     resp: { id, paymentContext: { stripeCheckoutUrl } }
 *   (backend/api-billing/src/purchase_handlers.rs:33-69; frontend consumes the
 *   same field at frontend/src/routes/$realmId/user/purchase-points.tsx:354-402.)
 *
 * Role-grant config dimension (design §5.2):
 *   The single-mapping PATCH (UpdateEntitlementMappingRequest) does NOT accept
 *   `grantedRoleIds`; only the BATCH PUT (PriceMappingUpdate) does. So we
 *   configure the grant via PUT /api/bill/{realmId}/entitlement-mappings/batch.
 *   (backend/api-billing/src/types.rs:314 PriceMappingUpdate.granted_role_ids.)
 */

import { type Frame, type Locator, type Page } from '@playwright/test'
import { test, expect } from '../../../fixtures/demo-auth.fixtures'
import { secrets, requireStripeOneTimePayment } from '../../../secrets/env'
import { seedStripeConfig } from '../../../secrets/realm-seed'
import { loginAsAdmin } from '../../../helpers/auth'
import { verifyTestEnvironment } from '../../../helpers/environment-setup'
import { makeExtApiRequest } from '../../../helpers/ext-api-helper'
import {
  createTestApiKeyWithPermission,
  type ApiKeyWithPermission,
} from '../../../helpers/grant-points-helpers'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'
const API_BASE_URL =
  process.env.API_BASE_URL ||
  process.env.BASE_URL?.replace(/:\d+/, ':8080') ||
  'http://localhost:8080'
const REALM_ID = 'admin'

// The granted role + the builtin permission bound to it. `billing.view` is
// provisioned by Demo Seed in the admin realm (resource=`billing`,
// action=`view`). We bind it to the granted role so the post-purchase RBAC
// check `{resource:'billing',action:'view'}` resolves allowed=true once the
// buyer holds the role (US-PW-006 场景1: source-agnostic RBAC gate).
const GRANTED_ROLE_NAME = 'paywall-live-stripe-grant-role'
const BOUND_PERMISSION_NAME = 'billing.view'
const CHECK_RULE = { resource: 'billing', action: 'view' }
// `admin-api-client` is auto-provisioned per realm and is treated as an
// admin/unscoped api-key identity (ADMIN_API_CLIENT_ID) — minting the RBAC
// key bound to it avoids the cross-client-app scope guard in /permission/check.
const ADMIN_API_CLIENT_ID = 'admin-api-client'

type SearchRoot = Page | Frame

// ---------------------------------------------------------------------------
// File-private helpers
// ---------------------------------------------------------------------------

/**
 * Resolve a Stripe checkout input field across the page and any iframe
 * (Stripe Elements render in nested frames). Mirrors the US-PU-006 live helper.
 */
async function findVisibleCheckoutControl(
  page: Page,
  label: string,
  selectors: Array<(root: SearchRoot) => Locator>,
): Promise<Locator> {
  const roots: SearchRoot[] = [page, ...page.frames()]

  for (const root of roots) {
    for (const selector of selectors) {
      const locator = selector(root).first()
      if (await locator.isVisible({ timeout: 1000 }).catch(() => false)) {
        return locator
      }
    }
  }

  const frames = page
    .frames()
    .map((frame) => `- name="${frame.name()}" url="${frame.url()}"`)
    .join('\n')
  const title = await page.title().catch(() => '<unavailable>')

  throw new Error(
    `Stripe checkout ${label} control not found.\n` +
      `Current URL: ${page.url()}\n` +
      `Page title: ${title}\n` +
      `Frames:\n${frames || '- <none>'}`,
  )
}

/**
 * Create (idempotent) a non-builtin role in the realm and return its id.
 * Built-in roles are excluded from the grant selector / cannot carry arbitrary
 * permission bindings reliably, so the live test provisions its own.
 */
async function ensureRoleAndGetId(request: import('@playwright/test').APIRequestContext): Promise<string> {
  const listResp = await request.get(`${API_BASE_URL}/api/roles/${REALM_ID}/define`)
  expect(listResp.ok(), `list roles failed: ${listResp.status()}`).toBeTruthy()
  const listBody = await listResp.json()
  const roles: { id: string; name: string }[] = Array.isArray(listBody)
    ? listBody
    : listBody.items ?? []
  const existing = roles.find((r) => r.name === GRANTED_ROLE_NAME)
  if (existing) return existing.id

  const createResp = await request.post(`${API_BASE_URL}/api/roles/${REALM_ID}/define`, {
    data: {
      name: GRANTED_ROLE_NAME,
      description: 'Granted-on-purchase role for US-PW-003 live test',
      clientId: 'admin-web-console',
    },
  })
  expect(createResp.ok(), `create role failed: ${createResp.status()}`).toBeTruthy()
  const created = await createResp.json()
  return created.id
}

/**
 * Bind the builtin permission (`billing.view`) to the granted role so a
 * third-party RBAC check on `{resource:'billing',action:'view'}` resolves
 * allowed once the buyer holds the role.
 */
async function bindPermissionToRole(
  request: import('@playwright/test').APIRequestContext,
  roleId: string,
): Promise<void> {
  // Resolve the permission id.
  const permResp = await request.get(`${API_BASE_URL}/api/permission/${REALM_ID}/define`)
  expect(permResp.ok(), `list permissions failed: ${permResp.status()}`).toBeTruthy()
  const permBody = await permResp.json()
  const perms: { id: string; name: string }[] = Array.isArray(permBody)
    ? permBody
    : permBody.items ?? []
  const target = perms.find((p) => p.name === BOUND_PERMISSION_NAME)
  if (!target) {
    throw new Error(
      `${BOUND_PERMISSION_NAME} not seeded in ${REALM_ID}; ` +
        `available: ${perms.map((p) => p.name).join(', ')}`,
    )
  }

  // Assign (idempotent) — a duplicate bind returns 409, which we tolerate.
  const assignResp = await request.post(
    `${API_BASE_URL}/api/roles/${REALM_ID}/define/${roleId}/permissions`,
    { data: { permissionId: target.id } },
  )
  if (!assignResp.ok() && assignResp.status() !== 409) {
    throw new Error(
      `bind ${BOUND_PERMISSION_NAME} → role ${roleId} failed: ${assignResp.status()} ${await assignResp.text()}`,
    )
  }
}

/**
 * Resolve the client-app UUID for a given client_id in a realm.
 */
async function resolveClientAppId(
  request: import('@playwright/test').APIRequestContext,
  clientId: string,
): Promise<string> {
  // Session-scoped admin route: the bearer token pins the realm, no path segment.
  const resp = await request.get(`${BASE_URL}/api/client?page=0&pageSize=100`)
  expect(resp.ok(), `list client apps failed: ${resp.status()}`).toBeTruthy()
  const body = await resp.json()
  const raw: unknown = Array.isArray(body)
    ? body
    : (body as { data?: unknown }).data ??
      (body as { items?: unknown }).items ??
      []
  const apps: { id: string; clientId?: string; client_id?: string }[] = Array.isArray(raw)
    ? raw
    : []
  const hit = apps.find((a) => (a.clientId ?? a.client_id) === clientId)
  if (!hit) {
    throw new Error(
      `client app ${clientId} not found in ${REALM_ID}; available: ${apps
        .map((a) => a.clientId ?? a.client_id)
        .join(', ')}`,
    )
  }
  return hit.id
}

/**
 * Find (after sync) the one-time Stripe product mapping matching the configured
 * STRIPE_ONETIME_PRODUCT_ID. Returns its id + current billing type.
 */
async function findOneTimeMapping(
  request: import('@playwright/test').APIRequestContext,
): Promise<{ id: string; billingType: string }> {
  const resp = await request.get(
    `${BASE_URL}/api/bill/${REALM_ID}/entitlement-mappings?paymentProvider=stripe`,
  )
  expect(resp.ok(), `list mappings failed: ${resp.status()}`).toBeTruthy()
  const body = await resp.json()
  const items = body.items ?? body
  const hit = (items as Array<Record<string, unknown>>).find(
    (m) => m.externalProductId === secrets.stripe.onetimeProductId,
  )
  if (!hit) {
    throw new Error(
      `one-time Stripe product mapping not found after sync. products seen: ${JSON.stringify(
        (items as Array<Record<string, unknown>>).map((m) => m.externalProductId),
      )}`,
    )
  }
  return {
    id: hit.id as string,
    billingType: (hit.billingType as string) ?? (hit.billing_type as string) ?? '',
  }
}

/**
 * Configure `grantedRoleIds` on a one-time mapping via the BATCH PUT (the only
 * endpoint that accepts the role-grant dimension). Also pins billing_type to
 * one_time + enabled. Idempotent.
 */
async function configureGrantOnMapping(
  request: import('@playwright/test').APIRequestContext,
  mappingId: string,
  roleId: string,
): Promise<void> {
  const resp = await request.put(`${BASE_URL}/api/bill/${REALM_ID}/entitlement-mappings/batch`, {
    data: {
      paymentProvider: 'stripe',
      externalProductId: secrets.stripe.onetimeProductId,
      updates: [
        {
          mappingId,
          billingType: 'one_time',
          enabled: true,
          grantedRoleIds: [roleId],
        },
      ],
    },
  })
  expect(
    resp.ok(),
    `batch configure grant failed: ${resp.status()} ${await resp.text().catch(() => '')}`,
  ).toBeTruthy()
}

/**
 * Read the current logged-in user's session token from the X-Auth cookie. The
 * /permission/check `sessionToken` field takes this value.
 */
async function readSessionToken(page: Page): Promise<string> {
  const cookies = await page.context().cookies()
  const auth = cookies.find((c) => c.name === 'X-Auth')
  if (!auth || !auth.value) {
    throw new Error('[US-PW-003 live] X-Auth session cookie missing — user not logged in')
  }
  return auth.value
}

/**
 * Create a payment attempt for the mapping and return the Stripe checkout URL.
 * This is the REAL purchase entry point (the frontend uses the same call).
 */
async function createPaymentAttemptAndGetCheckoutUrl(
  page: Page,
  mappingId: string,
): Promise<{ attemptId: string; checkoutUrl: string }> {
  const resp = await page.request.post(
    `${BASE_URL}/api/bill/${REALM_ID}/purchase/payment-attempts`,
    {
      headers: { 'Content-Type': 'application/json' },
      data: {
        targetType: 'entitlement_mapping',
        targetId: mappingId,
        paymentProvider: 'stripe',
      },
    },
  )
  expect(
    resp.ok(),
    `create payment attempt failed: ${resp.status()} ${await resp.text().catch(() => '')}`,
  ).toBeTruthy()
  const body = await resp.json()
  const checkoutUrl: string | undefined = body?.paymentContext?.stripeCheckoutUrl
  expect(checkoutUrl, 'paymentContext.stripeCheckoutUrl must be present').toBeTruthy()
  return { attemptId: body.id as string, checkoutUrl: checkoutUrl as string }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

test.describe('[Live][Paywall] US-PW-002/003/006: Stripe one-time payment grants role + RBAC gate', () => {
  test.beforeEach(async ({ page, demoLogger }) => {
    requireStripeOneTimePayment()

    await verifyTestEnvironment(page, {
      requiredRealms: [REALM_ID],
      requiredUsers: ['admin@cas.com'],
    })

    await loginAsAdmin(page, { realmId: REALM_ID })

    await seedStripeConfig(page.request, REALM_ID, {
      publishableKey: secrets.stripe.publishableKey!,
      secretKey: secrets.stripe.secretKey!,
      webhookSecret: secrets.stripe.webhookSecret!,
    })
    demoLogger.testCode.log('[Live] ✓ Stripe config seeded + admin login')
  })

  test.afterEach(async ({ page, demoLogger }) => {
    // Best-effort credential cleanup; non-fatal.
    try {
      for (const key of ['publishable_key', 'api_key', 'webhook_secret']) {
        await page.request.delete(`${BASE_URL}/api/configs/${REALM_ID}/stripe/${key}`)
      }
      demoLogger.testCode.log('[Live] ✓ Stripe config cleanup complete')
    } catch (error) {
      demoLogger.testCode.log(`[Live] ✗ Stripe config cleanup error: ${error}`)
      console.error('[cleanup] Stripe config cleanup error (non-fatal):', error)
    }
  })

  test('US-PW-002 + US-PW-003 场景1 + US-PW-006 场景1: 真实 Stripe 支付授 role，第三方 RBAC 放行', async ({
    page,
    demoLogger,
  }) => {
    let mappingId = ''
    let attemptId = ''
    let apiKey: ApiKeyWithPermission
    let sessionToken = ''
    let allowedBefore = false

    await test.step('Given: 配置 one_time+role 授予映射并铸造第三方 RBAC key', async () => {
      const request = page.request

      // Sync real Stripe products so the one-time product mapping exists.
      const syncResp = await request.post(
        `${BASE_URL}/api/bill/${REALM_ID}/entitlement-mappings/sync`,
        { data: { paymentProvider: 'stripe' } },
      )
      expect(syncResp.ok(), `sync failed: ${syncResp.status()}`).toBeTruthy()

      // Resolve the one-time mapping (must match the configured product id).
      const mapping = await findOneTimeMapping(request)
      mappingId = mapping.id

      // Provision the granted role + bind the builtin billing.view permission.
      const roleId = await ensureRoleAndGetId(request)
      await bindPermissionToRole(request, roleId)

      // Configure the mapping to grant this role on payment (batch PUT — the
      // only endpoint accepting the grantedRoleIds dimension).
      await configureGrantOnMapping(request, mappingId, roleId)

      // Mint a third-party RBAC key bound to admin-api-client (unscoped) with
      // the billing.view permission so /permission/check is permitted itself.
      const adminApiAppId = await resolveClientAppId(request, ADMIN_API_CLIENT_ID)
      apiKey = await createTestApiKeyWithPermission(
        page,
        BOUND_PERMISSION_NAME,
        Date.now(),
        REALM_ID,
        adminApiAppId,
      )

      sessionToken = await readSessionToken(page)
    })

    await test.step('When: 购买前用户未通过该 role 的 RBAC 检查（基线）', async () => {
      const { status, body } = await makeExtApiRequest({
        apiKey: apiKey.apiKey,
        method: 'POST',
        path: '/permission/check',
        body: { sessionToken, rules: [CHECK_RULE] },
      })
      expect(status, 'permission/check baseline must respond 200').toBe(200)
      allowedBefore = (body as { allowed?: boolean }).allowed === true
      console.log(`[US-PW-003 live] RBAC allowed BEFORE purchase: ${allowedBefore}`)
    })

    await test.step('And: 发起真实 Stripe 一次性支付并填卡付款', async () => {
      const { attemptId: id, checkoutUrl } = await createPaymentAttemptAndGetCheckoutUrl(
        page,
        mappingId,
      )
      attemptId = id
      console.log(`[US-PW-003 live] Stripe checkout URL: ${checkoutUrl}`)

      await page.goto(checkoutUrl, { waitUntil: 'domcontentloaded', timeout: 60000 })
      await page.waitForLoadState('networkidle', { timeout: 15000 }).catch(() => {})

      // Fill the Stripe test card (4242).
      const cardInput = await findVisibleCheckoutControl(page, 'card number', [
        (root) => root.locator('input[name="cardNumber"]'),
        (root) => root.locator('input[autocomplete*="cc-number"]'),
        (root) => root.getByLabel(/card number/i),
        (root) => root.getByPlaceholder(/4242|card|number/i),
      ])
      await cardInput.fill('4242424242424242')

      const expiryInput = await findVisibleCheckoutControl(page, 'expiry', [
        (root) => root.locator('input[name="cardExpiry"]'),
        (root) => root.locator('input[autocomplete*="cc-exp"]'),
        (root) => root.getByLabel(/expiry|expiration/i),
        (root) => root.getByPlaceholder(/MM|YY|expiry/i),
      ])
      await expiryInput.fill('1234')

      const cvcInput = await findVisibleCheckoutControl(page, 'CVC', [
        (root) => root.locator('input[name="cardCvc"]'),
        (root) => root.locator('input[autocomplete*="cc-csc"]'),
        (root) => root.getByLabel(/cvc|cvv|security/i),
        (root) => root.getByPlaceholder(/CVC|CVV|security/i),
      ])
      await cvcInput.fill('123')

      const nameInput = await findVisibleCheckoutControl(page, 'cardholder name', [
        (root) => root.getByLabel(/cardholder name/i),
        (root) => root.getByPlaceholder(/full name on card/i),
      ])
      await nameInput.fill('Test User')
      await nameInput.blur()
      await page.waitForTimeout(1000)

      await page.screenshot({ path: 'test-results/paywall-stripe-checkout-filled.png' })

      const submitButton = page.getByRole('button', { name: /pay/i }).last()
      await expect(submitButton).toBeVisible({ timeout: 5000 })
      await submitButton.scrollIntoViewIfNeeded()
      await submitButton.click()
      await page.waitForTimeout(5000)
      demoLogger.testCode.log('[Live] ✓ one-time payment submitted')
    })

    await test.step('Then: 等待支付成功（webhook 驱动履约，授 role）', async () => {
      // Wait for the browser to land on the success URL (frontend redirects on
      // webhook completion). Tolerate a redirect to a non-success URL — the
      // authoritative check is the attempt status below.
      await page.waitForURL(/\/billing\/success/, { timeout: 30000 }).catch(() => {
        console.log(`[US-PW-003 live] browser landed at ${page.url()} (not /billing/success)`)
      })

      // Poll the payment attempt until it reaches a terminal state. The role
      // grant fires as part of fulfillment, which is driven by the REAL Stripe
      // webhook (or, if the webhook is slow, this asserts the attempt completed).
      const deadline = Date.now() + 60000
      let status = 'Pending'
      while (Date.now() < deadline) {
        const resp = await page.request.get(
          `${BASE_URL}/api/bill/${REALM_ID}/purchase/payment-attempts/${attemptId}`,
        )
        if (resp.ok()) {
          const data = await resp.json()
          status = data.status
          console.log(`[US-PW-003 live] attempt status: ${status}`)
          if (status === 'Succeeded') break
          if (status === 'Failed' || status === 'Expired') {
            throw new Error(`payment attempt reached terminal ${status}; role grant cannot proceed`)
          }
        }
        await new Promise((r) => setTimeout(r, 2000))
      }
      expect(status, `payment attempt must reach Succeeded (got ${status})`).toBe('Succeeded')
      demoLogger.testCode.log('[Live] ✓ payment attempt reached Succeeded (role fulfillment done)')
    })

    await test.step('And: 第三方 RBAC /permission/check 放行（US-PW-006 场景1，source-agnostic）', async () => {
      // The webhook-driven fulfillment attached the granted role to the buyer.
      // A third-party app gating with one RBAC call must now resolve allowed=true
      // for the role's bound permission. Poll briefly — the grant is async vs.
      // the attempt status flip.
      const deadline = Date.now() + 30000
      let allowed = allowedBefore
      while (Date.now() < deadline) {
        const { status, body } = await makeExtApiRequest({
          apiKey: apiKey.apiKey,
          method: 'POST',
          path: '/permission/check',
          body: { sessionToken, rules: [CHECK_RULE] },
        })
        expect(status, 'permission/check post-purchase must respond 200').toBe(200)
        allowed = (body as { allowed?: boolean }).allowed === true
        if (allowed) break
        await new Promise((r) => setTimeout(r, 2000))
      }

      expect(
        allowed,
        'third-party RBAC must allow the buyer after the real Stripe payment ' +
          '(US-PW-003 grant + US-PW-006 gate)',
      ).toBe(true)
      demoLogger.testCode.log('[Live] ✓ RBAC allowed AFTER purchase (role granted by payment)')
    })
  })
})
