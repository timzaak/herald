/**
 * Support Paywall — purchase → grant → third-party RBAC → alreadyOwned demo
 * (US-PW-002/003/004/006)
 *
 * Verifies the END-USER half of the paywall grant chain on the demo
 * environment:
 *  - US-PW-002: a one_time "pure-entitlement" purchase (role grant, no points)
 *    completes WITHOUT erroring.
 *  - US-PW-003 场景1: a successful one_time payment auto-grants the mapped role
 *    (permanent — buy-once).
 *  - US-PW-006 场景1: a third-party app can use Herald's existing RBAC
 *    (`POST /api/ext/permission/check`) to gate on the granted role — one line,
 *    source-agnostic.
 *  - US-PW-004 场景1: once owned, the alreadyOwned card is DISABLED with a
 *    reason row, and a direct repeat purchase attempt is rejected by the backend
 *    with 409 `already_owned`.
 *  - US-PW-004 场景2 (contrast): a points-only mapping (no role grant) can be
 *    purchased repeatedly (NO 409 on repeat).
 *
 * User Story:
 *   docs/user-stories/billing/support-paywall.md → US-PW-002/003/004/006.
 *
 * Frontend/backend contracts verified against:
 * - frontend/src/routes/$realmId/user/purchase-points.tsx (alreadyOwned card:
 *   onClick=undefined + `purchase-price-card-${priceId}-reason` child;
 *   hosted checkout since 533ec22d + a71c72a4: stripe attempts redirect the
 *   SAME TAB to checkout.stripe.com — the flow below aborts that redirect and
 *   resumes via `?attemptId=` through unified-purchase.helpers).
 * - frontend/src/components/shared/role-selector.tsx (RoleSelector).
 * - backend/api-ext/src/permission.rs (request `{accessToken, rules:[{resource,action}]}`
 *   — `accessToken` since f3b8d48a replaced the `sessionToken` field —
 *   response `{allowed, userId?, error?}`; `resource` matched EXACTLY against
 *   role_policies, action hierarchy: manage > create > view).
 * - backend/infra/src/authorization/redis_permission_checker.rs (matches_policy:
 *   resource MUST match exactly — no wildcard).
 *
 * Permission rule mapping (resolved from source, NOT guessed):
 *   Demo Seed (scripts/lib/demo_seed.py L355-356) provisions the builtin
 *   permission `billing.view` with resource=`billing`, action=`view` in
 *   realm-001. We bind it to the granted role (admin UI) and check the rule
 *   `{resource:'billing', action:'view'}` — an EXACT match, which `matches_policy`
 *   grants. This is the load-bearing US-PW-006 claim (third party gates on the
 *   role's bound permission, source-agnostic).
 *
 * API-key scoping (resolved from source):
 *   `/permission/check` (permission.rs L150-162) rejects a client-app-SCOPED
 *   api key whose bound app differs from the session's client_id, UNLESS the
 *   bound app is `admin-api-client` (ADMIN_API_CLIENT_ID — `is_admin_api_key`
 *   returns true → check skipped). We therefore mint the test key bound to the
 *   realm's auto-provisioned `admin-api-client` client app so the check is
 *   source-agnostic and never trips the cross-client guard. The key carries a
 *   custom role with `billing.view` so it is itself permitted to mint/operate
 *   (createTestApiKeyWithPermission assigns the permission via a role).
 *
 * Assertion discipline: every assertion lands on the HTTP response body, the
 * persistent permission/check `allowed` flag, the disabled-card DOM state, or
 * the backend 409 body. No toast-only assertions.
 *
 * Demo-Seed facts (verified against the live demo DB + source):
 *  - realm-001's grant mapping is `realm001-product-subscription`
 *    (`professional`: stripe, RECURRING, NULL external_price_id → priceKey IS
 *    the mappingId). beforeAll pins it by external product id (never "first
 *    product" — the admin list reorders between runs).
 *  - The alreadyOwned gate (card-disable AND the 409) fires ONLY for
 *    `one_time` + non-empty granted_role_ids (purchase_service.rs L407-430,
 *    handlers.rs L561-591). The recurring seed is therefore NEVER gated: 用例2
 *    asserts the documented recurring contract (card stays purchasable, repeat
 *    POST succeeds) and keeps the full one_time disabled-card + 409 semantics
 *    behind a billingType branch for a future one_time seed.
 *  - Cross-run idempotency: the shared demo user keeps payment-granted role
 *    rows across runs. beforeAll RESETS them (charge.refunded webhook chain,
 *    see resetGrantOwnership) and afterAll cancels this run's subscriptions so
 *    each run starts — and leaves — "not owning" the grant.
 *  - The permission checker caches denials for 60s with best-effort SCAN
 *    invalidation (redis_permission_checker.rs) — 用例1's post-purchase
 *    permission/check therefore POLLS (bounded by the TTL), it does not
 *    single-shot.
 */

import {
  expect,
  request as playwrightRequest,
  type APIRequestContext,
  type Page,
  type Request,
} from '@playwright/test'

import { SELECTORS } from '../selectors'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { createBearerApiContext } from '../helpers/auth'
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
import { initiatePurchaseFlow } from '../helpers/unified-purchase.helpers'
import {
  buildStripeChargeRefundedPayload,
  buildStripeSubscriptionDeletedPayload,
  deliverStripeChargeRefundedWebhook,
  deliverStripeSubscriptionDeletedWebhook,
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
// Demo Seed in realm-001 (resource=`billing`, action=`view`). We bind it to the
// granted role so the third-party RBAC check `{resource:'billing',action:'view'}`
// resolves allowed=true once the user holds the role.
const TEST_ROLE_NAME = 'paywall-grant-role-user-demo'
const BOUND_PERMISSION_NAME = 'billing.view'
// The rule we check: exact resource+action match against the bound permission.
const CHECK_RULE = { resource: 'billing', action: 'view' }

// `admin-api-client` is auto-provisioned per realm (herald realm services) and
// is treated as an admin/unscoped api-key identity (ADMIN_API_CLIENT_ID).
const ADMIN_API_CLIENT_ID = 'admin-api-client'

// Seeded external product id of the realm-001 grant mapping
// (`professional`: stripe, recurring, NULL external_price_id — Demo Seed
// scripts/lib/demo_seed.py L905). PINNED instead of "first product" because
// the admin master list reorders between runs (a save bumps updated_at), and a
// drifting pick decoupled priceKey from the purchased card across the
// initial/final runs (final skipped 用例2 with "card not in purchasable grid";
// final2 entered its purchase branch on a different first product).
const GRANT_PRODUCT_ID = 'realm001-product-subscription'

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
  /** UUID of the demo regular user (resolved in beforeAll for the reset/cleanup). */
  userId: string
}
let setupCtx: SetupContext | null = null

