/**
 * SDK Grant Points Ext API Demo Tests (US-TP-017)
 *
 * Tests the ext API endpoint for granting points via SDK/third-party integration.
 * Covers US-TP-017 scenarios 1-6 from docs/user-stories/integration/sdk.md (Story 4).
 *
 * Scenarios:
 * - S1: Grant points with validity days (happy path, 200)
 * - S2: Grant permanent points (no validity, 200)
 * - S3: Amount = 0 validation error (400)
 * - S4: Missing points.manage permission (403)
 * - S5: User not found with valid UUID format (404)
 * - S6: Cross-realm access denied (403)
 *
 * @see docs/user-stories/integration/sdk.md (Story 4)
 * @see backend/api-ext/src/points.rs (grant_points_ext handler)
 */

import { test, expect } from '../fixtures/demo-page.fixtures'
import type { APIRequestContext } from '@playwright/test'
import { createBearerApiContext, DEMO_ADMIN } from '../helpers/auth'
import { LoginPage } from '../pages/login-page'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import {
  grantPointsViaExtApi,
  createTestApiKeyWithPermission,
  type ApiKeyWithPermission,
} from '../helpers/grant-points-helpers'
import { makeExtApiRequest } from '../helpers/ext-api-helper'
import { listBucketsViaApi } from '../helpers/bucket-helpers'
import { CREDIT_BUCKET_KEYS } from '../helpers/bucket-seed-ids'

// Non-existent but valid UUID for S5 (UserNotFound test).
// Must be valid UUID format -- a non-UUID string triggers 400 InvalidUserIdFormat instead of 404.
const NONEXISTENT_USER_ID = '00000000-0000-0000-0000-000000000000'

// Cross-realm ID for S6 (must differ from the API key's realm)
const CROSS_REALM_ID = 'nonexistent-realm'

// `admin-api-client` is auto-provisioned per realm (backend/domain/src/realm/services.rs)
// and API keys bound to it are EXEMPT from the client-app scope checks
// (ADMIN_API_CLIENT_ID, backend/api-ext/src/client_app_scope.rs).
// Since the scope hardening, `grant_points_ext` step 4b
// (backend/api-ext/src/points.rs) rejects keys bound to ordinary client apps
// unless the target bucket is covered by that app. The admin realm's seeded
// `primary-pool` has NO coverage rows at all — demo seed only binds coverage to
// realm-001's `points-demo-app` (scripts/lib/demo_seed.py) — so the happy-path
// key must bind to `admin-api-client` to act realm-wide. Same pattern as the
// support-paywall demos (billing-admin/support-paywall-subscription-revoke-demo).
const ADMIN_API_CLIENT_ID = 'admin-api-client'

