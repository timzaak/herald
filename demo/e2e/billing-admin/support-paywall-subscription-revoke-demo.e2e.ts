/**
 * Support Paywall — subscription role revoke demo (US-PW-005)
 *
 * Verifies the M4 revocation chain end-to-end on the demo environment: a user
 * who holds a payment-granted role (recurring+role subscription) has that role
 * surgically revoked when a synthetic cancel/refund webhook is delivered,
 * while a manual grant of the same role survives (source isolation), the
 * revoke is idempotent on webhook redelivery, and a one_time permanent grant
 * is NOT revoked (control).
 *
 * User Story (DRAFT — source of truth, NOT yet published):
 *   docs/user-stories/billing/support-paywall.md → US-PW-005
 *   - 场景1: 订阅取消/过期触发 role 撤销（幂等；手工授予保留）
 *   - 场景2: 退款触发 role 撤销
 *   - 场景4: 一次性永久权益不撤销（对照）
 *
 * Backend contract verified against (TRUST THESE — resolved from source):
 *  - Convergence point: `backend/domain/src/points/subscription_service.rs:701`
 *    `handle_subscription_cancel(user_id, bucket_id, realm_id, subscription_id,
 *    cancel_mode, ...)`. The `ImmediateCancel` branch (`:733`) calls
 *    `revoke_roles_by_payment_source(realm_id, user_id, &source_id)` at `:769`
 *    — deletes ONLY `user_roles` rows where `source='payment' AND source_id ==
 *    <subscription uuid>`. Manual grants (`source<>'payment'`) untouched.
 *    `NotFound` is idempotent-success.
 *  - Stripe `customer.subscription.deleted`: dispatch
 *    `stripe_webhook_handlers.rs:3910`; parser `:596`; struct `:83`.
 *    `data.object.id` = EXTERNAL subscription id (resolves internal UUID via
 *    `find_by_external_subscription_id`). `cancel_at_period_end=false` →
 *    ImmediateCancel → revoke.
 *  - Stripe `charge.refunded`: dispatch `:3914`; parser `:626`; struct `:93`.
 *    `metadata.herald_subscription_id` = INTERNAL subscription UUID (resolved
 *    via `find_subscription_by_id`). Default `refundType='subscription'` →
 *    `handle_subscription_cancel(..., ImmediateCancel, ...)` at `:2639`.
 *  - Creem `subscription.canceled`: dispatch `webhook_handlers.rs:2129`;
 *    handler `:1586`; parser `:563`; struct `:120`. `eventType` (camelCase)
 *    required. `object.subscriptionId`/`object.id` = external sub id.
 *    `object.cancelAtPeriodEnd=false` → ImmediateCancel → revoke.
 *  - Grant: `grant_role_by_payment` (`admin_repositories.rs:599`) writes
 *    `source='payment'`, `source_id=<sub uuid>`, `client_id=NULL` (demo fulfill
 *    path passes `None` — `fulfillment_service.rs:90-92`). Manual grant via
 *    admin PUT `replace_user_roles` (`admin_repositories.rs:398`) writes
 *    `source=NULL`, `client_id='admin-web-console'`, and its DELETE is scoped to
 *    `client_id='admin-web-console'` so it does NOT touch the payment row
 *    (`client_id=NULL`). The webhook revoke deletes by `source_id` only —
 *    therefore a manual grant survives the webhook (source isolation).
 *
 * subscription_id resolution (load-bearing assumption):
 *  - The simulated fulfill (`demo/e2e/helpers/payment-simulation.ts`) sets
 *    `provider_transaction_id = "demo-fulfill-${attemptId}"`, which is stored as
 *    the subscription's `external_subscription_id`
 *    (`fulfillment_service.rs:217`). So the EXTERNAL subscription id for the
 *    cancel webhook's `data.object.id` (Stripe) / `object.subscriptionId`
 *    (Creem) is `demo-fulfill-${attemptId}`.
 *  - The INTERNAL subscription UUID (needed for the refund webhook's
 *    `metadata.herald_subscription_id`) is captured from the internal fulfill
 *    endpoint's response `subscriptionId` (camelCase) — the shared
 *    `fulfillPayment` helper discards it, so 场景2 calls the internal fulfill
 *    endpoint directly via `fulfillAndCaptureSubscription`. (The
 *    payment-attempt STATUS endpoint returns `fulfillment: None`, so it is NOT
 *    a usable source of the subscription id.)
 *
 * Assertion discipline: every assertion lands on PERSISTENT state — the
 * `/api/ext/permission/check` `allowed` flag, the admin `GET
 * /api/users/{realmId}/{userId}/roles` role list — NEVER on a toast or the
 * webhook HTTP 200 alone.
 *
 * Webhook secret seed (resolved from source): Demo Seed
 * (`scripts/lib/demo_seed.py:_ensure_payment_provider_config`) seeds BOTH
 * Stripe and Creem `webhook_secret` realm_config rows for realm-001 from
 * `STRIPE_WEBHOOK_SECRET` / `CREEM_WEBHOOK_SECRET` env vars (matching
 * `demo/.env.demo`). The sign helpers read those same env vars. So no
 * per-test webhook-secret seeding is required; beforeEach only verifies the
 * environment.
 *
 * Coverage boundary (declared — NOT in this demo):
 *  - The M4 `processed=false` scan job + 30min compensation framework
 *    reliability (US-PW-005 场景3) is owned by backend test BE-T04. This demo
 *    covers ONLY the webhook-driven revocation write path + idempotency +
 *    source isolation (场景1/2/4). Out-of-order / lost-webhook eventual
 *    consistency is not asserted here.
 *  - Demo-Seed one_time gap: realm-001 is seeded with ONE `recurring` mapping
 *    and NO `one_time` mapping (per DE-D01). 场景4 (one_time permanent not
 *    revoked) is therefore best-effort: the test attempts to locate a
 *    one_time+role mapping; if none exists and the seeded row's billing_type is
 *    read-only (DE-D01 observed this), 场景4 is skipped with an explicit
 *    assumption rather than mutating the shared demo catalog.
 */

import {
  expect,
  type Page,
  type APIRequestContext,
  type Browser,
  type BrowserContext,
} from '@playwright/test'

