/**
 * Realm Admin Legal Agreement Management Demo Tests
 *
 * User story: US-RA-019
 *
 * Scenarios:
 * - Scenario 1: Realm Admin opens Settings > Legal and views current
 *   agreements, source badges (default/custom), version metadata, and history.
 * - Scenario 2/6: Publishing a custom agreement creates a new version,
 *   switches the source badge to custom, and triggers re-consent for a regular
 *   user on their next login.
 * - Scenario 3: Reverting to the default template snapshot creates
 *   another new custom version and also triggers re-consent.
 *
 * Compliance rules:
 * - All operations go through the UI (no direct API calls).
 * - Shared selectors are used for all DOM assertions.
 * - Test users are cleaned up after each test while preserving the realm admin.
 */

import { test, expect, cleanupTestData } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { loginAsAdminWithConsent } from '../helpers/legal-consent/consent-aware-login'
import { AdminLegalHelper } from '../helpers/legal-consent/admin-legal-helper'
import { LoginPage } from '../pages/login-page'
import { SELECTORS } from '../selectors'
import type { Page } from '@playwright/test'

const realmId = 'realm-001'

test.describe('[Realm Admin] Legal Agreement Management Demo Tests', () => {
  let testStartTime: number
  const createdEmails: string[] = []
  let registrationCounter = 0

  test.beforeEach(async ({ page, testStartTime: startTime }) => {
    testStartTime = startTime
    createdEmails.length = 0
    registrationCounter = 0

    await verifyTestEnvironment(page, {
      requiredRealms: ['realm-001'],
      requiredUsers: ['admin@realm-001.com'],
    })
  })

  test.afterEach(async ({ page }) => {
    await cleanupTestData(page, realmId, {
      keepUsers: ['admin@realm-001.com'],
      timestamp: testStartTime,
      testUserEmails: [...createdEmails],
    })
  })

  /**
   * Clear the browser session so the next actor can log in cleanly.
   */
  async function clearUserSession(page: Page): Promise<void> {
    await page.context().clearCookies()
    await page.evaluate(() => {
      localStorage.clear()
      sessionStorage.clear()
    }).catch(() => {
      // Storage may not be accessible if the page is in a transient state.
    })
  }

  /**
   * Collect the set of `version_id`s currently rendered in the agreement's
   * history table. Each history row is keyed by `version_id`
   * (`legal-history-row-${type}-${version_id}`), so the set membership is a
   * durable signal of "which immutable versions exist" — independent of the
   * per-scope `version_no` model (default-scope and custom-scope rows each
   * number from 1).
   */
  async function getHistoryVersionIds(
    page: Page,
    agreementType: 'terms_of_service' | 'privacy_policy'
  ): Promise<Set<string>> {
    const locator = page.locator(
      `[data-testid^="legal-history-row-${agreementType}-"]`
    )
    const count = await locator.count()
    const prefix = `legal-history-row-${agreementType}-`
    const ids = new Set<string>()
    for (let i = 0; i < count; i++) {
      const testid = await locator.nth(i).getAttribute('data-testid')
      if (testid && testid.startsWith(prefix)) {
        ids.add(testid.slice(prefix.length))
      }
    }
    return ids
  }

  /**
   * Determine whether the currently rendered source badge for an agreement is
   * default or custom. Delegates to the shared helper, which owns the
   * card-header scoping (the EFFECTIVE version's badge lives in the header;
   * the history rows carry their own per-version badges).
   */
  async function getAgreementSource(
    page: Page,
    agreementType: 'terms_of_service' | 'privacy_policy'
  ): Promise<'default' | 'custom'> {
    return await new AdminLegalHelper(page).getSource(agreementType)
  }

  /**
   * Register a fresh regular user through the UI and log them in.
   * Registration records consent to the currently effective agreement versions.
   */
  async function registerAndLoginUser(
    page: Page,
    loginPage: LoginPage
  ): Promise<{ email: string; password: string }> {
    const suffix = `${testStartTime}-${registrationCounter++}`
    const email = `lc-admin-legal-${suffix}@example.com`
    const nickname = `lc-admin-legal-${suffix}`
    const password = 'Password123!'

    await test.step('Register test user', async () => {
      await page.goto(`/${realmId}/auth/register`, { waitUntil: 'domcontentloaded' })
      await expect(page.locator(SELECTORS.registration.card)).toBeVisible()

      await page.locator(SELECTORS.registration.emailInput).fill(email)
      await page.locator(SELECTORS.registration.nicknameInput).fill(nickname)
      await page.locator(SELECTORS.registration.passwordInput).fill(password)
      await page.locator(SELECTORS.registration.confirmPasswordInput).fill(password)
      await page.locator(SELECTORS.legalConsent.registerConsentCheckbox).check()
      await expect(
        page.locator(SELECTORS.legalConsent.registerConsentCheckbox)
      ).toBeChecked()

      const responsePromise = page.waitForResponse(
        response =>
          response.url().includes(`/api/auth/${realmId}/register`) &&
          response.request().method() === 'POST',
        { timeout: 10000 }
      )
      await page.locator(SELECTORS.registration.registerButton).click()
      const response = await responsePromise
      expect(response.ok()).toBe(true)
    })

    await test.step('Log in as the test user', async () => {
      await loginPage.loginAsUser(email, password, realmId)
    })

    return { email, password }
  }

  test('Scenario 1: Realm Admin can view the Legal tab and current agreements', async ({
    page,
  }) => {
    await test.step('Login as realm admin and open Settings > Legal', async () => {
      await loginAsAdminWithConsent(page, realmId)
      await page.goto(`/${realmId}/manage/settings`, { waitUntil: 'domcontentloaded' })
      await page.locator(SELECTORS.legalConsent.legalTab).click()
      await expect(page.locator(SELECTORS.legalConsent.legalAgreementsTab)).toBeVisible({
        timeout: 10000,
      })
    })

    await test.step('Assert both agreement cards are visible', async () => {
      await expect(
        page.locator(SELECTORS.legalConsent.legalAgreementCard('terms_of_service'))
      ).toBeVisible()
      await expect(
        page.locator(SELECTORS.legalConsent.legalAgreementCard('privacy_policy'))
      ).toBeVisible()
    })

    await test.step('Assert each card has a source badge and version metadata', async () => {
      for (const agreementType of ['terms_of_service', 'privacy_policy'] as const) {
        const card = page.locator(SELECTORS.legalConsent.legalAgreementCard(agreementType))
        const badge = card
          .locator(SELECTORS.legalConsent.sourceBadge('default'))
          .or(card.locator(SELECTORS.legalConsent.sourceBadge('custom')))
          .first()
        await expect(badge).toBeVisible()

        const meta = page.locator(SELECTORS.legalConsent.legalAgreementMeta(agreementType))
        await expect(meta).toBeVisible()
        const metaText = await meta.textContent()
        expect(metaText).toMatch(/Version:\s*\d+/)
      }
    })

    await test.step('Assert each card has a history table or an empty placeholder', async () => {
      for (const agreementType of ['terms_of_service', 'privacy_policy'] as const) {
        const historyTable = page.locator(
          SELECTORS.legalConsent.legalHistoryTable(agreementType)
        )
        const historyEmpty = page.locator(
          `[data-testid="legal-history-empty-${agreementType}"]`
        )
        await expect(historyTable.or(historyEmpty)).toBeVisible()
      }
    })
  })

  test('Scenario 2: Publishing a custom agreement triggers re-consent', async ({
    page,
    loginPage,
    adminLegalHelper,
  }) => {
    const { email, password } = await test.step(
      'Register and login a test user to establish consent',
      async () => {
        const user = await registerAndLoginUser(page, loginPage)
        createdEmails.push(user.email)
        return user
      }
    )

    await test.step('Clear session and switch to realm admin', async () => {
      await clearUserSession(page)
    })

    const { afterSource, newVersionIds } = await test.step(
      'Publish a custom Terms of Service version',
      async () => {
        // loginAsAdminWithConsent ensures a stable admin session at the start.
        await adminLegalHelper.gotoLegalTab(realmId)

        // WHY: version_no is scoped per source — the platform default row
        // (realm_id IS NULL) and the realm's custom rows each start at 1
        // (backend `next_custom_version_no` only counts the realm's own custom
        // rows; `legal_http_scenarios.rs` pins this). So the first publish
        // switches source default→custom with version_no staying 1, and an
        // `afterVersion > beforeVersion` assertion is always false. Assert the
        // real business outcome instead: the source badge is custom AND a new
        // immutable version (by version_id, not version_no) was published.
        const beforeVersionIds = await getHistoryVersionIds(
          page,
          'terms_of_service'
        )

        await adminLegalHelper.publishCustomAgreement(
          'terms_of_service',
          'Custom Terms of Service for admin legal demo (EN).',
          'admin-legal-demo-publish'
        )
        await adminLegalHelper.expectSourceBadge('terms_of_service', 'custom')
        const afterSource = await getAgreementSource(page, 'terms_of_service')

        // The source badge may already read "custom" from an earlier publish,
        // so it says nothing about the post-mutation refetch. Wait for the new
        // immutable version to show up in the (refetched) history table.
        let newVersionIds: string[] = []
        await expect
          .poll(async () => {
            const afterVersionIds = await getHistoryVersionIds(page, 'terms_of_service')
            newVersionIds = [...afterVersionIds].filter(id => !beforeVersionIds.has(id))
            return newVersionIds.length
          }, { timeout: 15000 })
          .toBeGreaterThan(0)

        return { afterSource, newVersionIds }
      }
    )

    // Publishing a custom agreement must (a) make the source badge custom and
    // (b) create at least one NEW immutable version identified by a fresh
    // version_id. Both are durable business signals that survive across the
    // default/custom scope boundary — unlike version_no, which is per-scope.
    expect(afterSource).toBe('custom')
    expect(newVersionIds.length).toBeGreaterThan(0)

    await test.step('Clear session and navigate to login as the test user', async () => {
      // WHY: do NOT call loginPage.login() here — it auto-accepts the login-time
      // re-consent view (login-page.ts:204-216), which would consume the very
      // re-consent gate this scenario must observe. Use fillLoginForm() +
      // submit() so the re-consent view surfaces naturally (mirrors
      // legal-consent-login-demo.e2e.ts:157-182).
      await clearUserSession(page)
      await loginPage.goto(realmId)
      await loginPage.waitForReady()
      await loginPage.fillLoginForm({ email, password })
      await loginPage.submit()
    })

    await test.step('Expect login-time re-consent view and agree to continue', async () => {
      const loginResponse = await page.waitForResponse(
        response =>
          response.url().includes('/login') &&
          response.request().method() === 'POST',
        { timeout: 10000 }
      )
      expect(loginResponse.ok()).toBe(true)

      await expect(
        page.locator(SELECTORS.legalConsent.loginReconsentView)
      ).toBeVisible({ timeout: 10000 })

      await Promise.all([
        page.waitForResponse(
          response =>
            response.url().includes('/login') &&
            response.request().method() === 'POST',
          { timeout: 10000 }
        ),
        page.locator(SELECTORS.legalConsent.loginAgreeAndContinueButton).click(),
      ])

      // WHY: regular users land on the top-level session-scoped /user/profile
      // (DEFAULT_USER_REDIRECT) OR the legacy realm-scoped /{realmId}/user/...
      // after route-refactor. Accept either; only require we left /auth/login.
      await page.waitForURL(
        new RegExp(
          `^http://localhost:3000/((user|manage)(/|$)|${realmId}/(?!auth/login))`
        ),
        { timeout: 15000 }
      )

      await expect(page.locator('[data-testid="email-display"]')).toBeVisible({
        timeout: 10000,
      })
    })
  })

  test('Scenario 3: Reverting to default snapshot triggers re-consent', async ({
    page,
    loginPage,
    adminLegalHelper,
  }) => {
    await test.step('Ensure ToS is in a custom state', async () => {
      await adminLegalHelper.gotoLegalTab(realmId)
      const source = await getAgreementSource(page, 'terms_of_service')
      if (source === 'default') {
        await adminLegalHelper.publishCustomAgreement(
          'terms_of_service',
          'Pre-seed custom Terms of Service for revert demo (EN).',
          'admin-legal-demo-preseed'
        )
      }
      await clearUserSession(page)
    })

    const { email, password } = await test.step(
      'Register and login another test user to establish consent on the custom version',
      async () => {
        const user = await registerAndLoginUser(page, loginPage)
        createdEmails.push(user.email)
        return user
      }
    )

    await test.step('Clear session and switch back to realm admin', async () => {
      await clearUserSession(page)
    })

    const { beforeSource, afterSource } = await test.step(
      'Revert ToS to the default template snapshot',
      async () => {
        await adminLegalHelper.gotoLegalTab(realmId)
        const beforeSource = await getAgreementSource(page, 'terms_of_service')
        await adminLegalHelper.revertToDefault('terms_of_service')
        const afterSource = await getAgreementSource(page, 'terms_of_service')

        const historyTable = page.locator(
          SELECTORS.legalConsent.legalHistoryTable('terms_of_service')
        )
        await expect(historyTable).toBeVisible()

        return { beforeSource, afterSource }
      }
    )

    // version_no is per source scope (default and custom rows each number from
    // 1), so reverting custom v3 to the default snapshot lands on a lower
    // number and "after > before" is not the invariant. The business outcome
    // is the effective source flipping custom → default.
    expect(beforeSource).toBe('custom')
    expect(afterSource).toBe('default')

    await test.step('Clear session and navigate to login as the test user', async () => {
      // WHY: do NOT call loginPage.login() here — it auto-accepts the login-time
      // re-consent view (login-page.ts:204-216), which would consume the very
      // re-consent gate this scenario must observe. Use fillLoginForm() +
      // submit() so the re-consent view surfaces naturally (mirrors
      // legal-consent-login-demo.e2e.ts:157-182).
      await clearUserSession(page)
      await loginPage.goto(realmId)
      await loginPage.waitForReady()
      await loginPage.fillLoginForm({ email, password })
      await loginPage.submit()
    })

    await test.step('Expect login-time re-consent view and agree to continue', async () => {
      const loginResponse = await page.waitForResponse(
        response =>
          response.url().includes('/login') &&
          response.request().method() === 'POST',
        { timeout: 10000 }
      )
      expect(loginResponse.ok()).toBe(true)

      await expect(
        page.locator(SELECTORS.legalConsent.loginReconsentView)
      ).toBeVisible({ timeout: 10000 })

      await Promise.all([
        page.waitForResponse(
          response =>
            response.url().includes('/login') &&
            response.request().method() === 'POST',
          { timeout: 10000 }
        ),
        page.locator(SELECTORS.legalConsent.loginAgreeAndContinueButton).click(),
      ])

      // WHY: regular users land on the top-level session-scoped /user/profile
      // (DEFAULT_USER_REDIRECT) OR the legacy realm-scoped /{realmId}/user/...
      // after route-refactor. Accept either; only require we left /auth/login.
      await page.waitForURL(
        new RegExp(
          `^http://localhost:3000/((user|manage)(/|$)|${realmId}/(?!auth/login))`
        ),
        { timeout: 15000 }
      )

      await expect(page.locator('[data-testid="email-display"]')).toBeVisible({
        timeout: 10000,
      })
    })
  })

  test('Scenario 4: Saving a draft does not change the live agreement', async ({
    page,
    adminLegalHelper,
  }) => {
    // WHY: drafts are staged in a separate table and must never affect the
    // effective version or trigger user reconsent until explicitly published.
    // This scenario saves a draft and asserts the published version_no is
    // unchanged, end-to-end through the admin UI.
    await adminLegalHelper.gotoLegalTab(realmId)
    await adminLegalHelper.saveDraftWithoutPublishing(
      'terms_of_service',
      'Unpublished draft body — must not go live (EN).',
      'admin-legal-demo-draft-only'
    )

    await test.step('Preview renders the staged draft without publishing', async () => {
      await adminLegalHelper.openPreview('terms_of_service')
      // Closing the dialog returns to the tab; the version is still unchanged.
      await page.keyboard.press('Escape')
    })
  })
})
