/**
 * Realm Onboarding Guidance Demo Tests
 *
 * Covers the user-story acceptance paths for the realm-onboarding-guidance
 * feature: the storefront self-service signup CTA on the admin-realm login
 * page (US-OG-001) and the first-login console guidance — welcome dialog,
 * completion marker and checklist tour replay (US-OG-002/003).
 *
 * Scenario groups (one browser session per test, fresh localStorage per
 * context):
 *
 *   - US-OG-001 scenarios 1/2/3: with the platform self-service toggle ON, a
 *     visitor on /admin/auth/login sees the guidance block, the CTA enters
 *     /admin/auth/signup, and the demoted register link still opens the
 *     existing register page; with the toggle OFF the block is not rendered
 *     at all (fail-closed) while the login form stays intact.
 *   - US-OG-002 scenarios 1/2/3: a visitor completes self-service signup →
 *     lands in the NEW realm's console (session-scoped /manage) → the welcome
 *     dialog appears → dismissing writes the localStorage completion marker
 *     → a refresh does not re-show it → the starter checklist replays the
 *     console tour (driver.js popover).
 *   - US-OG-003 scenarios 1/2: an EXISTING admin-realm admin with no
 *     completion marker gets the same-shape guidance on first console entry;
 *     after dismissing, it never auto-reappears.
 *   - US-OG-001 scenario 4: a seeded non-admin realm's login page renders no
 *     guidance block and fires no signup-status query.
 *   - US-OG-002 scenario 4: clearing the completion marker (device switch /
 *     cleared browser data) re-shows the guidance exactly once, dismissable
 *     immediately.
 *
 * Acceptance assertions land on persistent observable results (URL, rendered
 * testids, localStorage marker keys), NOT on auto-dismissing sonner/toast.
 *
 * NOT-COVERED (explicitly declared):
 *   - US-OG-002 scenario 1's "copy follows interface language": key parity
 *     for en/zh-CN is gated by the frontend i18n orphan check and the
 *     language-switching mechanism is covered by the existing i18n demos;
 *     this demo's assertions are testid/URL based and locale independent.
 *   - The welcome-copy VARIANTS (fresh-signup vs generic): variant rendering
 *     is covered by the component test onboarding-welcome-dialog.test.tsx;
 *     both trigger paths here are asserted by the same-shape testids.
 *   - Narrow-viewport sidebar collapse dropping tour anchors: anchor
 *     filtering and the all-anchors-missing no-start degradation are covered
 *     by console-tour.test.tsx; this demo runs the desktop main path.
 *   - Walking the tour to its final Done step: the demo covers tour start
 *     and mid-way exit (close button); "finished tour writes the marker" is
 *     covered by the orchestrator component test.
 *
 * @see frontend/src/components/onboarding/
 */

import { test, expect, cleanupTestData } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { loginAsAdmin, logout, DEMO_ADMIN } from '../helpers/auth'
import type { UnifiedLogger } from '../helpers/unified-logger'
import { SettingsPage } from '../pages/settings-page'
import { SELECTORS } from '../selectors'
import type { Page } from '@playwright/test'

// The storefront CTA and the console guidance are admin-realm experiences;
// the unaffected-realm check uses a seeded tenant realm.
const ADMIN_REALM = 'admin'
const NON_ADMIN_REALM = 'realm1'
const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'
const LOGIN_URL = `${BASE_URL}/${ADMIN_REALM}/auth/login`
const SIGNUP_URL = `${BASE_URL}/${ADMIN_REALM}/auth/signup`

/**
 * localStorage completion markers `herald.onboarding.{realmId}.{userId}`
 * (value 'completed'). Read via page.evaluate — they are browser-local state
 * with no DOM surface.
 */
async function onboardingCompletionEntries(page: Page): Promise<Array<[string, string]>> {
  return page.evaluate(() =>
    Object.entries(window.localStorage).filter(([key]) => key.startsWith('herald.onboarding.'))
  )
}

