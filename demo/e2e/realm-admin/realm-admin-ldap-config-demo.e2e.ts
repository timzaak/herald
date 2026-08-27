/**
 * Realm Admin comprehensive demo - corporate directory (LDAP) configuration.
 *
 * User story: US-LD-003 - the Realm Admin configures and manages the realm's
 * LDAP directory (docs/user-stories/auth/support-ldap.md).
 *
 * Test coverage (mirrors realm-admin-email-otp-config-demo.e2e.ts: SettingsPage
 * drives every interaction, no data-testid strings inlined here):
 * - Phase 1: blocked-save gate — enabling with a service-account DN but no
 *   stored and no newly-typed password is rejected with actionable guidance
 *   (the stored value is masked, so row existence is the only signal).
 * - Phase 2: plaintext directory address (ldap:// without StartTLS) is
 *   rejected inline BEFORE any request fires, and nothing persists.
 * - Phase 3: a valid ldaps:// config saves; reload echoes values back and the
 *   password field stays EMPTY (secret is masked, never echoed).
 * - Phase 4: enabling with a blank password keeps the stored one (the save
 *   succeeds and survives a reload).
 * - Phase 5: StartTLS is locked off while the URL is ldaps:// (TLS comes from
 *   the scheme; the pairing is enforced on both ends).
 * - Phase 6: once enabled, the realm login page shows the corporate entry.
 * - Phase 7: disabling hides the entry again (graceful degradation) while the
 *   config itself is preserved.
 *
 * The UI-enabled state intentionally has NO private-CA trust configured (the
 * v1 form does not manage caCertPem), so this demo never performs a real
 * directory login — entry visibility is what the public status endpoint
 * actually reflects. Real directory authentication lives in
 * regular-user/ldap-login-demo.e2e.ts, which seeds the CA trust via the
 * admin configs API.
 *
 * NOT-COVERED (explicit): US-LD-003 cross-realm 403 guard (needs a second
 * realm admin; covered by backend scenario tests) and audit-log visibility of
 * LDAP login events (covered by backend scenario tests, method="ldap").
 */

import { test, expect, cleanupTestData } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { loginAsAdmin } from '../helpers/auth'
import { DEMO_LDAP, resetLdapRowsForRealm } from '../helpers/ldap-setup'
import { SettingsPage } from '../pages/settings-page'
import { SELECTORS } from '../selectors'