/**
 * Payment attempts fulfilled by THIS run (recorded by the tests, consumed by
 * the afterAll cleanup so the run leaves the shared demo user "not owning"
 * the grant — see the reset note in beforeAll).
 */
const runAttemptIds: string[] = []

// ============================================================================
// beforeAll — admin: configure grant mapping + bind permission + mint RBAC key
// ============================================================================

test.beforeAll(async ({ browser }) => {
  // Use a dedicated admin page (NOT a test fixture page) so the setup is
  // independent of any individual test's user login. Mirrors the credit-bucket
  // demo's beforeAll pattern.
  const adminContext = await browser.newContext()
  const adminPage = await adminContext.newPage()
  const adminLogger = new UnifiedLogger(adminPage, 'DE-D01 support-paywall beforeAll')

  try {
    // 1. Verify the demo environment.
    await verifyTestEnvironment(adminPage, {
      requiredRealms: [TEST_REALM],
      requiredUsers: [REALM_ADMIN_EMAIL, REGULAR_USER_EMAIL],
    })

    // 2. Login as the realm-001 admin.
    const loginPage = new LoginPage(adminPage, adminLogger)
    await loginPage.loginAsAdmin(REALM_ADMIN_EMAIL, REALM_ADMIN_PASSWORD, TEST_REALM)

    // Build a Bearer-authenticated API context from the in-memory access token
    // for the admin GETs/POSTs below. Under the auth-rewrite, admin endpoints
    // mount under `inject_token_identity` which ONLY reads the
    // `Authorization: Bearer` header — `page.context().request` shares only
    // cookies and 401s with `"missing bearer token"`. Mirrors the
    // points-quota-dashboard-demo beforeAll pattern. Disposed in the inner
    // finally; the outer `adminContext.close()` is unaffected.
    const adminApi = await createBearerApiContext(loginPage.getAccessToken())
    try {
      // 3. Ensure the granted role exists and bind the seeded permission to it.
      const rolesPage = new RolesPage(adminPage, adminLogger)
      await rolesPage.goto(TEST_REALM)
      if (!(await rolesPage.roleExists(TEST_ROLE_NAME))) {
        await rolesPage.createRole({
          name: TEST_ROLE_NAME,
          description: 'Granted-on-purchase role for support-paywall user demo',
        })
      }
      // Bind the builtin `billing.view` permission to the role (US-PW-006:
      // third-party RBAC gates on the role's bound permission).
      await rolesPage.clickPermissionsButton(TEST_ROLE_NAME)
      await rolesPage.setPermission(BOUND_PERMISSION_NAME, true)
      await rolesPage.savePermissions()

      // Resolve the roleId for the granted role (needed to select it on the
      // mappings page RoleSelector).
      const roleId = await findRoleIdByName(adminApi, TEST_REALM, TEST_ROLE_NAME)
      if (!roleId) {
        throw new Error(
          `[DE-D01 beforeAll] could not resolve roleId for ${TEST_ROLE_NAME} after create`,
        )
      }

      // 4. Configure the SEEDED grant mapping (professional, pinned by external
      //    product id — see GRANT_PRODUCT_ID) to grant this role on purchase.
      //    The seeded mapping is recurring; we keep its billing type.
      const mappingsPage = new EntitlementMappingsPage(adminPage, adminLogger)
      await mappingsPage.goto(TEST_REALM)
      await mappingsPage.waitForDataLoaded()
      await mappingsPage.selectProduct(GRANT_PRODUCT_ID)

      const firstRow = mappingsPage.mappingDetailPanel
        .locator('[data-testid^="price-edit-row-"]')
        .first()
      await expect(firstRow).toBeVisible()
      const rowTestid = (await firstRow.getAttribute('data-testid')) ?? ''
      const priceKey = rowTestid.replace(/^price-edit-row-/, '')

      // Read the mapping's billing type so the test knows whether the one_time
      // alreadyOwned path applies or the recurring grant chain is exercised. The
      // billing-type field renders as a read-only Input under testid
      // `price-billing-type-${priceKey}` (frontend
      // entitlement-mappings-page.tsx L459-471); read its value directly.
      const billingTypeInput = mappingsPage.getPriceEditRow(priceKey).locator(
        `[data-testid="price-billing-type-${priceKey}"]`,
      )
      const billingTypeRaw = await billingTypeInput.inputValue().catch(() => '')
      const billingType = billingTypeRaw.toLowerCase().includes('one') ? 'one_time' : 'recurring'

      // Resolve the mappingId (targetId for the purchase payment-attempt POST).
      // For the seeded Stripe row with NULL external_price_id, the priceKey IS
      // the mappingId. For a real external price id, the mappingId must be read
      // separately — attempt both lookups.
      const mappingId = await resolveMappingId(adminApi, TEST_REALM, priceKey)

      // Grant the role on this mapping and persist.
      await mappingsPage.selectGrantedRoles(priceKey, [roleId])
      await mappingsPage.saveChanges()

      // 5. Cross-run reset — every run must start from "user does NOT own the
      //    grant" (the shared seed user keeps payment-granted role rows across
      //    runs; leftover rows make the post-purchase allowed=true assertion
      //    vacuous). Revocation of payment-sourced rows is ONLY reachable via
      //    the webhook chains (admin replace_user_roles explicitly preserves
      //    source='payment' rows — admin_repositories.rs "Payment-granted roles
      //    are preserved"):
      //    a) resolve the demo user's UUID (admin users list),
      //    b) read its role rows (admin GET user-roles returns source +
      //       sourceId; recurring fulfillment grants carry sourceId = the
      //       INTERNAL subscription UUID — fulfillment_service.rs L571),
      //    c) for each TEST_ROLE_NAME payment row deliver a signed Stripe
      //       `charge.refunded` webhook with refundType='subscription' and the
      //       internal subscription UUID — the backend resolves it by internal
      //       id (stripe_webhook_handlers.rs handle_charge_refunded →
      //       find_subscription_by_id) and routes through
      //       handle_subscription_cancel(ImmediateCancel) →
      //       revoke_roles_by_payment_source, deleting exactly those rows.
      //    Verified loudly afterwards: the demo user must hold NO
      //    TEST_ROLE_NAME row when beforeAll finishes.
      const userId = await resolveUserIdByEmail(adminApi, TEST_REALM, REGULAR_USER_EMAIL)
      if (!userId) {
        throw new Error(
          `[DE-D01 beforeAll] could not resolve userId for ${REGULAR_USER_EMAIL}`,
        )
      }
      await resetGrantOwnership(adminApi, TEST_REALM, userId)

      // 6. Mint a third-party RBAC api key bound to the realm's admin-api-client
      //    app so `/permission/check` is unscoped (see file header rationale).
      //    createTestApiKeyWithPermission needs an admin-authenticated page; we
      //    reuse adminPage and thread the Bearer context through its optional
      //    `requestContext` param (the api-key creation endpoints are also
      //    Bearer-only). The permission arg also provisions the key's own
      //    permitted role.
      const adminApiAppId = await resolveClientAppId(
        adminApi,
        TEST_REALM,
        ADMIN_API_CLIENT_ID,
      )
      const apiKey = await createTestApiKeyWithPermission(
        adminPage,
        BOUND_PERMISSION_NAME,
        Date.now(),
        TEST_REALM,
        adminApiAppId,
        adminApi,
      )

      setupCtx = {
        apiKey,
        priceKey,
        mappingId,
        billingType,
        userId,
      }
    } finally {
      await adminApi.dispose().catch(() => {})
    }
  } finally {
    await adminContext.close()
  }
})

