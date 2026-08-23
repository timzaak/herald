/**
 * Demo Test Fixtures - Page Object Edition
 *
 * Purpose: Provide Page Object fixtures for better test organization.
 * Following Playwright official best practices for test fixtures.
 *
 * Key Improvements:
 * - Fixtures return Page Objects instead of raw Page
 * - Automatic setup (login + navigation)
 * - Type-safe fixtures
 * - Reduced test code duplication by 30-50%
 *
 * @see https://playwright.dev/docs/test-fixtures
 * @see https://playwright.dev/docs/pom
 */

import { test as base, type Page } from '@playwright/test'
import { UnifiedLogger } from '../helpers/unified-logger'
import { verifyTestEnvironment, cleanupDemoTestData } from '../helpers/environment-setup'
import {
  AdminLegalHelper,
  LegalConsentHelper,
  DeleteAccountHelper,
} from '../helpers/legal-consent'
import { LoginPage } from '../pages/login-page'
import { UsersPage } from '../pages/users-page'
import { RolesPage } from '../pages/roles-page'
import { PermissionsPage } from '../pages/permissions-page'
import { RealmsPage } from '../pages/realms-page'
import { ClientAppsPage } from '../pages/client-apps-page'
import { AuditPage } from '../pages/audit-page'
import { DashboardPage } from '../pages/dashboard-page'
import { ApiKeysPage } from '../pages/api-keys-page'
import { EntitlementMappingsPage } from '../pages/entitlement-mappings-page'
import { AdminSubscriptionListPage } from '../pages/admin-subscription-list-page'
import { SELECTORS } from '../selectors'

/**
 * Demo Page Object Fixtures
 *
 * Provides pre-configured Page Objects with:
 * - Environment verification completed
 * - Admin login completed
 * - Page navigation completed
 * - Logger initialized
 * - Session cleared between tests
 *
 * Usage:
 * ```typescript
 * import { test } from '../fixtures/demo-page.fixtures'
 *
 * test('should create user', async ({ usersPage }) => {
 *   // usersPage is already initialized and on the correct page
 *   await usersPage.createUser({ email: 'test@example.com' })
 * })
 * ```
 */