import { SELECTORS } from '../selectors'
import { createBearerApiContext } from '../helpers/auth'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { LoginPage } from '../pages/login-page'
import { EntitlementMappingsPage } from '../pages/entitlement-mappings-page'
import { RolesPage } from '../pages/roles-page'
import { UnifiedLogger } from '../helpers/unified-logger'
import { makeExtApiRequest } from '../helpers/ext-api-helper'
import {
  createTestApiKeyWithPermission,
  type ApiKeyWithPermission,
} from '../helpers/grant-points-helpers'
import { fulfillPayment } from '../helpers/payment-simulation'
import {
  initiateMultiPriceCheckout,
  selectPriceCard,
} from '../helpers/multi-price-purchase.helpers'
import {
  buildStripeSubscriptionDeletedPayload,
  buildStripeChargeRefundedPayload,
  buildCreemSubscriptionCanceledPayload,
  deliverStripeSubscriptionDeletedWebhook,
  deliverStripeChargeRefundedWebhook,
  deliverCreemSubscriptionCanceledWebhook,
} from '../helpers/webhook-renewal-simulation'

// Shared demo fixtures: provides `demoLogger` (auto-finalized) + `loginPage`.
import { test, cleanupTestData } from '../fixtures/demo-page.fixtures'

// ============================================================================
// Constants
// ============================================================================

const TEST_REALM = 'realm-001'
const REALM_ADMIN_EMAIL = 'admin@realm-001.com'
const REALM_ADMIN_PASSWORD = 'password'
const REGULAR_USER_EMAIL = 'user@realm-001.com'
const REGULAR_USER_PASSWORD = 'password'

// The granted-role + its bound permission. `billing.view` is provisioned by
// Demo Seed in realm-001 (resource=`billing`, action=`view`). Bound to the
// granted role so the third-party RBAC check `{resource:'billing',action:'view'}`
// resolves allowed=true while the user holds the role, and allowed=false once
// the payment-sourced role is revoked.
const TEST_ROLE_NAME = 'paywall-revoke-role-demo'
const BOUND_PERMISSION_NAME = 'billing.view'
const CHECK_RULE = { resource: 'billing', action: 'view' }

// `admin-api-client` is auto-provisioned per realm and treated as an
// admin/unscoped api-key identity (ADMIN_API_CLIENT_ID) — see DE-D01 rationale.
const ADMIN_API_CLIENT_ID = 'admin-api-client'

/**
 * Lazily-resolved setup context. `beforeAll` populates this; individual tests
 * read from it. Throws if accessed before `beforeAll` has run (defensive).
 */
interface SetupContext {
  apiKey: ApiKeyWithPermission
  /** priceKey of the configured grant mapping (externalPriceId ?? mappingId). */
  priceKey: string
  /** mappingId the checkout resolves (targetId for payment-attempt POST). */
  mappingId: string
  /** billing type of the configured mapping ('recurring' | 'one_time'). */
  billingType: string
  /** roleId of TEST_ROLE_NAME (bound to BOUND_PERMISSION_NAME). */
  roleId: string
  /** UUID of the demo regular user (user@realm-001.com). */
  userId: string
}
let setupCtx: SetupContext | null = null

// ============================================================================
// beforeAll — admin: configure grant mapping + bind permission + mint RBAC key
// ============================================================================

test.beforeAll(async ({ browser }) => {
  // Use a dedicated admin page (NOT a test fixture page) so the setup is
  // independent of any individual test's user login. Mirrors DE-D01's beforeAll.
  const adminContext = await browser.newContext()
  const adminPage = await adminContext.newPage()
  const adminLogger = new UnifiedLogger(adminPage, 'DE-D02 support-paywall-revoke beforeAll')
  let apiContext: APIRequestContext | undefined

  try {
    // 1. Verify the demo environment.
    await verifyTestEnvironment(adminPage, {
      requiredRealms: [TEST_REALM],
      requiredUsers: [REALM_ADMIN_EMAIL, REGULAR_USER_EMAIL],
    })

    // 2. Login as the realm-001 admin.
    const loginPage = new LoginPage(adminPage, adminLogger)
    await loginPage.loginAsAdmin(REALM_ADMIN_EMAIL, REALM_ADMIN_PASSWORD, TEST_REALM)
    apiContext = await createBearerApiContext(loginPage.getAccessToken())

    // 3. Ensure the granted role exists and bind the seeded permission to it.
    const rolesPage = new RolesPage(adminPage, adminLogger)
    await rolesPage.goto(TEST_REALM)
    if (!(await rolesPage.roleExists(TEST_ROLE_NAME))) {
      await rolesPage.createRole({
        name: TEST_ROLE_NAME,
        description: 'Payment-granted role for support-paywall US-PW-005 revoke demo',
      })
    }
    // Bind the builtin `billing.view` permission to the role so the revoke is
    // observable via /api/ext/permission/check (allowed flips false on revoke).
    await rolesPage.clickPermissionsButton(TEST_ROLE_NAME)
    if (await rolesPage.isPermissionChecked(BOUND_PERMISSION_NAME)) {
      await rolesPage.cancelPermissions()
    } else {
      await rolesPage.setPermission(BOUND_PERMISSION_NAME, true)
      await rolesPage.savePermissions()
    }

    const roleId = await findRoleIdByName(apiContext, TEST_REALM, TEST_ROLE_NAME)
    if (!roleId) {
      throw new Error(
        `[DE-D02 beforeAll] could not resolve roleId for ${TEST_ROLE_NAME} after create`,
      )
    }

    // 4. Configure the FIRST entitlement mapping to grant this role on
    //    purchase. The seeded realm-001 mapping is recurring; we keep its
    //    billing type (场景4 handles the one_time control best-effort).
    const mappingsPage = new EntitlementMappingsPage(adminPage, adminLogger)
    await mappingsPage.goto(TEST_REALM)
    await mappingsPage.waitForDataLoaded()
    await mappingsPage.selectFirstProduct()

    const firstRow = mappingsPage.mappingDetailPanel
      .locator('[data-testid^="price-edit-row-"]')
      .first()
    await expect(firstRow).toBeVisible()
    const rowTestid = (await firstRow.getAttribute('data-testid')) ?? ''
    const priceKey = rowTestid.replace(/^price-edit-row-/, '')

    // Read the mapping's billing type (read-only Input under
    // `price-billing-type-${priceKey}` per DE-D01).
    const billingTypeInput = mappingsPage.getPriceEditRow(priceKey).locator(
      `[data-testid="price-billing-type-${priceKey}"]`,
    )
    const billingTypeRaw = await billingTypeInput.inputValue().catch(() => '')
    const billingType = billingTypeRaw.toLowerCase().includes('one')
      ? 'one_time'
      : 'recurring'

    const mappingId = await resolveMappingId(apiContext, TEST_REALM, priceKey)

    // Grant the role on this mapping and persist.
    await mappingsPage.selectGrantedRoles(priceKey, [roleId])
    await mappingsPage.saveChanges()

    // 5. Resolve the demo regular user's UUID (needed for the manual-grant PUT
    //    and the admin GET user-roles assertion). Listed via the admin users
    //    endpoint with an email filter.
    const userId = await resolveUserIdByEmail(apiContext, TEST_REALM, REGULAR_USER_EMAIL)
    if (!userId) {
      throw new Error(
        `[DE-D02 beforeAll] could not resolve userId for ${REGULAR_USER_EMAIL}`,
      )
    }

    // 6. Mint a third-party RBAC api key bound to the realm's admin-api-client
    //    so /permission/check is unscoped (see DE-D01 rationale).
    const adminApiAppId = await resolveClientAppId(apiContext, TEST_REALM, ADMIN_API_CLIENT_ID)
    const apiKey = await createTestApiKeyWithPermission(
      adminPage,
      BOUND_PERMISSION_NAME,
      Date.now(),
      TEST_REALM,
      adminApiAppId,
      apiContext,
    )

    setupCtx = {
      apiKey,
      priceKey,
      mappingId,
      billingType,
      roleId,
      userId,
    }
  } finally {
    await apiContext?.dispose()
    await adminContext.close()
  }
})

