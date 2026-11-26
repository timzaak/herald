/**
 * Route Guard Re-consent Demo Tests
 *
 * User story: US-RU-012
 *
 * Scenarios:
 * - Core route guard: an already-logged-in regular user, after an admin
 *   publishes a new agreement version, navigates to a core route (/user/profile)
 *   and sees the post-login ReconsentDialog. Agreeing dismisses the dialog and
 *   allows navigation to complete.
 * - Logout from dialog: the same prompt offers a logout option. Clicking
 *   it signs the user out and redirects to the login page.
 * - Non-core route exemption: while re-consent is pending, navigating to
 *   a public legal agreement page does not show the ReconsentDialog.
 *
 * Compliance rules:
 * - All operations go through the UI (no direct API calls).
 * - Shared selectors are used for all DOM assertions.
 * - Test users are cleaned up after each test while preserving the realm admin.
 */

import { test, expect, cleanupTestData } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { AdminLegalHelper, LegalConsentHelper } from '../helpers/legal-consent'
import { LoginPage } from '../pages/login-page'
import { SELECTORS } from '../selectors'
import type { Page, Browser } from '@playwright/test'
import type { UnifiedLogger } from 'playwright-unified-logger'

test.describe('[Regular User] Route Guard Re-consent Demo Tests', () => {
  const realmId = 'realm-001'
  let testStartTime: number
  const createdEmails: string[] = []
  let registrationCounter = 0

  test.beforeEach(async ({ page, testStartTime: startTime }) => {
    testStartTime = startTime
    createdEmails.length = 0
    registrationCounter = 0

    await verifyTestEnvironment(page, {
      requiredRealms: ['realm-001'],
      requiredUsers: ['admin@realm-001.com'],
    })
  })

  test.afterEach(async ({ page }) => {
    await cleanupTestData(page, realmId, {
      keepUsers: ['admin@realm-001.com'],
      timestamp: testStartTime,
      testUserEmails: [...createdEmails],
    })
  })

  /**
   * Register a fresh regular user through the UI, then log them in while keeping
   * the same browser context open.
   */
  async function registerAndLoginUser(
    page: Page,
    loginPage: LoginPage,
    startTime: number
  ): Promise<{ email: string; password: string }> {
    const suffix = `${startTime}-${registrationCounter++}`
    const email = `lc-route-guard-${suffix}@example.com`
    const nickname = `lc-route-guard-${suffix}`
    const password = 'Password123!'

    await test.step('Register test user', async () => {
      await page.goto(`/${realmId}/auth/register`, { waitUntil: 'domcontentloaded' })
      await expect(page.locator(SELECTORS.registration.card)).toBeVisible()

      await page.locator(SELECTORS.registration.emailInput).fill(email)
      await page.locator(SELECTORS.registration.nicknameInput).fill(nickname)
      await page.locator(SELECTORS.registration.passwordInput).fill(password)
      await page.locator(SELECTORS.registration.confirmPasswordInput).fill(password)
      await page.locator(SELECTORS.legalConsent.registerConsentCheckbox).check()
      await expect(
        page.locator(SELECTORS.legalConsent.registerConsentCheckbox)
      ).toBeChecked()

      const responsePromise = page.waitForResponse(
        response =>
          response.url().includes(`/api/auth/${realmId}/register`) &&
          response.request().method() === 'POST',
        { timeout: 10000 }
      )
      await page.locator(SELECTORS.registration.registerButton).click()
      const response = await responsePromise
      console.log(`[RouteGuardReconsentDemo] Registration response status: ${response.status()}`)
      expect(response.ok()).toBe(true)
    })

    await test.step('Log in as the test user', async () => {
      await loginPage.loginAsUser(email, password, realmId)
    })

    return { email, password }
  }

  /**
   * Publish a new custom Terms of Service version from a separate admin browser
   * context so the regular user's session on the main page is preserved.
   */
  async function publishNewAgreementVersion(
    browser: Browser,
    logger: UnifiedLogger | undefined
  ): Promise<void> {
    const adminContext = await browser.newContext()
    const adminPage = await adminContext.newPage()
    const adminLegalHelper = new AdminLegalHelper(adminPage, logger)

    try {
      await adminLegalHelper.gotoLegalTab(realmId)
      await adminLegalHelper.publishCustomAgreement(
        'terms_of_service',
        'Updated Terms of Service for route guard re-consent demo (EN).',
        'route-guard-reconsent-demo'
      )
    } finally {
      await adminContext.close()
    }
  }

  test.describe('User Story: Re-consent triggered by core route guard [US-RU-012]', () => {
    test('Scenario 1: agreeing on a core route dismisses the re-consent dialog', async ({
      page,
      browser,
      loginPage,
      legalConsentHelper,
      demoLogger,
    }) => {
      const { email } = await registerAndLoginUser(page, loginPage, testStartTime)
      createdEmails.push(email)

      await test.step('Admin publishes a new custom Terms of Service version', async () => {
        await publishNewAgreementVersion(browser, demoLogger)
      })

      await test.step('Navigate to a core route and expect the re-consent dialog', async () => {
        await page.goto(`/${realmId}/user/profile`, { waitUntil: 'domcontentloaded' })
        await legalConsentHelper.expectReconsentDialogVisible()
      })

      await test.step('Click agree and continue, then reach the profile page', async () => {
        await legalConsentHelper.agreeAndContinue()
        await expect(
          page.locator(SELECTORS.legalConsent.reconsentDialogTitle)
        ).toBeHidden({ timeout: 10000 })
        await expect(page).toHaveURL(new RegExp(`/user/profile/?$`))
        await expect(page.locator('[data-testid="email-display"]')).toContainText(email, {
          timeout: 10000,
        })
      })
    })

    test('Scenario 2: clicking logout in the dialog signs out and redirects to login', async ({
      page,
      browser,
      loginPage,
      legalConsentHelper,
      demoLogger,
    }) => {
      const { email } = await registerAndLoginUser(page, loginPage, testStartTime)
      createdEmails.push(email)

      await test.step('Admin publishes a new custom Terms of Service version', async () => {
        await publishNewAgreementVersion(browser, demoLogger)
      })

      await test.step('Navigate to a core route and expect the re-consent dialog', async () => {
        await page.goto(`/${realmId}/user/profile`, { waitUntil: 'domcontentloaded' })
        await legalConsentHelper.expectReconsentDialogVisible()
      })

      await test.step('Click logout and assert redirect to login without session', async () => {
        await Promise.all([
          page.waitForURL(`**/${realmId}/auth/login`, { timeout: 15000 }),
          legalConsentHelper.logoutFromDialog(),
        ])

        await expect(page.locator(SELECTORS.login.title)).toBeVisible({
          timeout: 10000,
        })
        await expect(
          page.locator(SELECTORS.legalConsent.reconsentDialogTitle)
        ).not.toBeVisible()

        const cookies = await page.context().cookies()
        const xAuthCookie = cookies.find(cookie => cookie.name === 'X-Auth')
        expect(xAuthCookie).toBeUndefined()
      })
    })

    test('Scenario 3: public legal route is not interrupted while re-consent is pending', async ({
      page,
      browser,
      loginPage,
      legalConsentHelper,
      demoLogger,
    }) => {
      const { email } = await registerAndLoginUser(page, loginPage, testStartTime)
      createdEmails.push(email)

      await test.step('Admin publishes a new custom Terms of Service version', async () => {
        await publishNewAgreementVersion(browser, demoLogger)
      })

      await test.step('Navigate to the public agreement page and verify no dialog', async () => {
        await legalConsentHelper.gotoAgreementPage(realmId, 'terms_of_service')
        await legalConsentHelper.expectAgreementPage('terms_of_service')
        await expect(
          page.locator(SELECTORS.legalConsent.reconsentDialogTitle)
        ).not.toBeVisible()
      })
    })
  })
})