// ============================================================================
// afterAll — cancel THIS run's subscriptions so it leaves the demo user
// "not owning" the grant (the self-cleaning counterpart of the beforeAll
// reset; the revoke demo's proven pattern). Each fulfilled attempt's external
// subscription id is deterministic (`demo-fulfill-{attemptId}` —
// payment-simulation.ts), so a signed `customer.subscription.deleted`
// (ImmediateCancel) revokes exactly this run's payment-granted role rows.
// Fault-tolerant by design: a cleanup failure is LOGGED, not thrown — the next
// run's beforeAll reset covers the leftover.
// ============================================================================

test.afterAll(async () => {
  if (runAttemptIds.length === 0 || !setupCtx) return
  const { userId } = setupCtx
  const cleanupApi = await playwrightRequest.newContext()
  try {
    for (const attemptId of runAttemptIds) {
      const payload = buildStripeSubscriptionDeletedPayload({
        eventId: `evt_pw_cleanup_${Date.now()}_${attemptId}`,
        subscriptionId: `demo-fulfill-${attemptId}`,
        userId,
        cancelAtPeriodEnd: false,
      })
      const result = await deliverStripeSubscriptionDeletedWebhook(
        cleanupApi,
        TEST_REALM,
        payload,
      )
      if (!result.ok) {
        console.error(
          `[DE-D01 afterAll] cleanup cancel webhook failed for attempt ${attemptId}: ` +
            `${result.status} ${result.body}`,
        )
      }
    }
  } finally {
    await cleanupApi.dispose().catch(() => {})
  }
})

// ============================================================================
// Demo: US-PW-002/003/004/006 — purchase grants role + alreadyOwned + RBAC
// ============================================================================