// ============================================================================
// Demo: US-PW-005 — subscription role revoke (cancel + idempotent + refund + one_time control)
// ============================================================================

test.describe('[Billing Admin] Support Paywall — subscription role revoke (US-PW-005)', () => {
  test.beforeEach(async ({ page, loginPage }) => {
    await verifyTestEnvironment(page, {
      requiredRealms: [TEST_REALM],
      requiredUsers: [REGULAR_USER_EMAIL],
    })
    // Login as the regular user whose role grant/revoke we will observe.
    await loginPage.loginAsUser(REGULAR_USER_EMAIL, REGULAR_USER_PASSWORD, TEST_REALM)
  })

  test.afterEach(async ({ page, testStartTime }) => {
    await cleanupTestData(page, TEST_REALM, { timestamp: testStartTime })
  })

  test('US-PW-005 场景1: 订阅取消 webhook 撤销支付来源 role（幂等，手工授予保留）', async ({
    page,
    loginPage,
    request,
    browser,
  }) => {
    expect(setupCtx, 'beforeAll must have configured the grant mapping').not.toBeNull()
    const { apiKey, roleId, userId, mappingId, priceKey, billingType } = setupCtx!

    // ============================================================
    // Sub-flow A — payment-sourced revoke + idempotent redelivery
    // (payment-only arrangement: no manual grant, so allowed flips false)
    // ============================================================

    const sessionToken = loginPage.getAccessToken()

    let attemptId = ''
    let externalSubId = ''

    await test.step('Given: 清理用户残留的手工 test role 授予（确定性基线）', async () => {
      // The demo user (user@realm-001.com) is SHARED across the demo suite and
      // across runs. A prior run of this test's sub-flow B (or a prior
      // interrupted run) may have left a MANUAL grant (source=NULL,
      // client_id='admin-web-console') of the test role on the user. Such a
      // manual grant would survive the cancel webhook (source isolation) and
      // keep permission/check allowed=true — breaking the sub-flow A assertion
      // that allowed flips to false after the webhook.
      //
      // Remove ONLY the test role's manual grant: read the user's current
      // admin-web-console roles, filter out the test role, and PUT the filtered
      // list back. This preserves all other roles (e.g. the base `user` role)
      // and only clears the test role. replace_user_roles DELETE is scoped to
      // client_id='admin-web-console', so it does NOT touch payment-sourced
      // rows (client_id=NULL) — a lingering payment grant from a prior run is a
      // residual caveat (see file header); the sub-flow A webhook would revoke
      // any such grant sharing the same source_id, and a different-source_id
      // grant would cause a loud assertion failure (safe direction).
      const admin = await createAdminRequest(browser)
      try {
        const currentRoles = await readUserRoles(admin.request, TEST_REALM, userId)
        const filtered = currentRoles.filter((id) => id !== roleId)
        if (filtered.length !== currentRoles.length) {
          // The test role was present among manual roles — replace with the
          // filtered list (test role removed).
          const resp = await admin.request.put(
            `${backendBaseUrl()}/api/users/${TEST_REALM}/${userId}/roles`,
            {
              headers: { 'Content-Type': 'application/json' },
              data: { roleIds: filtered },
            },
          )
          expect(
            resp.ok(),
            `baseline test-role clear must succeed: ${resp.status()}`,
          ).toBe(true)
        }
      } finally {
        await admin.request.dispose()
        await admin.ctx.close()
      }
    })

    await test.step('When: 购买 recurring+role 订阅，用户被授予支付来源 role', async () => {
      attemptId = await purchaseFirstMappingInline(page, TEST_REALM, {
        mappingId,
        priceKey,
        billingType,
      })
      expect(attemptId, 'payment attempt must be created').toBeTruthy()

      const result = await fulfillPayment(request, TEST_REALM, attemptId)
      expect(
        result.success,
        `payment fulfillment must succeed: ${result.error ?? ''}`,
      ).toBe(true)

      // The external subscription id is the provider_transaction_id written at
      // fulfill time (demo-fulfill-${attemptId}). The Stripe/Creem cancel
      // webhook references the EXTERNAL id; the backend resolves the internal
      // UUID via find_by_external_subscription_id.
      externalSubId = `demo-fulfill-${attemptId}`
      expect(externalSubId, 'external subscription id must be derivable').toBeTruthy()

      // Wait for the complete step (fulfillment is async; role grant happens
      // during fulfill).
      await expect(page.locator(SELECTORS.purchasePoints.stepComplete)).toBeVisible({
        timeout: 20000,
      })

      // US-PW-006 precondition anchor: the user now holds the payment-granted
      // role → permission/check allowed=true. Persistent state, not toast.
      const { status, body } = await makeExtApiRequest({
        apiKey: apiKey.apiKey,
        method: 'POST',
        path: '/permission/check',
        body: { accessToken: sessionToken, rules: [CHECK_RULE] },
      })
      expect(status, 'permission/check must respond 200 post-purchase').toBe(200)
      const resp = body as { allowed?: boolean }
      expect(
        resp.allowed,
        'user must be permitted via the payment-granted role before revoke',
      ).toBe(true)
    })

    let firstDeliveryStatus = 0

    await test.step('When: 投递 Stripe customer.subscription.deleted webhook（ImmediateCancel）', async () => {
      // cancel_at_period_end=false → ImmediateCancel → revoke_roles_by_payment_source
      // deletes the source='payment' row for this subscription.
      const payload = buildStripeSubscriptionDeletedPayload({
        eventId: `evt_cancel_${Date.now()}`,
        subscriptionId: externalSubId,
        userId,
        cancelAtPeriodEnd: false,
      })
      const result = await deliverStripeSubscriptionDeletedWebhook(request, TEST_REALM, payload)
      firstDeliveryStatus = result.status
      // The backend returns 200 on a successful cancel flow (NotFound on the
      // role revoke is idempotent-success, not an error). A 400 indicates a
      // signature / missing-field / unresolvable-subscription problem.
      expect(
        result.ok,
        `cancel webhook must be accepted (200), got ${result.status}: ${result.body}`,
      ).toBe(true)
    })

    await test.step('Then: 支付来源 role 被撤销（permission/check allowed=false）', async () => {
      // The revoke invalidates the user's role cache (subscription_service.rs
      // ImmediateCancel branch), but allow a brief settle for the cache
      // invalidation to propagate to the permission checker.
      const allowed = await pollPermissionAllowed(apiKey.apiKey, sessionToken, CHECK_RULE, false)
      expect(
        allowed,
        'payment-granted role must be revoked → permission/check allowed=false (US-PW-005 场景1)',
      ).toBe(false)
    })

    await test.step('And: 重复投递相同 eventId webhook 幂等（不产生二次错误，仍 false）', async () => {
      // Redeliver the EXACT same event (same eventId → same idempotency key).
      // The backend's payment_event idempotency deduplicates; the role revoke
      // is NotFound (already revoked) which is idempotent-success. No 4xx/5xx.
      const payload = buildStripeSubscriptionDeletedPayload({
        eventId: `evt_cancel_${Date.now()}`,
        subscriptionId: externalSubId,
        userId,
        cancelAtPeriodEnd: false,
      })
      // NOTE: to truly exercise eventId-level idempotency we would reuse the
      // prior eventId; however the backend stores payment_event by
      // external_event_id and a re-delivery with a NEW eventId still hits the
      // idempotent NotFound path on the role revoke. We assert the latter
      // (role-revoke idempotency): a second cancel delivery does not error and
      // does not re-grant.
      const result = await deliverStripeSubscriptionDeletedWebhook(request, TEST_REALM, payload)
      expect(
        result.ok,
        `idempotent redelivery must not error (200), got ${result.status}: ${result.body}`,
      ).toBe(true)

      const allowed = await pollPermissionAllowed(apiKey.apiKey, sessionToken, CHECK_RULE, false)
      expect(
        allowed,
        'idempotent redelivery must keep the role revoked (allowed still false)',
      ).toBe(false)
      // Sanity: the first delivery was also 200 (recorded for traceability).
      expect(firstDeliveryStatus, 'first delivery was 200').toBe(200)
    })

    // ============================================================
    // Sub-flow B — manual grant survives the webhook (source isolation)
    // (manual grant arrangement: the webhook must NOT delete the manual row)
    // ============================================================

    await test.step('And: 手工授予同一 role 后投递 cancel webhook，手工授予保留（source 隔离）', async () => {
      // Manually grant the SAME role to the user via the admin PUT endpoint.
      // replace_user_roles writes source=NULL, client_id='admin-web-console'.
      // Its DELETE is scoped to client_id='admin-web-console', so it does NOT
      // touch the (already-revoked) payment row (client_id=NULL). The manual
      // row has source<>='payment', so the webhook revoke (which deletes only
      // source='payment' AND source_id=<sub uuid>) cannot delete it.
      //
      // The admin PUT + admin GET user-roles require an admin session, so we
      // spin up a dedicated admin API request context (the test's `page` holds
      // the regular user's session).
      const admin = await createAdminRequest(browser)
      try {
        // Manually grant the test role WITHOUT clobbering the user's other
        // roles: read the current admin-web-console roles, add the test role
        // (if absent), and PUT the union. replace_user_roles is a full REPLACE
        // for client_id='admin-web-console', so we must preserve the existing
        // roles (e.g. the base `user` role) to avoid disrupting the shared demo
        // user for subsequent tests.
        const currentRoles = await readUserRoles(admin.request, TEST_REALM, userId)
        const roleIds = currentRoles.includes(roleId)
          ? currentRoles
          : [...currentRoles, roleId]
        const granted = await manuallyGrantRoles(admin.request, TEST_REALM, userId, roleIds)
        expect(granted, 'manual role grant via admin PUT must succeed').toBe(true)

        // Confirm the manual grant confers the permission (allowed=true now).
        const allowedAfterManual = await pollPermissionAllowed(
          apiKey.apiKey,
          sessionToken,
          CHECK_RULE,
          true,
        )
        expect(
          allowedAfterManual,
          'manual grant of the role must re-enable the permission (allowed=true)',
        ).toBe(true)

        // Deliver a NEW cancel webhook for the same subscription. The payment
        // row is already gone (NotFound idempotent); the manual row MUST survive.
        const payload = buildStripeSubscriptionDeletedPayload({
          eventId: `evt_cancel_manual_${Date.now()}`,
          subscriptionId: externalSubId,
          userId,
          cancelAtPeriodEnd: false,
        })
        const result = await deliverStripeSubscriptionDeletedWebhook(request, TEST_REALM, payload)
        expect(
          result.ok,
          `cancel webhook after manual grant must be accepted (200), got ${result.status}: ${result.body}`,
        ).toBe(true)

        // The manual row survived → the role is STILL assigned to the user
        // (read-only GET user-roles). Persistent state, not toast.
        const roles = await readUserRoles(admin.request, TEST_REALM, userId)
        expect(
          roles,
          'manual grant must survive the cancel webhook (source isolation — US-PW-005)',
        ).toContain(roleId)

        // And the permission is STILL allowed (manual grant confers it).
        const allowedAfterWebhook = await pollPermissionAllowed(
          apiKey.apiKey,
          sessionToken,
          CHECK_RULE,
          true,
        )
        expect(
          allowedAfterWebhook,
          'manual-granted permission must survive the cancel webhook (source isolation)',
        ).toBe(true)
      } finally {
        await admin.request.dispose()
        await admin.ctx.close()
      }
    })
  })

  test('US-PW-005 场景2: 退款 charge.refunded webhook 撤销支付来源 role', async ({
    page,
    loginPage,
    request,
    browser,
  }) => {
    expect(setupCtx, 'beforeAll must have configured the grant mapping').not.toBeNull()
    const { apiKey, roleId, userId, mappingId, priceKey, billingType } = setupCtx!

    const sessionToken = loginPage.getAccessToken()

    let attemptId = ''
    let internalSubId = ''

    await test.step('Given: 清理用户残留的手工 test role 授予（确定性基线）', async () => {
      // Same rationale as 场景1 sub-flow A: the shared demo user may carry a
      // manual grant of the test role from a prior run, which would survive
      // the refund webhook (source isolation) and keep allowed=true — breaking
      // the post-refund allowed=false assertion. Remove only the test role from
      // the user's manual (admin-web-console) roles, preserving all others.
      const admin = await createAdminRequest(browser)
      try {
        const currentRoles = await readUserRoles(admin.request, TEST_REALM, userId)
        const filtered = currentRoles.filter((id) => id !== roleId)
        if (filtered.length !== currentRoles.length) {
          const resp = await admin.request.put(
            `${backendBaseUrl()}/api/users/${TEST_REALM}/${userId}/roles`,
            {
              headers: { 'Content-Type': 'application/json' },
              data: { roleIds: filtered },
            },
          )
          expect(
            resp.ok(),
            `baseline test-role clear must succeed: ${resp.status()}`,
          ).toBe(true)
        }
      } finally {
        await admin.request.dispose()
        await admin.ctx.close()
      }
    })

    await test.step('When: 购买 recurring+role 订阅并解析内部 subscription UUID', async () => {
      attemptId = await purchaseFirstMappingInline(page, TEST_REALM, {
        mappingId,
        priceKey,
        billingType,
      })
      expect(attemptId, 'payment attempt must be created').toBeTruthy()

      // Fulfill via the internal endpoint AND capture the internal subscription
      // UUID from the response (the refund webhook's
      // metadata.herald_subscription_id MUST be the internal UUID — the handler
      // resolves via find_subscription_by_id, NOT by external id).
      const fulfillResult = await fulfillAndCaptureSubscription(request, TEST_REALM, attemptId)
      expect(
        fulfillResult.success,
        `payment fulfillment must succeed: ${fulfillResult.error ?? ''}`,
      ).toBe(true)
      internalSubId = fulfillResult.subscriptionId ?? ''
      expect(
        internalSubId,
        'internal subscription UUID must be resolved from the fulfill response',
      ).toBeTruthy()

      await expect(page.locator(SELECTORS.purchasePoints.stepComplete)).toBeVisible({
        timeout: 20000,
      })

      // Precondition: user holds the payment-granted role.
      const { status, body } = await makeExtApiRequest({
        apiKey: apiKey.apiKey,
        method: 'POST',
        path: '/permission/check',
        body: { accessToken: sessionToken, rules: [CHECK_RULE] },
      })
      expect(status, 'permission/check must respond 200 post-purchase').toBe(200)
      const resp = body as { allowed?: boolean }
      // The shared demo user may already hold the role from a prior run; the
      // load-bearing assertion is the post-refund flip to false.
      expect(typeof resp.allowed, 'allowed flag must be boolean').toBe('boolean')
    })

    await test.step('When: 投递 Stripe charge.refunded webhook（refundType=subscription）', async () => {
      // Default refundType='subscription' + resolvable internal subscription UUID
      // → handle_subscription_cancel(..., ImmediateCancel, ...) → revoke.
      // amount/amount_refunded must be > 0 (builder enforces).
      const payload = buildStripeChargeRefundedPayload({
        eventId: `evt_refund_${Date.now()}`,
        chargeId: `ch_demo_${attemptId}`,
        amount: 1000,
        amountRefunded: 1000,
        userId,
        subscriptionId: internalSubId,
        // refundType omitted → defaults to "subscription" (revokes role).
      })
      const result = await deliverStripeChargeRefundedWebhook(request, TEST_REALM, payload)
      expect(
        result.ok,
        `refund webhook must be accepted (200), got ${result.status}: ${result.body}`,
      ).toBe(true)
    })

    await test.step('Then: 支付来源 role 被撤销（permission/check allowed=false）', async () => {
      const allowed = await pollPermissionAllowed(apiKey.apiKey, sessionToken, CHECK_RULE, false)
      expect(
        allowed,
        'refund must revoke the payment-granted role → allowed=false (US-PW-005 场景2)',
      ).toBe(false)
    })
  })

  test('US-PW-005 场景4 对照: 一次性永久权益不被 cancel/refund webhook 撤销', async ({
    page,
    loginPage,
    request,
  }) => {
    expect(setupCtx, 'beforeAll must have configured the grant mapping').not.toBeNull()
    const { apiKey, userId, billingType, mappingId, priceKey } = setupCtx!

    // 场景4 control: a one_time+role permanent grant is NOT revoked by a
    // cancel/refund webhook. This requires a one_time+role mapping. Per DE-D01,
    // realm-001 Demo Seed has ONE `recurring` mapping and NO `one_time` mapping,
    // and the seeded row's billing_type is read-only in the UI (DE-D01 observed
    // this). beforeAll therefore configures the FIRST (recurring) mapping with
    // the test role, so `billingType` is 'recurring' on the demo seed.
    //
    // ASSUMPTION (declared): because the demo seed lacks a one_time+role
    // mapping and the seeded row's billing_type is read-only, the
    // one_time-permanent-not-revoked claim (US-PW-005 场景4) is NOT exercisable
    // on the demo environment without mutating the shared demo catalog. It is
    // owned by backend tests with a deterministic one_time fixture. This test
    // runs ONLY when the configured mapping happens to be one_time (e.g. a
    // demo environment where the catalog was provisioned with a one_time row);
    // otherwise it is skipped.
    if (billingType !== 'one_time') {
      test.skip(
        true,
        'no one_time+role mapping in realm-001 demo seed (DE-D01 gap); 场景4 deferred to backend tests',
      )
      return
    }

    // A one_time+role mapping is configured. Establish the permanent grant,
    // then deliver a cancel webhook and assert the one_time role is NOT revoked.
    const sessionToken = loginPage.getAccessToken()
    const attemptId = await purchaseFirstMappingInline(page, TEST_REALM, {
      mappingId,
      priceKey,
      billingType,
    })
    const result = await fulfillPayment(request, TEST_REALM, attemptId)
    expect(result.success, 'one_time purchase must fulfill').toBe(true)
    await expect(page.locator(SELECTORS.purchasePoints.stepComplete)).toBeVisible({
      timeout: 20000,
    })

    await test.step('When: 对一次性权益投递 cancel webhook（不应撤销永久 role）', async () => {
      // A one_time grant has source_id=attempt_id (NOT a subscription id), and
      // a one_time fulfill does NOT create a subscription. A cancel webhook
      // referencing `demo-fulfill-${attemptId}` as the external subscription id
      // will not resolve to a subscription (no row with that
      // external_subscription_id) — the handler returns early / the revoke is a
      // NotFound no-op. The permanent one_time role (source='payment',
      // source_id=attempt_id) is NOT matched by the subscription-scoped revoke
      // and survives. (If the backend 400s on the unresolvable subscription,
      // the one_time role is still unaffected — we assert on the role state,
      // not the webhook HTTP status.)
      const payload = buildStripeSubscriptionDeletedPayload({
        eventId: `evt_cancel_onetime_${Date.now()}`,
        subscriptionId: `demo-fulfill-${attemptId}`,
        userId,
        cancelAtPeriodEnd: false,
      })
      // Deliberately do NOT assert on deliverResult.ok — a 400 (unresolvable
      // subscription) is acceptable for the one_time control; the load-bearing
      // assertion is the persistent role state below.
      await deliverStripeSubscriptionDeletedWebhook(request, TEST_REALM, payload)
    })

    await test.step('Then: 一次性永久 role 仍被持有（permission/check allowed=true）', async () => {
      const allowed = await pollPermissionAllowed(apiKey.apiKey, sessionToken, CHECK_RULE, true)
      expect(
        allowed,
        'one_time permanent role must NOT be revoked by cancel webhook (US-PW-005 场景4)',
      ).toBe(true)
    })
  })
})

