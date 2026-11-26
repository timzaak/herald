/**
 * Device Code Error Scenarios & Admin Config Toggle Demo Tests
 *
 * Test Coverage:
 * - Test 1: Invalid and Expired Device Code (US-DC-002 scenario 3)
 *   - Enter invalid user_code that was never issued
 *   - Verify error message displayed
 *   - Verify frontend filters invalid characters from input
 * - Test 2: User Denies Device Authorization (US-DC-002 scenario 5 + US-DC-003 scenario 5)
 *   - User denies device authorization
 *   - Verify UI shows denied text
 *   - Verify CLI receives access_denied error on token poll
 * - Test 3: Admin Enables and Disables Device Code Grant (US-DC-004 all scenarios)
 *   - Create Client App with device code grant enabled via form page
 *   - Verify deviceAuthorize succeeds
 *   - Edit app to disable device code grant via form page
 *   - Verify deviceAuthorize fails with unauthorized_client
 *   - Create Client App without device code grant (default disabled)
 *   - Verify deviceAuthorize fails
 *
 * API Exception: deviceAuthorize and deviceTokenPoll are CLI-side API endpoints
 * with no browser UI. Direct API calls are intentional.
 *
 * @see docs/user-stories/auth/device-code.md
 */

import { test, expect, cleanupTestData } from './fixtures/demo-page.fixtures'
import { DeviceVerificationPage } from './pages/device-page'
import { ClientAppsPage } from './pages/client-apps-page'
import {
  seedDeviceCodeClientApp,
  deviceAuthorize,
  deviceTokenPoll,
} from './helpers/device-api'
import { verifyTestEnvironment } from './helpers/environment-setup'
import { createBearerApiContext, DEMO_ADMIN } from './helpers/auth'

const DEVICE_CODE_GRANT_SWITCH = '[data-testid="device-code-grant-switch"]'