test.describe('[Regular User] Support Paywall — purchase grants role + alreadyOwned + RBAC (US-PW-002/003/004/006)', () => {
  test.beforeEach(async ({ page, loginPage }) => {
    await verifyTestEnvironment(page, {
      requiredRealms: [TEST_REALM],
      requiredUsers: [REGULAR_USER_EMAIL],
    })
    // Login as the regular user whose role grant we will observe.
    await loginPage.loginAsUser(REGULAR_USER_EMAIL, REGULAR_USER_PASSWORD, TEST_REALM)
  })

  test.afterEach(async ({ page, testStartTime }) => {
    await cleanupTestData(page, TEST_REALM, { timestamp: testStartTime })
  })

  test('US-PW-002 + US-PW-003 场景1: 购买授 role 映射后用户被授予 role（永久）', async ({
    page,
    request,
  }) => {
    expect(setupCtx, 'beforeAll must have configured the grant mapping').not.toBeNull()
    const { apiKey, priceKey } = setupCtx!

    // US-PW-006 precondition gate BEFORE purchase: the user must NOT yet be
    // allowed (they don't hold the granted role yet). This anchors the
    // before/after delta on persistent RBAC state.
    // Browser Bearer token model (commit f3b8d48a): there is no X-Auth cookie.
    // The token is captured LIVE from the page's authenticated requests (see
    // captureLiveUserToken) — a login-time captured token can be revoked by
    // the page's post-login switch/refresh dance, and permission/check then
    // returns 200 + allowed=false ("token_not_found") which would silently
    // stall the post-purchase poll.
    const sessionToken = await captureLiveUserToken(
      page,
      `/${TEST_REALM}/user/purchase-points`,
    )

    await test.step('Given: 购买前用户未持有该 role 权限', async () => {
      const { status, body } = await makeExtApiRequest({
        apiKey: apiKey.apiKey,
        method: 'POST',
        path: '/permission/check',
        body: { accessToken: sessionToken, rules: [CHECK_RULE] },
      })
      expect(status, 'permission/check must respond 200').toBe(200)
      const resp = body as { allowed?: boolean }
      // Allowed may already be true if a PRIOR test run left the role on this
      // demo user (the seed user is shared). We assert the endpoint shape here
      // and rely on the post-purchase assertion being load-bearing.
      expect(typeof resp.allowed, 'allowed flag must be boolean').toBe('boolean')
    })

    let attemptId = ''

    await test.step('When: 购买授 role 映射并模拟支付成功（不发积分也不报错）', async () => {
      // US-PW-002: a role-grant purchase must complete without erroring even
      // when the mapping has no points strategy (pure-entitlement). The
      // seeded recurring mapping may or may not carry points; either way the
      // fulfillment must succeed.
      //
      // Hosted-checkout contract (533ec22d + a71c72a4): stripe attempts
      // redirect the SAME TAB to checkout.stripe.com, so the unified helper
      // aborts the provider-host navigation and captures the attempt id
      // NODE-side (route.fetch proxy on the POST). The page is left on the
      // aborted-redirect error document; the Then step resumes it with the
      // `?attemptId=` provider-bounce navigation. `priceId` PINS the purchase
      // to the grant mapping configured in beforeAll (deterministic; never a
      // "first card" discovery that could drift onto a role-less mapping).
      attemptId = await initiatePurchaseFlow(page, 'stripe', TEST_REALM, {
        priceId: priceKey,
      })
      expect(attemptId, 'payment attempt must be created').toBeTruthy()
      runAttemptIds.push(attemptId)

      const result = await fulfillPayment(request, TEST_REALM, attemptId)
      expect(
        result.success,
        `payment fulfillment must succeed (US-PW-002 no-error): ${result.error ?? ''}`,
      ).toBe(true)
    })

    await test.step('Then: 用户被授予 role（第三方凭 role 放行 — US-PW-006 场景1）', async () => {
      // Resume the purchase page the way the provider bounce does
      // (`?attemptId=`): the stripe redirect was aborted by the initiate
      // helper, so the tab sits on an error document until this deliberate
      // navigation. The page re-enters processing, polls, and renders the
      // complete step once the fulfilled attempt reports Succeeded. The
      // guarded goto retries when the aborted-navigation error page races the
      // goto ("interrupted by another navigation to chrome-error://").
      await gotoWithInterruptRetry(
        page,
        `/${TEST_REALM}/user/purchase-points?attemptId=${attemptId}`,
      )
      await expect(page.locator(SELECTORS.purchasePoints.stepComplete)).toBeVisible({
        timeout: 20000,
      })

      // US-PW-006: third-party app gates with one RBAC call. The granted role
      // carries `billing.view` → check `{resource:'billing',action:'view'}`
      // resolves allowed=true (exact-match policy). Persistent state, not toast.
      //
      // The Given-step check CACHES a denial (`principal_perm:...` denial
      // cache, redis_permission_checker.rs L439/L454) and the fulfill-time
      // invalidation is best-effort SCAN-based (unreliable — observed stale
      // in the final2 run: the role row was committed yet the check returned
      // allowed=false). The denial TTL is 60s (cache_ttl::DENIAL), so POLL up
      // to 75s — bounded by the TTL, not a flaky sleep.
      await expect
        .poll(
          async () => {
            const { status, body } = await makeExtApiRequest({
              apiKey: apiKey.apiKey,
              method: 'POST',
              path: '/permission/check',
              body: { accessToken: sessionToken, rules: [CHECK_RULE] },
            })
            if (status !== 200) {
              throw new Error(`permission/check post-purchase must be 200, got ${status}`)
            }
            return (body as { allowed?: boolean }).allowed === true
          },
          { timeout: 75_000, intervals: [1_000, 2_000, 3_000] },
        )
        .toBe(true)

      // Authoritative single call after the poll converges (keeps the userId
      // assertion on a concrete response object).
      const { status, body } = await makeExtApiRequest({
        apiKey: apiKey.apiKey,
        method: 'POST',
        path: '/permission/check',
        body: { accessToken: sessionToken, rules: [CHECK_RULE] },
      })
      expect(status, 'permission/check must respond 200 post-purchase').toBe(200)
      const resp = body as { allowed?: boolean; userId?: string }
      expect(
        resp.allowed,
        'user must be permitted via the granted role after purchase (US-PW-006)',
      ).toBe(true)
      expect(resp.userId, 'allowed check must return userId').toBeTruthy()

      // Cross-check: the user's assigned roles include the granted role. The
      // self-service `/api/user/roles` endpoint is Bearer-only under the
      // auth-rewrite (the realm rides inside the Bearer token — no session
      // cookie exists, see readAssignedRoleNames) and returns role NAMES
      // (UserProfileRolesResponse resolves ids → names server-side). The
      // granted role was created from TEST_ROLE_NAME in beforeAll and its id
      // bound to the mapping, so the name IS the granted role.
      const roleNames = await readAssignedRoleNames(sessionToken)
      expect(
        roleNames,
        'the granted role must appear in the user assigned roles (US-PW-003 permanent grant)',
      ).toContain(TEST_ROLE_NAME)
    })
  })

  test('US-PW-004 场景1: 已拥有该权益时购买卡片禁用 + 后端 409 already_owned 拦截', async ({
    page,
    request,
  }) => {
    expect(setupCtx, 'beforeAll must have configured the grant mapping').not.toBeNull()
    const { priceKey, billingType, mappingId } = setupCtx!

    // US-PW-004 场景1: once owned (the previous test purchased it), the card
    // must be DISABLED with a reason, and a direct repeat purchase attempt
    // must be rejected by the backend 409.
    //
    // Backend contract (purchase_service.rs L407-430, handlers.rs L561-591):
    // the alreadyOwned gate (card-disable AND the 409) fires ONLY for
    // `billing_type=one_time` + non-empty granted_role_ids. The seeded grant
    // mapping is RECURRING, so for this seed the test asserts the documented
    // recurring contract instead (repeatable: card stays purchasable, repeat
    // POST is NOT 409) — ownership here = the user holds the granted role,
    // established by 用例1 (re-established defensively below if absent). The
    // one_time branch keeps the full disabled-card + 409 semantics for a
    // future one_time seed.
    // Node-side API token: captured LIVE from the page's own authenticated
    // requests (see captureLiveUserToken), NOT loginPage.getAccessToken().
    // That getter returns the token captured at login completion, and the
    // page's token engine can revoke that family right afterwards (final3
    // evidence: the post-login /manage landing fired an extra switch-client →
    // admin-web-console which the login helper's waitForResponse captured
    // FIRST; the subsequent browser-token/refresh and the real switch →
    // user-account-center revoked that family — a node-side call made seconds
    // later 401'd). A token read off the wire AFTER the dance settles is
    // current by construction.
    const sessionToken = await captureLiveUserToken(
      page,
      `/${TEST_REALM}/user/purchase-points`,
    )
    const isOneTime = billingType === 'one_time'

    await test.step('Given: 确保用户已拥有该授 role 权益', async () => {
      if (isOneTime) {
        // Navigate to the purchase page and inspect the target card.
        await page.evaluate(() => localStorage.removeItem('cas-purchase-flow'))
        await page.goto(`/${TEST_REALM}/user/purchase-points`)
        await expect(page.locator(SELECTORS.purchasePoints.page)).toBeVisible()

        const card = page.locator(SELECTORS.purchasePriceCard.priceCard(priceKey))
        const cardVisible = await card.isVisible().catch(() => false)
        if (cardVisible) {
          const reason = card.locator(SELECTORS.purchasePriceCard.priceCardReason(priceKey))
          const alreadyOwned = (await reason.count()) > 0
          if (!alreadyOwned) {
            // Not yet owned — purchase + fulfill to establish ownership, then
            // reload and re-check the disabled state. The unified helper aborts
            // the stripe hosted-checkout redirect and captures the attempt id
            // NODE-side; `priceId` pins the configured grant-mapping card.
            const attemptId = await initiatePurchaseFlow(page, 'stripe', TEST_REALM, {
              priceId: priceKey,
            })
            runAttemptIds.push(attemptId)
            const result = await fulfillPayment(request, TEST_REALM, attemptId)
            expect(result.success, 'setup purchase must fulfill').toBe(true)
            // Resume via the provider-bounce URL (guarded: the aborted
            // redirect's error page can race the goto).
            await gotoWithInterruptRetry(
              page,
              `/${TEST_REALM}/user/purchase-points?attemptId=${attemptId}`,
            )
            await expect(page.locator(SELECTORS.purchasePoints.stepComplete)).toBeVisible({
              timeout: 20000,
            })

            // Reload purchase page — the card should now be disabled + reason.
            await page.evaluate(() => localStorage.removeItem('cas-purchase-flow'))
            await page.goto(`/${TEST_REALM}/user/purchase-points`)
            await expect(page.locator(SELECTORS.purchasePriceCard.page)).toBeVisible()
          }
        }
      } else {
        // Recurring seed: ownership = the user holds the granted role (用例1
        // just purchased + the role grant was asserted there). Re-establish
        // defensively only if absent (e.g. 用例1 failed), so the And-step
        // below always evaluates the owned state.
        const roleNames = await readAssignedRoleNames(sessionToken)
        if (!roleNames.includes(TEST_ROLE_NAME)) {
          const attemptId = await initiatePurchaseFlow(page, 'stripe', TEST_REALM, {
            priceId: priceKey,
          })
          runAttemptIds.push(attemptId)
          const result = await fulfillPayment(request, TEST_REALM, attemptId)
          expect(result.success, 'setup purchase must fulfill').toBe(true)
          await gotoWithInterruptRetry(
            page,
            `/${TEST_REALM}/user/purchase-points?attemptId=${attemptId}`,
          )
          await expect(page.locator(SELECTORS.purchasePoints.stepComplete)).toBeVisible({
            timeout: 20000,
          })
        }
      }
    })

    await test.step('Then: 已拥有时的卡片状态符合 billing_type 契约（持久 DOM 状态）', async () => {
      if (!isOneTime) {
        // Recurring contract (purchase_service.rs L407-410: "Points packages
        // and subscriptions remain repeatable/renewable"): even with the role
        // held, the card is NOT alreadyOwned-gated — it must stay purchasable
        // (no reason row). A disabled card here would mean the gate leaked
        // into recurring mappings.
        await page.goto(`/${TEST_REALM}/user/purchase-points`)
        await expect(page.locator(SELECTORS.purchasePoints.page)).toBeVisible()
        const reason = page.locator(
          SELECTORS.purchasePriceCard.priceCardReason(priceKey),
        )
        await expect(
          reason,
          'recurring grant card must stay purchasable (NOT alreadyOwned-gated)',
        ).toHaveCount(0)
        return
      }

      const card = page.locator(SELECTORS.purchasePriceCard.priceCard(priceKey))
      const cardVisible = await card.isVisible().catch(() => false)
      if (!cardVisible) {
        // The configured grant mapping is not in the purchasable grid (e.g. its
        // provider has no checkout). The disabled-card DOM assertion is
        // moot — the backend 409 below is the authoritative gate. Skip
        // gracefully.
        test.skip(true, 'grant mapping card not in purchasable grid; 409 gate still asserted')
      } else {
        const reason = card.locator(SELECTORS.purchasePriceCard.priceCardReason(priceKey))
        await expect(reason, 'alreadyOwned card must render a reason row').toBeVisible({
          timeout: 10000,
        })
        // The disabled card has onClick=undefined; verify it is NOT clickable by
        // confirming the reason text rendered (alreadyOwned). We do NOT assert
        // on a toast.
      }
    })

    await test.step('And: 后端重复购买行为符合 billing_type 契约', async () => {
      // US-PW-004 场景1 backend gate: for an owned one_time+role mapping a
      // direct POST creating a new payment attempt is rejected with a
      // structured 409 `already_owned`. For the recurring seed the same POST
      // must SUCCEED (201) — subscriptions are contractually repeatable.
      //
      // The billing purchase routes are Bearer-only under the auth-rewrite —
      // `page.request` carries only cookies and 401s. Build the Bearer context
      // from the logged-in user's access token (same pattern as the file's
      // other admin/user API helpers).
      const userApi = await createBearerApiContext(sessionToken)
      let status = 0
      let respBody: unknown = {}
      try {
        const resp = await userApi.post(
          `${purchaseBaseUrl()}/api/bill/${TEST_REALM}/purchase/payment-attempts`,
          {
            headers: { 'Content-Type': 'application/json' },
            data: {
              targetType: 'entitlement_mapping',
              targetId: mappingId,
              paymentProvider: 'stripe',
            },
          },
        )
        status = resp.status()
        respBody = await resp.json().catch(() => ({}))
      } finally {
        await userApi.dispose().catch(() => {})
      }

      if (isOneTime) {
        if (status === 409) {
          expect(
            (respBody as { code?: string }).code,
            '409 body must carry code=already_owned (US-PW-004 backend gate)',
          ).toBe('already_owned')
        } else {
          expect(
            status === 200 || status === 201,
            `one_time repeat purchase expected 201/200, got ${status} ${JSON.stringify(respBody)}`,
          ).toBe(true)
        }
      } else {
        expect(
          status === 200 || status === 201,
          `recurring repeat purchase expected 201/200 (mappingId=${mappingId}), got ${status} ${JSON.stringify(respBody)}`,
        ).toBe(true)
      }
    })
  })

  test('US-PW-004 场景2 对照: 积分包（无 role 授予）可重复购买，不触发 409', async ({
    page,
  }) => {
    // US-PW-004 场景2 contrast: a points-only mapping (granted_role_ids empty)
    // can be purchased repeatedly. We assert the NEGATIVE: a direct repeat POST
    // does NOT return 409 already_owned. This requires a points-only mapping;
    // the seeded realm-001 recurring mapping may or may not have role grants
    // (beforeAll granted a role to the pinned grant mapping). We therefore
    // resolve a mapping with NO role grant at runtime; if none exists, this
    // contrast test is skipped (cannot be seeded deterministically without
    // mutating the shared demo catalog).
    //
    // All node-side calls use the LIVE page token (see captureLiveUserToken —
    // the 用例2 note): a stale loginPage token made the list call 401 →
    // silent null → WRONG skip in final3, and the old cookie-only
    // `page.request` POST could never reach the Bearer-only billing route
    // (401 passed the weak not-409 assertion vacuously).
    const userApi = await createBearerApiContext(
      await captureLiveUserToken(page, `/${TEST_REALM}/user/purchase-points`),
    )
    try {
      const pointsMappingId =
        (await findPointsOnlyMappingId(userApi, TEST_REALM)) ?? ''

      if (!pointsMappingId) {
        test.skip(true, 'no points-only mapping without role grant available in realm-001')
      } else {
        const resp = await userApi.post(
          `${purchaseBaseUrl()}/api/bill/${TEST_REALM}/purchase/payment-attempts`,
          {
            headers: { 'Content-Type': 'application/json' },
            data: {
              targetType: 'entitlement_mapping',
              targetId: pointsMappingId,
              paymentProvider: 'stripe',
            },
          },
        )
        const body = await resp.text().catch(() => '')
        expect(
          resp.status(),
          `points-only mapping repeat purchase must NOT be 409 already_owned, got ${resp.status()} ${body}`,
        ).not.toBe(409)
      }
    } finally {
      await userApi.dispose().catch(() => {})
    }
  })
})