/** Simulate a device switch / cleared browser data for the onboarding marker. */
async function clearOnboardingCompletionMarkers(page: Page): Promise<void> {
  await page.evaluate(() => {
    for (const key of Object.keys(window.localStorage)) {
      if (key.startsWith('herald.onboarding.')) window.localStorage.removeItem(key)
    }
  })
}

/**
 * Login to the admin console, open the settings platform-signup tab via
 * SettingsPage.gotoDirect() (robust against the credential-switch cache
 * race) and flip the self-service toggle, polling the persisted switch
 * state. Returns the ready SettingsPage; callers assign it to the
 * describe-scoped `settingsPage` so afterEach resets the toggle.
 */
async function setPlatformSignupEnabled(
  page: Page,
  demoLogger: UnifiedLogger,
  enabled: boolean
): Promise<SettingsPage> {
  await loginAsAdmin(page, { realmId: ADMIN_REALM })
  const settings = new SettingsPage(page, demoLogger, ADMIN_REALM)
  await settings.gotoDirect()
  await settings.switchToPlatformSignupTab()
  if (enabled) {
    await settings.enablePlatformSignup()
  } else {
    await settings.disablePlatformSignup()
  }
  await settings.savePlatformSignupConfig()
  // Persisted assertion: the switch reflects the enabled state, not a toast.
  await expect.poll(() => settings.isPlatformSignupEnabled(), { timeout: 15000 }).toBe(enabled)
  return settings
}

async function dismissWelcomeDialog(page: Page): Promise<void> {
  await page.locator(SELECTORS.onboarding.welcomeDismissButton).click()
  await expect(page.locator(SELECTORS.onboarding.welcomeDialog)).toBeHidden()
}

/** Assert the localStorage completion marker (`herald.onboarding.*` = 'completed') is written. */
async function expectOnboardingCompletionMarkerWritten(page: Page): Promise<void> {
  const entries = await onboardingCompletionEntries(page)
  expect(entries.some(([, value]) => value === 'completed'), JSON.stringify(entries)).toBe(true)
}

/**
 * Reload the console and assert the welcome dialog does NOT re-show: wait
 * for `consoleAnchor` (an always-resident console testid) first so a slow
 * page cannot false-pass the hidden assertion.
 */
async function expectWelcomeDialogNotReshownAfterReload(
  page: Page,
  consoleAnchor: string
): Promise<void> {
  await page.reload({ waitUntil: 'domcontentloaded' })
  await expect(page.locator(consoleAnchor)).toBeVisible({ timeout: 20000 })
  await expect(page.locator(SELECTORS.onboarding.welcomeDialog)).toHaveCount(0)
}

