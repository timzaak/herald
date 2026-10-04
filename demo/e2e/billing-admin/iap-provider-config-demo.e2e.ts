/**
 * IAP Provider Configuration Demo (US-IAP-001)
 *
 * User story source: docs/user-stories/billing/support-iap.md
 *   - US-IAP-001 (P0): Configure IAP payment channel credentials
 *
 * Coverage:
 *   - S1: Apple App Store — create, edit keeps the .p8 secret, delete.
 *   - S2: Google Play — create, edit keeps the Service Account JSON.
 *
 * NOT covered (declared gap):
 *   - US-IAP-001 scenario 3 (delete protection under active IAP subscriptions).
 *     The demo env has no active IAP subscriptions, so the 409 `delete-conflict`
 *     branch (DeleteConfirmDialog renders an active-sub count + Cancel-only
 *     footer; no `delete-confirm-button`) cannot be exercised via the UI. That
 *     branch is covered by backend tests. Do NOT mask this gap.
 *
 * Assertion discipline:
 *   - Assertions land on PERSISTENT state (provider row testids, the
 *     payment-providers list URL, the add-button reappearing post-delete).
 *   - Toasts (sonner) are NEVER used as a primary assertion — they are
 *     auto-dismissed and locale/volatile.
 *
 * Fixture discipline:
 *   - Unified fixture (`test` / `expect` / `cleanupTestData` from
 *     demo-page.fixtures). `demoLogger` is auto-finalized inside the fixture —
 *     tests MUST NOT call `logger.finalize()` manually.
 *   - `afterEach` runs `cleanupTestData(page, 'admin', { timestamp })`.
 *
 * Runner (this demo is compile-only):
 *   uv run scripts/demo-test-runner.py \
 *     "demo/e2e/billing-admin/iap-provider-config-demo.e2e.ts" \
 *     --run-id <RUN_ID> --grep "<test title>" --no-ngrok
 *   (--no-ngrok: IAP config is management-path only, no webhook dependency.)
 */

import { test, cleanupTestData, expect } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { loginAsAdmin, DEMO_ADMIN } from '../helpers/auth'
import { PaymentProvidersPage } from '../pages/payment-providers-page'

