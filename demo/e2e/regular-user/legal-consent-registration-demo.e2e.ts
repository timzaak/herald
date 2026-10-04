/**
 * Legal Consent Registration Demo Tests
 *
 * User Story: docs/user-stories/core/legal-consent-account-deletion.md [US-RU-011]
 *
 * Scenarios:
 * - US-RU-011 Scenario 1: Registration succeeds with the consent checkbox checked
 *   and the Terms of Service / Privacy Policy links are clickable.
 * - US-RU-011 Scenario 2: Registration is blocked when the consent checkbox is
 *   not checked.
 *
 * - All operations go through the UI (no direct API calls).
 * - Environment configuration is managed by Demo Seed.
 * - Tests focus on user interaction verification.
 */

import { test, expect, cleanupTestData } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { LegalConsentHelper } from '../helpers/legal-consent'
import { SELECTORS } from '../selectors'

test.describe('[Regular User] Legal Consent Registration Demo Tests', () => {
  const realmId = 'realm-001'
  let testStartTime: number
  let createdEmail = ''

  test.beforeEach(async ({ page, testStartTime: startTime }) => {
    testStartTime = startTime
    createdEmail = ''

    await verifyTestEnvironment(page, {
      requiredRealms: ['realm-001'],
      requiredUsers: ['admin@realm-001.com'],
    })
  })

  test.afterEach(async ({ page }) => {
    await cleanupTestData(page, realmId, {
      keepUsers: ['admin@realm-001.com'],
      timestamp: testStartTime,
      testUserEmails: [createdEmail].filter(Boolean),
    })
  })

  async function navigateToRegistrationPage(page: any): Promise<void> {
    await page.goto(`/${realmId}/auth/register`)
    await page.waitForLoadState('domcontentloaded')
  }

  test.describe('User Story: Register with legal consent [US-RU-011]', () => {
    test('Scenario 1: Registration succeeds with consent checked and agreement links clickable', async ({ page }) => {
      const legalConsentHelper = new LegalConsentHelper(page)
      const email = `lc-reg-${testStartTime}@example.com`
      const nickname = `lc-reg-${testStartTime}`

      await test.step('Step 1: Navigate to registration page', async () => {
        await navigateToRegistrationPage(page)
      })

      await test.step('Step 2: Verify registration page and consent elements are visible', async () => {
        await expect(page.locator(SELECTORS.registration.card)).toBeVisible()
        await expect(page.locator(SELECTORS.legalConsent.registerConsentCheckbox)).toBeVisible()
        await expect(page.locator(SELECTORS.legalConsent.termsOfServiceLink)).toBeVisible()
        await expect(page.locator(SELECTORS.legalConsent.privacyPolicyLink)).toBeVisible()
      })

      await test.step('Step 3: Click Terms of Service link and verify agreement page', async () => {
        await page.locator(SELECTORS.legalConsent.termsOfServiceLink).click()
        await legalConsentHelper.expectAgreementPage('terms_of_service')
        // The agreement page no longer renders a separate title element; verify
        // we landed on the terms_of_service legal route (the success card is
        // already asserted by expectAgreementPage).
        await expect(page).toHaveURL(/\/legal\/terms_of_service/)

        await page.goBack()
        await page.waitForLoadState('domcontentloaded')
        await expect(page.locator(SELECTORS.registration.card)).toBeVisible()
      })

      await test.step('Step 4: Fill registration form', async () => {
        await page.locator(SELECTORS.registration.emailInput).fill(email)
        await page.locator(SELECTORS.registration.nicknameInput).fill(nickname)
        await page.locator(SELECTORS.registration.passwordInput).fill('Password123!')
        await page.locator(SELECTORS.registration.confirmPasswordInput).fill('Password123!')
      })

      await test.step('Step 5: Check consent checkbox', async () => {
        await page.locator(SELECTORS.legalConsent.registerConsentCheckbox).check()
        await expect(page.locator(SELECTORS.legalConsent.registerConsentCheckbox)).toBeChecked()
      })

      await test.step('Step 6: Submit registration and verify redirect', async () => {
        const turnstileContainer = page.locator(SELECTORS.registration.turnstileContainer)
        const isTurnstileVisible = await turnstileContainer.isVisible().catch(() => false)

        if (isTurnstileVisible) {
          console.log('[LegalConsentRegistration] Turnstile widget is visible; skipping submit assertion')
          return
        }

        const responsePromise = page.waitForResponse(`**/api/auth/${realmId}/register`, { timeout: 10000 })
        await page.locator(SELECTORS.registration.registerButton).click()
        const response = await responsePromise
        console.log(`[LegalConsentRegistration] API response status: ${response.status()}`)

        await page.waitForURL(`**/auth/login`, { timeout: 3000 }).catch(() => {
          // TanStack Router may navigate directly to the dashboard if auto-login
          // is enabled; both destinations are treated as success.
        })

        const currentUrl = page.url()
        const isOnLogin = currentUrl.includes('/auth/login')
        const isOnDashboard = currentUrl.includes('/dashboard')

        console.log(`[LegalConsentRegistration] Current URL after submit: ${currentUrl}`)

        if (!isOnLogin && !isOnDashboard) {
          throw new Error(`Registration succeeded but unexpected URL: ${currentUrl}`)
        }

        createdEmail = email
      })
    })

    test('Scenario 2: Registration is blocked without consent checkbox checked', async ({ page }) => {
      const email = `lc-reg-no-consent-${testStartTime}@example.com`
      const nickname = `lc-reg-no-consent-${testStartTime}`

      await test.step('Step 1: Navigate to registration page', async () => {
        await navigateToRegistrationPage(page)
      })

      await test.step('Step 2: Fill registration form without checking consent', async () => {
        await expect(page.locator(SELECTORS.registration.card)).toBeVisible()
        await page.locator(SELECTORS.registration.emailInput).fill(email)
        await page.locator(SELECTORS.registration.nicknameInput).fill(nickname)
        await page.locator(SELECTORS.registration.passwordInput).fill('Password123!')
        await page.locator(SELECTORS.registration.confirmPasswordInput).fill('Password123!')
      })

      await test.step('Step 3: Submit registration and verify consent validation blocks submission', async () => {
        await page.locator(SELECTORS.registration.registerButton).click()

        const submitButton = page.locator(SELECTORS.registration.registerButton)
        const isButtonDisabled = await submitButton.isDisabled().catch(() => false)

        const consentError = page.locator(SELECTORS.legalConsent.registerConsentError)
        const isConsentErrorVisible = await consentError.isVisible().catch(() => false)

        expect(isButtonDisabled || isConsentErrorVisible).toBe(true)

        const currentUrl = page.url()
        expect(currentUrl).toContain('/register')
      })
    })
  })
})
