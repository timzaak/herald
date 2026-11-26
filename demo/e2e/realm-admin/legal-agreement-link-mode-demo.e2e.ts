import { test, expect } from '../fixtures/demo-page.fixtures'
import { AdminLegalHelper } from '../helpers/legal-consent'
import { SELECTORS } from '../selectors'

const realmId = 'realm-001'
const baseUrl = process.env.BASE_URL || 'http://localhost:3000'

test.describe('[Realm Admin] Legal agreement external-link mode', () => {
  // The published link outlives this test. Restore the default Terms so later
  // specs that open the public agreement page are not handed a
  // `target="_blank"` link instead of inline content.
  test.afterEach(async ({ page }) => {
    const helper = new AdminLegalHelper(page)
    await helper.gotoLegalTab(realmId)

    const terms = page.locator(SELECTORS.legalConsent.legalAgreementCard('terms_of_service'))
    const linkPublished = await terms
      .locator(SELECTORS.legalConsent.legalModeBadge('link'))
      .isVisible()
      .catch(() => false)
    if (linkPublished) {
      await helper.revertToDefault('terms_of_service')
    }
  })

  test('publishes an external Terms link while Privacy remains independently configured', async ({ page }) => {
    const helper = new AdminLegalHelper(page)
    await helper.gotoLegalTab(realmId)
    await helper.publishLinkAgreement('terms_of_service', 'https://example.com/terms', `link-${Date.now()}`)

    const terms = page.locator(SELECTORS.legalConsent.legalAgreementCard('terms_of_service'))
    const privacy = page.locator(SELECTORS.legalConsent.legalAgreementCard('privacy_policy'))
    await expect(terms.locator(SELECTORS.legalConsent.legalModeBadge('link'))).toBeVisible()
    await expect(privacy).toBeVisible()

    await page.goto(`${baseUrl}/${realmId}/auth/login`)
    const publicTermsLink = page.locator('[data-testid="terms-of-service-link"]')
    await expect(publicTermsLink).toHaveAttribute('href', 'https://example.com/terms')
    await expect(publicTermsLink).toHaveAttribute('target', '_blank')
  })
})