// Shared config: the toggle is enabled for the visitor scenarios and reset to
// its fail-closed default (false) in afterEach, following the
// platform-signup-demo reset pattern so no open public signup entry leaks
// into other demos.
test.describe('[Super Admin] Realm Onboarding Guidance Demo', () => {
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
    // 1. Best-effort: reset the platform-signup toggle to its fail-closed
    //    default (false). resetPlatformSignupConfig is internally
    //    try/catch'd and never hard-fails the run. forceRelogin is REQUIRED:
    //    loginAsAdmin short-circuits when already on a /manage URL, and the
    //    signup scenario ends on session-scoped /manage in the NEWLY created
    //    realm — without forceRelogin it would skip login and stay in the
    //    wrong realm.
    if (settingsPage) {
      try {
        await loginAsAdmin(page, { realmId: ADMIN_REALM, forceRelogin: true })
        settingsPage = new SettingsPage(page, demoLogger, ADMIN_REALM)
        await settingsPage.gotoDirect()
        await settingsPage.resetPlatformSignupConfig()
        // Reset consumed: clear the flag so tests that never touched the
        // toggle skip the full re-login + reset instead of redoing it.
        settingsPage = undefined
      } catch (error) {
        console.warn('[onboarding-guidance-demo] resetPlatformSignupConfig failed:', error)
      }
    }

    // 2. Realm lifecycle note: the signup scenario provisions a NEW realm.
    //    Realm deletion is deliberately unsupported by the system (see the
    //    platform-signup-demo afterEach note), so created realms PERSIST in
    //    the demo env. The per-run timestamp slug keeps successive runs from
    //    colliding; re-seed via scripts/demo-start.py if the list grows.

    // 3. MANDATORY: clear demo test data within the admin realm.
    await cleanupTestData(page, ADMIN_REALM, {
      keepUsers: [DEMO_ADMIN.email],
      timestamp: testStartTime,
    })
  })

  // ===========================================================================
  // US-OG-001: storefront login guidance follows the platform-signup toggle;
  // the register entry stays functional while demoted.
  // ===========================================================================
  test('US-OG-001: storefront login CTA follows the platform-signup toggle', async ({
    page,
    demoLogger,
  }) => {
    await test.step('Enable the platform self-service signup toggle', async () => {
      settingsPage = await setPlatformSignupEnabled(page, demoLogger, true)
    })

    await test.step('Visitor sees the guidance block on the storefront login', async () => {
      // logout() already lands on LOGIN_URL in its finally block
      await logout(page)

      // The guidance block renders with the toggle ON (scenario 1) and the
      // existing login form stays intact.
      await expect(page.locator(SELECTORS.loginSignupCta.block)).toBeVisible({
        timeout: 15000,
      })
      await expect(page.locator(SELECTORS.login.container)).toBeVisible()

      // Scenario 3: the demoted register link still renders (admin-realm
      // registration is enabled in the demo seed) alongside the CTA.
      await expect(page.locator(SELECTORS.login.registerLink)).toBeVisible()
    })

    await test.step('CTA enters the self-service signup page', async () => {
      await page.locator(SELECTORS.loginSignupCta.link).click()

      // The CTA targets the admin-realm-hosted self-service signup page.
      await page.waitForURL(
        (url) => url.pathname === `/${ADMIN_REALM}/auth/signup`,
        { timeout: 15000 }
      )
      await expect(page.locator(SELECTORS.platformSignup.card)).toBeVisible({ timeout: 15000 })
      await expect(page.locator(SELECTORS.platformSignup.disabledNotice)).toBeHidden()
    })

    await test.step('Demoted register link still opens the existing register page', async () => {
      await page.goto(LOGIN_URL, { waitUntil: 'domcontentloaded' })
      const registerLink = page.locator(SELECTORS.login.registerLink)
      await expect(registerLink).toBeVisible({ timeout: 15000 })
      await registerLink.click()

      await page.waitForURL(
        (url) => url.pathname === `/${ADMIN_REALM}/auth/register`,
        { timeout: 15000 }
      )
      // The existing register page renders its own card — the register
      // capability is unchanged behind the visual demotion.
      await expect(page.locator(SELECTORS.registration.card)).toBeVisible({ timeout: 15000 })
    })

    await test.step('Toggle OFF: fail-closed, the block is not rendered', async () => {
      settingsPage = await setPlatformSignupEnabled(page, demoLogger, false)

      // Gate the absence assertion on the actual status response: the block
      // renders only on `enabled === true`, so a bare count-0 could pass
      // while the query is still in flight. Mount the listener BEFORE
      // logout() — its finally-block navigation to LOGIN_URL already fires
      // the /signup/status request, so no extra reload is needed.
      const statusResponse = page.waitForResponse(
        (response) => response.url().includes('/signup/status'),
        { timeout: 15000 }
      )
      await logout(page)

      // Scenario 2: with the toggle off the status resolves disabled, the
      // block is absent from the DOM (fail-closed), and the login form keeps
      // working.
      const response = await statusResponse
      expect(await response.json()).toEqual({ enabled: false })
      await expect(page.locator(SELECTORS.loginSignupCta.block)).toHaveCount(0)
      await expect(page.locator(SELECTORS.login.container)).toBeVisible({ timeout: 15000 })
      demoLogger.testCode.log('Toggle off: CTA absent, login form intact')
    })
  })

  // ===========================================================================
  // US-OG-002: a fresh self-service signup admin gets the first-login
  // guidance once, and can replay the tour from the starter checklist.
  // ===========================================================================
  test('US-OG-002: fresh signup admin gets first-login guidance once', async ({
    page,
    demoLogger,
  }) => {
    await test.step('Enable the platform self-service signup toggle', async () => {
      settingsPage = await setPlatformSignupEnabled(page, demoLogger, true)
    })

    const stamp = testStartTime
    const realmSlug = `onboarding-demo-${stamp}`

    await test.step('Visitor completes self-service signup', async () => {
      await logout(page)
      await page.goto(SIGNUP_URL, { waitUntil: 'domcontentloaded' })
      await expect(page.locator(SELECTORS.platformSignup.card)).toBeVisible({ timeout: 15000 })

      await page.locator(SELECTORS.platformSignup.realmNameInput).fill(`Onboarding Demo ${stamp}`)
      // Explicit realm slug → deterministic realm id for the marker key.
      await page.locator(SELECTORS.platformSignup.realmSlugInput).fill(realmSlug)
      await page
        .locator(SELECTORS.platformSignup.emailInput)
        .fill(`onboarding-demo-${stamp}@signup.test`)
      await page.locator(SELECTORS.platformSignup.passwordInput).fill('OnboardingDemo123!')
      await page.locator(SELECTORS.platformSignup.submitButton).click()

      // The success redirect is session-scoped /manage with no realm prefix
      // (the new realm is carried by the auth store session).
      await page.waitForURL((url) => url.pathname === '/manage', { timeout: 30000 })
    })

    await test.step('Welcome dialog appears in the new realm console', async () => {
      // Scenario 1: the guidance appears on the console's first render. The
      // dialog offers both exits (start tour / dismiss) — persistent testids.
      await expect(page.locator(SELECTORS.onboarding.welcomeDialog)).toBeVisible({
        timeout: 20000,
      })
      await expect(page.locator(SELECTORS.onboarding.tourStartButton)).toBeVisible()
      demoLogger.testCode.log('Welcome dialog visible in new realm console')
    })

    await test.step('Dismiss writes the completion marker', async () => {
      await dismissWelcomeDialog(page)

      // The marker is the persistent business state that prevents re-showing:
      // herald.onboarding.{newRealmId}.{userId} = 'completed'.
      await expectOnboardingCompletionMarkerWritten(page)
    })

    await test.step('Refresh does not re-show; the checklist persists', async () => {
      await expectWelcomeDialogNotReshownAfterReload(page, SELECTORS.sidebar.menuUsers)

      // Scenario 3 precondition: the starter checklist is a persistent
      // resident of the dashboard, independent of the marker.
      await expect(page.locator(SELECTORS.onboarding.tasksCard)).toBeVisible()
    })

    await test.step('Replay the console tour from the checklist', async () => {
      await page.locator(SELECTORS.onboarding.tourRestartButton).click()

      // The tour is the re-skinned driver.js popover: the class hook is the
      // stable observable (the lazy tour chunk may take a moment to load).
      const popover = page.locator(SELECTORS.onboarding.tourPopover)
      await expect(popover).toBeVisible({ timeout: 15000 })

      // Mid-way exit (close button) tears the popover down without breaking
      // the console — the checklist stays available for another replay.
      await popover.locator('.driver-popover-close-btn').click()
      await expect(popover).toBeHidden()
      await expect(page.locator(SELECTORS.onboarding.tasksCard)).toBeVisible()
      demoLogger.testCode.log('Tour replayed from checklist and exited via close button')
    })
  })

  // ===========================================================================
  // US-OG-003: an existing admin-realm admin with no completion marker gets
  // the same guidance on first console entry; it never auto-reappears after.
  // ===========================================================================
  test('US-OG-003: existing admin gets the same guidance on first entry', async ({
    page,
    demoLogger,
  }) => {
    await test.step('Existing admin logs in (no completion marker)', async () => {
      // Fresh browser context → empty localStorage → no marker for the
      // admin-realm DEMO_ADMIN user. dismissOnboarding: false keeps the
      // guidance open — this scenario asserts it.
      await loginAsAdmin(page, { realmId: ADMIN_REALM, dismissOnboarding: false })
      await page.waitForURL((url) => url.pathname.startsWith('/manage'), { timeout: 30000 })
    })

    await test.step('Same-shape welcome dialog appears', async () => {
      // Scenario 1: the non-signup path triggers the same guidance shape.
      await expect(page.locator(SELECTORS.onboarding.welcomeDialog)).toBeVisible({
        timeout: 20000,
      })
      demoLogger.testCode.log('Welcome dialog visible for existing admin')
    })

    await test.step('Dismiss → marker written → refresh does not re-show', async () => {
      await dismissWelcomeDialog(page)
      await expectOnboardingCompletionMarkerWritten(page)
      await expectWelcomeDialogNotReshownAfterReload(page, SELECTORS.sidebar.menuDashboard)
    })
  })

  // ===========================================================================
  // US-OG-001 scenario 4: a seeded non-admin realm's login page is untouched
  // by the storefront guidance — no block, no status query.
  // ===========================================================================
  test('US-OG-001: non-admin realm login page stays unaffected', async ({ page, demoLogger }) => {
    await test.step('Seeded tenant realm exists', async () => {
      // realm1 is a seeded tenant realm; the public config endpoint is its
      // public existence proof.
      const response = await page.request.get(`${BASE_URL}/api/public-config/${NON_ADMIN_REALM}`)
      expect(response.status(), `realm ${NON_ADMIN_REALM} must exist in the demo seed`).toBe(200)
    })

    await test.step('Visitor opens the tenant realm login page', async () => {
      // Watch for any signup-status query from here on: the guidance block is
      // never mounted outside the admin realm, so the query must not fire.
      const statusRequests: string[] = []
      page.on('request', (request) => {
        if (request.url().includes('/signup/status')) statusRequests.push(request.url())
      })

      await page.goto(`${BASE_URL}/${NON_ADMIN_REALM}/auth/login`, {
        waitUntil: 'domcontentloaded',
      })
      await expect(page.locator(SELECTORS.login.container)).toBeVisible({ timeout: 15000 })
      await expect(page.locator(SELECTORS.loginSignupCta.block)).toHaveCount(0)

      // Best-effort settle for late mount-time requests; the request
      // listener above captures anything that fires regardless.
      await page.waitForLoadState('networkidle').catch(() => {})
      expect(statusRequests, 'signup-status query must not fire on a tenant login page').toEqual(
        []
      )
      demoLogger.testCode.log(`Tenant realm ${NON_ADMIN_REALM}: no CTA, no status query`)
    })
  })

  // ===========================================================================
  // US-OG-002 scenario 4: clearing the completion marker (device switch /
  // cleared browser data) re-shows the guidance exactly once.
  // ===========================================================================
  test('US-OG-002: cleared marker re-shows the guidance once', async ({ page, demoLogger }) => {
    await test.step('Existing admin completes the guidance', async () => {
      // dismissOnboarding: false — the marker-clearing scenario below needs
      // the guidance to appear and be dismissed by the test itself.
      await loginAsAdmin(page, { realmId: ADMIN_REALM, dismissOnboarding: false })
      await page.waitForURL((url) => url.pathname.startsWith('/manage'), { timeout: 30000 })
      await expect(page.locator(SELECTORS.onboarding.welcomeDialog)).toBeVisible({
        timeout: 20000,
      })
      await dismissWelcomeDialog(page)
    })

    await test.step('Device switch (cleared marker) re-shows the guidance', async () => {
      await clearOnboardingCompletionMarkers(page)
      expect(await onboardingCompletionEntries(page)).toEqual([])

      await page.reload({ waitUntil: 'domcontentloaded' })
      await expect(page.locator(SELECTORS.sidebar.menuDashboard)).toBeVisible({ timeout: 20000 })
      await expect(page.locator(SELECTORS.onboarding.welcomeDialog)).toBeVisible({ timeout: 20000 })
      demoLogger.testCode.log('Guidance re-shown once after marker cleared')
    })

    await test.step('Immediate dismiss → not re-shown afterwards', async () => {
      await dismissWelcomeDialog(page)
      await expectWelcomeDialogNotReshownAfterReload(page, SELECTORS.sidebar.menuDashboard)
    })
  })
})