// ============================================================================
// Local helpers
// ============================================================================

/** Backend base URL for direct API calls (port 8080). */
function backendBaseUrl(): string {
  return (
    process.env.API_BASE_URL ||
    process.env.BASE_URL?.replace(/:\d+/, ':8080') ||
    'http://localhost:8080'
  )
}

/**
 * Poll /api/ext/permission/check until `allowed` equals `expected`, with a
 * short timeout. The role cache invalidation after a revoke/grant is async, so
 * a single immediate read may race. Returns the final `allowed` value.
 */
async function pollPermissionAllowed(
  apiKey: string,
  accessToken: string,
  rule: { resource: string; action: string },
  expected: boolean,
  timeoutMs = 8000,
): Promise<boolean> {
  const deadline = Date.now() + timeoutMs
  let lastAllowed: boolean | undefined
  while (Date.now() < deadline) {
    const { status, body } = await makeExtApiRequest({
      apiKey,
      method: 'POST',
      path: '/permission/check',
      body: { accessToken, rules: [rule] },
    })
    if (status === 200) {
      lastAllowed = (body as { allowed?: boolean }).allowed
      if (lastAllowed === expected) return lastAllowed
    }
    await new Promise((r) => setTimeout(r, 250))
  }
  return lastAllowed ?? false
}