export const test = base.extend<{
  demoLogger: UnifiedLogger
  loginPage: LoginPage
  usersPage: UsersPage
  realmAdminPage: UsersPage
  rolesPage: RolesPage
  permissionsPage: PermissionsPage
  realmsPage: RealmsPage
  clientAppsPage: ClientAppsPage
  auditPage: AuditPage
  dashboardPage: DashboardPage
  apiKeyPage: ApiKeysPage
  entitlementMappingsPage: EntitlementMappingsPage
  adminSubscriptionListPage: AdminSubscriptionListPage
  adminLegalHelper: AdminLegalHelper
  legalConsentHelper: LegalConsentHelper
  deleteAccountHelper: DeleteAccountHelper
  testStartTime: number
  page: Page
}>({
  /**
   * Fixture: Page with Session Clearing
   *
   * Automatically clears localStorage and sessionStorage before each test.
   * This ensures clean state between tests and prevents authentication issues.
   */
  page: async ({ page }, use) => {
    // Clear storage before using the page
    await clearBrowserStorage(page)
    await use(page)
    // Clear storage after test completes
    await clearBrowserStorage(page)
  },

  /**
   * Fixture: Demo Logger
   *
   * Auto-finalized logger with test title.
   */
  demoLogger: async ({ page }, use, testInfo) => {
    const logger = new UnifiedLogger(page, testInfo.title)
    await use(logger)
    logger.printSummary('[Demo] Test Summary')
    await logger.finalize()
  },

  /**
   * Fixture: Test Start Time
   *
   * For cleanup operations.
   */
  testStartTime: async ({}, use) => {
    const startTime = Date.now()
    await use(startTime)
  },

  /**
   * Fixture: Login Page
   *
   * Provides LoginPage instance.
   * Does NOT perform login - test controls when to login.
   *
   * Use for:
   * - Testing login functionality
   * - Custom login flows
   * - Testing authentication errors
   */
  loginPage: async ({ page, demoLogger }, use) => {
    const loginPage = new LoginPage(page, demoLogger)
    await use(loginPage)
  },

  /**
   * Fixture: Users Page
   *
   * Automatically:
   * 1. Verifies environment
   * 2. Logs in as admin (for admin realm) or realm-specific admin (for other realms)
   * 3. Navigates to /{realmId}/users
   *
   * Use for:
   * - User management operations
   * - User CRUD tests
   * - User permission tests
   *
   * Note: This fixture defaults to admin realm. For realm-specific tests,
   * use realmAdminPage fixture instead.
   */
  usersPage: async ({ page, demoLogger, testStartTime, loginPage }, use) => {
    // Verify environment
    await verifyTestEnvironment(page, {
      requiredRealms: ['admin'],
      requiredUsers: ['admin@cas.com'],
    })

    // Login and navigate to admin realm
    await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')

    const usersPage = new UsersPage(page, demoLogger)
    await usersPage.goto('admin')

    await use(usersPage)
  },

  /**
   * Fixture: Realm Admin Users Page
   *
   * Automatically:
   * 1. Verifies environment (admin realm)
   * 2. Logs in as admin
   * 3. Creates realm1 if it doesn't exist
   * 4. Logs in as realm1-admin
   * 5. Navigates to /realm1/admin/users
   *
   * Use for:
   * - Realm isolation tests
   * - Realm-specific user management
   * - Cross-realm access validation
   */
  realmAdminPage: async ({ page, demoLogger, testStartTime, loginPage }, use) => {
    // Verify environment (admin realm only)
    await verifyTestEnvironment(page, {
      requiredRealms: ['admin'],
      requiredUsers: ['admin@cas.com'],
    })

    // Login as admin to create realm1
    await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')

    // Create RealmsPage and navigate to realms page
    const realmsPage = new RealmsPage(page, demoLogger)
    await realmsPage.goto()

    // Ensure realm1 exists
    await ensureRealm1Exists(page, demoLogger, realmsPage)

    // Login as realm1-admin
    await loginPage.loginAsAdmin('realm1-admin@test.com', 'password', 'realm1')

    const usersPage = new UsersPage(page, demoLogger)
    await usersPage.goto('realm1')

    await use(usersPage)
  },

  /**
   * Fixture: Roles Page
   *
   * Automatically:
   * 1. Verifies environment
   * 2. Logs in as admin
   * 3. Navigates to /admin/roles
   *
   * Use for:
   * - Role management operations
   * - Role CRUD tests
   * - Role permission assignment tests
   */
  rolesPage: async ({ page, demoLogger, testStartTime, loginPage }, use) => {
    await verifyTestEnvironment(page, {
      requiredRealms: ['admin'],
      requiredUsers: ['admin@cas.com'],
    })

    await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')

    const rolesPage = new RolesPage(page, demoLogger)
    await rolesPage.goto()

    await use(rolesPage)
  },

  /**
   * Fixture: Permissions Page
   *
   * Automatically:
   * 1. Verifies environment
   * 2. Logs in as admin
   * 3. Navigates to /admin/permissions
   *
   * Use for:
   * - Permission management operations
   * - Permission CRUD tests
   * - Permission validation tests
   */
  permissionsPage: async ({ page, demoLogger, testStartTime, loginPage }, use) => {
    await verifyTestEnvironment(page, {
      requiredRealms: ['admin'],
      requiredUsers: ['admin@cas.com'],
    })

    await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')

    const permissionsPage = new PermissionsPage(page, demoLogger)
    await permissionsPage.goto()

    await use(permissionsPage)
  },

  /**
   * Fixture: Realms Page
   *
   * Provides RealmsPage instance.
   * Does NOT perform login or navigation - test controls when to login/navigate.
   *
   * Use for:
   * - Realm management operations
   * - Realm CRUD tests
   * - Realm configuration tests
   * - Multi-user/role scenarios
   */
  realmsPage: async ({ page, demoLogger }, use) => {
    const realmsPage = new RealmsPage(page, demoLogger)
    await use(realmsPage)
  },

  /**
   * Fixture: Client Apps Page
   *
   * Automatically:
   * 1. Verifies environment
   * 2. Logs in as admin
   * 3. Navigates to /{realmId}/client-apps
   *
   * Use for:
   * - Client App management operations
   * - Client App CRUD tests
   * - Client App configuration tests
   */
  clientAppsPage: async ({ page, demoLogger, testStartTime, loginPage }, use) => {
    await verifyTestEnvironment(page, {
      requiredRealms: ['admin'],
      requiredUsers: ['admin@cas.com'],
    })

    await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')

    const clientAppsPage = new ClientAppsPage(page, demoLogger)
    await clientAppsPage.goto()

    await use(clientAppsPage)
  },

  /**
   * Fixture: Audit Page
   *
   * Automatically:
   * 1. Verifies environment
   * 2. Logs in as admin
   * 3. Navigates to /{realmId}/manage/audit
   *
   * Use for:
   * - Audit log viewing and filtering
   * - Audit event detail inspection
   * - Audit log pagination tests
   */
  auditPage: async ({ page, demoLogger, testStartTime, loginPage }, use) => {
    await verifyTestEnvironment(page, {
      requiredRealms: ['admin'],
      requiredUsers: ['admin@cas.com'],
    })

    await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')

    const auditPage = new AuditPage(page, demoLogger)
    await auditPage.goto()

    await use(auditPage)
  },

  /**
   * Fixture: Dashboard Page
   *
   * Automatically:
   * 1. Verifies environment
   * 2. Logs in as admin
   * 3. Navigates to dashboard via sidebar click
   *
   * Use for:
   * - Dashboard stats display tests
   * - Auth trend chart tests
   * - Quick navigation tests
   * - Error state and retry tests
   */
  dashboardPage: async ({ page, demoLogger, testStartTime, loginPage }, use) => {
    await verifyTestEnvironment(page, {
      requiredRealms: ['admin'],
      requiredUsers: ['admin@cas.com'],
    })

    await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')

    const dashboardPage = new DashboardPage(page, demoLogger)
    await dashboardPage.goto()

    // Defensively reload /manage so the dashboard stats query is issued AFTER
    // the admin-web-console credential is in auth-storage. Without this, the
    // initial /manage mount during login navigation races the switch-client
    // request: the stats GET fires before the credential switch completes and
    // fails with 403 ("Access denied: admin console credential required").
    // The global QueryClient `retry: false` + 4xx-no-retry policy then caches
    // that error permanently, so `dashboard-stats-row` never renders and
    // US-RA-010's synchronous `isStatsRowVisible()` assertion fails.
    //
    // By the time loginAsAdmin() returns, switch-client has already completed
    // (login-page.ts waits for it), so a full page reload re-mounts the SPA
    // with a fresh in-memory React Query cache and the correct bearer token,
    // making the stats GET return 200 deterministically. US-RA-011/012 already
    // tolerated the race via looser timeouts / re-navigation; the reload
    // unifies all three tests on the post-credential path without weakening
    // any assertion.
    await page.reload({ waitUntil: 'networkidle', timeout: 15000 })

    await use(dashboardPage)
  },

  /**
   * Fixture: API Keys Page
   *
   * Automatically:
   * 1. Verifies environment
   * 2. Logs in as admin
   * 3. Navigates to /{realmId}/manage/api-keys
   *
   * Use for:
   * - API Key management operations
   * - API Key CRUD tests
   * - API Key reveal and delete tests
   */
  apiKeyPage: async ({ page, demoLogger, testStartTime, loginPage }, use) => {
    await verifyTestEnvironment(page, {
      requiredRealms: ['admin'],
      requiredUsers: ['admin@cas.com'],
    })

    await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')

    const apiKeyPage = new ApiKeysPage(page, demoLogger)
    await apiKeyPage.goto()

    await use(apiKeyPage)
  },

  /**
   * Fixture: Entitlement Mappings Page (master-detail)
   *
   * Automatically:
   * 1. Verifies environment
   * 2. Logs in as admin
   * 3. Navigates to /{realmId}/manage/billing/entitlement-mappings (by route —
   *    the sidebar entry testid is i18n-derived and must not be relied on)
   *
   * Use for:
   * - Multi-price master-detail configuration (US-EM-007)
   * - Entitlement mapping product-list viewing and filtering
   * - Provider product sync (returns {productsSynced, pricesSynced})
   * - Protected-price 409 dialog (Cancel-only) and webhook-unresolved banner
   *
   * The POM exposes selectProduct/configureFixedPointRule/saveChanges/sync;
   * see pages/entitlement-mappings-page.ts for the full method surface.
   */
  entitlementMappingsPage: async ({ page, demoLogger, testStartTime, loginPage }, use) => {
    await verifyTestEnvironment(page, {
      requiredRealms: ['admin'],
      requiredUsers: ['admin@cas.com'],
    })

    await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')

    const entitlementMappingsPage = new EntitlementMappingsPage(page, demoLogger)
    await entitlementMappingsPage.goto('admin')

    await use(entitlementMappingsPage)
  },

  /**
   * Fixture: Admin Subscription List Page
   *
   * Automatically:
   * 1. Verifies environment
   * 2. Logs in as admin
   * 3. Navigates to /{realmId}/manage/billing/subscriptions
   *
   * Use for:
   * - Subscription projection list viewing and filtering
   * - Subscription status verification tests
   */
  adminSubscriptionListPage: async ({ page, demoLogger, testStartTime, loginPage }, use) => {
    await verifyTestEnvironment(page, {
      requiredRealms: ['admin'],
      requiredUsers: ['admin@cas.com'],
    })

    await loginPage.loginAsAdmin('admin@cas.com', 'password', 'admin')

    const adminSubscriptionListPage = new AdminSubscriptionListPage(page, demoLogger)
    await adminSubscriptionListPage.goto('admin')

    await use(adminSubscriptionListPage)
  },

  /**
   * Fixture: Admin Legal Helper
   *
   * Provides a consent-aware AdminLegalHelper instance.
   * The helper logs itself in when navigating to Settings > Legal.
   */
  adminLegalHelper: async ({ page, demoLogger }, use) => {
    const helper = new AdminLegalHelper(page, demoLogger)
    await use(helper)
  },

  /**
   * Fixture: Legal Consent Helper
   *
   * Provides a LegalConsentHelper for public agreement pages and the post-login
   * re-consent dialog.
   */
  legalConsentHelper: async ({ page, demoLogger }, use) => {
    const helper = new LegalConsentHelper(page, demoLogger)
    await use(helper)
  },

  /**
   * Fixture: Delete Account Helper
   *
   * Provides a DeleteAccountHelper for the self-service account deletion flow.
   */
  deleteAccountHelper: async ({ page, demoLogger }, use) => {
    const helper = new DeleteAccountHelper(page, demoLogger)
    await use(helper)
  },
})

