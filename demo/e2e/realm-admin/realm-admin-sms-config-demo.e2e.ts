/** US-PL-004: channel configuration gates phone login and survives a real save. */
import { test, expect } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { REALM_ADMINS, createBearerApiContext } from '../helpers/auth'
import { LoginPage } from '../pages/login-page'
import { SettingsPage } from '../pages/settings-page'
import { SmsLoginPage } from '../pages/sms-login-page'
import { disablePhoneLoginAndClearChannel, getPhoneLoginConfig } from '../helpers/phone-login-setup'
import { SELECTORS } from '../selectors'

const REALM_ID = 'realm-001'

/** Read the realm's phone-login config with the admin's post-login token. */
async function readPhoneLoginConfig(adminLogin: LoginPage) {
  const api = await createBearerApiContext(adminLogin.getAccessToken())
  try {
    return await getPhoneLoginConfig(api, REALM_ID)
  } finally {
    await api.dispose()
  }
}

test.describe('[Realm Admin] SMS channel and phone-login configuration', () => {
  test.beforeEach(async ({ page }) => {
    await verifyTestEnvironment(page, {
      requiredRealms: [REALM_ID], requiredUsers: ['admin@realm-001.com'],
    })
  })

  test.afterEach(async ({ page, demoLogger }) => {
    await disablePhoneLoginAndClearChannel(page, demoLogger, REALM_ID)
  })

  test('US-PL-004: admin configures the SMS channel and toggles phone login', async ({ page, demoLogger }) => {
    const settings = new SettingsPage(page, demoLogger, REALM_ID)
    const adminLogin = new LoginPage(page, demoLogger)
    const publicLogin = new SmsLoginPage(page, demoLogger)
    const admin = REALM_ADMINS[REALM_ID]

    await test.step('Given: unconfigured channel and an authenticated Realm administrator', async () => {
      await disablePhoneLoginAndClearChannel(page, demoLogger, REALM_ID)
      await adminLogin.loginAsAdmin(admin.email, admin.password, REALM_ID)
      await settings.goto()
      await settings.switchToSmsTab()
    })

    await test.step('Unconfigured channel: fail closed and explain the disabled switch', async () => {
      await expect(settings.smsStatusBadge).toBeVisible()
      await expect.poll(() => settings.isSmsConfigured()).toBe(false)
      await expect(settings.smsMissingFields).toBeVisible()
      await expect(settings.phoneLoginSwitch).toBeDisabled()
      await page.locator(SELECTORS.phoneOtp.phoneLoginSwitchTooltipTrigger).hover()
      await expect(page.locator(SELECTORS.phoneOtp.phoneLoginSwitchTooltip)).toBeVisible()
    })

    await test.step('Configure: save the Aliyun channel through the real batch API', async () => {
      await settings.configureAliyunSms({
        accessKeyId: 'LTAI_demo_config_key',
        accessKeySecret: 'aliyun-demo-config-secret',
        signName: 'HeraldDemo', templateCode: 'SMS_demo_config_template',
      })
      await settings.saveSmsConfig()
      await expect.poll(() => settings.isSmsConfigured()).toBe(true)
      await expect(settings.phoneLoginSwitch).toBeEnabled()
    })

    await test.step('Test SMS: display persistent delivery feedback (synthetic delivery only)', async () => {
      await page.route('**/api/configs/sms/test', async route => {
        expect(route.request().postDataJSON().phone).toBe('13800138000')
        await route.fulfill({
          status: 200, contentType: 'application/json',
          body: JSON.stringify({ message: 'ok', maskedPhone: '138****8000' }),
        })
      })
      await settings.sendTestSms('13800138000')
      await expect(settings.smsTestSuccess).toBeVisible()
    })

    await test.step('Enable: persist the switch and expose the SMS login entry', async () => {
      await settings.enablePhoneLogin()
      await settings.savePhoneLoginConfig()
      expect((await readPhoneLoginConfig(adminLogin)).enabled).toBe(true)
      await publicLogin.open(REALM_ID)
      await expect(publicLogin.entry).toBeVisible()
    })

    await test.step('Disable: persist the switch and remove the public SMS entry', async () => {
      await adminLogin.loginAsAdmin(admin.email, admin.password, REALM_ID)
      await settings.goto()
      await settings.switchToSmsTab()
      await expect.poll(() => settings.isSmsConfigured()).toBe(true)
      await expect(settings.phoneLoginSwitch).toHaveAttribute('data-state', 'checked')
      await settings.disablePhoneLogin()
      await settings.savePhoneLoginConfig()
      expect((await readPhoneLoginConfig(adminLogin)).enabled).toBe(false)
      await publicLogin.open(REALM_ID)
      await expect(publicLogin.entry).toHaveCount(0)
    })
  })
})