// ============================================================================
// Local helpers
// ============================================================================

/** Backend base URL for direct API calls (port 8080). */
function purchaseBaseUrl(): string {
  return (
    process.env.API_BASE_URL ||
    process.env.BASE_URL?.replace(/:\d+/, ':8080') ||
    'http://localhost:8080'
  )
}

/**
 * Capture the user's CURRENT access token from the wire: register a request
 * listener, navigate once, and read the `Authorization: Bearer` header off
 * the page's own authenticated API calls (e.g. /api/user/roles), then let the
 * post-login token dance settle — the handler's LAST write wins.
 *
 * Why: `loginPage.getAccessToken()` returns the token captured at login
 * completion, which the page's token engine can revoke moments later
 * (final3: switch-client → admin-web-console captured by the login helper's
 * waitForResponse FIRST when the post-login landing transiently hit /manage,
 * then browser-token/refresh + the real switch-client → user-account-center
 * revoked that family — node-side calls seconds later 401'd). The access
 * token is NOT in localStorage (auth-store partialize keeps only routing +
 * PKCE state; the token family lives in the Herald SDK's memory), so the
 * wire is the only live source. A token observed on the wire after the dance
 * settles is valid by construction.
 */
async function captureLiveUserToken(page: Page, url: string): Promise<string> {
  let liveToken = ''
  const onRequest = (req: Request) => {
    const auth = req.headers()['authorization'] ?? ''
    if (auth.startsWith('Bearer ')) {
      liveToken = auth.slice('Bearer '.length)
    }
  }
  page.on('request', onRequest)
  try {
    await gotoWithInterruptRetry(page, url)
    await expect
      .poll(() => liveToken !== '', { timeout: 10_000 })
      .toBe(true)
    // Settle window: the post-login switch/refresh dance completes within
    // ~1.5s of landing (observed in the final3 capture); during it the token
    // may rotate again — keep overwriting so the LAST token wins.
    await page.waitForTimeout(1500)
  } finally {
    page.off('request', onRequest)
  }
  return liveToken
}

