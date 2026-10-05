/**
 * Platform Self-Service Realm Signup Demo Tests
 *
 * Covers the P0 main-story acceptance path for the realm-create feature's
 * self-service signup entry, as specified in design §6.2 (Demo / E2E):
 *
 *   - US-SR-001 / US-SR-002: a visitor opens the admin-realm-hosted public
 *     signup page, fills in the form and submits → the platform provisions a
 *     NEW realm, issues a first-party session for the new realm, and lands the
 *     visitor in that realm's management console as its realm-admin
 *     (DEC-realm-create-002/004/012).
 *   - US-SR-004 scenario 1: an Admin Realm admin disables the platform
 *     self-service toggle → a visitor opening the signup page sees the
 *     fail-closed notice and cannot submit (DEC-realm-create-009/013).
 *
 * User stories:
 *   docs/user-stories/core/realm-create.md
 *   - US-SR-001: 自助注册开通新 Realm
 *   - US-SR-002: 开通后立即管理新 Realm
 *   - US-SR-004: 平台自助开通开关控制
 *
 * Single browser session with multiple steps, following the "one browser
 * session" principle from the demo testing spec. Acceptance assertions land on
 * persistent observable results (URL navigation to /manage, sidebar management
 * entries, the disabled-notice card), NOT on auto-dismissing sonner/toast.
 *
 * NOT-COVERED (explicitly declared): design §6.2 lists a third demo item —
 * "同 IP 第 3 次 → 断言限额提示" (US-SR-001 scenario 4, the 2-per-24h same-IP
 * quota, DEC-realm-create-007/011). This CANNOT be demonstrated in the demo
 * environment: the signup handler calls `rate_limit_hit` (NOT
 * `rate_limit_hit_forced`), and `RateLimitConfig.enforce_in_dev` defaults to
 * `false`, so the quota is skipped entirely in non-production/demo contexts —
 * the 3rd attempt provisions successfully instead of returning 429. The
 * backend scenario test `test_signup_ip_limit_24h`
 * (backend/api/src/tests/scenarios/signup_scenarios.rs) documents and asserts
 * exactly this actual (non-429) behavior. Forcing the quota in the demo would
 * require switching the signup handler to `rate_limit_hit_forced`, which
 * changes production security throttling behavior to satisfy a demo and is
 * rejected by the demo / accept contracts (do not alter prod code to make a
 * test pass). The live 429 path is exercised in environments that opt into
 * enforce_in_dev.
 *
 * Also not covered here:
 *   - US-SR-001 scenario 3 (realmSlug conflict) and scenario 5 (Turnstile).
 *     Slug-conflict status semantics (400 vs 409, DEC-realm-create-014) are
 *     verified by the backend signup scenarios; Turnstile is off by default in
 *     the demo seed (DEC-realm-create-008), so the signup form renders no
 *     human-verification widget during these runs.
 *   - US-SR-002 scenario 2 (cross-realm isolation). Realm isolation is
 *     enforced server-side and covered by the backend signup scenarios
 *     (`test_signup_isolation`). On the frontend, an authenticated non-admin
 *     hitting /admin/manage is redirected to /user/profile by the root loader
 *     (frontend/src/routes/__root.tsx) before any manage data loads — that is
 *     generic routing behavior, not specific to the signup feature, so it is
 *     not asserted here.
 *
 * @see docs/user-stories/core/realm-create.md
 * @see backend/api-auth/src/signup.rs
 * @see frontend/src/routes/$realmId/auth/signup.tsx
 */

import { test, expect, cleanupTestData } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { loginAsAdmin, logout, DEMO_ADMIN } from '../helpers/auth'
import { SettingsPage } from '../pages/settings-page'
import { SELECTORS } from '../selectors'

// Self-service signup is hosted exclusively by the admin realm (DEC-001). All
// navigation targets the admin-realm public entry.
const ADMIN_REALM = 'admin'
const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'
const SIGNUP_URL = `${BASE_URL}/${ADMIN_REALM}/auth/signup`

/**
 * Navigate directly to the session-scoped settings page and wait for it ready.
 *
 * SettingsPage.goto() navigates via a sidebar-menu click. After the signup
 * success scenario, the in-memory React Query cache can hold a 403 from the
 * admin-web-console credential-switch race (documented in demo-page.fixtures.ts
 * dashboardPage fixture): a stale cached error on the settings query prevents
 * the `settings-page` container from rendering within the 10s timeout. The
 * robust fix — used by RealmsPage.goto() (direct URL) and the dashboardPage
 * fixture (reload after login) — is to navigate directly to the session-scoped
 * /manage/settings URL, clear the in-memory React Query cache, and reload so
 * the SPA remounts with the correct admin-web-console bearer and a fresh cache.
 */
