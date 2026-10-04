/**
 * Demo Test Fixtures (Basic Edition)
 *
 * Purpose: Reduce beforeEach duplication across demo tests by providing
 * reusable fixtures for common setup operations.
 *
 * Note: For advanced usage with Page Objects, see demo-page.fixtures.ts
 *
 * @see https://playwright.dev/docs/test-fixtures
 * @see demo-page.fixtures.ts - Page Object fixtures (recommended)
 */

import { test as base, type Page } from '@playwright/test'
import { UnifiedLogger } from '../helpers/unified-logger'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { loginAsAdmin } from '../helpers/auth'
import { LoginPage } from '../pages/login-page'

/**
 * Demo test fixtures
 *
 * Extends Playwright's base test with demo-specific fixtures:
 * - demoLogger: Pre-configured UnifiedLogger
 * - authenticatedPage: Page with admin login completed
 * - testStartTime: Timestamp for test data cleanup
 */
export const test = base.extend<{
  demoLogger: UnifiedLogger
  authenticatedPage: Page
  testStartTime: number
}>({
  /**
   * Fixture: Demo Logger
   *
   * Creates a UnifiedLogger instance with test title.
   * Automatically finalized after test.
   *
   * Usage:
   * ```typescript
   * test('my test', async ({ demoLogger, page }) => {
   *   console.log('Test started') // Logs captured by UnifiedLogger
   * })
   * ```
   */
  demoLogger: async ({ page }, use, testInfo) => {
    const logger = new UnifiedLogger(page, testInfo.title)
    await use(logger)
    // Auto-finalize after test
    logger.printSummary('[Demo] Test Summary')
    await logger.finalize()
  },

  /**
   * Fixture: Test Start Time
   *
   * Records test start time for cleanup operations.
   *
   * Usage:
   * ```typescript
   * test.afterEach(async ({ page, testStartTime }) => {
   *   await cleanupDemoTestData(page, 'admin', {
   *     timestamp: testStartTime,
   *   })
   * })
   * ```
   */
  testStartTime: async ({}, use) => {
    const startTime = Date.now()
    await use(startTime)
  },

  /**
   * Fixture: Authenticated Page
   *
   * Performs environment verification and admin login.
   * Returns a page ready for admin operations.
   *
   * Usage:
   * ```typescript
   * test('admin operation', async ({ authenticatedPage }) => {
   *   // Page is already logged in as admin
   *   await authenticatedPage.goto('/admin/users')
   * })
   * ```
   *
   * Configuration:
   * - Realm: 'admin' (default)
   * - User: 'admin@cas.com' (default)
   * - Validation Level: 'basic' (fast)
   *
   * @note This fixture adds ~5-10 seconds to test setup time
   */
  authenticatedPage: async ({ page, demoLogger, testStartTime }, use) => {
    // Verify test environment (fast mode)
    await verifyTestEnvironment(page, {
      requiredRealms: ['admin'],
      requiredUsers: ['admin@cas.com'],
    })

    // Login as admin
    await loginAsAdmin(page, {
      realmId: 'admin',
    })

    // Use authenticated page in test
    await use(page)
  },
})

/**
 * Demo test with realm-specific authentication
 *
 * Extends demo fixtures with realm-specific login.
 */
export const realmTest = base.extend<{
  demoLogger: UnifiedLogger
  authenticatedRealmPage: Page
  realmId: string
  testStartTime: number
}>({
  demoLogger: async ({ page }, use, testInfo) => {
    const logger = new UnifiedLogger(page, testInfo.title)
    await use(logger)
    logger.printSummary('[Realm Demo] Test Summary')
    await logger.finalize()
  },

  testStartTime: async ({}, use) => {
    const startTime = Date.now()
    await use(startTime)
  },

  /**
   * Realm ID fixture
   *
   * Override in tests to specify target realm.
   *
   * Usage:
   * ```typescript
   * realmTest('realm admin test', async ({ authenticatedRealmPage }) => {
   *   // Logged into realmId realm
   * }, { realmId: 'my-realm' })
   * ```
   */
  realmId: async ({}, use) => {
    await use('admin') // Default realm
  },

  /**
   * Fixture: Authenticated Realm Page
   *
   * Performs login to specified realm.
   */
  authenticatedRealmPage: async ({ page, demoLogger, realmId, testStartTime }, use) => {
    await verifyTestEnvironment(page, {
      requiredRealms: [realmId],
      requiredUsers: ['admin@cas.com'],
    })

    await loginAsAdmin(page, {
      realmId,
    })

    await use(page)
  },
})

/**
 * Demo test with custom login page
 *
 * Allows using LoginPage instance instead of helper function.
 */
export const loginPageTest = base.extend<{
  demoLogger: UnifiedLogger
  loginPage: LoginPage
  testStartTime: number
}>({
  demoLogger: async ({ page }, use, testInfo) => {
    const logger = new UnifiedLogger(page, testInfo.title)
    await use(logger)
    logger.printSummary('[Login Demo] Test Summary')
    await logger.finalize()
  },

  testStartTime: async ({}, use) => {
    const startTime = Date.now()
    await use(startTime)
  },

  /**
   * Fixture: Login Page
   *
   * Provides LoginPage instance with logger.
   * Does NOT perform login - test controls login flow.
   *
   * Usage:
   * ```typescript
   * loginPageTest('login flow', async ({ loginPage }) => {
   *   await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')
   * })
   * ```
   */
  loginPage: async ({ page, demoLogger }, use) => {
    const loginPage = new LoginPage(page, demoLogger)
    await use(loginPage)
  },
})

// Re-export expect for convenience
export { expect } from '@playwright/test'