// Re-export expect for convenience
export { expect, type Page } from '@playwright/test'

/**
 * Cleanup Helper
 *
 * Use in test.afterEach to clean up test data.
 *
 * @example
 * ```typescript
 * import { test, cleanupTestData } from '../fixtures/demo-page.fixtures'
 *
 * test.afterEach(async ({ usersPage, testStartTime }) => {
 *   await cleanupTestData(usersPage.page, 'admin', {
 *     timestamp: testStartTime,
 *   })
 * })
 * ```
 */
export async function cleanupTestData(
  page: Parameters<typeof cleanupDemoTestData>[0],
  realmId: Parameters<typeof cleanupDemoTestData>[1],
  options?: Parameters<typeof cleanupDemoTestData>[2]
) {
  await cleanupDemoTestData(page, realmId, options)
}

/**
 * Ensure realm1 exists for testing
 *
 * This helper ensures realm1 is available for realm-specific tests.
 * It creates realm1 if it doesn't exist.
 *
 * @param page Playwright Page object
 * @param demoLogger Logger instance
 * @param realmsPage RealmsPage instance (must be on realms page)
 */
export async function ensureRealm1Exists(
  page: Page,
  demoLogger: UnifiedLogger,
  realmsPage: RealmsPage
): Promise<void> {
  console.log('[ensureRealm1Exists] 检查 realm1 是否存在...')

  try {
    // Wait for page to be stable before checking
    await page.waitForLoadState('networkidle', { timeout: 5000 }).catch(() => {
      console.log('[ensureRealm1Exists] Network idle timeout, continuing anyway')
    })

    const realm1Exists = await realmsPage.realmExists('realm1')
    if (!realm1Exists) {
      console.log('[ensureRealm1Exists] realm1 不存在，创建 realm1...')
      // Page already ready after goto() or when fixture initializes
      await realmsPage.createRealm({
        id: 'realm1',
        name: 'Realm 1',
        adminEmail: 'realm1-admin@test.com',
        adminPassword: 'password',
      }, false)
      console.log('[ensureRealm1Exists] realm1 创建成功')
    } else {
      console.log('[ensureRealm1Exists] realm1 已存在')
    }
  } catch (error) {
    console.error('[ensureRealm1Exists] 检查或创建 realm1 时出错:', error)
    throw new Error(`Failed to ensure realm1 exists: ${error}`)
  }
}