async function navigateToSettings(settingsPage: SettingsPage): Promise<void> {
  const page = settingsPage.page
  await page.goto(`${BASE_URL}/manage/settings`, { waitUntil: 'domcontentloaded' })
  // Clear the in-memory React Query cache (exposed on window in main.tsx) so a
  // stale cached 403 from a credential-switch race cannot prevent the settings
  // content from rendering after the reload.
  await page.evaluate(() => {
    const w = window as typeof window & { __REACT_QUERY_CLIENT__?: { clear: () => void } }
    w.__REACT_QUERY_CLIENT__?.clear()
  }).catch(() => {})
  await page.reload({ waitUntil: 'networkidle', timeout: 15000 })
  await settingsPage.waitForReady()
}

// Shared config so the two scenarios form one logical main-story path: the
// toggle is enabled for the success scenario and restored to its seeded
// default (true) in afterEach.
test.describe('[Super Admin] Platform Self-Service Realm Signup Demo', () => {
  let testStartTime: number
  let settingsPage: SettingsPage | undefined

  test.beforeEach(async ({ page, testStartTime: startTime }) => {
    testStartTime = startTime

    await verifyTestEnvironment(page, {
      requiredRealms: [ADMIN_REALM],
      requiredUsers: [DEMO_ADMIN.email],
      skipRealmVerification: true,
    })
  })

  test.afterEach(async ({ page, demoLogger }) => {
    // 1. Best-effort: restore the platform-signup toggle to its seeded
    //    default (true) so the demo env is left in its seeded state and the
    //    storefront signup CTA stays visible. resetPlatformSignup-
    //    Config is internally try/catch'd and never hard-fails the run.
    if (settingsPage) {
      try {
        // Re-login to admin realm (the success scenario leaves the session on
        // the session-scoped /manage in the NEWLY created realm). forceRelogin
        // is REQUIRED: loginAsAdmin short-circuits when already on a /manage
        // URL, and the success flow lands on session-scoped /manage — without
        // forceRelogin it would skip login and stay in the wrong realm.
        await loginAsAdmin(page, { realmId: ADMIN_REALM, forceRelogin: true })
        settingsPage = new SettingsPage(page, demoLogger, ADMIN_REALM)
        await navigateToSettings(settingsPage)
        await settingsPage.resetPlatformSignupConfig()
      } catch (error) {
        console.warn('[platform-signup-demo] resetPlatformSignupConfig failed:', error)
      }
    }

    // 2. Realm lifecycle note: the signup success scenario provisions a NEW
    //    realm. Realm deletion is deliberately unsupported by the system — the
    //    domain RealmService::delete_realm trait method exists but is NOT
    //    exposed via any HTTP route (backend test `test_delete_realm_not_
    //    supported` asserts this), and the frontend realms table exposes only
    //    an Edit action (no delete). So created realms PERSIST in the demo env.
    //    The success scenario uses a per-run timestamp slug (signup-demo-<stamp>)
    //    so successive runs never collide. If the demo realm list grows too
    //    large over time, reset it by re-seeding the demo database
    //    (`scripts/demo-start.py` rebuilds herald_demo).

    // 3. MANDATORY: clear demo test data within the admin realm.
    await cleanupTestData(page, ADMIN_REALM, {
      keepUsers: [DEMO_ADMIN.email],
      timestamp: testStartTime,
    })
  })

  // ===========================================================================
  // US-SR-001 + US-SR-002: visitor self-signs up and is immediately landed in
  // the new realm's management console as its realm-admin.
  // ===========================================================================
  test('US-SR-001/002: visitor self-signs up and lands in the new realm console', async ({
    page,
    demoLogger,
  }) => {
    // -------------------------------------------------------------------------
    // Setup: enable the platform self-service toggle (the demo seed ships it
    // enabled, so this is an idempotent, self-contained setup step).
    // -------------------------------------------------------------------------
    await test.step('Enable the platform self-service signup toggle', async () => {
      await loginAsAdmin(page, { realmId: ADMIN_REALM })
      settingsPage = new SettingsPage(page, demoLogger, ADMIN_REALM)
      await navigateToSettings(settingsPage)
      await settingsPage.switchToPlatformSignupTab()
      await settingsPage.enablePlatformSignup()
      await settingsPage.savePlatformSignupConfig()

      // Persisted assertion: the switch reflects the enabled state, not a toast.
      await expect.poll(async () => settingsPage!.isPlatformSignupEnabled(), {
        timeout: 15000,
      }).toBe(true)
    })

    // -------------------------------------------------------------------------
    // Act: a fresh, unauthenticated visitor opens the public signup page and
    // submits the form. An explicit per-run realm slug makes the provisioned
    // realm deterministic and identifiable in the realms list. (The success
    // redirect URL is session-scoped /manage with no realm prefix, so the realm
    // id is not read back from the URL — the explicit slug is the only handle.)
    // -------------------------------------------------------------------------
    const stamp = testStartTime
    const realmSlug = `signup-demo-${stamp}`
    const realmName = `Signup Demo Realm ${stamp}`
    const email = `signup-demo-${stamp}@signup.test`
    const password = 'SignupDemo123!'

    await test.step('Visitor opens the public signup page', async () => {
      await logout(page)
      await page.goto(SIGNUP_URL, { waitUntil: 'domcontentloaded' })

      // The signup card (enabled state) must be visible, NOT the disabled
      // notice. This distinguishes US-SR-001 scenario 1 from US-SR-004.
      await expect(page.locator(SELECTORS.platformSignup.card)).toBeVisible({
        timeout: 15000,
      })
      await expect(page.locator(SELECTORS.platformSignup.disabledNotice)).toBeHidden()
    })

    await test.step('Visitor fills and submits the signup form', async () => {
      await page.locator(SELECTORS.platformSignup.realmNameInput).fill(realmName)
      // Explicit realm slug → deterministic realm id for cleanup.
      await page.locator(SELECTORS.platformSignup.realmSlugInput).fill(realmSlug)
      await page.locator(SELECTORS.platformSignup.emailInput).fill(email)
      await page.locator(SELECTORS.platformSignup.passwordInput).fill(password)

      await page.locator(SELECTORS.platformSignup.submitButton).click()
    })

    // -------------------------------------------------------------------------
    // Assert (US-SR-001 + US-SR-002): a NEW realm was provisioned and the
    // visitor is landed in that realm's management console as realm-admin. The
    // success token is issued for the NEW realm (DEC-012) and the frontend's
    // completeSignup → redirectPathForPermissions yields DEFAULT_ADMIN_REDIRECT
    // = '/manage' (a session-scoped path: the new realm is carried by the auth
    // store session, NOT by the URL — see realm-routing.ts realmPath()).
    // -------------------------------------------------------------------------
    await test.step('Provisioned realm: landed in the management console', async () => {
      // Session-scoped /manage has NO realm prefix — assert the prefix-less
      // path, not /{realmId}/manage. The new realm id is NOT in the URL.
      await page.waitForURL((url) => url.pathname === '/manage', { timeout: 30000 })
      demoLogger.testCode.log(`Landed at session-scoped /manage: ${page.url()}`)
    })

    await test.step('New realm-admin: sidebar management entries are visible', async () => {
      // US-SR-002 scenario 1: the new realm-admin sees management entries
      // (users, settings). Assert on persistent sidebar entries, not toast.
      await expect(page.locator(SELECTORS.sidebar.menuUsers)).toBeVisible({ timeout: 20000 })
      await expect(page.locator(SELECTORS.sidebar.menuSettings)).toBeVisible()
      demoLogger.testCode.log('realm-admin management entries visible in new realm')
    })
  })

  // ===========================================================================
  // US-SR-004 scenario 1: with the toggle disabled, the signup page shows the
  // fail-closed notice and submission is unavailable.
  // ===========================================================================
  test('US-SR-004: disabled toggle shows the signup-disabled notice', async ({
    page,
    demoLogger,
  }) => {
    await test.step('Disable the platform self-service signup toggle', async () => {
      await loginAsAdmin(page, { realmId: ADMIN_REALM })
      settingsPage = new SettingsPage(page, demoLogger, ADMIN_REALM)
      await navigateToSettings(settingsPage)
      await settingsPage.switchToPlatformSignupTab()
      await settingsPage.disablePlatformSignup()
      await settingsPage.savePlatformSignupConfig()

      await expect.poll(async () => settingsPage!.isPlatformSignupEnabled(), {
        timeout: 15000,
      }).toBe(false)
    })

    await test.step('Visitor opens the public signup page (toggle off)', async () => {
      await logout(page)
      await page.goto(SIGNUP_URL, { waitUntil: 'domcontentloaded' })

      // Fail-closed: the disabled notice is shown and the signup card / form
      // are NOT rendered (DEC-013). Submission is unavailable by construction.
      await expect(page.locator(SELECTORS.platformSignup.disabledNotice)).toBeVisible({
        timeout: 15000,
      })
      await expect(page.locator(SELECTORS.platformSignup.card)).toBeHidden()
      await expect(page.locator(SELECTORS.platformSignup.submitButton)).toBeHidden()
      demoLogger.testCode.log('Disabled notice visible; signup form not rendered')
    })
  })
})