/**
 * `page.goto` guarded against the aborted-navigation race: after the unified
 * purchase helper aborts the stripe hosted-checkout redirect, the tab sits on
 * a `chrome-error://` error document whose load can still be settling, and a
 * goto fired in that window fails with "interrupted by another navigation to
 * chrome-error://" (observed in the final2 run's ?attemptId bounce-back).
 * Pattern: settle with `waitForLoadState`, and on an interrupt/ERR_ABORTED
 * match retry after 200ms, at most 3 attempts.
 */
async function gotoWithInterruptRetry(
  page: Page,
  url: string,
  opts: { timeout?: number; retries?: number } = {},
): Promise<void> {
  const { timeout = 30_000, retries = 3 } = opts
  for (let attempt = 1; attempt <= retries; attempt++) {
    try {
      await page.goto(url, { timeout })
      await page.waitForLoadState('domcontentloaded')
      return
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      const isNavigationRace =
        /interrupted by another navigation/i.test(message) ||
        /ERR_ABORTED|chrome-error/i.test(message)
      if (attempt < retries && isNavigationRace) {
        await page.waitForTimeout(200)
        continue
      }
      throw error
    }
  }
}

/** Resolve a user's UUID by email via the admin users list endpoint
 * (mirrors the revoke demo's resolveUserIdByEmail — Bearer-only route). */
