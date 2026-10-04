/**
 * Realm Admin Email Configuration Demo Test
 *
 * User Stories:
 * - US-RA-013: Configure Realm email service (Resend, SMTP, switch provider, persistence)
 * - US-RA-014: Test email UI flow (no actual delivery verification)
 * - US-RA-015: Feature switch gating (requireEmailVerification enabled when email configured)
 *
 * Test Structure:
 * - One test.describe per user story group
 * - One test() inside each describe
 * - Multiple test.step() phases inside each test
 * - afterEach with cleanupTestData
 *
 * Known Limitations:
 * - Test assumes clean initial state (no email config). If the test environment
 *   does not reset realm_config between runs, Phase 1 may fail. This is acceptable
 *   for demo tests running against a seed-reset environment.
 * - US-RA-014 test email flow accepts either success or error outcome (demo env
 *   may not have real SMTP).
 */

import { test, expect, cleanupTestData } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { loginAsAdmin } from '../helpers/auth'
import { SettingsPage } from '../pages/settings-page'

test.describe('[Realm Admin] Email Configuration Demo', () => {
  let testStartTime: number
  let settingsPage: SettingsPage
  const realmId = 'admin'

  test.afterEach(async ({ page }) => {
    await cleanupTestData(page, realmId, {
      keepUsers: ['admin@cas.com'],
      timestamp: testStartTime,
    })
  })

  // ==========================================================================
  // US-RA-013 + US-RA-014: Email Configuration and Test Email Flow
  // ==========================================================================

  test.describe('US-RA-013 + US-RA-014: Configure Realm Email and Test Email', () => {
    test('Email configuration lifecycle: Resend, SMTP, test email, persistence', async ({ page, demoLogger }) => {
      testStartTime = Date.now()
      settingsPage = new SettingsPage(page, demoLogger, realmId)

      // Environment verification and admin login
      await verifyTestEnvironment(page, {
        requiredRealms: [realmId],
        requiredUsers: ['admin@cas.com'],
        skipRealmVerification: true,
        skipDatabaseCheck: false,
        skipRedisCheck: false,
      })

      await loginAsAdmin(page, { realmId })

      // =====================================================================
      // Phase 1: Verify initial email status and form visibility
      // =====================================================================

      await test.step('Phase 1: Navigate to Settings Email tab and verify form', async () => {
        await test.step('Navigate to Settings and switch to Email tab', async () => {
          await settingsPage.goto()
          await settingsPage.waitForReady()
          await settingsPage.switchToEmailTab()
        })

        await test.step('Verify email form is visible (save button present)', async () => {
          await expect(settingsPage.emailSaveButton).toBeVisible()
          demoLogger.testCode.log('Email configuration form is visible')
        })

        await test.step('Verify email status badge is visible', async () => {
          await expect(settingsPage.emailStatusBadge).toBeVisible()
          const statusText = await settingsPage.getEmailStatusBadgeText()
          demoLogger.testCode.log(`Initial email status: ${statusText}`)
        })
      })

      // =====================================================================
      // Phase 2: Configure Resend provider — US-RA-013 scenario 1
      // =====================================================================

      await test.step('Phase 2: Configure Resend provider (US-RA-013 scenario 1)', async () => {
        await settingsPage.configureResend({
          provider: 'resend',
          fromAddress: 'demo-test@example.com',
          resendApiKey: 're_demo_test_key',
        })

        await settingsPage.saveEmailConfig()

        await test.step('Verify status badge shows configured state', async () => {
          const isConfigured = await settingsPage.isEmailConfigured()
          expect(isConfigured).toBeTruthy()
          demoLogger.testCode.log('Resend provider configured successfully')
        })
      })

      // =====================================================================
      // Phase 3: Switch provider to SMTP — US-RA-013 scenario 2 + 4
      // =====================================================================

      await test.step('Phase 3: Switch to SMTP provider (US-RA-013 scenario 2)', async () => {
        await settingsPage.configureSmtp({
          provider: 'smtp',
          fromAddress: 'noreply@example.com',
          smtpHost: 'smtp.demo-test.example.com',
          smtpPort: '587',
          smtpEncryption: 'starttls',
          smtpUsername: 'demo-user@example.com',
          smtpPassword: 'demo_password',
        })

        await settingsPage.saveEmailConfig()

        await test.step('Verify status badge still shows configured state', async () => {
          const isConfigured = await settingsPage.isEmailConfigured()
          expect(isConfigured).toBeTruthy()
          demoLogger.testCode.log('SMTP provider configured successfully (provider switched)')
        })
      })

      // =====================================================================
      // Phase 4: Test email UI flow — US-RA-014 (UI-only)
      // =====================================================================

      await test.step('Phase 4: Test email UI flow (US-RA-014)', async () => {
        await settingsPage.sendTestEmail('demo-test-recipient@example.com')

        await test.step('Wait for test email result (success or error)', async () => {
          // Accept either outcome — demo env may not have real SMTP/Resend
          const successLocator = settingsPage.emailTestSuccess
          const errorLocator = settingsPage.emailTestError

          // Wait for either success or error to appear
          await expect(async () => {
            const successVisible = await successLocator.isVisible().catch(() => false)
            const errorVisible = await errorLocator.isVisible().catch(() => false)
            expect(successVisible || errorVisible).toBeTruthy()
          }).toPass({ timeout: 15000 })

          const successVisible = await successLocator.isVisible().catch(() => false)
          if (successVisible) {
            demoLogger.testCode.log('Test email sent successfully (demo env has email delivery)')
          } else {
            demoLogger.testCode.log('Test email send returned error (expected in demo env without real SMTP)')
          }
        })
      })

      // =====================================================================
      // Phase 5: Verify configuration persistence — US-RA-013 scenario 4
      // =====================================================================

      await test.step('Phase 5: Verify configuration persistence (US-RA-013 scenario 4)', async () => {
        await page.reload()
        await settingsPage.waitForReady()
        await settingsPage.switchToEmailTab()

        await test.step('Verify SMTP fields still populated after reload', async () => {
          // Verify the from address was persisted
          const fromAddressValue = await settingsPage.emailFromAddressInput.inputValue()
          expect(fromAddressValue).toBe('noreply@example.com')

          // Verify SMTP host was persisted
          const smtpHostValue = await settingsPage.emailSmtpHostInput.inputValue()
          expect(smtpHostValue).toBe('smtp.demo-test.example.com')

          // Verify SMTP port was persisted
          const smtpPortValue = await settingsPage.emailSmtpPortInput.inputValue()
          expect(smtpPortValue).toBe('587')

          // Verify SMTP username was persisted
          const smtpUsernameValue = await settingsPage.emailSmtpUsernameInput.inputValue()
          expect(smtpUsernameValue).toBe('demo-user@example.com')

          demoLogger.testCode.log('Email configuration persisted after page reload')
        })

        await test.step('Verify status badge shows configured after reload', async () => {
          const isConfigured = await settingsPage.isEmailConfigured()
          expect(isConfigured).toBeTruthy()
          demoLogger.testCode.log('Email configured status persists after reload')
        })
      })
    })
  })

  // ==========================================================================
  // US-RA-015: Email-dependent feature switch gating
  // ==========================================================================

  test.describe('US-RA-015: Feature Switch Gating', () => {
    test('requireEmailVerification switch is enabled when email is configured', async ({ page, demoLogger }) => {
      testStartTime = Date.now()
      settingsPage = new SettingsPage(page, demoLogger, realmId)

      await verifyTestEnvironment(page, {
        requiredRealms: [realmId],
        requiredUsers: ['admin@cas.com'],
        skipRealmVerification: true,
        skipDatabaseCheck: false,
        skipRedisCheck: false,
      })

      await loginAsAdmin(page, { realmId })

      // =====================================================================
      // Phase 1: Verify requireEmailVerification enabled when email configured
      // =====================================================================

      await test.step('Phase 1: Verify email verification switch is enabled (email is configured from previous test)', async () => {
        await test.step('Navigate to Settings Registration tab', async () => {
          await settingsPage.goto()
          await settingsPage.waitForReady()
          await settingsPage.switchToRegistrationTab()
        })

        await test.step('Verify requireEmailVerification switch is not disabled', async () => {
          const isDisabled = await settingsPage.requireEmailVerificationSwitch.isDisabled()
          expect(isDisabled).toBeFalsy()
          demoLogger.testCode.log('Email verification switch is enabled because email is configured')
        })
      })
    })
  })
})
