import { expect } from '@playwright/test'
import { BasePage } from './base-page'
import { clearSessionData } from '../helpers/auth'
import { SELECTORS } from '../selectors'

export class SmsLoginPage extends BasePage {
  readonly entry = this.page.locator(SELECTORS.phoneOtp.loginRouteToggle)
  readonly notRegistered = this.page.locator(SELECTORS.phoneOtp.notRegisteredMessage)
  readonly registerLink = this.page.locator(SELECTORS.phoneOtp.registerLink)

  async open(realmId: string): Promise<void> {
    await clearSessionData(this.page)
    await this.goto(`/${realmId}/auth/login`)
    await expect(this.page.locator(SELECTORS.login.container)).toBeVisible()
  }

  async selectSms(): Promise<void> {
    await this.smartClick(this.entry)
    await expect(this.page.locator(SELECTORS.phoneOtp.form)).toBeVisible()
  }

  async send(phone: string) {
    await this.fillField(this.page.locator(SELECTORS.phoneOtp.phoneInput), phone)
    const sent = this.page.waitForResponse('**/api/auth/*/login/sms-otp/send')
    await this.smartClick(this.page.locator(SELECTORS.phoneOtp.sendButton))
    return await sent
  }

  async enterCode(code: string): Promise<void> {
    await expect(this.page.locator(SELECTORS.phoneOtp.codeInput)).toBeVisible()
    for (let index = 0; index < code.length; index++) {
      await this.page.locator(SELECTORS.phoneOtp.codeDigit(index)).fill(code[index])
    }
  }

  async verify(): Promise<void> {
    const verified = this.page.waitForResponse('**/api/auth/*/login/sms-otp/verify')
    await this.smartClick(this.page.locator(SELECTORS.phoneOtp.verifyButton))
    expect((await verified).ok(), 'SMS proof must be verified by the real backend').toBe(true)
  }
}