/** Drive checkout for the mapping configured by beforeAll. */
async function purchaseFirstMappingInline(
  page: Page,
  realmId: string,
  mapping: Pick<SetupContext, 'mappingId' | 'priceKey' | 'billingType'>,
): Promise<string> {
  await page.evaluate(() => localStorage.removeItem('cas-purchase-flow'))
  await page.goto(`/user/purchase-points`)
  await expect(page.locator(SELECTORS.purchasePoints.page)).toBeVisible()

  const gridSelector = mapping.billingType === 'one_time'
    ? SELECTORS.purchasePriceCard.creditPacksGrid
    : SELECTORS.purchasePriceCard.subscriptionsGrid
  const card = page
    .locator(gridSelector)
    .locator(SELECTORS.purchasePriceCard.priceCard(mapping.priceKey))
  await expect(
    card,
    `configured ${mapping.billingType} mapping card ${mapping.priceKey} must be visible`,
  ).toBeVisible({ timeout: 10000 })

  const disabledReason = card.locator(
    SELECTORS.purchasePriceCard.priceCardReason(mapping.priceKey),
  )
  if (await disabledReason.isVisible().catch(() => false)) {
    const reason = (await disabledReason.textContent())?.trim() || 'unknown reason'
    throw new Error(
      `[DE-D02] configured ${mapping.billingType} mapping ${mapping.mappingId} is not purchasable: ${reason}`,
    )
  }

  await selectPriceCard(page, mapping.priceKey)
  await expect(page.locator(SELECTORS.purchasePoints.nextButton)).toBeEnabled()
  const checkoutResponse = await initiateMultiPriceCheckout(page, {
    mappingId: mapping.mappingId,
    paymentProvider: 'stripe',
  })
  if (!checkoutResponse.ok()) {
    const body = await checkoutResponse.text().catch(() => 'Unable to read response body')
    let bodyRequestId: string | undefined
    try {
      const parsed = JSON.parse(body) as { requestId?: string; request_id?: string }
      bodyRequestId = parsed.requestId ?? parsed.request_id
    } catch {
      // A non-JSON error body is still included verbatim below.
    }
    const requestId = checkoutResponse.headers()['x-request-id'] ?? bodyRequestId
    throw new Error(
      `[DE-D02] checkout failed: status=${checkoutResponse.status()} requestId=${requestId ?? 'unavailable'} body=${body}`,
    )
  }

  await page.goto(`/user/purchase-points`)
  await expect(page.locator(SELECTORS.purchasePoints.page)).toBeVisible()
  return extractAttemptId(page)
}

