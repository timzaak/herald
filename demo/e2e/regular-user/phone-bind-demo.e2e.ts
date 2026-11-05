/** US-PL-001: binding changes require reauth and proof of the new phone. */
import { test, expect } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { enablePhoneLoginForRealm, disablePhoneLoginAndClearChannel } from '../helpers/phone-login-setup'
import { injectSmsBindCode, getSmsBindCode, clearSmsBindCode, closeSmsOtpRedis } from '../helpers/sms-otp-redis-helper'
import { getUserIdByEmail, getUserPhoneByEmail, clearUserPhoneByEmail, closePhoneDbPool } from '../helpers/phone-db-helper'
import { PhoneSecurityPage } from '../pages/phone-security-page'

const REALM_ID = 'realm-001'
const USER_EMAIL = 'user@realm-001.com'
const USER_PASSWORD = 'password'
const DEMO_CODE = '135790'
const BIND_PHONE = '13800138000'
const REBIND_PHONE = '13900139000'

test.describe('[Regular User] Phone Bind/Rebind/Unbind Demo Tests', () => {

  test.beforeEach(async ({ page }) => {
    await verifyTestEnvironment(page, {
      requiredRealms: [REALM_ID],
      requiredUsers: ['admin@realm-001.com', USER_EMAIL],
    })
    await clearUserPhoneByEmail(USER_EMAIL, REALM_ID)
  })

  test.afterEach(async ({ page, demoLogger }) => {
    try {
      await disablePhoneLoginAndClearChannel(page, demoLogger, REALM_ID)
    } finally {
      const userId = await getUserIdByEmail(USER_EMAIL, REALM_ID)
      await clearUserPhoneByEmail(USER_EMAIL, REALM_ID)
      if (userId) {
        await clearSmsBindCode(REALM_ID, userId, BIND_PHONE)
        await clearSmsBindCode(REALM_ID, userId, REBIND_PHONE)
      }
    }
  })

  test.afterAll(async () => {
    await closeSmsOtpRedis()
    await closePhoneDbPool()
  })

  test('US-PL-001: user binds, rebinds and unbinds a phone via Security settings', async ({ page, demoLogger, loginPage }) => {
    const accountId = await getUserIdByEmail(USER_EMAIL, REALM_ID)
    expect(accountId, 'The user must exist before testing phone ownership').not.toBeNull()
    const security = new PhoneSecurityPage(page, demoLogger)

    // Only SMS delivery is synthetic; reauth, confirm and unbind stay real.
    await page.route('**/api/user/phone/bind/send', async route => {
      const phone = route.request().postDataJSON().phone
      expect([BIND_PHONE, REBIND_PHONE]).toContain(phone)
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          message: 'ok', expiresInSeconds: 300,
          maskedPhone: phone === BIND_PHONE ? '138****8000' : '139****9000',
        }),
      })
    })

    await test.step('Given: phone login enabled and the user opens Security', async () => {
      await enablePhoneLoginForRealm(page, demoLogger, REALM_ID)
      await loginPage.loginAsUser(USER_EMAIL, USER_PASSWORD, REALM_ID)
      await security.open(REALM_ID)
      await expect(security.boundValue).toHaveCount(0)
    })

    await test.step('Bind: reauth and SMS proof persist the first phone', async () => {
      await security.beginBinding(USER_PASSWORD, BIND_PHONE)
      expect(await getUserPhoneByEmail(USER_EMAIL, REALM_ID), 'Sending alone must not bind').toBeNull()
      await injectSmsBindCode(REALM_ID, accountId!, BIND_PHONE, DEMO_CODE)
      expect(await getSmsBindCode(REALM_ID, accountId!, BIND_PHONE)).toBe(DEMO_CODE)
      await security.confirmBinding(DEMO_CODE)
      expect(await getUserPhoneByEmail(USER_EMAIL, REALM_ID)).toBe(`+86${BIND_PHONE}`)
      expect(await getSmsBindCode(REALM_ID, accountId!, BIND_PHONE), 'Proof must be consumed').toBeNull()
      await security.open(REALM_ID)
      await expect(security.boundValue).toHaveText('138****8000')
    })

    await test.step('Rebind: the old phone stays until the new phone is verified', async () => {
      await security.beginBinding(USER_PASSWORD, REBIND_PHONE)
      expect(await getUserPhoneByEmail(USER_EMAIL, REALM_ID), 'Pending rebind must retain the old phone').toBe(`+86${BIND_PHONE}`)
      await injectSmsBindCode(REALM_ID, accountId!, REBIND_PHONE, DEMO_CODE)
      expect(await getSmsBindCode(REALM_ID, accountId!, REBIND_PHONE)).toBe(DEMO_CODE)
      await security.confirmBinding(DEMO_CODE)
      expect(await getUserPhoneByEmail(USER_EMAIL, REALM_ID)).toBe(`+86${REBIND_PHONE}`)
      expect(await getSmsBindCode(REALM_ID, accountId!, REBIND_PHONE)).toBeNull()
      await security.open(REALM_ID)
      await expect(security.boundValue).toHaveText('139****9000')
    })

    await test.step('Unbind: confirmation and reauth remove the persisted phone', async () => {
      await security.unbind(USER_PASSWORD)
      expect(await getUserPhoneByEmail(USER_EMAIL, REALM_ID)).toBeNull()
      await security.open(REALM_ID)
      await expect(security.boundValue).toHaveCount(0)
      await expect(security.bindButton).toBeVisible()
    })
  })
})
