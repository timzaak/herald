/** US-PL-002: SMS login authenticates bound users and never registers unknown phones. */
import { test, expect } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { enablePhoneLoginForRealm, disablePhoneLoginAndClearChannel } from '../helpers/phone-login-setup'
import { injectSmsLoginCode, getSmsLoginCode, clearSmsLoginCode, closeSmsOtpRedis } from '../helpers/sms-otp-redis-helper'
import { bindUserPhoneByEmail, clearUserPhoneByEmail, closePhoneDbPool } from '../helpers/phone-db-helper'
import { SmsLoginPage } from '../pages/sms-login-page'
import { PhoneSecurityPage } from '../pages/phone-security-page'

const REALM_ID = 'realm-001'
const REGISTERED_EMAIL = 'user@realm-001.com'
const REGISTERED_PHONE = '13800138000'
const DEMO_CODE = '246810'

test.describe('[Regular User] SMS-OTP Login Demo Tests', () => {
  let unregisteredPhone: string | undefined

  test.beforeEach(async ({ page }) => {
    unregisteredPhone = undefined
    await verifyTestEnvironment(page, {
      requiredRealms: [REALM_ID],
      requiredUsers: ['admin@realm-001.com', REGISTERED_EMAIL],
    })
  })

  test.afterEach(async ({ page, demoLogger }) => {
    try {
      await disablePhoneLoginAndClearChannel(page, demoLogger, REALM_ID)
    } finally {
      await clearUserPhoneByEmail(REGISTERED_EMAIL, REALM_ID)
      await clearSmsLoginCode(REALM_ID, REGISTERED_PHONE)
      if (unregisteredPhone) await clearSmsLoginCode(REALM_ID, unregisteredPhone)
    }
  })

  test.afterAll(async () => {
    await closeSmsOtpRedis()
    await closePhoneDbPool()
  })

  test('US-PL-002: bound user logs in with SMS OTP code', async ({ page, demoLogger }) => {
    const login = new SmsLoginPage(page, demoLogger)
    await test.step('Given: phone login enabled and a verified phone binding seeded', async () => {
      await enablePhoneLoginForRealm(page, demoLogger, REALM_ID)
      expect(await bindUserPhoneByEmail(REGISTERED_EMAIL, REGISTERED_PHONE, REALM_ID)).not.toBeNull()
    })

    await test.step('Send: show the SMS code form with the masked recipient', async () => {
      // Only delivery is synthetic; verification and session issuance stay real.
      await page.route('**/api/auth/*/login/sms-otp/send', async route => {
        expect(route.request().postDataJSON().phone).toBe(REGISTERED_PHONE)
        await route.fulfill({
          status: 200, contentType: 'application/json',
          body: JSON.stringify({ message: 'ok', expiresInSeconds: 300, maskedPhone: '138****8000' }),
        })
      })
      await login.open(REALM_ID)
      await login.selectSms()
      expect((await login.send(REGISTERED_PHONE)).ok()).toBe(true)
      await expect(page.getByText('138****8000', { exact: false })).toBeVisible()
    })

    await test.step('Verify: consume the proof and restore an authenticated session on reload', async () => {
      await injectSmsLoginCode(REALM_ID, REGISTERED_PHONE, DEMO_CODE)
      expect(await getSmsLoginCode(REALM_ID, REGISTERED_PHONE)).toBe(DEMO_CODE)
      await login.enterCode(DEMO_CODE)
      await login.verify()
      await expect(page).toHaveURL(new RegExp(`/${REALM_ID}/user/profile(?:\\?|$)`))
      expect(await getSmsLoginCode(REALM_ID, REGISTERED_PHONE), 'The login proof must be consumed').toBeNull()
      const security = new PhoneSecurityPage(page, demoLogger)
      await security.open(REALM_ID)
      await expect(security.boundValue).toHaveText('138****8000')
    })
  })

  test('US-PL-002: unregistered phone is rejected with guidance, no code stored', async ({ page, demoLogger }) => {
    const login = new SmsLoginPage(page, demoLogger)
    unregisteredPhone = `139${String(Date.now()).slice(-8)}`
    await enablePhoneLoginForRealm(page, demoLogger, REALM_ID)
    await login.open(REALM_ID)
    await login.selectSms()
    // This branch must reach the backend: unknown phones never contact the SMS provider.
    const sent = await login.send(unregisteredPhone)
    expect(sent.status()).toBe(409)
    expect(await sent.json()).toMatchObject({ code: 'phone_not_registered' })
    await expect(login.notRegistered).toBeVisible()
    await expect(login.registerLink).toHaveAttribute('href', `/${REALM_ID}/auth/register`)
    expect(await getSmsLoginCode(REALM_ID, unregisteredPhone)).toBeNull()
  })

  test('US-PL-002-degradation: disabling phone login hides the entry', async ({ page, demoLogger }) => {
    const login = new SmsLoginPage(page, demoLogger)
    await enablePhoneLoginForRealm(page, demoLogger, REALM_ID)
    await login.open(REALM_ID)
    await expect(login.entry).toBeVisible()
    await disablePhoneLoginAndClearChannel(page, demoLogger, REALM_ID)
    await login.open(REALM_ID)
    await expect(login.entry).toHaveCount(0)
  })
})