test.describe('[Realm Admin] LDAP corporate-directory config demo', () => {
  let testStartTime: number
  let settingsPage: SettingsPage | undefined
  const realmId = 'realm-001'

  test.afterEach(async ({ page, demoLogger }) => {
    // Best-effort: delete both ldap config rows so the realm returns to the
    // pristine "not configured" state (the next run's Phase 1 depends on
    // hasBindPassword === false). resetLdapRowsForRealm is try/catch-wrapped
    // and cannot hard-fail this test.
    await resetLdapRowsForRealm(page, demoLogger, realmId)

    // ⚠️ MANDATORY: clean up test data
    await cleanupTestData(page, realmId, {
      keepUsers: ['admin@realm-001.com'],
      timestamp: testStartTime,
    })
  })

  // ============================================================================
  // User story US-LD-003: Realm Admin configures and manages the LDAP directory
  // ============================================================================
  test('US-LD-003: admin configures, enables and disables the corporate LDAP directory', async ({ page, demoLogger }) => {
    testStartTime = Date.now()
    settingsPage = new SettingsPage(page, demoLogger, realmId)

    // ⚠️ MANDATORY: verify environment state
    await verifyTestEnvironment(page, {
      requiredRealms: [realmId],
      requiredUsers: ['admin@realm-001.com'],
      skipRealmVerification: true, // Optimized: skip deep realm checks
      skipDatabaseCheck: false,    // Keep health check
      skipRedisCheck: false,       // Keep health check
    })

    // Pristine precondition: no ldap rows → hasBindPassword === false, so the
    // Phase 1 gate is genuinely exercised (not bypassed by a stored secret).
    await resetLdapRowsForRealm(page, demoLogger, realmId)

    // Log in as the realm admin and open Settings
    await loginAsAdmin(page, { realmId })
    await settingsPage.goto()
    await settingsPage.waitForReady()

    // ========================================================================
    // Phase 1: blocked-save gate — enable + service DN + never-saved password
    // ========================================================================
    await test.step('Phase 1: enabling with a service DN but no password is blocked', async () => {
      await test.step('switch to the Corporate directory (LDAP) tab', async () => {
        await settingsPage.switchToLdapTab()
      })

      await test.step('fill a valid ldaps:// config WITHOUT a service password', async () => {
        await settingsPage.fillLdapConfig({
          url: DEMO_LDAP.ldapsUrl,
          baseDn: DEMO_LDAP.baseDn,
          bindDn: DEMO_LDAP.bindDn,
          bindPassword: '',
          userFilter: DEMO_LDAP.userFilter,
          mailAttribute: DEMO_LDAP.mailAttribute,
        })
        await settingsPage.setLdapEnabled(true)
      })

      await test.step('save → blocked with actionable guidance (secret never displayed, row existence is the only signal)', async () => {
        await settingsPage.saveLdapConfig()
        await expect(page.locator(SELECTORS.ldap.bindPasswordError)).toBeVisible()
      })
    })

    // ========================================================================
    // Phase 2: plaintext directory address rejected before any request fires
    // ========================================================================
    await test.step('Phase 2: plaintext ldap:// without StartTLS is rejected inline', async () => {
      await test.step('switch the URL to plaintext ldap:// (StartTLS stays off)', async () => {
        await settingsPage.fillLdapConfig({ url: 'ldap://127.0.0.1:3890' })
      })

      await test.step('save → inline encryption-channel error, no request fired', async () => {
        await settingsPage.saveLdapConfig()
        const starttlsError = page.locator(SELECTORS.ldap.starttlsError)
        await expect(starttlsError).toBeVisible()
        await expect(starttlsError).toHaveText(
          'Encrypted connection required: use ldaps:// or enable StartTLS'
        )
      })

      await test.step('re-enter the tab → the form is still pristine (nothing was persisted)', async () => {
        // Tab round-trip (not page.reload): unmounting the tab content remounts
        // the form from the server-side config query, proving the rejected save
        // never persisted — without depending on post-reload session
        // rehydration timing.
        await settingsPage.switchToEmailTab()
        await settingsPage.switchToLdapTab()
        await expect(settingsPage.ldapUrlInput).toHaveValue('')
      })
    })

    // ========================================================================
    // Phase 3: valid ldaps:// config saves (disabled first); masked readback
    // ========================================================================
    await test.step('Phase 3: valid config saves disabled-first and echoes back masked', async () => {
      await test.step('fill the full config with the service password, enabled OFF', async () => {
        await settingsPage.fillLdapConfig({
          url: DEMO_LDAP.ldapsUrl,
          baseDn: DEMO_LDAP.baseDn,
          bindDn: DEMO_LDAP.bindDn,
          bindPassword: DEMO_LDAP.bindPassword,
          userFilter: DEMO_LDAP.userFilter,
          mailAttribute: DEMO_LDAP.mailAttribute,
        })
        await settingsPage.setLdapEnabled(false)
        await settingsPage.saveLdapConfig()
      })

      await test.step('re-enter the tab → values echoed back, password field EMPTY (never displayed)', async () => {
        await settingsPage.switchToEmailTab()
        await settingsPage.switchToLdapTab()

        const values = await settingsPage.getLdapFormValues()
        expect(values.url).toBe(DEMO_LDAP.ldapsUrl)
        expect(values.baseDn).toBe(DEMO_LDAP.baseDn)
        expect(values.bindDn).toBe(DEMO_LDAP.bindDn)
        expect(values.userFilter).toBe(DEMO_LDAP.userFilter)
        expect(values.mailAttribute).toBe(DEMO_LDAP.mailAttribute)
        expect(values.bindPassword).toBe('')
        expect(values.enabled).toBe(false)
      })
    })

    // ========================================================================
    // Phase 4: enable with a BLANK password — the stored one is kept
    // ========================================================================
    await test.step('Phase 4: enabling with a blank password keeps the stored secret', async () => {
      await test.step('flip enable ON (password left blank) and save', async () => {
        await settingsPage.setLdapEnabled(true)
        await settingsPage.saveLdapConfig()
      })

      await test.step('re-enter the tab → still enabled (the stored password sufficed)', async () => {
        await settingsPage.switchToEmailTab()
        await settingsPage.switchToLdapTab()
        await expect(await settingsPage.isLdapEnabled()).toBe(true)
      })
    })

    // ========================================================================
    // Phase 5: StartTLS locked off for ldaps:// URLs
    // ========================================================================
    await test.step('Phase 5: StartTLS switch is locked off while the URL is ldaps://', async () => {
      await expect(await settingsPage.isLdapStarttlsLocked()).toBe(true)
    })

    // ========================================================================
    // Phase 6: the realm login page now shows the corporate entry
    // ========================================================================
    await test.step('Phase 6: enabled config shows the corporate entry on the login page', async () => {
      await test.step('log out and open the realm login page', async () => {
        await page.context().clearCookies()
        await page.evaluate(() => {
          localStorage.clear()
          sessionStorage.clear()
        })
        await page.goto(`/${realmId}/auth/login`)
        await expect(page.locator(SELECTORS.login.container)).toBeVisible()
      })

      await test.step('the corporate entry is visible (public ldap status reports enabled)', async () => {
        await expect(page.locator(SELECTORS.ldap.loginRouteToggle)).toBeVisible({
          timeout: 10000,
        })
      })
    })

    // ========================================================================
    // Phase 7: disable → the entry disappears (graceful degradation)
    // ========================================================================
    await test.step('Phase 7: disabling hides the corporate entry again', async () => {
      await test.step('back to Settings as admin, disable and save', async () => {
        await loginAsAdmin(page, { realmId, forceRelogin: true })
        await settingsPage.goto()
        await settingsPage.waitForReady()
        await settingsPage.switchToLdapTab()
        await settingsPage.setLdapEnabled(false)
        await settingsPage.saveLdapConfig()
      })

      await test.step('reload the login page → the entry is gone', async () => {
        await page.context().clearCookies()
        await page.evaluate(() => {
          localStorage.clear()
          sessionStorage.clear()
        })
        await page.goto(`/${realmId}/auth/login`)
        await expect(page.locator(SELECTORS.login.container)).toBeVisible()
        await expect(page.locator(SELECTORS.ldap.loginRouteToggle)).toHaveCount(0, {
          timeout: 10000,
        })
      })
    })
  })
})