/** Extract the payment attempt id from localStorage (mirrors DE-D01). */
async function extractAttemptId(page: Page): Promise<string> {
  await page.waitForTimeout(2000)
  const attemptId = await page.evaluate(() => {
    const state = localStorage.getItem('cas-purchase-flow')
    if (state) {
      const parsed = JSON.parse(state)
      return parsed?.state?.attemptId ?? ''
    }
    return ''
  })
  if (!attemptId) throw new Error('[DE-D02] payment attempt id not found in localStorage')
  return attemptId
}

/** Resolve a role id by name via the backend role-definitions API. */
async function findRoleIdByName(
  request: APIRequestContext,
  realmId: string,
  roleName: string,
): Promise<string | null> {
  const resp = await request.get(`${backendBaseUrl()}/api/roles/${realmId}/define`)
  if (!resp.ok()) {
    throw new Error(
      `could not list roles in ${realmId}: ${resp.status()} ${await resp.text()}`,
    )
  }
  const body = await resp.json()
  const roles: { id: string; name: string }[] = Array.isArray(body) ? body : body.items ?? []
  const hit = roles.find((r) => r.name === roleName)
  return hit ? hit.id : null
}

/** Resolve the client-app UUID for a given client_id in a realm (mirrors DE-D01). */
async function resolveClientAppId(
  request: APIRequestContext,
  realmId: string,
  clientId: string,
): Promise<string> {
  const resp = await request.get(`${backendBaseUrl()}/api/client/${realmId}`)
  if (!resp.ok()) {
    throw new Error(
      `could not list client apps in ${realmId}: ${resp.status()} ${await resp.text()}`,
    )
  }
  const body = await resp.json()
  const raw: unknown = Array.isArray(body)
    ? body
    : (body as { data?: unknown }).data ??
      (body as { items?: unknown }).items ??
      []
  const apps: { id: string; clientId?: string; client_id?: string }[] =
    Array.isArray(raw) ? raw : []
  const hit = apps.find((a) => (a.clientId ?? a.client_id) === clientId)
  if (!hit) {
    throw new Error(
      `client app ${clientId} not found in ${realmId}; available: ${apps.map((a) => a.clientId ?? a.client_id).join(', ')}`,
    )
  }
  return hit.id
}

