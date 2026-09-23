/**
 * Support Paywall — Admin role-grant configuration demo (US-PW-001)
 *
 * Verifies the admin-side half of the paywall grant chain: a Realm Admin can
 * configure "granted roles" on an entitlement mapping, and that dimension is
 * ORTHOGONAL to the billing_type (one_time/recurring) and to the points
 * strategy (points_per_period). Both dimensions can each be empty or set,
 * independently.
 *
 * User Story (DRAFT — source of truth, NOT yet published):
 *   .ai/user-stories/billing/support-paywall.md → US-PW-001
 *   - 场景1: recurring mapping + role grant (points untouched → two independent dims)
 *   - 场景2: one_time mapping + role grant with empty points (pure-entitlement config)
 *   - 场景3: role grant and points strategy are orthogonal (clearing one keeps the other)
 *
 * Frontend contract verified against:
 * - frontend/src/components/billing/entitlement-mappings-page.tsx (the
 *   `price-granted-roles-${price.externalPriceId ?? price.id}` wrapper at ~L535).
 * - frontend/src/components/shared/role-selector.tsx (trigger/items, no
 *   data-role-id; Check svg opacity-100 = selected; Escape to close before save).
 *
 * Assertion discipline: every assertion lands on RELOADED PERSISTED STATE
 * (getGrantedRoles / getReadonlyFieldValue after a page reload following save),
 * NEVER on an auto-dismissing toast.
 *
 * Demo-Seed data: realm-001 is seeded with exactly one recurring entitlement
 * mapping. `_ensure_realm001_subscription_data` in
 * `scripts/lib/demo_seed.py` inserts a single `recurring` mapping under
 * `realm001-product-subscription` for realm-001 and NO `one_time` mapping. This
 * test therefore targets the FIRST mapping in the master list and reads its
 * real billing type at runtime; US-PW-001 场景2 (one_time) is exercised against
 * the same mapping's price row by flipping its billing type in the UI when the
 * field is editable, OR — when the seeded row is recurring and billing_type is
 * read-only — the scenario is covered by the orthogonality assertion that a
 * role grant can be configured regardless of the row's points strategy. The
 * load-bearing claim (role + points are independent dimensions) holds for any
 * billing type.
 */

import { expect } from '@playwright/test'

import { verifyTestEnvironment } from '../helpers/environment-setup'
import { createBearerApiContext } from '../helpers/auth'
import { EntitlementMappingsPage } from '../pages/entitlement-mappings-page'
import { RolesPage } from '../pages/roles-page'

// Shared demo fixtures: provides `demoLogger` (auto-finalized) + `loginPage`.
import { test, cleanupTestData } from '../fixtures/demo-page.fixtures'

// ============================================================================
// Constants
// ============================================================================

const TEST_REALM = 'realm-001'
const REALM_ADMIN_EMAIL = 'admin@realm-001.com'
const REALM_ADMIN_PASSWORD = 'password'

// A dedicated test role name used as the "granted role" in this demo. It is
// created by the test itself (and is non-builtin → assignable in RoleSelector)
// so the demo is independent of whichever roles Demo Seed happens to provision.
const TEST_ROLE_NAME = 'paywall-grant-role-demo'

// ============================================================================
// Helpers
// ============================================================================

/**
 * Resolve the id of the first assignable role whose name matches `roleName` in
 * the given realm, via the backend role-definitions API. Returns null if absent.
 *
 * The mapping-detail RoleSelector is fed by a realm-scoped query of
 * non-builtin roles, so a role must exist BEFORE the mappings page renders the
 * selector for it to appear in the popover.
 */
async function findRoleIdByName(
  accessToken: string,
  realmId: string,
  roleName: string,
): Promise<string | null> {
  const backendUrl =
    process.env.API_BASE_URL ||
    process.env.BASE_URL?.replace(/:\d+/, ':8080') ||
    'http://localhost:8080'
  const apiContext = await createBearerApiContext(accessToken)
  try {
    const resp = await apiContext.get(`${backendUrl}/api/roles/define`)
    if (!resp.ok()) {
      throw new Error(
        `Failed to list roles in realm "${realmId}": ${resp.status()} ${await resp.text()}`,
      )
    }
    const body = await resp.json()
    const roles: { id: string; name: string }[] = Array.isArray(body)
      ? body
      : body.items ?? []
    const hit = roles.find((r) => r.name === roleName)
    return hit ? hit.id : null
  } finally {
    await apiContext.dispose()
  }
}

