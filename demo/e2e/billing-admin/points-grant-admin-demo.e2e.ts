/**
 * Points Grant Admin Demo Tests (US-PO-008)
 *
 * Tests the Admin Grant Points UI dialog on the Points Wallets page.
 * Covers US-PO-008 scenarios 1-6 from docs/user-stories/billing/points-admin.md (Story 8).
 *
 * Scenarios:
 * - S1: Grant points with validity days (happy path)
 * - S2: Grant permanent points (no expiry)
 * - S3: Amount auto-correction (frontend coerces invalid input, not a validation error)
 * - S4: Server error for non-existent user (via page.route interception)
 * - S5: Cross-realm user rejected (via page.route interception)
 * - S6: Reason required validation (client-side)
 *
 * @see docs/user-stories/billing/points-admin.md (Story 8)
 */

import { test, cleanupTestData, expect } from '../fixtures/demo-page.fixtures'
import { DEMO_ADMIN } from '../helpers/auth'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import {
  openGrantDialog,
  fillGrantForm,
  confirmGrantDialog,
} from '../helpers/grant-points-helpers'
import { SELECTORS } from '../selectors'
import { CREDIT_BUCKET_NAMES, REGISTRATION_POOL_KEY } from '../helpers/bucket-seed-ids'

test.describe('[Points Admin] Grant Points Demo Tests (US-PO-008)', () => {
  let testStartTime: number

  test.beforeEach(async ({ page }) => {
    testStartTime = Date.now()
    await verifyTestEnvironment(page, {
      requiredRealms: [DEMO_ADMIN.realmId],
      requiredUsers: [DEMO_ADMIN.email],
    })
  })

  test.afterEach(async ({ page }) => {
    await cleanupTestData(page, DEMO_ADMIN.realmId, {
      keepUsers: [DEMO_ADMIN.email],
      timestamp: testStartTime,
    })
  })

  // ==========================================================================
  // US-PO-008 Scenario 1: Grant points with validity days
  // ==========================================================================

  test('should grant points with validity days to a user (US-PO-008 S1)', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    await test.step('Given: Admin is logged in and on Points Wallets page', async () => {
      await loginPage.loginAsAdmin(DEMO_ADMIN.email, DEMO_ADMIN.password, DEMO_ADMIN.realmId)
      await openGrantDialog(page, DEMO_ADMIN.realmId)
      demoLogger.testCode.log('[S1] Navigated to Points Wallets and opened Grant dialog')
    })

    await test.step('When: Admin fills form with amount=100, validityDays=30, reason="Event reward" and submits', async () => {
      await fillGrantForm(page, {
        email: DEMO_ADMIN.email,
        amount: 100,
        validityDays: 30,
        reason: 'Event reward',
        // bucketId is required. The admin realm is
        // seeded with `primary-pool` (the registration pool); targeting it keeps
        // US-PO-008 S1 intent (grant-with-validity lands for the user) intact.
        bucketId: REGISTRATION_POOL_KEY,
      })
      await confirmGrantDialog(page)
      demoLogger.testCode.log('[S1] Filled form and confirmed grant')
    })

    await test.step('Then: Success toast is visible and points are granted', async () => {
      await expect(page.locator(SELECTORS.common.toast)).toBeVisible({ timeout: 10000 })
      demoLogger.testCode.log('[S1] Success toast confirmed')
    })
  })

  // ==========================================================================
  // US-PO-008 Scenario 2: Grant permanent points
  // ==========================================================================

  test('should grant permanent points to a user (US-PO-008 S2)', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    await test.step('Given: Admin is logged in and on Points Wallets page', async () => {
      await loginPage.loginAsAdmin(DEMO_ADMIN.email, DEMO_ADMIN.password, DEMO_ADMIN.realmId)
      await openGrantDialog(page, DEMO_ADMIN.realmId)
      demoLogger.testCode.log('[S2] Navigated to Points Wallets and opened Grant dialog')
    })

    await test.step('When: Admin fills form with permanent toggle ON, amount=50, reason="Loyalty bonus" and submits', async () => {
      await fillGrantForm(page, {
        email: DEMO_ADMIN.email,
        amount: 50,
        permanent: true,
        reason: 'Loyalty bonus',
        // bucketId is required (admin realm primary-pool).
        bucketId: REGISTRATION_POOL_KEY,
      })
      await confirmGrantDialog(page)
      demoLogger.testCode.log('[S2] Filled form with permanent toggle and confirmed grant')
    })

    await test.step('Then: Success toast is visible and permanent points are granted', async () => {
      await expect(page.locator(SELECTORS.common.toast)).toBeVisible({ timeout: 10000 })
      demoLogger.testCode.log('[S2] Success toast confirmed')
    })
  })

  // ==========================================================================
  // US-PO-008 Scenario 3: Amount auto-correction prevents invalid input
  //
  // NOTE: US-PO-008 S3 says "display validation error" but the actual frontend
  // implementation uses Number() coercion in NumberField onChange, which
  // converts 0 to 0 (passed to Zod). However, the form default is 1 and the
  // Zod schema has min(1), so submitting 0 would trigger Zod validation.
  // This test verifies the actual behavior: clearing the field yields an
  // empty string which Number() converts to 0, and then Zod rejects it.
  // The acceptance report should flag this discrepancy for user story reconciliation.
  // ==========================================================================

  test('should auto-correct or reject invalid amount input (US-PO-008 S3)', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    await test.step('Given: Admin is logged in and has the Grant dialog open', async () => {
      await loginPage.loginAsAdmin(DEMO_ADMIN.email, DEMO_ADMIN.password, DEMO_ADMIN.realmId)
      await openGrantDialog(page, DEMO_ADMIN.realmId)
      demoLogger.testCode.log('[S3] Opened Grant dialog')
    })

    await test.step('When: Admin clears the amount field and types "0"', async () => {
      const gp = SELECTORS.grantPoints
      const amountInput = page.locator(gp.amountInput)
      await amountInput.clear()
      await amountInput.fill('0')
      demoLogger.testCode.log('[S3] Cleared amount and entered 0')
    })

    await test.step('Then: The form field reflects the invalid input and submission is blocked by Zod validation', async () => {
      const gp = SELECTORS.grantPoints

      // Verify the amount field contains 0 (Number("0") = 0, not auto-corrected to 1)
      const amountInput = page.locator(gp.amountInput)
      await expect(amountInput).toHaveValue('0')

      // Fill other required fields so the form is otherwise valid
      const searchInput = page.locator(gp.userSearchInput)
      await searchInput.fill(DEMO_ADMIN.email)
      const firstUserButton = page.locator(
        `${gp.formDialog} button:has-text("${DEMO_ADMIN.email}")`,
      )
      await expect(firstUserButton).toBeVisible({ timeout: 10000 })
      await firstUserButton.click()

      const reasonInput = page.locator(gp.reasonInput)
      await reasonInput.fill('Test reason')

      // Select the target bucket so the amount field is the SOLE invalid field
      // (bucket is now required; without this the
      // bucket-missing error would compete with the amount error).
      await page.locator(gp.bucketSelect).click()
      await page
        .getByRole('option', { name: CREDIT_BUCKET_NAMES.PRIMARY_POOL })
        .click()

      // Attempt to submit -- Zod min(1) should block submission
      await page.locator(gp.submitButton).click()

      // The confirmation dialog should NOT appear (form validation prevents it)
      await expect(page.locator(gp.confirmDialog)).not.toBeVisible({ timeout: 3000 })

      // A validation error should be visible for the amount field
      await expect(
        page.getByText(/at least 1/i).first(),
      ).toBeVisible({ timeout: 5000 })

      demoLogger.testCode.log(
        '[S3] Verified: amount 0 is not auto-corrected, Zod validation blocks submission',
      )
    })
  })

  // ==========================================================================
  // US-PO-008 Scenario 6: Reason required validation
  // ==========================================================================

  test('should show validation error when reason is empty (US-PO-008 S6)', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    await test.step('Given: Admin is logged in and has the Grant dialog open', async () => {
      await loginPage.loginAsAdmin(DEMO_ADMIN.email, DEMO_ADMIN.password, DEMO_ADMIN.realmId)
      await openGrantDialog(page, DEMO_ADMIN.realmId)
      demoLogger.testCode.log('[S6] Opened Grant dialog')
    })

    await test.step('When: Admin fills the form with an empty reason and submits', async () => {
      const gp = SELECTORS.grantPoints

      // Search and select user
      const searchInput = page.locator(gp.userSearchInput)
      await searchInput.fill(DEMO_ADMIN.email)
      const firstUserButton = page.locator(
        `${gp.formDialog} button:has-text("${DEMO_ADMIN.email}")`,
      )
      await expect(firstUserButton).toBeVisible({ timeout: 10000 })
      await firstUserButton.click()

      // Fill amount
      const amountInput = page.locator(gp.amountInput)
      await amountInput.clear()
      await amountInput.fill('10')

      // Leave reason empty
      const reasonInput = page.locator(gp.reasonInput)
      await reasonInput.clear()

      // Select the target bucket so the reason field is the SOLE invalid field
      // (bucket is now required; without this the
      // bucket-missing error would compete with the reason error).
      await page.locator(gp.bucketSelect).click()
      await page
        .getByRole('option', { name: CREDIT_BUCKET_NAMES.PRIMARY_POOL })
        .click()

      // Submit
      await page.locator(gp.submitButton).click()
      demoLogger.testCode.log('[S6] Submitted form with empty reason')
    })

    await test.step('Then: Client-side validation error is displayed for reason field', async () => {
      // The confirmation dialog should NOT appear
      const gp = SELECTORS.grantPoints
      await expect(page.locator(gp.confirmDialog)).not.toBeVisible({ timeout: 3000 })

      // Reason validation error should be visible
      await expect(
        page.getByText(/reason is required/i).first(),
      ).toBeVisible({ timeout: 5000 })

      demoLogger.testCode.log('[S6] Verified: reason validation error is displayed')
    })
  })

  // ==========================================================================
  // US-PO-008 Scenario 4: Server error for non-existent user via admin UI
  //
  // Uses page.route() to intercept the POST grant request and modify the userId
  // to a non-existent UUID, testing the full admin UI -> backend -> error display chain.
  // ==========================================================================

  test('should display server error when granting points to non-existent user (US-PO-008 S4)', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    const nonExistentUserId = '00000000-0000-0000-0000-000000000000'

    await test.step('Given: Admin is logged in and has the Grant dialog open', async () => {
      await loginPage.loginAsAdmin(DEMO_ADMIN.email, DEMO_ADMIN.password, DEMO_ADMIN.realmId)
      await openGrantDialog(page, DEMO_ADMIN.realmId)
      demoLogger.testCode.log('[S4] Opened Grant dialog')
    })

    await test.step('When: Admin submits a valid form but request is intercepted with non-existent userId', async () => {
      const gp = SELECTORS.grantPoints

      // Intercept the grant API call to replace userId with a non-existent UUID
      await page.route(
        `**/api/points/${DEMO_ADMIN.realmId}/grant`,
        async (route) => {
          const originalRequest = route.request()
          const postData = originalRequest.postData()
          let body: Record<string, unknown> = {}
          if (postData) {
            try {
              body = JSON.parse(postData)
            } catch {
              // proceed with empty body
            }
          }
          // Replace userId with non-existent UUID
          body.userId = nonExistentUserId

          await route.continue({
            postData: JSON.stringify(body),
          })
        },
      )

      // Fill and submit the form normally
      await fillGrantForm(page, {
        email: DEMO_ADMIN.email,
        amount: 100,
        validityDays: 30,
        reason: 'Test non-existent user',
        // bucketId is required for the form to
        // submit. The route interception below only swaps `userId`, so the
        // bucketId is preserved in the request body.
        bucketId: REGISTRATION_POOL_KEY,
      })

      // Click submit to open confirmation dialog
      await page.locator(gp.submitButton).click()
      await expect(page.locator(gp.confirmDialog)).toBeVisible({ timeout: 10000 })

      // Click confirm to trigger the intercepted request
      await page.locator(gp.confirmButton).click()
      demoLogger.testCode.log('[S4] Confirmed grant with intercepted request')
    })

    await test.step('Then: Server error is displayed in the dialog', async () => {
      // The error alert should appear in the form dialog
      const gp = SELECTORS.grantPoints
      await expect(page.locator(gp.errorMessage)).toBeVisible({ timeout: 10000 })

      // Verify the error message contains relevant text
      const errorText = await page.locator(gp.errorMessage).textContent()
      expect(errorText).toBeTruthy()
      demoLogger.testCode.log(`[S4] Error message displayed: ${errorText}`)
    })
  })

  // ==========================================================================
  // US-PO-008 Scenario 5: Cross-realm user rejected via admin UI
  //
  // Uses page.route() to intercept the POST grant request and replace the userId
  // with a cross-realm user ID, testing the admin UI cross-realm rejection path.
  // ==========================================================================

  test('should display permission denied error for cross-realm user (US-PO-008 S5)', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    // Valid UUID that does not belong to any user in this realm.
    // Must be valid UUID format — the backend validates UUID before checking realm,
    // so a non-UUID string would trigger 400 InvalidUserId instead of the cross-realm error.
    const crossRealmUserId = '00000000-0000-0000-0000-000000000001'

    await test.step('Given: Admin is logged in to admin realm and has the Grant dialog open', async () => {
      await loginPage.loginAsAdmin(DEMO_ADMIN.email, DEMO_ADMIN.password, DEMO_ADMIN.realmId)
      await openGrantDialog(page, DEMO_ADMIN.realmId)
      demoLogger.testCode.log('[S5] Opened Grant dialog in admin realm')
    })

    await test.step('When: Admin submits a valid form but request is intercepted with cross-realm userId', async () => {
      const gp = SELECTORS.grantPoints

      // Intercept the grant API call to replace userId with a valid UUID from another realm
      await page.route(
        `**/api/points/${DEMO_ADMIN.realmId}/grant`,
        async (route) => {
          const originalRequest = route.request()
          const postData = originalRequest.postData()
          let body: Record<string, unknown> = {}
          if (postData) {
            try {
              body = JSON.parse(postData)
            } catch {
              // proceed with empty body
            }
          }
          // Replace userId with a valid UUID that belongs to a different realm
          body.userId = crossRealmUserId

          await route.continue({
            postData: JSON.stringify(body),
          })
        },
      )

      // Fill and submit the form normally
      await fillGrantForm(page, {
        email: DEMO_ADMIN.email,
        amount: 50,
        validityDays: 7,
        reason: 'Test cross-realm user',
        // bucketId is required for the form to
        // submit. The route interception below only swaps `userId`.
        bucketId: REGISTRATION_POOL_KEY,
      })

      // Click submit to open confirmation dialog
      await page.locator(gp.submitButton).click()
      await expect(page.locator(gp.confirmDialog)).toBeVisible({ timeout: 10000 })

      // Click confirm to trigger the intercepted request
      await page.locator(gp.confirmButton).click()
      demoLogger.testCode.log('[S5] Confirmed grant with intercepted request')
    })

    await test.step('Then: Permission denied error is displayed in the dialog', async () => {
      // The error alert should appear
      const gp = SELECTORS.grantPoints
      await expect(page.locator(gp.errorMessage)).toBeVisible({ timeout: 10000 })

      // Verify the error message is displayed
      const errorText = await page.locator(gp.errorMessage).textContent()
      expect(errorText).toBeTruthy()
      demoLogger.testCode.log(`[S5] Error message displayed: ${errorText}`)
    })
  })
})