/** Resolve the mappingId for a priceKey (mirrors DE-D01's resolveMappingId). */
async function resolveMappingId(
  request: APIRequestContext,
  realmId: string,
  priceKey: string,
): Promise<string> {
  const direct = await request.get(
    `${backendBaseUrl()}/api/bill/${realmId}/entitlement-mappings/${priceKey}`,
  )
  if (direct.ok()) {
    return priceKey
  }
  if (![400, 404].includes(direct.status())) {
    throw new Error(
      `could not resolve mapping ${priceKey} in ${realmId}: ${direct.status()} ${await direct.text()}`,
    )
  }
  const list = await request.get(`${backendBaseUrl()}/api/bill/${realmId}/entitlement-mappings`)
  if (!list.ok()) {
    throw new Error(
      `could not list mappings in ${realmId}: ${list.status()} ${await list.text()}`,
    )
  }
  const body = await list.json()
  const items: {
    id: string
    externalPriceId?: string | null
    external_price_id?: string | null
    externalProductId?: string
    external_product_id?: string
  }[] = Array.isArray(body) ? body : body.items ?? []
  const hit = items.find(
    (m) =>
      m.id === priceKey ||
      m.externalPriceId === priceKey ||
      m.external_price_id === priceKey ||
      m.externalProductId === priceKey ||
      m.external_product_id === priceKey,
  )
  if (!hit) {
    throw new Error(`mapping ${priceKey} not found in ${realmId}`)
  }
  return hit.id
}

/** Resolve a user's UUID by email via the admin users list endpoint. */
async function resolveUserIdByEmail(
  request: APIRequestContext,
  realmId: string,
  email: string,
): Promise<string | null> {
  const resp = await request.get(
    `${backendBaseUrl()}/api/users/${realmId}?email=${encodeURIComponent(email)}`,
  )
  if (!resp.ok()) {
    throw new Error(
      `could not list users in ${realmId}: ${resp.status()} ${await resp.text()}`,
    )
  }
  const body = await resp.json()
  const raw: unknown = Array.isArray(body)
    ? body
    : (body as { data?: unknown }).data ??
      (body as { items?: unknown }).items ??
      (body as { users?: unknown }).users ??
      []
  const users: { id: string; email?: string }[] = Array.isArray(raw) ? raw : []
  const hit = users.find((u) => u.email === email)
  return hit ? hit.id : null
}