test.describe('[SDK Ext API] Grant Points Demo Tests (US-TP-017)', () => {
  // Shared across tests within this describe block (same worker).
  let apiKeyWithPermission: ApiKeyWithPermission
  let apiKeyWithoutPermission: ApiKeyWithPermission
  let adminUserUuid: string
  /**
   * Target Credit Bucket UUID for happy-path grants (bucketId is now
   * REQUIRED on every ext-API grant). Resolved in `beforeAll`
   * from the seeded admin-realm `primary-pool` directory via the admin HTTP
   * API (same authenticated session used to create the API keys). Empty string
   * sentinel mirrors `adminUserUuid` — error-path tests (S4-S6) tolerate a
   * missing value because the realm/permission/user check fails first.
   */
  let targetBucketId: string
  let setupStartTime: number

  test.beforeAll(async ({ browser }) => {
    setupStartTime = Date.now()

    const context = await browser.newContext()
    const page = await context.newPage()
    let apiContext: APIRequestContext | undefined

    try {
      const loginPage = new LoginPage(page)
      await loginPage.loginAsAdmin(
        DEMO_ADMIN.email,
        DEMO_ADMIN.password,
        DEMO_ADMIN.realmId,
      )
      apiContext = await createBearerApiContext(loginPage.getAccessToken())

      // Resolve the admin realm's auto-provisioned `admin-api-client` UUID and
      // bind the happy-path key to it: the key must pass the 4b bucket-coverage
      // scope check (see ADMIN_API_CLIENT_ID rationale above) so S1/S2 reach
      // 200 and S5 reaches the 404 UserNotFound branch instead of an early 403.
      const adminApiAppId = await resolveClientAppId(
        apiContext,
        DEMO_ADMIN.realmId,
        ADMIN_API_CLIENT_ID,
      )

      // Create primary API key (nominally with points.manage permission)
      // NOTE: createTestApiKeyWithPermission's permission param is a placeholder;
      // actual permissions are determined by roles assigned to the API key.
      // The demo environment must have a role with points.manage assigned for S1-S3 to pass.
      apiKeyWithPermission = await createTestApiKeyWithPermission(
        page,
        'points.manage',
        setupStartTime,
        DEMO_ADMIN.realmId,
        adminApiAppId,
        apiContext,
      )

      // Create secondary API key (with a different nominal permission, NOT points.manage)
      apiKeyWithoutPermission = await createTestApiKeyWithPermission(
        page,
        'clients.view',
        setupStartTime + 1,
        DEMO_ADMIN.realmId,
        '',
        apiContext,
      )

      // Look up admin user UUID via admin API (not ext API, which requires specific permissions).
      const backendUrl =
        process.env.API_BASE_URL ||
        process.env.BASE_URL?.replace(/:\d+/, ':8080') ||
        'http://localhost:8080'

      const usersResponse = await apiContext.get(
        `${backendUrl}/api/users/admin?search=${encodeURIComponent(DEMO_ADMIN.email)}`,
      )
      if (usersResponse.ok()) {
        const usersBody = await usersResponse.json()
        const items = usersBody.items ?? []
        const adminUser = items.find(
          (u: { email: string }) => u.email === DEMO_ADMIN.email,
        )
        if (adminUser) {
          adminUserUuid = adminUser.id
        }
      }

      if (!adminUserUuid) {
        console.warn(
          '[SDK Grant Points] Could not resolve admin user UUID. ' +
            'Happy-path tests (S1-S3) will fail. ' +
            'Ensure the demo environment has the admin user seeded.',
        )
        adminUserUuid = ''
      }

      // Resolve the target Credit Bucket UUID (the ext-API grant now requires
      // `bucketId`). The admin realm is seeded with
      // a `primary-pool` (registration pool) bucket by
      // `scripts/lib/demo_seed.py::_ensure_credit_buckets`; reuse that as the
      // target for every happy-path grant. Failures fall back to '' and rely
      // on the same warn-and-continue pattern as `adminUserUuid`.
      try {
        const buckets = await listBucketsViaApi(page, DEMO_ADMIN.realmId, apiContext)
        const primary = buckets.find(
          (b) => b.bucketKey === CREDIT_BUCKET_KEYS.PRIMARY_POOL,
        )
        targetBucketId = primary?.id ?? ''
      } catch (error) {
        console.warn(
          '[SDK Grant Points] Could not resolve admin-realm primary-pool bucket:',
          error,
        )
        targetBucketId = ''
      }

      if (!targetBucketId) {
        console.warn(
          '[SDK Grant Points] primary-pool bucket not found in admin realm. ' +
            'Happy-path tests (S1-S3) will fail with 400 grant_bucket_required. ' +
            'Ensure scripts/lib/demo_seed.py::_ensure_credit_buckets has run.',
        )
      }
    } finally {
      await apiContext?.dispose()
      await context.close()
    }
  })

  test.beforeEach(async ({ page }) => {
    await verifyTestEnvironment(page, {
      requiredRealms: [DEMO_ADMIN.realmId],
      requiredUsers: [DEMO_ADMIN.email],
    })
  })

  test.afterAll(async ({ browser }) => {
    if (!setupStartTime) return

    const context = await browser.newContext()
    const page = await context.newPage()
    let apiContext: APIRequestContext | undefined

    try {
      const loginPage = new LoginPage(page)
      await loginPage.loginAsAdmin(
        DEMO_ADMIN.email,
        DEMO_ADMIN.password,
        DEMO_ADMIN.realmId,
      )
      apiContext = await createBearerApiContext(loginPage.getAccessToken())

      const backendUrl =
        process.env.API_BASE_URL ||
        process.env.BASE_URL?.replace(/:\d+/, ':8080') ||
        'http://localhost:8080'

      // Cleanup: delete test client apps created during beforeAll.
      // (`grant-test-app-${suffix1}` is never created anymore — the happy-path
      // key binds to the seeded admin-api-client instead — so its lookup no-ops.)
      const suffix1 = setupStartTime
      const suffix2 = setupStartTime + 1
      const appNames = [`grant-test-app-${suffix1}`, `grant-test-app-${suffix2}`]

      for (const appName of appNames) {
        try {
          // List client apps to find the one to delete
          const listRes = await apiContext.get(`${backendUrl}/api/client/admin`)
          if (!listRes.ok()) continue
          const listBody = await listRes.json()
          const items = listBody.items ?? []
          const app = items.find((item: { clientId: string }) => item.clientId === appName)
          if (!app?.id) continue

          // Delete the client app
          await apiContext.delete(`${backendUrl}/api/client/admin/${app.id}`)
        } catch (error) {
          console.warn(`[Cleanup] Failed to delete client app "${appName}":`, error)
        }
      }
    } catch (error) {
      console.warn('[Cleanup] Failed to delete test client apps:', error)
    } finally {
      await apiContext?.dispose()
      await context.close()
    }
  })

  // ==========================================================================
  // US-TP-017 Scenario 1: Grant points with validity days (happy path)
  // ==========================================================================
  test('S1: Grant points with validity days returns 200 with transaction details', async () => {
    const amount = 100
    const reason = 'Level up bonus'
    const validityDays = 30

    await test.step('Given: SDK is initialized with API key that has points.manage permission', async () => {
      expect(apiKeyWithPermission.apiKey.length).toBeGreaterThan(0)
      expect(adminUserUuid.length).toBeGreaterThan(0)
    })

    let response: { status: number; responseBody: unknown }

    await test.step('When: Grant 100 points with 30-day validity to a user', async () => {
      response = await grantPointsViaExtApi(
        apiKeyWithPermission.apiKey,
        DEMO_ADMIN.realmId,
        {
          userId: adminUserUuid,
          amount,
          reason,
          validityDays,
          bucketId: targetBucketId,
        },
      )
    })

    await test.step('Then: Response status is 200 and contains transaction details with expiresAt', async () => {
      expect(response!.status).toBe(200)

      const body = response!.responseBody as Record<string, unknown>
      expect(body.transactionId).toBeDefined()
      expect(typeof body.transactionId).toBe('string')
      expect(body.amount).toBe(amount)
      // Validity grant must have a non-null expiresAt
      expect(body.expiresAt).toBeDefined()
      expect(body.expiresAt).not.toBeNull()
    })
  })

  // ==========================================================================
  // US-TP-017 Scenario 2: Grant permanent points (no validity)
  // ==========================================================================
  test('S2: Grant permanent points returns 200 with null expiresAt', async () => {
    const amount = 50
    const reason = 'Permanent reward'

    await test.step('Given: SDK is initialized with API key that has points.manage permission', async () => {
      expect(apiKeyWithPermission.apiKey.length).toBeGreaterThan(0)
      expect(adminUserUuid.length).toBeGreaterThan(0)
    })

    let response: { status: number; responseBody: unknown }

    await test.step('When: Grant 50 permanent points (no validityDays) to a user', async () => {
      response = await grantPointsViaExtApi(
        apiKeyWithPermission.apiKey,
        DEMO_ADMIN.realmId,
        {
          userId: adminUserUuid,
          amount,
          reason,
          // No validityDays -- permanent grant
          bucketId: targetBucketId,
        },
      )
    })

    await test.step('Then: Response status is 200 and expiresAt is null (permanent)', async () => {
      expect(response!.status).toBe(200)

      const body = response!.responseBody as Record<string, unknown>
      expect(body.transactionId).toBeDefined()
      expect(body.amount).toBe(amount)
      // Permanent grants have no expiry
      expect(body.expiresAt).toBeNull()
    })
  })

  // ==========================================================================
  // US-TP-017 Scenario 3: Amount = 0 validation error (400)
  // Backend: grant_points_ext checks amount <= 0 and returns 400 InvalidAmount
  // ==========================================================================
  test('S3: Amount = 0 returns 400 validation error', async () => {
    await test.step('Given: SDK is initialized with a valid API key', async () => {
      expect(apiKeyWithPermission.apiKey.length).toBeGreaterThan(0)
      expect(adminUserUuid.length).toBeGreaterThan(0)
    })

    let response: { status: number; responseBody: unknown }

    await test.step('When: Attempt to grant 0 points', async () => {
      response = await grantPointsViaExtApi(
        apiKeyWithPermission.apiKey,
        DEMO_ADMIN.realmId,
        {
          userId: adminUserUuid,
          amount: 0,
          reason: 'Invalid amount test',
          // The amount=0 check fails first, so the value is incidental here.
          bucketId: targetBucketId,
        },
      )
    })

    await test.step('Then: Response status is 400 with validation error', async () => {
      expect(response!.status).toBe(400)

      const body = response!.responseBody as Record<string, unknown>
      // Error response format: { code: number, message: string }
      expect(body.message).toBeDefined()
    })
  })

  // ==========================================================================
  // US-TP-017 Scenario 4: Missing points.manage permission (403)
  //
  // Uses the secondary API key created without points.manage permission.
  // Note: createTestApiKeyWithPermission's permission param is a placeholder;
  // the demo environment must have role-based differentiation for this test
  // to distinguish between keys with and without points.manage.
  // If no role differentiation exists, both keys behave identically and
  // this test will return the same status as S1 (either both 403 or both 200).
  // ==========================================================================
  test('S4: API key without points.manage permission returns 403', async () => {
    await test.step('Given: SDK uses API key WITHOUT points.manage permission', async () => {
      expect(apiKeyWithoutPermission.apiKey.length).toBeGreaterThan(0)
      // This API key was created with 'clients.view' nominal permission, not points.manage.
      // Actual permission enforcement depends on role assignment in the demo environment.
    })

    let response: { status: number; responseBody: unknown }

    await test.step('When: Attempt to grant points using unprivileged API key', async () => {
      response = await grantPointsViaExtApi(
        apiKeyWithoutPermission.apiKey,
        DEMO_ADMIN.realmId,
        {
          userId: adminUserUuid || NONEXISTENT_USER_ID,
          amount: 100,
          reason: 'Should be rejected',
          // The permission check fails before bucket validation.
          bucketId: targetBucketId,
        },
      )
    })

    await test.step('Then: Response status is 403 (permission denied)', async () => {
      expect(response!.status).toBe(403)

      const body = response!.responseBody as Record<string, unknown>
      expect(body.message).toBeDefined()
    })
  })

  // ==========================================================================
  // US-TP-017 Scenario 5: User not found (404) with valid UUID format
  // Backend: valid UUID but no matching user returns 404 UserNotFound
  // ==========================================================================
  test('S5: Non-existent user with valid UUID returns 404', async () => {
    await test.step('Given: SDK is initialized and target user does not exist', async () => {
      expect(apiKeyWithPermission.apiKey.length).toBeGreaterThan(0)
      // NONEXISTENT_USER_ID is a valid UUID format but not assigned to any user
    })

    let response: { status: number; responseBody: unknown }

    await test.step('When: Attempt to grant points to non-existent user', async () => {
      response = await grantPointsViaExtApi(
        apiKeyWithPermission.apiKey,
        DEMO_ADMIN.realmId,
        {
          userId: NONEXISTENT_USER_ID,
          amount: 100,
          reason: 'User not found test',
          // The 4b bucket-scope check runs BEFORE the user-existence lookup
          // (backend/api-ext/src/points.rs), so the scope-exempt key is what
          // lets this request reach the 404 UserNotFound branch.
          bucketId: targetBucketId,
        },
      )
    })

    await test.step('Then: Response status is 404 (user not found)', async () => {
      expect(response!.status).toBe(404)

      const body = response!.responseBody as Record<string, unknown>
      expect(body.message).toBeDefined()
    })
  })

  // ==========================================================================
  // US-TP-017 Scenario 6: Cross-realm access denied (403)
  // Backend: realm isolation check occurs before permission check
  // ==========================================================================
  test('S6: Cross-realm grant returns 403 forbidden', async () => {
    await test.step('Given: API key belongs to admin realm', async () => {
      expect(apiKeyWithPermission.apiKey.length).toBeGreaterThan(0)
      // apiKeyWithPermission was created in admin realm
    })

    let response: { status: number; responseBody: unknown }

    await test.step('When: Attempt to grant points in a different realm', async () => {
      response = await grantPointsViaExtApi(
        apiKeyWithPermission.apiKey,
        CROSS_REALM_ID,
        {
          userId: NONEXISTENT_USER_ID,
          amount: 100,
          reason: 'Cross-realm test',
          // The cross-realm check fails before bucket validation.
          bucketId: targetBucketId,
        },
      )
    })

    await test.step('Then: Response status is 403 (cross-realm forbidden)', async () => {
      expect(response!.status).toBe(403)

      const body = response!.responseBody as Record<string, unknown>
      expect(body.message).toBeDefined()
    })
  })
})

/** Resolve the client-app UUID for a given client_id in a realm (mirrors DE-D01). */
async function resolveClientAppId(
  request: APIRequestContext,
  realmId: string,
  clientId: string,
): Promise<string> {
  const backendUrl =
    process.env.API_BASE_URL ||
    process.env.BASE_URL?.replace(/:\d+/, ':8080') ||
    'http://localhost:8080'
  const resp = await request.get(`${backendUrl}/api/client/${realmId}`)
  if (!resp.ok()) {
    throw new Error(
      `could not list client apps in ${realmId}: ${resp.status()} ${await resp.text()}`,
    )
  }
  const body = await resp.json()
  const raw: unknown = Array.isArray(body)
    ? body
    : (body as { data?: unknown }).data ??
      (body as { items?: unknown }).items ??
      []
  const apps: { id: string; clientId?: string; client_id?: string }[] =
    Array.isArray(raw) ? raw : []
  const hit = apps.find((a) => (a.clientId ?? a.client_id) === clientId)
  if (!hit) {
    throw new Error(
      `client app ${clientId} not found in ${realmId}; available: ${apps.map((a) => a.clientId ?? a.client_id).join(', ')}`,
    )
  }
  return hit.id
}