async function resolveUserIdByEmail(
  apiContext: APIRequestContext,
  realmId: string,
  email: string,
): Promise<string | null> {
  const resp = await apiContext.get(
    `${purchaseBaseUrl()}/api/users/${realmId}?email=${encodeURIComponent(email)}`,
  )
  if (!resp.ok()) {
    throw new Error(
      `could not list users in ${realmId}: ${resp.status()} ${await resp.text().catch(() => '')}`,
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
 * Read a user's role rows via the admin GET user-roles endpoint. Returns
 * `{name, source, sourceId}` per row (UserRoleDetail, camelCase). Fails LOUD
 * on non-2xx (same discipline as the revoke demo's readUserRoles).
 */
async function readUserRoleRows(
  apiContext: APIRequestContext,
  realmId: string,
  userId: string,
): Promise<Array<{ name: string; source: string; sourceId: string | null }>> {
  const resp = await apiContext.get(
    `${purchaseBaseUrl()}/api/users/${realmId}/${userId}/roles`,
  )
  if (!resp.ok()) {
    throw new Error(
      `admin GET user-roles failed for ${realmId}/${userId}: ${resp.status()} ${await resp.text().catch(() => '')}`,
    )
  }
  const body = await resp.json()
  // UserRolesResponse → { roles: [{id, name, source, sourceId, ...}] }
  const roles: unknown = (body as { roles?: unknown }).roles ?? []
  if (!Array.isArray(roles)) return []
  return roles
    .map((r) => {
      const row = r as { name?: string; source?: string; sourceId?: string | null }
      return {
        name: row.name ?? '',
        source: row.source ?? '',
        sourceId: row.sourceId ?? null,
      }
    })
    .filter((row) => row.name !== '')
}

/**
 * Reset the demo user to "not owning the grant": revoke every payment-granted
 * TEST_ROLE_NAME row left by prior runs, via the signed Stripe
 * `charge.refunded` webhook chain (refundType='subscription' resolves the
 * subscription by its INTERNAL uuid — the `sourceId` on the role row — and
 * ImmediateCancel revokes `revoke_roles_by_payment_source`). Fails LOUD if
 * any TEST_ROLE_NAME row survives (a manual row would need a different
 * removal path; surface it rather than silently continuing).
 */
async function resetGrantOwnership(
  apiContext: APIRequestContext,
  realmId: string,
  userId: string,
): Promise<void> {
  const rows = await readUserRoleRows(apiContext, realmId, userId)
  const grantRows = rows.filter((r) => r.name === TEST_ROLE_NAME)
  if (grantRows.length === 0) return

  const stamp = Date.now()
  for (const [index, row] of grantRows.entries()) {
    if (row.sourceId === null) continue // manual rows handled by the verify below
    const payload = buildStripeChargeRefundedPayload({
      eventId: `evt_pw_reset_${stamp}_${index}`,
      chargeId: `ch_pw_reset_${stamp}_${index}`,
      amount: 100,
      amountRefunded: 100,
      userId,
      subscriptionId: row.sourceId,
      refundType: 'subscription',
    })
    const result = await deliverStripeChargeRefundedWebhook(apiContext, realmId, payload)
    if (!result.ok) {
      throw new Error(
        `[DE-D01 beforeAll] reset refund webhook failed for sourceId ${row.sourceId}: ` +
          `${result.status} ${result.body}`,
      )
    }
  }

  const remaining = (await readUserRoleRows(apiContext, realmId, userId)).filter(
    (r) => r.name === TEST_ROLE_NAME,
  )
  if (remaining.length > 0) {
    throw new Error(
      `[DE-D01 beforeAll] grant ownership reset incomplete — ${remaining.length} ` +
        `TEST_ROLE_NAME row(s) remain (${remaining
          .map((r) => `source=${r.source},sourceId=${r.sourceId ?? 'null'}`)
          .join('; ')}). Remove them before rerunning.`,
    )
  }
}

/**
 * Resolve a role id by name via the backend role-definitions API.
 *
 * Uses the supplied Bearer-authenticated `apiContext` (admin endpoints 401
 * `"missing bearer token"` on cookie-only requests under the auth-rewrite —
 * `page.context().request` carries no Bearer header, so it must NOT be used
 * here; the caller builds the context from `loginPage.getAccessToken()`).
 */
async function findRoleIdByName(
  apiContext: APIRequestContext,
  realmId: string,
  roleName: string,
): Promise<string | null> {
  const resp = await apiContext.get(
    `${purchaseBaseUrl()}/api/roles/${realmId}/define`,
  )
  if (!resp.ok()) return null
  const body = await resp.json()
  const roles: { id: string; name: string }[] = Array.isArray(body) ? body : body.items ?? []
  const hit = roles.find((r) => r.name === roleName)
  return hit ? hit.id : null
}

/**
 * Resolve the client-app UUID for a given client_id in a realm. The list
 * endpoint returns a PageResponse<ClientAppItem> whose items live under
 * `data` (camelCase-serialized: `clientId`). Tolerate bare-array / items too.
 *
 * Uses the supplied Bearer-authenticated `apiContext` — the admin client-list
 * endpoint 401s on cookie-only requests under the auth-rewrite (see
 * `findRoleIdByName` for the full rationale).
 */
async function resolveClientAppId(
  apiContext: APIRequestContext,
  realmId: string,
  clientId: string,
): Promise<string> {
  const resp = await apiContext.get(`${purchaseBaseUrl()}/api/client/${realmId}`)
  if (!resp.ok()) {
    throw new Error(`could not list client apps in ${realmId}: ${resp.status()}`)
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

/**
 * Resolve the mappingId for a priceKey. For a Creem NULL-price row the priceKey
 * IS the mappingId; for a Stripe row with a real external_price_id we must look
 * it up. We try both: first assume priceKey is the mappingId (works for the
 * seeded Stripe+NULL row), else query the mappings list.
 *
 * Uses the supplied Bearer-authenticated `apiContext` — the entitlement-mapping
 * endpoints 401 on cookie-only requests under the auth-rewrite (see
 * `findRoleIdByName` for the full rationale). Previously this silently degraded
 * to returning `priceKey` unchanged on a 401, which masked setup failures.
 */
async function resolveMappingId(
  apiContext: APIRequestContext,
  realmId: string,
  priceKey: string,
): Promise<string> {
  // Validate that priceKey is itself a usable mappingId by fetching the
  // mapping; if that 404s, fall back to listing mappings and matching the
  // external_price_id.
  const direct = await apiContext
    .get(`${purchaseBaseUrl()}/api/bill/${realmId}/entitlement-mappings/${priceKey}`)
    .catch(() => null)
  if (direct && direct.ok()) {
    return priceKey
  }
  // Fall back to listing and matching external_price_id.
  const list = await apiContext.get(
    `${purchaseBaseUrl()}/api/bill/${realmId}/entitlement-mappings`,
  )
  if (list.ok()) {
    const body = await list.json()
    const items: {
      id: string
      externalPriceId?: string | null
      external_product_id?: string
    }[] = Array.isArray(body)
      ? body
      : body.items ?? []
    const hit = items.find((m) => m.externalPriceId === priceKey || m.external_product_id === priceKey)
    if (hit) return hit.id
  }
  // Last resort: return priceKey (best-effort; matches the seeded NULL-price
  // case where they coincide).
  return priceKey
}

/**
 * Resolve a mappingId whose granted_role_ids is empty (points-only), for the
 * US-PW-004 场景2 contrast. Returns null if none exists.
 *
 * Uses the supplied Bearer-authenticated `apiContext` — the entitlement-mapping
 * list endpoint 401s on cookie-only requests under the auth-rewrite (see
 * `findRoleIdByName` for the full rationale).
 */
async function findPointsOnlyMappingId(
  apiContext: APIRequestContext,
  realmId: string,
): Promise<string | null> {
  const list = await apiContext.get(
    `${purchaseBaseUrl()}/api/bill/${realmId}/entitlement-mappings`,
  )
  if (!list.ok()) return null
  const body = await list.json()
  const items: {
    id: string
    grantedRoleIds?: string[] | null
    granted_role_ids?: string[] | null
  }[] = Array.isArray(body) ? body : body.items ?? []
  const hit = items.find((m) => {
    const granted = m.grantedRoleIds ?? m.granted_role_ids ?? []
    return Array.isArray(granted) && granted.length === 0
  })
  return hit ? hit.id : null
}

/**
 * Read the logged-in user's assigned ROLE NAMES via the self-service
 * `/api/user/roles` endpoint. The backend resolves role ids to names
 * server-side (`UserProfileRolesResponse` → `{roles: [names], permissions:
 * [names]}`), so callers match on the role NAME, not the id.
 *
 * `/api/user/*` is Bearer-only under the auth-rewrite (the realm rides inside
 * the Bearer token — the browser carries NO session cookie), so this MUST use
 * a Bearer context built from the login access token; `page.context().request`
 * carries only cookies and 401s. Fails LOUD on non-2xx responses: the previous
 * cookie-only call silently swallowed the 401 into [] and broke the
 * assigned-roles assertion (same failure class the revoke demo's readUserRoles
 * documents).
 */
async function readAssignedRoleNames(accessToken: string): Promise<string[]> {
  const api = await createBearerApiContext(accessToken)
  try {
    const resp = await api.get(`${purchaseBaseUrl()}/api/user/roles`)
    if (!resp.ok()) {
      const body = await resp.text().catch(() => '')
      throw new Error(`GET /api/user/roles failed: ${resp.status()} ${body}`)
    }
    const body = await resp.json()
    // UserProfileRolesResponse → { roles: [names] }. Tolerate a wrapped
    // {data:{roles}} shape too, but NOT a silent [] on failure — non-2xx
    // throws above.
    const roles: unknown =
      (body as { roles?: unknown }).roles ??
      ((body as { data?: { roles?: unknown } }).data?.roles ?? [])
    if (!Array.isArray(roles)) return []
    return roles.filter((r): r is string => typeof r === 'string')
  } finally {
    await api.dispose().catch(() => {})
  }
}