test.describe('[Device Code] Error Scenarios & Admin Config Toggle', () => {
  let testStartTime: number

  test.beforeEach(async ({ page, testStartTime: startTime }) => {
    testStartTime = startTime

    await verifyTestEnvironment(page, {
      requiredRealms: ['admin'],
      requiredUsers: ['admin@cas.com'],
    })
  })

  test.afterEach(async ({ page }) => {
    await cleanupTestData(page, DEMO_ADMIN.realmId, {
      timestamp: testStartTime,
    })
  })

  // ===========================================================================
  // Test 1: Invalid and Expired Device Code (US-DC-002 scenario 3)
  // ===========================================================================

  test('Invalid and Expired Device Code', async ({ page, loginPage, demoLogger }) => {
    const realmId = DEMO_ADMIN.realmId

    await test.step('Given: Admin is logged in', async () => {
      await loginPage.loginAsAdmin(DEMO_ADMIN.email, DEMO_ADMIN.password, realmId)
      console.log('Admin logged in')
    })

    const devicePage = new DeviceVerificationPage(page, demoLogger)

    await test.step('When: User navigates to device verification page', async () => {
      await devicePage.goto(realmId)
      console.log('Navigated to device verification page')
    })

    await test.step('And: User enters invalid device code (US-DC-002 scenario 3)', async () => {
      // "BCDFGHJK" is 8 valid chars but does not correspond to any real device_code
      await devicePage.enterCode('BCDFGHJK')
      console.log('Entered invalid user code: BCDFGHJK')
    })

    await test.step('Then: Verify error message displayed', async () => {
      await expect(devicePage.error).toBeVisible({ timeout: 10000 })
      const errorText = await devicePage.getErrorText()
      // Error message should indicate the code is invalid or not found
      const lowerError = errorText.toLowerCase()
      expect(
        lowerError.includes('not found') ||
        lowerError.includes('invalid') ||
        lowerError.includes('expired') ||
        lowerError.includes('不存在'),
        `Expected error text to contain "not found" or "invalid", got: "${errorText}"`,
      ).toBe(true)
      console.log(`Error message verified: "${errorText}"`)
    })

    await test.step('And: Verify frontend filters invalid characters', async () => {
      // Navigate back to the input page for a fresh test
      await devicePage.goto(realmId)

      // Type lowercase characters - the input should filter them out
      await expect(devicePage.codeInput).toBeVisible()
      await devicePage.codeInput.fill('abcdefgh')
      const filteredValue = await devicePage.codeInput.inputValue()
      // After filtering, none of the original lowercase chars should remain
      const invalidChars = 'abcdefgh'
      for (const ch of invalidChars) {
        expect(
          !filteredValue.includes(ch),
          `Lowercase "${ch}" should have been filtered out, but input is: "${filteredValue}"`,
        ).toBe(true)
      }
      console.log(`Input filtering verified: "abcdefgh" -> "${filteredValue}"`)
    })
  })

  // ===========================================================================
  // Test 2: User Denies Device Authorization (US-DC-002 scenario 5 + US-DC-003 scenario 5)
  // ===========================================================================

  test('User Denies Device Authorization', async ({ page, loginPage, demoLogger }) => {
    const realmId = DEMO_ADMIN.realmId

    let clientId: string

    await test.step('Given: Admin is logged in', async () => {
      await loginPage.loginAsAdmin(DEMO_ADMIN.email, DEMO_ADMIN.password, realmId)
      console.log('Admin logged in')
    })

    const apiContext = await createBearerApiContext(loginPage.getAccessToken())
    try {
    await test.step('And: Seed Client App with Device Code Grant enabled', async () => {
      const result = await seedDeviceCodeClientApp(apiContext, realmId)
      clientId = result.clientId
      console.log(`Seeded device code client app (clientId=${clientId})`)
    })

    let deviceCode: string
    let userCode: string

    await test.step('When: CLI initiates device authorization request', async () => {
      const response = await deviceAuthorize(undefined, realmId, clientId)
      expect(response.device_code).toBeDefined()
      expect(response.user_code).toBeDefined()
      deviceCode = response.device_code
      userCode = response.user_code
      console.log(`Device authorization requested (user_code=${userCode})`)
    })

    const devicePage = new DeviceVerificationPage(page, demoLogger)

    await test.step('And: User enters user_code on verification page', async () => {
      await devicePage.goto(realmId)
      await devicePage.enterCode(userCode)
      await devicePage.waitForVerified()
      console.log('User code entered and verified')
    })

    await test.step('When: User clicks "Deny" (US-DC-002 scenario 5)', async () => {
      await devicePage.deny()
      console.log('User denied the device authorization')
    })

    await test.step('Then: Verify denial result shown', async () => {
      const resultText = await devicePage.getResultText()
      const lowerResult = resultText.toLowerCase()
      expect(
        lowerResult.includes('denied') || lowerResult.includes('拒绝'),
        `Expected result text to contain "denied", got: "${resultText}"`,
      ).toBe(true)
      console.log(`Denial result verified: "${resultText}"`)
    })

    await test.step('Then: CLI receives access_denied error (US-DC-003 scenario 5)', async () => {
      const tokenResponse = await deviceTokenPoll(undefined, realmId, deviceCode)
      if ('error' in tokenResponse) {
        expect(tokenResponse.error).toBe('access_denied')
        console.log(`Token poll error verified: "${tokenResponse.error}"`)
      } else {
        throw new Error('Expected access_denied error but got a token response')
      }
    })
    } finally {
      await apiContext.dispose()
    }
  })

  // ===========================================================================
  // Test 3: Admin Enables and Disables Device Code Grant (US-DC-004)
  // ===========================================================================

  test('Admin Enables and Disables Device Code Grant', async ({ page, loginPage, demoLogger }) => {
    const realmId = DEMO_ADMIN.realmId
    const ts = testStartTime

    await test.step('Given: Admin is logged in', async () => {
      await loginPage.loginAsAdmin(DEMO_ADMIN.email, DEMO_ADMIN.password, realmId)
      console.log('Admin logged in')
    })

    const apiContext = await createBearerApiContext(loginPage.getAccessToken())
    try {
    const clientAppsPage = new ClientAppsPage(page, demoLogger)

    // -------------------------------------------------------------------------
    // US-DC-004 Scenario 1: Enable Device Code Grant
    // -------------------------------------------------------------------------

    let createdAppId: string
    let createdClientId: string

    await test.step('When: Admin creates Client App with Device Code Grant enabled (US-DC-004 scenario 1)', async () => {
      // Navigate to page
      await clientAppsPage.goto(realmId)

      // Open create form page
      await clientAppsPage.gotoCreatePage()

      // Fill Basic tab
      await clientAppsPage.fillBasicTab({
        clientId: `dc-grant-test-${ts}`,
        name: `DC Grant Test App ${ts}`,
        description: 'Test app for device code grant toggle',
      })

      // Fill Redirect URIs (required by Zod validation)
      await clientAppsPage.fillRedirectUrisTab([`https://dc-grant-${ts}.example.com/callback`])

      // Switch to Security tab and toggle device code grant ON
      await clientAppsPage.fillSecurityTab({
        deviceCodeGrantEnabled: true,
      })

      // Verify switch is ON
      const dcSwitch = page.locator(DEVICE_CODE_GRANT_SWITCH)
      await expect(dcSwitch).toHaveAttribute('aria-checked', 'true')
      console.log('Device code grant switch toggled ON')

      // Submit
      await clientAppsPage.submitForm()
      console.log('Client app created with device code grant enabled')

      // Navigate to list page to verify
      await clientAppsPage.goto(realmId)
      await clientAppsPage.waitForClientAppByName(`DC Grant Test App ${ts}`)
    })

    await test.step('Then: Get created Client App client_id', async () => {
      const response = await apiContext.get(`${process.env.BASE_URL || 'http://localhost:3000'}/api/client`)
      expect(response.ok()).toBe(true)
      const appsPayload = await response.json()
      const apps = Array.isArray(appsPayload) ? appsPayload : appsPayload.items
      const createdApp = apps.find((app: { name: string }) => app.name === `DC Grant Test App ${ts}`)
      expect(createdApp).toBeDefined()
      createdAppId = createdApp.id
      createdClientId = createdApp.clientId ?? createdApp.client_id
      console.log(`Found client app: id=${createdAppId}, client_id=${createdClientId}`)
    })

    await test.step('Then: deviceAuthorize succeeds (200, includes device_code)', async () => {
      const response = await deviceAuthorize(undefined, realmId, createdClientId)
      expect(response.device_code).toBeDefined()
      expect(response.user_code).toBeDefined()
      console.log(`deviceAuthorize succeeded for enabled app (user_code=${response.user_code})`)
    })

    // -------------------------------------------------------------------------
    // US-DC-004 Scenario 2: Disable Device Code Grant
    // -------------------------------------------------------------------------

    await test.step('When: Admin edits Client App and disables Device Code Grant (US-DC-004 scenario 2)', async () => {
      // Navigate to list
      await clientAppsPage.goto(realmId)

      // Open edit page by name
      await clientAppsPage.gotoEditPage(`DC Grant Test App ${ts}`)

      // Switch to Security tab
      await clientAppsPage.switchTab('security')

      // Toggle device code grant OFF
      const dcSwitch = page.locator(DEVICE_CODE_GRANT_SWITCH)
      await expect(dcSwitch).toBeVisible()
      await dcSwitch.click()
      // Verify switch is OFF
      await expect(dcSwitch).toHaveAttribute('aria-checked', 'false')
      console.log('Device code grant switch toggled OFF')

      // Submit changes
      await clientAppsPage.submitForm()
      console.log('Client app updated with device code grant disabled')
    })

    await test.step('Then: deviceAuthorize returns error (403 unauthorized_client)', async () => {
      try {
        await deviceAuthorize(undefined, realmId, createdClientId)
        throw new Error('Expected deviceAuthorize to fail for disabled client')
      } catch (err) {
        const message = (err as Error).message
        expect(message).toContain('403')
        console.log(`deviceAuthorize correctly failed: ${message}`)
      }
    })

    // -------------------------------------------------------------------------
    // US-DC-004 Scenario 3: Default State - New Client App has device code grant disabled
    // -------------------------------------------------------------------------

    let defaultAppId: string
    let defaultClientId: string

    await test.step('When: Admin creates Client App without enabling Device Code Grant (US-DC-004 scenario 3)', async () => {
      // Navigate to page
      await clientAppsPage.goto(realmId)

      // Open create form page
      await clientAppsPage.gotoCreatePage()

      // Fill Basic tab
      await clientAppsPage.fillBasicTab({
        clientId: `dc-grant-default-${ts}`,
        name: `DC Grant Default App ${ts}`,
        description: 'Test app with default device code grant (disabled)',
      })

      // Configure redirect URI for the default OAuth flow
      await clientAppsPage.fillRedirectUrisTab([`https://default-${ts}.example.com/callback`])

      // Switch to Security tab and verify default is OFF
      await clientAppsPage.switchTab('security')
      const dcSwitch = page.locator(DEVICE_CODE_GRANT_SWITCH)
      await expect(dcSwitch).toHaveAttribute('aria-checked', 'false')
      console.log('Device code grant is OFF by default (verified)')

      // Submit
      await clientAppsPage.submitForm()
      console.log('Client app created with default device code grant (disabled)')

      // Navigate to list page
      await clientAppsPage.goto(realmId)
      await clientAppsPage.waitForClientAppByName(`DC Grant Default App ${ts}`)
    })

    await test.step('Then: Get default Client App client_id', async () => {
      const response = await apiContext.get(`${process.env.BASE_URL || 'http://localhost:3000'}/api/client`)
      expect(response.ok()).toBe(true)
      const appsPayload = await response.json()
      const apps = Array.isArray(appsPayload) ? appsPayload : appsPayload.items
      const defaultApp = apps.find((app: { name: string }) => app.name === `DC Grant Default App ${ts}`)
      expect(defaultApp).toBeDefined()
      defaultAppId = defaultApp.id
      defaultClientId = defaultApp.clientId ?? defaultApp.client_id
      console.log(`Found default client app: id=${defaultAppId}, client_id=${defaultClientId}`)
    })

    await test.step('Then: deviceAuthorize fails (default disabled)', async () => {
      try {
        await deviceAuthorize(undefined, realmId, defaultClientId)
        throw new Error('Expected deviceAuthorize to fail for default-disabled client')
      } catch (err) {
        const message = (err as Error).message
        expect(message).toContain('403')
        console.log(`deviceAuthorize correctly failed for default app: ${message}`)
      }
    })
    } finally {
      await apiContext.dispose()
    }
  })
})
