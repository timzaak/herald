import { expect } from '@playwright/test'
import { BasePage } from './base-page'
import { SELECTORS } from '../selectors'

export class PhoneSecurityPage extends BasePage {
  readonly boundValue = this.page.locator(SELECTORS.phoneOtp.boundValue)
  readonly bindButton = this.page.locator(SELECTORS.phoneOtp.bindButton)

  async open(realmId: string): Promise<void> {
    await this.goto(`/${realmId}/user/security`)
    await this.smartClick(this.page.locator(SELECTORS.phoneOtp.securityTab))
    await expect(this.bindButton).toBeVisible()
  }

  async beginBinding(password: string, phone: string): Promise<void> {
    await this.smartClick(this.bindButton)
    await expect(this.page.locator(SELECTORS.phoneOtp.bindDialog)).toBeVisible()
    await this.fillField(this.page.locator(SELECTORS.phoneOtp.bindPasswordInput), password)
    const verified = this.page.waitForResponse(response =>
      new URL(response.url()).pathname === '/api/user/reauth/verify'
      && response.request().method() === 'POST'
    )
    await this.smartClick(this.page.locator(SELECTORS.phoneOtp.bindVerifyFactorButton))
    expect((await verified).ok(), 'Binding must require successful reauthentication').toBe(true)
    await this.fillField(this.page.locator(SELECTORS.phoneOtp.bindPhoneInput), phone)
    await this.smartClick(this.page.locator(SELECTORS.phoneOtp.bindSendButton))
    await expect(this.page.locator(SELECTORS.phoneOtp.bindCodeStep)).toBeVisible()
  }

  async confirmBinding(code: string): Promise<void> {
    for (let index = 0; index < code.length; index++) {
      await this.page.locator(SELECTORS.phoneOtp.bindCodeDigit(index)).fill(code[index])
    }
    const confirmed = this.page.waitForResponse('**/api/user/phone/bind/confirm')
    await this.smartClick(this.page.locator(SELECTORS.phoneOtp.bindConfirmButton))
    expect((await confirmed).ok(), 'The real backend must accept and persist the binding').toBe(true)
    await expect(this.page.locator(SELECTORS.phoneOtp.bindDonePanel)).toBeVisible()
    await this.smartClick(this.page.locator(SELECTORS.phoneOtp.bindDoneButton))
  }

  async unbind(password: string): Promise<void> {
    await this.smartClick(this.page.locator(SELECTORS.phoneOtp.unbindButton))
    await expect(this.page.locator(SELECTORS.phoneOtp.unbindDialog)).toBeVisible()
    await this.smartClick(this.page.locator(SELECTORS.phoneOtp.confirmUnbindButton))
    await this.fillField(this.page.locator(SELECTORS.phoneOtp.unbindPasswordInput), password)
    const removed = this.page.waitForResponse('**/api/user/phone/unbind')
    await this.smartClick(this.page.locator(SELECTORS.phoneOtp.unbindVerifyFactorButton))
    expect((await removed).ok(), 'The real backend must authorize and remove the binding').toBe(true)
    await expect(this.page.locator(SELECTORS.phoneOtp.unbindDonePanel)).toBeVisible()
    await this.smartClick(this.page.locator(SELECTORS.phoneOtp.unbindDoneButton))
  }
}