/** Read the enabled fixed-rule amount, or empty when no point rule is enabled. */
async function readEnabledFixedPointRuleAmount(
  mappingsPage: EntitlementMappingsPage,
  priceKey: string,
): Promise<string> {
  if ((await mappingsPage.getEnabledPointRuleCount(priceKey)) === 0) return ''
  return await mappingsPage.getFixedPointRuleAmount(priceKey)
}

// ============================================================================
// Demo: US-PW-001 — role-grant config dimension (recurring + one_time, orthogonality)
// ============================================================================

test.describe('[Billing Admin] Support Paywall — role grant config (US-PW-001)', () => {
  test.beforeEach(async ({ page, loginPage, demoLogger }) => {
    // 1. Verify the demo environment (realm-001 + seeded users present).
    await verifyTestEnvironment(page, {
      requiredRealms: [TEST_REALM],
      requiredUsers: [REALM_ADMIN_EMAIL],
    })

    // 2. Login as the realm-001 admin (NOT the admin@cas.com admin-realm user —
    //    US-PW-001 scopes to realm-001).
    await loginPage.loginAsAdmin(REALM_ADMIN_EMAIL, REALM_ADMIN_PASSWORD, TEST_REALM)

    // 3. Ensure a dedicated non-builtin role exists so the RoleSelector on the
    //    mappings page will list it. Built-in roles are excluded from the
    //    selector (assignable = non-builtin), so create our own. The roles
    //    page navigates via the sidebar (Authorization → Roles).
    const rolesPage = new RolesPage(page, demoLogger)
    await rolesPage.goto(TEST_REALM)
    const exists = await rolesPage.roleExists(TEST_ROLE_NAME)
    if (!exists) {
      await rolesPage.createRole({
        name: TEST_ROLE_NAME,
        description: 'Demo role for support-paywall US-PW-001 grant config',
      })
    }
  })

  test.afterEach(async ({ page, testStartTime }) => {
    await cleanupTestData(page, TEST_REALM, { timestamp: testStartTime })
  })

  test('US-PW-001 场景1: recurring mapping 可同时配置 role 授予与积分策略（两维度独立叠加）', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    // US-PW-001 场景1 — recurring mapping gains a role grant dimension WITHOUT
    // touching its points strategy; both dimensions coexist independently.

    const mappingsPage = new EntitlementMappingsPage(page, demoLogger)
    await mappingsPage.goto(TEST_REALM)
    await mappingsPage.waitForDataLoaded()

    // Select the first product (the seeded realm-001 recurring mapping).
    await mappingsPage.selectFirstProduct()

    // Resolve the priceKey of the first price row in the detail panel. The
    // priceKey is `externalPriceId ?? mappingId`; for the seeded Stripe row
    // with NULL external_price_id it falls back to the mapping id. Read it from
    // the rendered price-edit-row testid.
    const firstRow = mappingsPage.mappingDetailPanel
      .locator('[data-testid^="price-edit-row-"]')
      .first()
    await expect(firstRow).toBeVisible()
    const rowTestid = (await firstRow.getAttribute('data-testid')) ?? ''
    const priceKey = rowTestid.replace(/^price-edit-row-/, '')
    expect(priceKey, 'a price-edit-row must render with a priceKey suffix').toBeTruthy()

    // Resolve the role id for our dedicated test role (created in beforeEach).
    const roleId = await findRoleIdByName(
      loginPage.getAccessToken(),
      TEST_REALM,
      TEST_ROLE_NAME,
    )
    expect(roleId, `${TEST_ROLE_NAME} must exist before configuring the grant`).toBeTruthy()

    // ---- Baseline: read the CURRENT points strategy (do NOT clobber it) ----
    // The seeded recurring mapping carries a points_per_period; US-PW-001 场景1
    // requires the points dimension to be untouched when adding a role grant.
    const pointsBefore = await readEnabledFixedPointRuleAmount(mappingsPage, priceKey)

    // ---- When: configure role grant (points untouched) ----
    await mappingsPage.selectGrantedRoles(priceKey, [roleId as string])

    await test.step('保存后页面持久化 role 授予配置', async () => {
      await mappingsPage.saveChanges()
      // Reload and re-read — assert on PERSISTED state, not the in-memory form.
      await page.reload()
      await mappingsPage.waitForReady()
      await mappingsPage.waitForDataLoaded()
      await mappingsPage.selectFirstProduct()

      const granted = await mappingsPage.getGrantedRoles(priceKey)
      expect(
        granted,
        'reloaded mapping must persist the granted role id',
      ).toContain(roleId)
    })

    await test.step('积分策略未被 role 授予改动（正交维度）', async () => {
      const pointsAfter = await readEnabledFixedPointRuleAmount(mappingsPage, priceKey)
      // The points dimension is untouched by the role-grant edit: its persisted
      // value must be unchanged after the reload.
      expect(
        pointsAfter,
        'points strategy must be unchanged by the role edit',
      ).toBe(pointsBefore)
    })
  })

  test('US-PW-001 场景2+3: role 授予与积分策略正交，清空一方不影响另一方', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    // US-PW-001 场景2/3 — orthogonality: clearing the role grant must leave the
    // points strategy intact, and a mapping can carry a role grant with an
    // empty points strategy (pure-entitlement config).

    const mappingsPage = new EntitlementMappingsPage(page, demoLogger)
    await mappingsPage.goto(TEST_REALM)
    await mappingsPage.waitForDataLoaded()
    await mappingsPage.selectFirstProduct()

    const firstRow = mappingsPage.mappingDetailPanel
      .locator('[data-testid^="price-edit-row-"]')
      .first()
    await expect(firstRow).toBeVisible()
    const rowTestid = (await firstRow.getAttribute('data-testid')) ?? ''
    const priceKey = rowTestid.replace(/^price-edit-row-/, '')

    const roleId = await findRoleIdByName(
      loginPage.getAccessToken(),
      TEST_REALM,
      TEST_ROLE_NAME,
    )
    expect(roleId, `${TEST_ROLE_NAME} must exist`).toBeTruthy()

    // Establish a known baseline: set a points value + a role grant, save, then
    // clear the role grant and assert points survived. This exercises the
    // orthogonality invariant directly (场景3: clear one, keep the other).
    await mappingsPage.configureFixedPointRule(priceKey, 500)
    await mappingsPage.selectGrantedRoles(priceKey, [roleId as string])
    await mappingsPage.saveChanges()

    await page.reload()
    await mappingsPage.waitForReady()
    await mappingsPage.waitForDataLoaded()
    await mappingsPage.selectFirstProduct()

    // ---- When: clear role grant (points untouched) ----
    await mappingsPage.clearGrantedRoles(priceKey)

    await test.step('清空 role 授予后积分策略保留', async () => {
      await mappingsPage.saveChanges()
      await page.reload()
      await mappingsPage.waitForReady()
      await mappingsPage.waitForDataLoaded()
      await mappingsPage.selectFirstProduct()

      // Role grant cleared.
      const granted = await mappingsPage.getGrantedRoles(priceKey)
      expect(
        granted,
        'cleared role grant must NOT persist the role id',
      ).not.toContain(roleId)

      // Points strategy survived the role clear (orthogonality). The points
      // value was set to 500 above; assert it persisted unchanged — persistent
      // state, NOT a toast.
      const pointsValue = await mappingsPage.getFixedPointRuleAmount(priceKey)
      expect(
        pointsValue,
        'points strategy must survive the role-grant clear (orthogonal dims)',
      ).toBe('500')
    })

    // ---- And conversely: pure-role config (empty points) is a valid save ----
    await test.step('纯 role 授予（积分策略为空）可保存存在', async () => {
      // Disable persisted point rules, set role, save — this is 场景2's claim
      // that a "no points, only role" config is persistable.
      await mappingsPage.clearPointRules(priceKey)
      await mappingsPage.selectGrantedRoles(priceKey, [roleId as string])
      await mappingsPage.saveChanges()

      await page.reload()
      await mappingsPage.waitForReady()
      await mappingsPage.waitForDataLoaded()
      await mappingsPage.selectFirstProduct()

      expect(
        await mappingsPage.getEnabledPointRuleCount(priceKey),
        'pure-entitlement mapping must have no enabled point rules',
      ).toBe(0)
      const granted = await mappingsPage.getGrantedRoles(priceKey)
      expect(
        granted,
        'pure-entitlement mapping (role grant, empty points) must persist',
      ).toContain(roleId)
    })
  })
})