/**
 * Clear browser storage (localStorage, sessionStorage, and React Query cache)
 *
 * This helper clears all storage to ensure clean state between tests.
 * It prevents authentication issues from stale session data and React Query cache.
 *
 * @param page Playwright Page object
 */
export async function clearBrowserStorage(page: Page): Promise<void> {
  try {
    // Clear React Query cache (in-memory) BEFORE clearing cookies
    // This ensures the cache is cleared while the page is still loaded
    await page.evaluate(() => {
      // Try to access the QueryClient exposed via main.tsx
      const windowObj = window as typeof window & {
        __REACT_QUERY_CLIENT__?: { clear: () => void }
      }

      if (windowObj.__REACT_QUERY_CLIENT__) {
        try {
          windowObj.__REACT_QUERY_CLIENT__.clear()
          console.log('[clearBrowserStorage] React Query cache cleared via __REACT_QUERY_CLIENT__')
        } catch (error) {
          console.log('[clearBrowserStorage] Failed to clear React Query cache:', error)
        }
      }
    })

    // Clear localStorage
    await page.evaluate(() => {
      localStorage.clear()
    })

    // Clear sessionStorage
    await page.evaluate(() => {
      sessionStorage.clear()
    })

    // Clear any React Query persistence in localStorage
    // (if persistQueryClient plugin is ever added)
    await page.evaluate(() => {
      Object.keys(localStorage).forEach((key) => {
        if (key.includes('react-query') || key.includes('REACT_QUERY') || key.startsWith('RQ')) {
          localStorage.removeItem(key)
        }
      })
    })

    // Clear all cookies AFTER clearing the cache
    const context = page.context()
    await context.clearCookies()

    console.log('[clearBrowserStorage] Browser storage and React Query cache cleared successfully')
  } catch (error) {
    // Ignore errors when clearing storage (page might not have context yet)
    console.log('[clearBrowserStorage] Note: Could not clear storage:', error)
  }
}
