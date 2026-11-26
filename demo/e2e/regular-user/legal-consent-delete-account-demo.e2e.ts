/**
 * Self-delete Account Demo Tests
 *
 * User story: US-RU-014
 *
 * Scenarios:
 * - Scenario 1: A regular user can self-delete their account from the
 *   Security page after password confirmation, and is then redirected to the
 *   login page.
 * - Scenario 3: After deletion, the original credentials can no longer
 *   log in; the UI shows a generic login error.
 * - Audit visibility: The self-delete action produces a `user.delete` audit event
 *   with `details.method = "self_service"` visible to the Realm Admin.
 *
 * Compliance rules:
 * - All operations go through the UI (no direct API calls).
 * - Shared selectors are used for all DOM assertions.
 * - Test users are cleaned up after each test while preserving the realm admin.
 */

import { test, expect, cleanupTestData } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { DeleteAccountHelper, loginAsAdminWithConsent } from '../helpers/legal-consent'
import { LoginPage } from '../pages/login-page'
import { AuditPage } from '../pages/audit-page'
import { SELECTORS } from '../selectors'
import type { Page } from '@playwright/test'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'

test.describe('[Regular User] Legal Consent Self-delete Account Demo Tests', () => {
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
   * Register a fresh regular user through the UI.
   */
  async function registerTestUser(page: Page): Promise<{ email: string; password: string }> {
    const suffix = `${testStartTime}-${registrationCounter++}`
    const email = `lc-delete-${suffix}@example.com`
    const nickname = `lc-delete-${suffix}`
    const password = 'Password123!'

    await test.step('Navigate to registration page', async () => {
      await page.goto(`/${realmId}/auth/register`, { waitUntil: 'domcontentloaded' })
      await expect(page.locator(SELECTORS.registration.card)).toBeVisible()
    })

    await test.step('Fill registration form and consent', async () => {
      await page.locator(SELECTORS.registration.emailInput).fill(email)
      await page.locator(SELECTORS.registration.nicknameInput).fill(nickname)
      await page.locator(SELECTORS.registration.passwordInput).fill(password)
      await page.locator(SELECTORS.registration.confirmPasswordInput).fill(password)
      await page.locator(SELECTORS.legalConsent.registerConsentCheckbox).check()
      await expect(page.locator(SELECTORS.legalConsent.registerConsentCheckbox)).toBeChecked()
    })

    await test.step('Submit registration', async () => {
      const responsePromise = page.waitForResponse(
        response =>
          response.url().includes(`/api/auth/${realmId}/register`) &&
          response.request().method() === 'POST',
        { timeout: 10000 }
      )
      await page.locator(SELECTORS.registration.registerButton).click()
      const response = await responsePromise
      console.log(`[SelfDeleteAccountDemo] Registration response status: ${response.status()}`)
      expect(response.ok()).toBe(true)
    })

    return { email, password }
  }

  /**
   * Clear session state so the next login starts from an anonymous context.
   */
  async function clearSession(page: Page): Promise<void> {
    await page.context().clearCookies()
    await page.evaluate(() => {
      localStorage.clear()
      sessionStorage.clear()
    })
  }

  /**
   * Log in a regular user and capture the user id from the login response.
   */
  async function loginAndCaptureUserId(
    page: Page,
    loginPage: LoginPage,
    email: string,
    password: string
  ): Promise<string> {
    await clearSession(page)
    await loginPage.goto(realmId)
    await loginPage.waitForReady()

    const responsePromise = page.waitForResponse(
      response =>
        response.url().includes('/login') && response.request().method() === 'POST',
      { timeout: 10000 }
    )

    await loginPage.fillLoginForm({ email, password })
    await loginPage.submit()

    const loginResponse = await responsePromise
    expect(loginResponse.ok()).toBe(true)

    const loginData = (await loginResponse.json()) as { userId?: string }
    const userId = loginData.userId
    expect(userId).toBeDefined()
    expect(userId).not.toBe('')

    // Post-login the main domain lands on a realm-prefixed route: regular users
    // go to /{realmId}/user/profile.
    await page.waitForURL(
      new RegExp(`^http://localhost:3000/${realmId}/(user/profile|manage/)`),
      { timeout: 15000 }
    )

    return userId!
  }

  /**
   * Register a test user, then log them in and return credentials plus user id.
   */
  async function registerAndLoginTestUser(
    page: Page,
    loginPage: LoginPage
  ): Promise<{ email: string; password: string; userId: string }> {
    const { email, password } = await registerTestUser(page)
    const userId = await loginAndCaptureUserId(page, loginPage, email, password)
    return { email, password, userId }
  }

  test.describe('User Story: Self-delete account [US-RU-014]', () => {
    test('Scenario 1: successful self-delete redirects to the login page', async ({
      page,
      loginPage,
      deleteAccountHelper,
    }) => {
      const { email, password } = await registerAndLoginTestUser(page, loginPage)
      createdEmails.push(email)

      await test.step('Navigate to Security page and delete the account', async () => {
        await deleteAccountHelper.gotoSecurityPage(realmId)
        await deleteAccountHelper.openDeleteDialog()
        await deleteAccountHelper.fillPassword(password)
        await deleteAccountHelper.submitDelete()
      })

      await test.step('Assert redirect to login page and success feedback', async () => {
        await deleteAccountHelper.expectSuccessRedirectToLogin(realmId)
      })

      await test.step('Attempt to log in with original credentials and fail', async () => {
        await clearSession(page)
        await loginPage.goto(realmId)
        await loginPage.waitForReady()

        const responsePromise = page.waitForResponse(
          response =>
            response.url().includes('/login') && response.request().method() === 'POST',
          { timeout: 10000 }
        )

        await loginPage.fillLoginForm({ email, password })
        await loginPage.submit()

        const loginResponse = await responsePromise
        expect(loginResponse.ok()).toBe(false)

        await expect(page.locator(SELECTORS.login.errorMessage)).toBeVisible({ timeout: 10000 })

        const cookies = await page.context().cookies()
        const xAuthCookie = cookies.find(cookie => cookie.name === 'X-Auth')
        expect(xAuthCookie).toBeUndefined()
      })
    })

    test('Scenario 2: wrong password is rejected and the dialog stays open', async ({
      page,
      loginPage,
      deleteAccountHelper,
    }) => {
      const { email, password } = await registerAndLoginTestUser(page, loginPage)
      createdEmails.push(email)

      await test.step('Navigate to Security page and open delete dialog', async () => {
        await deleteAccountHelper.gotoSecurityPage(realmId)
        await deleteAccountHelper.openDeleteDialog()
      })

      await test.step('Submit an incorrect password', async () => {
        await deleteAccountHelper.fillPassword('WrongPassword123!')
        await deleteAccountHelper.submitDelete()
      })

      await test.step('Assert error is shown and dialog remains open', async () => {
        await deleteAccountHelper.expectError()
        await expect(page.locator(SELECTORS.legalConsent.deleteAccountDialog)).toBeVisible()
      })
    })

    test('Scenario 3: self-delete audit event is visible to the Realm Admin', async ({
      page,
      loginPage,
      deleteAccountHelper,
    }) => {
      const { email, password, userId } = await registerAndLoginTestUser(page, loginPage)
      createdEmails.push(email)

      await test.step('Self-delete the test user', async () => {
        await deleteAccountHelper.gotoSecurityPage(realmId)
        await deleteAccountHelper.openDeleteDialog()
        await deleteAccountHelper.fillPassword(password)
        await deleteAccountHelper.submitDelete()
        await deleteAccountHelper.expectSuccessRedirectToLogin(realmId)
      })

      await test.step('Log in as Realm Admin and open the audit log', async () => {
        await loginAsAdminWithConsent(page, realmId)
        await page.goto(`${BASE_URL}/${realmId}/manage/audit`, { waitUntil: 'domcontentloaded' })
      })

      const auditPage = new AuditPage(page)

      await test.step('Filter by user.delete action', async () => {
        await auditPage.waitForReady()
        await auditPage.filterByAction('user.delete')
        await expect(page.locator(SELECTORS.audit.table)).toBeVisible()
      })

      await test.step('Narrow to the deleted user and verify the audit row', async () => {
        await auditPage.filterByActor(userId)
        await page.waitForTimeout(500)
        await expect(page.locator(SELECTORS.audit.tableLoading)).toBeHidden({ timeout: 5000 })

        const rowCount = await auditPage.getRowCount()
        expect(rowCount).toBeGreaterThan(0)

        const rowTexts = await auditPage.getRowTexts(0)
        const rowText = rowTexts.join(' ')
        expect(rowText).toContain(userId)
        // The action filter above already constrained rows to `user.delete`,
        // and the table renders the action via `formatAuditAction` (a localized
        // label, not the raw `user.delete` string), so asserting the raw action
        // text here would always fail. The delete event's identity is proven by
        // the filter + rowCount>0 + the userId match; the detail sheet below
        // verifies the self_service method marker.
      })

      await test.step('Open event detail and verify self_service method marker', async () => {
        await auditPage.openDetailSheet(0)
        const detailJson = await auditPage.getDetailJson()
        const details = JSON.parse(detailJson) as Record<string, unknown>

        expect(details.method).toContain('self_service')

        const detailText = await auditPage.getDetailFieldText('Target')
        expect(detailText).toContain(userId)
      })
    })
  })
})