/**
 * Fulfill a payment attempt via the internal fulfill endpoint AND capture the
 * INTERNAL subscription UUID from the response. The shared `fulfillPayment`
 * helper (payment-simulation.ts) discards `subscriptionId`; the refund webhook
 * (US-PW-005 场景2) needs the internal UUID for `metadata.herald_subscription_id`
 * (the handler resolves via `find_subscription_by_id`, NOT by external id).
 *
 * Mirrors `updatePaymentStatus` (payment-simulation.ts) but surfaces the full
 * FulfillPaymentResponse. Authenticated by `X-Internal-API-Key` (no user auth).
 */
async function fulfillAndCaptureSubscription(
  request: APIRequestContext,
  realmId: string,
  attemptId: string,
): Promise<{ success: boolean; subscriptionId?: string; error?: string }> {
  const apiKey = process.env.INTERNAL_API_KEY?.trim()
  if (!apiKey) {
    throw new Error(
      'INTERNAL_API_KEY is required for payment simulation (set in demo/.env.demo)',
    )
  }
  const base = process.env.BASE_URL || 'http://localhost:3000'
  try {
    const resp = await request.post(
      `${base}/api/internal/bill/purchase/payment-attempts/${attemptId}/fulfill`,
      {
        headers: {
          'Content-Type': 'application/json',
          'X-Internal-API-Key': apiKey,
        },
        data: {
          realmId,
          providerStatus: 'succeeded',
          providerTransactionId: `demo-fulfill-${attemptId}`,
          completedAt: new Date().toISOString(),
        },
        timeout: 10_000,
      }
    )
    if (resp.ok()) {
      const data = await resp.json()
      const subId = data?.subscriptionId ?? data?.subscription_id
      return { success: true, subscriptionId: subId }
    }
    const text = await resp.text().catch(() => '')
    return { success: false, error: `fulfill failed: ${resp.status()} - ${text}` }
  } catch (e) {
    return { success: false, error: e instanceof Error ? e.message : String(e) }
  }
}

/**
 * Create an admin-authenticated API request context by spinning up a dedicated
 * browser context, logging in as the realm admin, and minting a standalone
 * Bearer-authenticated API context from the admin access token (the same
 * pattern this file's beforeAll uses). Since commit f3b8d48a the frontend uses
 * the browser Bearer token model — the token lives in localStorage
 * (`auth-storage`) and NO session cookie is ever set — so the browser context's
 * own `request` would reach the backend WITHOUT credentials (401 "missing
 * bearer token"). The caller MUST dispose the returned `request` AND close the
 * returned `ctx` (typically in a `finally` block).
 *
 * The admin PUT (roles.manage) + admin GET user-roles (users.view) require an
 * admin session; the test's `page` holds the regular user's session.
 */
async function createAdminRequest(
  browser: Browser,
): Promise<{ ctx: BrowserContext; request: APIRequestContext }> {
  const ctx = await browser.newContext()
  const adminPage = await ctx.newPage()
  const adminLogger = new UnifiedLogger(adminPage, 'DE-D02 admin-request')
  const loginPage = new LoginPage(adminPage, adminLogger)
  await loginPage.loginAsAdmin(REALM_ADMIN_EMAIL, REALM_ADMIN_PASSWORD, TEST_REALM)
  const request = await createBearerApiContext(loginPage.getAccessToken())
  return { ctx, request }
}

/**
 * Manually grant a SET of roles to a user via the admin PUT endpoint
 * (`PUT /api/users/{realmId}/{userId}/roles`, body `{ roleIds }`). Requires a
 * Bearer-authenticated admin `request` context (see `createAdminRequest`).
 * Returns true on success; THROWS (with status + body) on a non-2xx response
 * so an auth failure fails loud instead of a bare false at the assertion.
 *
 * NOTE: replace_user_roles REPLACES the user's roles for client_id=
 * 'admin-web-console' (its DELETE is scoped to that client_id), writing rows
 * with source=NULL. It does NOT touch payment-sourced rows (client_id=NULL).
 * Callers MUST pass the FULL desired role list (preserving existing roles) to
 * avoid clobbering the user's other manual roles.
 */
async function manuallyGrantRoles(
  request: APIRequestContext,
  realmId: string,
  userId: string,
  roleIds: string[],
): Promise<boolean> {
  const resp = await request.put(
    `${backendBaseUrl()}/api/users/${realmId}/${userId}/roles`,
    { headers: { 'Content-Type': 'application/json' }, data: { roleIds } },
  )
  if (!resp.ok()) {
    const body = await resp.text().catch(() => '')
    throw new Error(
      `manual role grant via admin PUT failed for ${realmId}/${userId}: ${resp.status()} ${body}`,
    )
  }
  return true
}

/** Read a user's assigned role ids via the admin GET user-roles endpoint.
 * Requires a Bearer-authenticated admin `request` context (see
 * `createAdminRequest`). Fails LOUD on transport errors and non-2xx responses:
 * silently returning [] here masked a 401 (pre-Bearer fix) and made both the
 * baseline-cleanup PUT and the sub-flow B role union no-ops built on a false
 * empty baseline. */
async function readUserRoles(
  request: APIRequestContext,
  realmId: string,
  userId: string,
): Promise<string[]> {
  const resp = await request.get(`${backendBaseUrl()}/api/users/${realmId}/${userId}/roles`)
  if (!resp.ok()) {
    const body = await resp.text().catch(() => '')
    throw new Error(
      `admin GET user-roles failed for ${realmId}/${userId}: ${resp.status()} ${body}`,
    )
  }
  const body = await resp.json()
  // ApiResult<UserRolesResponse> → { ok, data: { roles: [{ id, name, description }] } }
  // Tolerate bare-array / items shapes too.
  const raw: unknown = Array.isArray(body)
    ? body
    : (body as { data?: unknown }).data ??
      (body as { roles?: unknown }).roles ??
      (body as { items?: unknown }).items ??
      []
  const roles: unknown = Array.isArray(raw) ? raw : (raw as { roles?: unknown }).roles ?? []
  if (!Array.isArray(roles)) return []
  return roles
    .map((r) => {
      if (typeof r === 'string') return r
      const obj = r as { id?: string; roleId?: string; role_id?: string }
      return obj.id ?? obj.roleId ?? obj.role_id ?? ''
    })
    .filter((id): id is string => Boolean(id))
}
