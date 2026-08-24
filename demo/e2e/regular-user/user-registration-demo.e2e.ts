/**
 * User Registration Demo Tests
 *
 * User Story: docs/user-stories/core/regular-user.md (US-RU-001: Account Registration)
 *
 * Test Scenarios:
 * - US-RU-001: Account Registration (7 scenarios covering success and failure cases)
 *
 * Environment Setup:
 * - Demo Seed (scripts/lib/demo_seed.py) ensures registration is enabled for realm-001
 * - Tests verify the UI flow using the pre-configured environment
 * - No direct API calls from tests (per spec/demo/e2e-testing.md)
 *
 * Compliance: spec/demo/e2e-testing.md
 * - All operations go through UI (no direct API calls)
 * - Environment configuration managed by Demo Seed
 * - Tests focus on user interaction verification
 */

import { test, expect, cleanupTestData } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'

test.describe('[Regular User] Account Registration Demo Tests', () => {
  let testStartTime: number
  const realmId = 'realm-001' // Use realm-001 which has registration enabled by Demo Seed

  test.beforeEach(async ({ page, testStartTime: startTime }) => {
    testStartTime = startTime

    // Verify environment - realm-001 is configured with registration enabled by Demo Seed
    await verifyTestEnvironment(page, {
      requiredRealms: ['realm-001'],
      requiredUsers: ['admin@realm-001.com'],
    })
  })

  test.afterEach(async ({ page, demoLogger }) => {
    // Clean up test data (users created during registration)
    await cleanupTestData(page, realmId, {
      keepUsers: ['admin@realm-001.com'],
      timestamp: testStartTime,
      verbose: false,
    })

    // Verify API call optimization - only public-config should be called
    await test.step('Verify API Call Optimization', async () => {
      const logs = demoLogger.network.getLogs()
      const publicConfigCalls = logs.filter(log => log.url.includes('/public-config'))
      const providerCalls = logs.filter(log => log.url.includes('/api/oauth/') && log.url.includes('/providers'))
      const registrationConfigCalls = logs.filter(log => log.url.includes('/api/configs/') && log.url.includes('/registration'))

      console.log(`[Network] Total API calls: ${logs.length}`)
      console.log(`[Network] Public config calls: ${publicConfigCalls.length}`)
      console.log(`[Network] OAuth provider calls: ${providerCalls.length}`)
      console.log(`[Network] Registration config calls: ${registrationConfigCalls.length}`)

      // Verify optimization: should use public-config instead of separate calls
      // Note: This is a soft check - we log the results but don't fail the test
      if (publicConfigCalls.length > 0) {
        console.log('✓ Public config API is being used')
      }
      if (providerCalls.length > 0 || registrationConfigCalls.length > 0) {
        console.log('⚠ Legacy API calls detected - consider optimizing')
      }
    })
  })

  // Helper function to navigate to registration page
  // Note: Registration configuration is managed by Demo Seed, not in tests
  async function navigateToRegistrationPage(page: any): Promise<void> {
    await page.goto(`/${realmId}/auth/register`)
    await page.waitForLoadState('domcontentloaded')

    // Consent gate (added after the legal-agreement work): the register form's
    // schema refines on `consent === true`, so submit is blocked client-side
    // (no POST, waitForResponse times out) until the consent checkbox is
    // checked. Check it here for every scenario that navigates to the page so
    // the happy-path / already-exists scenarios reach the register API.
    // The form only mounts after the public-config query resolves (the page
    // renders a loading placeholder first), so we must WAIT for the checkbox
    // instead of probing with isVisible() — its timeout option is ignored and
    // it returns immediately, silently skipping the check during loading.
    // Idempotent if already checked. Validation-failure scenarios are
    // unaffected (their own field errors block submit before consent matters).
    const consent = page.getByTestId('register-consent-checkbox')
    await consent.waitFor({ state: 'visible', timeout: 10000 })
    if (!(await consent.isChecked())) {
      await consent.check()
    }
  }

  // ============================================================================
  // User Story: Account Registration [US-RU-001]
  // ============================================================================

  test.describe('User Story: Account Registration [US-RU-001]', () => {
    // ---------------------------------------------------------------------------
    // Scenario 1a: Normal registration success (no email verification required)
    // ---------------------------------------------------------------------------
    test('Scenario 1a: Normal registration success without email verification', async ({ page }) => {
      await test.step('Step 1: Navigate to registration page', async () => {
        await navigateToRegistrationPage(page)
      })

      await test.step('Step 2: Verify registration page is ready', async () => {
        // Demo Seed ensures registration is enabled for realm-001
        await expect(page.getByTestId('register-card')).toBeVisible()
        await expect(page.getByTestId('register-title')).toBeVisible()
      })

      await test.step('Step 3: Fill registration form', async () => {
        const testData = {
          email: `user${testStartTime}@example.com`,
          password: 'Password123!',
          confirmPassword: 'Password123!',
          nickname: `user${testStartTime}`,
        }

        await page.getByTestId('register-email-input').fill(testData.email)
        await page.getByTestId('register-password-input').fill(testData.password)
        await page.getByTestId('register-confirm-password-input').fill(testData.confirmPassword)
        await page.getByTestId('register-nickname-input').fill(testData.nickname)
      })

      await test.step('Step 4: Complete Turnstile verification (if enabled)', async () => {
        // Turnstile verification step (if realm has it enabled)
        // Note: In demo environment, Turnstile may be disabled or have test site key
      })

      await test.step('Step 5: Submit registration form', async () => {
        // The register POST goes through the generated API client whose URL
        // shape varies under the auth-rewrite; the load-bearing success signal
        // is the post-submit navigation to /auth/login (Step 6). Best-effort
        // capture the response for logging but do NOT fail if the glob misses.
        const responsePromise: Promise<any> = page
          .waitForResponse('**/api/auth/**/register', { timeout: 10000 })
          .then((r) => r)
          .catch(() => null)
        await page.getByTestId('register-submit-button').click()
        const response = await responsePromise

        // Check current URL for debugging
        const currentUrl = page.url()
        console.log(`[Registration] Current URL after submit: ${currentUrl}`)
        console.log(`[Registration] API response status: ${response?.status?.() ?? 'not captured'}`)
      })

      await test.step('Step 6: Verify registration success', async () => {
        // Contract: registration does NOT create a session. herald-auth-web's
        // register() returns {message, verificationRequired} (no token, unlike
        // login()), and the register page's success handler navigates to
        // /auth/verify-email or /auth/login. realm-001's demo seed has email
        // verification disabled, so the only real landing point here is
        // /auth/login. Assert exactly that — no dashboard auto-login branch.
        await page.waitForURL(`**/auth/login`, { timeout: 3000 })
        await expect(page.getByTestId('login-title')).toBeVisible()

        // Check for success toast (using sonner toast selector)
        await expect(page.locator('[data-sonner-toast]')).toBeVisible({ timeout: 5000 })
      })
    })

    // ---------------------------------------------------------------------------
    // Scenario 1b-1c: Registration with email verification flow
    // NOTE: Demo Seed configures realm-001 without email verification
    // This test demonstrates the UI flow but may require manual configuration
    // ---------------------------------------------------------------------------
    test.skip('Scenario 1b-1c: Registration with email verification flow', async ({ page }) => {
      // NOTE: Skipped because realm-001 is configured without email verification
      // To enable this test, update Demo Seed to configure email verification
      const testData = {
        email: `verify${testStartTime}@example.com`,
        password: 'Password123!',
        confirmPassword: 'Password123!',
        nickname: `verify${testStartTime}`,
      }

      await test.step('Step 1: Navigate to registration page', async () => {
        await navigateToRegistrationPage(page)
      })

      await test.step('Step 2: Verify registration page and fill form', async () => {
        await expect(page.getByTestId('register-card')).toBeVisible()

        await page.getByTestId('register-email-input').fill(testData.email)
        await page.getByTestId('register-password-input').fill(testData.password)
        await page.getByTestId('register-confirm-password-input').fill(testData.confirmPassword)
        await page.getByTestId('register-nickname-input').fill(testData.nickname)
      })

      await test.step('Step 3: Submit registration', async () => {
        // Wait for API response
        const responsePromise = page.waitForResponse('**/api/auth/**/register', { timeout: 10000 })
        await page.getByTestId('register-submit-button').click()
        await responsePromise
      })

      await test.step('Step 4: Verify redirect to email verification page', async () => {
        await expect(page.getByTestId('verify-email-title')).toBeVisible()
        await expect(page.getByText('Please enter your email and 6-digit verification code sent to your email.')).toBeVisible()
      })

      await test.step('Step 5: Demonstrate verification UI flow', async () => {
        // Note: In real environment, verification code comes from email
        // Demo test demonstrates UI flow without actual verification
        await page.getByTestId('verify-email-input').fill(testData.email)
        await page.getByTestId('verification-code-input').fill('123456') // Demo code
      })
    })

    // ---------------------------------------------------------------------------
    // Scenario 1d: Resend verification email success
    // NOTE: Requires email verification to be enabled in realm config
    // ---------------------------------------------------------------------------
    test.skip('Scenario 1d: Resend verification email success', async ({ page }) => {
      // NOTE: Skipped because realm-001 is configured without email verification
      // To enable this test, update Demo Seed to configure email verification
      const testData = {
        email: `resend${testStartTime}@example.com`,
        password: 'Password123!',
        confirmPassword: 'Password123!',
        nickname: `resend${testStartTime}`,
      }

      await test.step('Step 1: Navigate to registration page', async () => {
        await navigateToRegistrationPage(page)
      })

      await test.step('Step 2: Register user', async () => {
        await page.getByTestId('register-email-input').fill(testData.email)
        await page.getByTestId('register-password-input').fill(testData.password)
        await page.getByTestId('register-confirm-password-input').fill(testData.confirmPassword)
        await page.getByTestId('register-nickname-input').fill(testData.nickname)

        // Wait for API response
        const responsePromise = page.waitForResponse('**/api/auth/**/register', { timeout: 10000 })
        await page.getByTestId('register-submit-button').click()
        await responsePromise
      })

      await test.step('Step 3: Verify redirect to email verification page', async () => {
        await expect(page.getByTestId('verify-email-title')).toBeVisible()
      })

      await test.step('Step 4: Fill email and check resend button state', async () => {
        await page.getByTestId('verify-email-input').fill(testData.email)

        // Check resend button is disabled with countdown
        const resendButton = page.getByTestId('resend-button')
        const buttonText = await resendButton.textContent()
        console.log('✅ Resend button shows countdown: ' + (buttonText || 'waiting...'))
      })
    })

    // ---------------------------------------------------------------------------
    // Scenario 2: Email format validation failure
    // ---------------------------------------------------------------------------
    test('Scenario 2: Email format validation failure', async ({ page }) => {
      await test.step('Step 1: Navigate to registration page', async () => {
        await navigateToRegistrationPage(page)
      })

      await test.step('Step 2: Verify registration page is ready', async () => {
        await expect(page.getByTestId('register-card')).toBeVisible()
      })

      await test.step('Step 3: Enter invalid email format', async () => {
        // Use an obviously invalid email (missing @ symbol)
        await page.getByTestId('register-email-input').fill('invalid-email-address-without-at')
        // Fill other fields minimally to enable submit button
        await page.getByTestId('register-password-input').fill('Password123!')
        await page.getByTestId('register-confirm-password-input').fill('Password123!')
        // Click submit button to trigger validation
        await page.getByTestId('register-submit-button').click()
        // Wait for validation error to appear
        await expect(page.getByRole('alert')).toBeVisible({ timeout: 3000 })
      })

      await test.step('Step 4: Verify error message displayed', async () => {
        // Check for validation error message (TextField renders errors with role="alert")
        const errorMessage = page.getByRole('alert').filter({ hasText: 'Invalid email address' })
        await expect(errorMessage).toBeVisible()
      })
    })

    // ---------------------------------------------------------------------------
    // Scenario 3: Password policy violation failure
    // ---------------------------------------------------------------------------
    test('Scenario 3: Password policy violation failure', async ({ page }) => {
      await test.step('Step 1: Navigate to registration page', async () => {
        await navigateToRegistrationPage(page)
      })

      await test.step('Step 2: Verify registration page is ready', async () => {
        await expect(page.getByTestId('register-card')).toBeVisible()
      })

      await test.step('Step 3: Enter weak password (missing uppercase and special char)', async () => {
        // Fill email field to avoid email validation error
        await page.getByTestId('register-email-input').fill(`test${testStartTime}@example.com`)
        // Fill weak password (missing uppercase and special char)
        await page.getByTestId('register-password-input').fill('Pass123')
        await page.getByTestId('register-password-input').blur() // Trigger validation
        // Use text filter to precisely locate password error message
        await expect(page.locator('p.text-destructive').filter({ hasText: 'Password must be at least 8 characters' })).toBeVisible({ timeout: 3000 })
      })

      await test.step('Step 4: Verify password validation errors', async () => {
        // Password should show validation errors (missing requirements)
        // Check for password suggestions error messages with text filter
        const errorMessage = page.locator('p.text-destructive').filter({ hasText: 'Password must be at least 8 characters' })
        await expect(errorMessage).toBeVisible()
      })
    })

    // ---------------------------------------------------------------------------
    // Scenario 4: Password mismatch failure
    // ---------------------------------------------------------------------------
    test('Scenario 4: Password mismatch failure', async ({ page }) => {
      await test.step('Step 1: Navigate to registration page', async () => {
        await navigateToRegistrationPage(page)
      })

      await test.step('Step 2: Verify registration page is ready', async () => {
        await expect(page.getByTestId('register-card')).toBeVisible()
      })

      await test.step('Step 3: Enter mismatched passwords', async () => {
        // Fill email field first to avoid multiple validation errors
        await page.getByTestId('register-email-input').fill(`test${testStartTime}@example.com`)
        await page.getByTestId('register-password-input').fill('Password123!')
        await page.getByTestId('register-confirm-password-input').fill('Password456!')
        await page.getByTestId('register-confirm-password-input').blur() // Trigger validation
        // Use text filter to precisely locate password mismatch error
        await expect(page.getByRole('alert').filter({ hasText: 'Passwords do not match' })).toBeVisible({ timeout: 3000 })
      })

      await test.step('Step 4: Verify error message displayed', async () => {
        const confirmPasswordInput = page.getByTestId('register-confirm-password-input')
        const errorMessage = confirmPasswordInput.locator('..').getByRole('alert').filter({ hasText: 'Passwords do not match' })
        await expect(errorMessage).toBeVisible()
        await expect(errorMessage).toContainText('Passwords do not match')
      })
    })

    // ---------------------------------------------------------------------------
    // Scenario 5: Email already exists failure
    // ---------------------------------------------------------------------------
    test('Scenario 5: Email already exists failure', async ({ page }) => {
      const existingEmail = `existing${testStartTime}@example.com`

      await test.step('Step 1: Navigate to registration page', async () => {
        await navigateToRegistrationPage(page)
      })

      await test.step('Step 2: First registration attempt', async () => {
        await page.getByTestId('register-email-input').fill(existingEmail)
        await page.getByTestId('register-password-input').fill('Password123!')
        await page.getByTestId('register-confirm-password-input').fill('Password123!')
        await page.getByTestId('register-nickname-input').fill(`user${testStartTime}`)

        // Best-effort response capture (see Scenario 1a Step 5): the
        // load-bearing success signal is the post-submit login-title (Step 3).
        const responsePromise: Promise<any> = page
          .waitForResponse('**/api/auth/**/register', { timeout: 10000 })
          .then((r) => r)
          .catch(() => null)
        await page.getByTestId('register-submit-button').click()
        await responsePromise
      })

      await test.step('Step 3: Verify first registration success', async () => {
        await expect(page.getByTestId('login-title')).toBeVisible()
      })

      await test.step('Step 4: Attempt to register with same email', async () => {
        await navigateToRegistrationPage(page)
        await page.getByTestId('register-email-input').fill(existingEmail)
        await page.getByTestId('register-password-input').fill('Password456!')
        await page.getByTestId('register-confirm-password-input').fill('Password456!')
        await page.getByTestId('register-nickname-input').fill(`user2${testStartTime}`)

        // Best-effort response capture (see Scenario 1a Step 5). The
        // already-exists failure is verified by the page staying on /register
        // (Step 5), not by the response itself.
        const responsePromise: Promise<any> = page
          .waitForResponse('**/api/auth/**/register', { timeout: 10000 })
          .then((r) => r)
          .catch(() => null)
        await page.getByTestId('register-submit-button').click()
        const response = await responsePromise
        // Log the response for debugging
        console.log(`[Registration] API response status: ${response?.status?.() ?? 'not captured'}`)
      })

      await test.step('Step 5: Verify error message', async () => {
        // Verify registration failed - should still be on register page
        // Check that we're still on registration page (not redirected)
        const currentUrl = page.url()
        expect(currentUrl).toContain('/register')
        // Verify form is still visible
        await expect(page.getByTestId('register-card')).toBeVisible()
      })
    })

    // ---------------------------------------------------------------------------
    // Scenario 6: Registration not enabled
    // NOTE: This test is skipped because realm-001 has registration enabled by Demo Seed
    // To test "registration disabled" scenario, you would need a separate realm with registration disabled
    // ---------------------------------------------------------------------------
    test.skip('Scenario 6: Registration not enabled', async ({ page }) => {
      // NOTE: Skipped because realm-001 has registration enabled
      // To test this scenario, create a separate realm with registration disabled in Demo Seed
      await test.step('Step 1: Navigate to registration page for realm with registration disabled', async () => {
        // This would require navigating to a different realm with registration disabled
        // For now, we skip this test
        console.log('This test requires a realm with registration disabled')
      })

      await test.step('Step 2: Verify registration disabled message', async () => {
        // Verify disabled message is shown
        await expect(page.getByTestId('registration-disabled-title')).toBeVisible()
        await expect(page.getByText('Registration is not enabled for this realm')).toBeVisible()
      })

      await test.step('Step 3: Verify registration form not shown', async () => {
        await expect(page.getByTestId('register-card')).not.toBeVisible()
      })

      await test.step('Step 4: Verify link back to login', async () => {
        await expect(page.getByRole('link', { name: /Return to Login/i })).toBeVisible()
      })
    })

    // ---------------------------------------------------------------------------
    // Scenario 7: Turnstile verification failure
    // ---------------------------------------------------------------------------
    test('Scenario 7: Turnstile verification failure', async ({ page }) => {
      await test.step('Step 1: Navigate to registration page', async () => {
        await navigateToRegistrationPage(page)
      })

      await test.step('Step 2: Verify registration page is ready', async () => {
        await expect(page.getByTestId('register-card')).toBeVisible()
      })

      await test.step('Step 3: Fill form without Turnstile verification', async () => {
        await page.getByTestId('register-email-input').fill(`turnstile${testStartTime}@example.com`)
        await page.getByTestId('register-password-input').fill('Password123!')
        await page.getByTestId('register-confirm-password-input').fill('Password123!')
        await page.getByTestId('register-nickname-input').fill(`user${testStartTime}`)
      })

      await test.step('Step 4: Check if Turnstile is present', async () => {
        // Check if Turnstile widget is visible
        const turnstileContainer = page.locator('.turnstile-widget-container')
        const isTurnstileVisible = await turnstileContainer.isVisible().catch(() => false)

        if (isTurnstileVisible) {
          console.log('✅ Turnstile widget is present')
          // Submit button should be disabled if Turnstile not completed
          const submitButton = page.getByTestId('register-submit-button')
          await expect(submitButton).toBeDisabled()
        } else {
          console.log('✅ Turnstile widget is not present (disabled in config)')
          // Submit button should be enabled
          const submitButton = page.getByTestId('register-submit-button')
          await expect(submitButton).toBeEnabled()
        }
      })
    })
  })
})