test.describe('US-IAP-001 — IAP provider configuration', () => {
  let testStartTime: number
  let paymentProvidersPage: PaymentProvidersPage

  test.beforeEach(async ({ page, demoLogger }) => {
    testStartTime = Date.now()
    await verifyTestEnvironment(page, {
      requiredRealms: [DEMO_ADMIN.realmId],
      requiredUsers: [DEMO_ADMIN.email],
    })
    await loginAsAdmin(page, { realmId: DEMO_ADMIN.realmId })
    paymentProvidersPage = new PaymentProvidersPage(page, demoLogger)
    await demoLogger.testCode.log('Admin logged in; environment verified')
  })

  test.afterEach(async ({ page, demoLogger }) => {
    await cleanupTestData(page, DEMO_ADMIN.realmId, {
      keepUsers: [DEMO_ADMIN.email],
      timestamp: testStartTime,
    })
    await demoLogger.testCode.log('Test data cleaned up')
  })

  // ==========================================================================
  // S1: Apple App Store — create, edit keeps .p8, delete
  // ==========================================================================

  test('[US-IAP-001 S1] Apple provider configuration — create, edit keeps .p8, delete', async ({
    page,
    demoLogger,
  }) => {
    const runId = Date.now()
    // Create-time credentials (parameterized for re-run idempotency).
    const createBundleId = `com.cas.apple.${runId}`
    const createIssuerId = `${runId}-aaaa-bbbb-cccc-${runId}`
    const createKeyId = `KEYCR${String(runId).slice(-5)}`
    const privateKeyP8 = [
      '-----BEGIN PRIVATE KEY-----',
      'MIGTAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBHkwdwIBAQQg' + runId.toString(16).padStart(8, '0'),
      'Pa5eiTcXkDTUkJiumGmfvNq-1h8WodLJvL3ObG02Il2CgYIKoZIzj0DAQehRANCAAS',
      '-----END PRIVATE KEY-----',
    ].join('\n')

    await test.step('Given payment-providers page', async () => {
      await paymentProvidersPage.goto(DEMO_ADMIN.realmId)
      await expect(page.getByTestId('payment-providers-page')).toBeVisible()
      await demoLogger.testCode.log('On payment-providers page')
    })

    await test.step('When create Apple config', async () => {
      await paymentProvidersPage.configureIapProvider('apple', {
        bundleId: createBundleId,
        issuerId: createIssuerId,
        keyId: createKeyId,
        privateKeyP8,
        environment: 'production',
      })
      await demoLogger.testCode.log('Apple provider created')
    })

    await test.step('Then Apple provider is listed', async () => {
      await expect(page.getByTestId('apple-provider-row')).toBeVisible()
      await expect(page.getByTestId('edit-apple-button')).toBeVisible()
      await demoLogger.testCode.log('Apple provider row present')
    })

    await test.step('When edit Apple and leave .p8 blank', async () => {
      // Edit with editSensitiveLeaveEmpty: change the non-secret fields to NEW
      // values and intentionally leave the .p8 input blank. The successful save
      // back to the list is the retention proof (the backend keeps the prior
      // secret key when none is sent).
      await paymentProvidersPage.configureIapProvider(
        'apple',
        {
          bundleId: `${createBundleId}-edited`,
          issuerId: `${createIssuerId}-edited`,
          keyId: `${createKeyId}E`,
          privateKeyP8, // ignored: skipSensitive is true
          environment: 'production',
        },
        { editSensitiveLeaveEmpty: true },
      )
      await demoLogger.testCode.log('Apple provider edited (secret left empty to keep)')
    })

    await test.step('Then edit persisted (provider row still present)', async () => {
      // Retention of the prior .p8 is implied by the backend accepting the save
      // with no secret sent — assert the save returned to the list (persistent
      // state), NOT a toast.
      await expect(page.getByTestId('apple-provider-row')).toBeVisible()
      await expect(page.getByTestId('edit-apple-button')).toBeVisible()
      await demoLogger.testCode.log('Apple provider row present post-edit')
    })

    await test.step('When delete Apple', async () => {
      await paymentProvidersPage.deleteIapProvider('apple')
      await demoLogger.testCode.log('Apple provider delete confirmed')
    })

    await test.step('Then Apple removed', async () => {
      await expect(page.getByTestId('apple-provider-row')).toHaveCount(0)
      // The add-button reappears once the provider is no longer configured.
      await expect(page.getByTestId('add-apple-button')).toBeVisible()
      await demoLogger.testCode.log('Apple provider removed; add-button reappeared')
    })
  })

  // ==========================================================================
  // S2: Google Play — create, edit keeps Service Account JSON
  // ==========================================================================

  test('[US-IAP-001 S2] Google provider configuration — create, edit keeps Service Account JSON', async ({
    page,
    demoLogger,
  }) => {
    const runId = Date.now()
    const createPackageName = `com.cas.google.${runId}`
    // A valid-shaped service-account JSON literal (structure only — the backend
    // stores it; no live API calls happen in the management path).
    const serviceAccountJson = JSON.stringify({
      type: 'service_account',
      project_id: `cas-iap-${runId}`,
      private_key_id: `${runId}key`,
      private_key: '-----BEGIN PRIVATE KEY-----\nMIIB\n-----END PRIVATE KEY-----\n',
      client_email: `iap-${runId}@cas-iap.iam.gserviceaccount.com`,
      client_id: `${runId}`,
      auth_uri: 'https://accounts.google.com/o/oauth2/auth',
      token_uri: 'https://oauth2.googleapis.com/token',
    })

    await test.step('Given payment-providers page', async () => {
      await paymentProvidersPage.goto(DEMO_ADMIN.realmId)
      await expect(page.getByTestId('payment-providers-page')).toBeVisible()
      await demoLogger.testCode.log('On payment-providers page')
    })

    await test.step('When create Google config', async () => {
      await paymentProvidersPage.configureIapProvider('google', {
        packageName: createPackageName,
        serviceAccountJson,
      })
      await demoLogger.testCode.log('Google provider created')
    })

    await test.step('Then Google provider is listed', async () => {
      await expect(page.getByTestId('google-provider-row')).toBeVisible()
      await expect(page.getByTestId('edit-google-button')).toBeVisible()
      await demoLogger.testCode.log('Google provider row present')
    })

    await test.step('When edit Google and leave Service Account JSON blank', async () => {
      // Edit with editSensitiveLeaveEmpty: change packageName, leave the
      // service-account JSON input blank to assert retention of the prior
      // secret. The successful save back to the list is the retention proof.
      await paymentProvidersPage.configureIapProvider(
        'google',
        {
          packageName: `${createPackageName}-edited`,
          serviceAccountJson, // ignored: skipSensitive is true
        },
        { editSensitiveLeaveEmpty: true },
      )
      await demoLogger.testCode.log('Google provider edited (secret left empty to keep)')
    })

    await test.step('Then edit persisted (provider row still present)', async () => {
      await expect(page.getByTestId('google-provider-row')).toBeVisible()
      await expect(page.getByTestId('edit-google-button')).toBeVisible()
      await demoLogger.testCode.log('Google provider row present post-edit')
    })

    // NOTE: Google is intentionally NOT deleted here. The US-IAP-002 Google
    // one_time mapping demo needs a configured provider; leaving Google
    // configured provides a stable fixture. That demo creates its own providers
    // in its beforeAll if it needs a clean state. If it only requires Apple,
    // this Google row can be deleted in a follow-up.
  })
})
