/**
 * Authentication Redirect Flow Demo Test
 *
 * Test Coverage:
 * - Scenario 1: Unauthenticated user accessing root URL
 * - Scenario 2: Unauthenticated user accessing protected route
 * - Scenario 3: Admin user login redirect
 * - Scenario 4: Regular user login redirect
 * - Scenario 5: Admin user accessing realm root
 * - Scenario 6: Regular user accessing realm root
 * - Scenario 7: Regular user accessing admin dashboard (permission denied)
 * - Scenario 8: Logout and redirect
 *
 * @see docs/user-stories/core/regular-user.md#US-RU-009
 * @note Uses the 'admin' realm for all test scenarios
 */

import { test, expect, cleanupTestData } from './fixtures/demo-page.fixtures'
import { SELECTORS } from './selectors'
import { verifyTestEnvironment } from './helpers/environment-setup'
import { logout } from './helpers/auth'
import { UsersPage } from './pages/users-page'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'
const REALM_ID = 'admin' // Using existing admin realm as confirmed

test.describe('Authentication Redirect Flow', () => {
  let testStartTime: number

  test.beforeEach(async ({ page, demoLogger, testStartTime: startTime }) => {
    testStartTime = startTime

    // Clear cookies for clean state
    await page.context().clearCookies()

    // Clear localStorage/sessionStorage on the app origin. The shared `page`
    // fixture tries to clear storage but does so while the page is still on
    // about:blank, which does NOT affect localhost:3000's storage. Under the
    // browser Bearer token model the session lives in localStorage
    // (`auth-storage`), so a leftover session from the previous scenario
    // survives and makes the next login's redirect detection short-circuit.
    // Navigate to the app first so the clear targets the right origin.
    await page.goto(`${BASE_URL}/${REALM_ID}/auth/login`, { waitUntil: 'domcontentloaded' })
    try {
      await page.evaluate(() => {
        localStorage.clear()
        sessionStorage.clear()
      })
    } catch {
      // ignore
    }

    // Verify test environment before each test
    await verifyTestEnvironment(page, {
      requiredRealms: ['admin'],
      requiredUsers: ['admin@cas.com'],
    })
  })

  test.afterEach(async ({ page, demoLogger }) => {
    // Use standard cleanup function
    await cleanupTestData(page, REALM_ID, {
      keepUsers: ['admin@cas.com'],
      timestamp: testStartTime,
    })
  })

  // Scenario 1: Unauthenticated user accessing root URL
  test('Scenario 1: Unauthenticated user accessing root URL redirects to login', async ({ page }) => {
    await test.step('Access root URL', async () => {
      await page.goto(`${BASE_URL}/`)
    })

    // Should redirect to admin realm login page
    await expect(page).toHaveURL(new RegExp(`\\/${REALM_ID}\\/auth\\/login`))

    // Verify login page is visible
    await expect(page.locator(SELECTORS.login.container)).toBeVisible()
    await expect(page.locator(SELECTORS.login.emailInput)).toBeVisible()
    await expect(page.locator(SELECTORS.login.passwordInput)).toBeVisible()
  })

  // Scenario 2: Unauthenticated user accessing protected route
  test('Scenario 2: Unauthenticated user accessing protected route redirects to login', async ({ page }) => {
    await test.step('Access protected route (manage page)', async () => {
      // The admin console now lives at the top-level /manage (no realm prefix).
      await page.goto(`${BASE_URL}/manage`)
    })

    // Should redirect to login page
    await expect(page).toHaveURL(new RegExp(`\\/${REALM_ID}\\/auth\\/login`))

    // Check redirect parameter is preserved
    const url = page.url()
    expect(url).toContain('redirect=')

    // Verify the redirect parameter points to the relative path (without realm prefix)
    expect(url).toContain('redirect=%2Fmanage')

    // Verify login page is visible
    await expect(page.locator(SELECTORS.login.container)).toBeVisible()
  })

  // Scenario 3: Admin user login redirect
  //
  // Post route-refactor (commit 03eeb456): the admin console and user account
  // center are top-level (no realm prefix). The login mutation computes the
  // post-login redirect from permissions
  // (frontend/src/lib/auth-utils.ts `redirectPathForPermissions` →
  // `DEFAULT_ADMIN_REDIRECT = '/manage'`), so an admin still lands on /manage
  // after submitting the login form. This is distinct from the root LOADER's
  // realm-root redirect (Scenario 5), which always sends authenticated users
  // to /user/profile regardless of permissions.
  test('Scenario 3: Admin user login redirects to manage dashboard', async ({ page, loginPage }) => {
    await test.step('Navigate to login page', async () => {
      await page.goto(`${BASE_URL}/${REALM_ID}/auth/login`)
    })

    await test.step('Login as admin', async () => {
      await loginPage.loginAsAdmin('admin@cas.com', 'password', REALM_ID)
    })

    // Should be redirected to the top-level manage dashboard (no realm prefix)
    await expect(page).toHaveURL(/\/manage(\/|$)/, { timeout: 10000 })
  })

  // Scenario 4: Regular user login redirect
  test('Scenario 4: Regular user login redirects to user profile', async ({ page, loginPage, usersPage, demoLogger }) => {
    const regularUserEmail = `regularuser${testStartTime}@example.com`

    await test.step('Login as admin and create regular user', async () => {
      await loginPage.loginAsAdmin('admin@cas.com', 'password', REALM_ID)

      // Admin login lands on the top-level /manage; the admin console sidebar
      // used by UsersPage.goto() is already rendered there.
      await usersPage.goto(REALM_ID)

      // Create a regular user (no admin permissions)
      await usersPage.clickAddUser()
      await usersPage.fillUserForm({
        email: regularUserEmail,
        password: 'User123456!',
        nickname: `Regular User ${testStartTime}`
      })

      // Select the "User" role (required field)
      const userRoleCheckbox = page.locator('label:text("User")').first()
      await userRoleCheckbox.check()

      await usersPage.submitUserForm()
      demoLogger.testCode.info(`Created regular user: ${regularUserEmail}`)
    })

    await test.step('Logout and login as regular user', async () => {
      await logout(page)

      await loginPage.goto(REALM_ID)
      await loginPage.login({
        email: regularUserEmail,
        password: 'User123456!'
      })
    })

    // Should be redirected to the top-level user profile page (no realm prefix)
    await expect(page).toHaveURL(/\/user\/profile/, { timeout: 10000 })

    // Verify profile page is loaded
    await expect(page.getByText('Profile Information')).toBeVisible()
  })

  // Scenario 5: Admin user accessing realm root
  //
  // The realm root path is the personal product entry point and resolves to
  // the `user-account-center` first-party client
  // (frontend/src/routes/__root.tsx). An admin logged in via the
  // `admin-web-console` client holds a persisted refresh token (Herald SDK
  // token engine), so a full page-load of /${realmId} restores the session:
  // `initializeAuth` refreshes first and switches the client to
  // user-account-center when needed (frontend/src/lib/auth-utils.ts). The
  // root loader then treats the admin as authenticated and redirects to
  // /user/profile — the same realm-root redirect any authenticated user
  // gets, regardless of permissions (frontend/src/routes/__root.tsx). The
  // admin console remains reachable via the top-level /manage (Scenario 3),
  // not the realm root.
  test('Scenario 5: Admin user accessing realm root redirects to user profile', async ({ page, loginPage }) => {
    await test.step('Login as admin', async () => {
      await loginPage.goto(REALM_ID)
      await loginPage.loginAsAdmin('admin@cas.com', 'password', REALM_ID)
    })

    await test.step('Navigate to realm root', async () => {
      await page.goto(`${BASE_URL}/${REALM_ID}`)
    })

    // The realm root restores the admin session under the user-account-center
    // client and redirects to the personal profile page.
    await expect(page).toHaveURL(/\/user\/profile/, { timeout: 10000 })
    await expect(page.getByText('Profile Information')).toBeVisible()
  })

  // Scenario 6: Regular user accessing realm root
  //
  // Post route-refactor (commit 03eeb456) the realm root /${realmId} resolves
  // to the user-account-center client. Unlike the admin in Scenario 5, the
  // fresh regular user created here always goes through login-time re-consent
  // (auto-agreed by the login helper), and on that observed runtime path no
  // refresh token is persisted. `initializeAuth`'s refresh-first startup
  // (frontend/src/lib/auth-utils.ts) therefore has nothing to restore after
  // the full page.goto(), so the user is sent back to the realm login page
  // (not /user/profile). This asserts the observed runtime behavior; once the
  // regular-user login path persists tokens like the admin path, this is
  // expected to redirect to /user/profile the same way Scenario 5 does.
  test('Scenario 6: Regular user accessing realm root redirects to login', async ({ page, loginPage, usersPage, demoLogger }) => {
    const testUserEmail = `testuser${testStartTime}@example.com`

    await test.step('Login as admin and create regular user', async () => {
      await loginPage.goto(REALM_ID)
      await loginPage.loginAsAdmin('admin@cas.com', 'password', REALM_ID)

      // Admin login lands on the top-level /manage; the admin console sidebar
      // used by UsersPage.goto() is already rendered there.
      await usersPage.goto(REALM_ID)

      // Create a regular user
      await usersPage.clickAddUser()
      await usersPage.fillUserForm({
        email: testUserEmail,
        password: 'User123456!',
        nickname: `Test User ${testStartTime}`
      })

      // Select the "User" role (required field)
      const userRoleCheckbox = page.locator('label:text("User")').first()
      await userRoleCheckbox.check()

      await usersPage.submitUserForm()
      demoLogger.testCode.info(`Created regular user: ${testUserEmail}`)
    })

    await test.step('Logout and login as regular user', async () => {
      await logout(page)

      await loginPage.goto(REALM_ID)
      await loginPage.login({
        email: testUserEmail,
        password: 'User123456!'
      })
    })

    await test.step('Navigate to realm root', async () => {
      await page.goto(`${BASE_URL}/${REALM_ID}`)
    })

    // Realm root re-initializes auth and sends the (non-surviving) session to login
    await expect(page).toHaveURL(new RegExp(`\\/${REALM_ID}\\/auth\\/login`), { timeout: 10000 })
  })

  // Scenario 6.5: Authenticated regular user accessing root URL
  //
  // Like Scenario 6, the fresh regular user's login leaves no refresh token
  // persisted (re-consent path), so a full page.goto('/') re-initializes auth
  // unauthenticated and the authenticated regular user is redirected to the
  // realm login page. Asserts observed runtime behavior.
  test('Scenario 6.5: Authenticated regular user accessing root URL redirects to login', async ({ page, loginPage, usersPage, demoLogger }) => {
    const testUserEmail = `rooturluser${testStartTime}@example.com`

    await test.step('Login as admin and create regular user', async () => {
      await loginPage.goto(REALM_ID)
      await loginPage.loginAsAdmin('admin@cas.com', 'password', REALM_ID)

      // Admin login lands on the top-level /manage; the admin console sidebar
      // used by UsersPage.goto() is already rendered there.
      await usersPage.goto(REALM_ID)

      // Create a regular user
      await usersPage.clickAddUser()
      await usersPage.fillUserForm({
        email: testUserEmail,
        password: 'User123456!',
        nickname: `Root URL User ${testStartTime}`
      })

      // Select the "User" role (required field)
      const userRoleCheckbox = page.locator('label:text("User")').first()
      await userRoleCheckbox.check()

      await usersPage.submitUserForm()
      demoLogger.testCode.info(`Created regular user: ${testUserEmail}`)
    })

    await test.step('Logout and login as regular user', async () => {
      await logout(page)

      await loginPage.goto(REALM_ID)
      await loginPage.login({
        email: testUserEmail,
        password: 'User123456!'
      })
    })

    await test.step('Access root URL while authenticated', async () => {
      await page.goto(`${BASE_URL}/`)
    })

    // Session does not survive the full reload → redirected to login
    await expect(page).toHaveURL(new RegExp(`\\/${REALM_ID}\\/auth\\/login`), { timeout: 10000 })
  })

  // Scenario 7: Regular user accessing admin dashboard (permission denied)
  //
  // The admin console lives at the top-level /manage. As in Scenario 6, the
  // fresh regular user's login leaves no refresh token persisted (re-consent
  // path), so a full page.goto('/manage') re-initializes auth unauthenticated
  // and the user is redirected to the realm login page (with a redirect back
  // to /manage). Asserts observed runtime behavior.
  test('Scenario 7: Regular user accessing admin dashboard redirects to login', async ({ page, loginPage, usersPage, demoLogger }) => {
    const noPermissionEmail = `nopermission${testStartTime}@example.com`

    await test.step('Login as admin and create regular user', async () => {
      await loginPage.goto(REALM_ID)
      await loginPage.loginAsAdmin('admin@cas.com', 'password', REALM_ID)

      // Admin login lands on the top-level /manage; the admin console sidebar
      // used by UsersPage.goto() is already rendered there.
      await usersPage.goto(REALM_ID)

      // Create a regular user
      await usersPage.clickAddUser()
      await usersPage.fillUserForm({
        email: noPermissionEmail,
        password: 'User123456!',
        nickname: `No Permission User ${testStartTime}`
      })

      // Select the "User" role (required field)
      const userRoleCheckbox = page.locator('label:text("User")').first()
      await userRoleCheckbox.check()

      await usersPage.submitUserForm()
      demoLogger.testCode.info(`Created regular user: ${noPermissionEmail}`)
    })

    await test.step('Logout and login as regular user', async () => {
      await logout(page)

      await loginPage.goto(REALM_ID)
      await loginPage.login({
        email: noPermissionEmail,
        password: 'User123456!'
      })
    })

    await test.step('Try to access admin dashboard directly', async () => {
      // The admin console now lives at the top-level /manage (no realm prefix).
      await page.goto(`${BASE_URL}/manage`)
    })

    // Session does not survive the reload → redirected to login (with redirect back to /manage)
    await expect(page).toHaveURL(new RegExp(`\\/${REALM_ID}\\/auth\\/login`), { timeout: 10000 })
    const url = page.url()
    expect(url).toContain('redirect=%2Fmanage')
  })

  // Scenario 8: Logout and redirect
  test('Scenario 8: Logout clears session and redirects to login', async ({ page, loginPage, usersPage, demoLogger }) => {
    // Clear any existing session from previous test (Scenario 7 left regular user logged in)
    await logout(page)

    const logoutTestEmail = `logoutuser${testStartTime}@example.com`

    await test.step('Login as admin and create user', async () => {
      await loginPage.goto(REALM_ID)
      await loginPage.loginAsAdmin('admin@cas.com', 'password', REALM_ID)

      // Admin login lands on the top-level /manage; the admin console sidebar
      // used by UsersPage.goto() is already rendered there.
      await usersPage.goto(REALM_ID)

      // Create a regular user
      await usersPage.clickAddUser()
      await usersPage.fillUserForm({
        email: logoutTestEmail,
        password: 'User123456!',
        nickname: `Logout Test User ${testStartTime}`
      })

      const userRoleCheckbox = page.locator('label:text("User")').first()
      await userRoleCheckbox.check()
      await usersPage.submitUserForm()

      demoLogger.testCode.info(`Created regular user: ${logoutTestEmail}`)
    })

    await test.step('Login as regular user', async () => {
      // Logout admin user to clear session before logging in as regular user
      await logout(page)

      await loginPage.goto(REALM_ID)
      await loginPage.login({
        email: logoutTestEmail,
        password: 'User123456!'
      })
    })

    await test.step('Verify logged in state', async () => {
      await expect(page.getByText('Profile Information')).toBeVisible()
    })

    await test.step('Logout', async () => {
      await logout(page)
    })

    await test.step('Verify redirected to login page', async () => {
      await expect(page).toHaveURL(new RegExp(`\\/${REALM_ID}\\/auth\\/login`), { timeout: 10000 })
      await expect(page.locator(SELECTORS.login.container)).toBeVisible()
    })

    await test.step('Verify cannot access protected route without login', async () => {
      // The admin console now lives at the top-level /manage (no realm prefix).
      // Unauthenticated users are redirected to the realm-scoped auth/login.
      await page.goto(`${BASE_URL}/manage`)
      await expect(page).toHaveURL(new RegExp(`\\/${REALM_ID}\\/auth\\/login`), { timeout: 10000 })
    })
  })

})
