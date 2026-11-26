import { Page } from '@playwright/test'
import { loginAsAdmin } from '../auth'
import { dismissOnboardingWelcomeDialogIfPresent } from '../onboarding-dismiss'
import { SELECTORS } from '../../selectors'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'

/**
 * Login as the realm admin and transparently agree to any login-time
 * re-consent prompt. This keeps cleanup / admin scenarios stable after a
 * scenario test publishes or reverts a legal agreement version.
 */
export async function loginAsAdminWithConsent(
  page: Page,
  realmId: string,
  options?: { forceRelogin?: boolean }
): Promise<void> {
  console.log(`[ConsentAwareLogin] Logging in as admin for realm: ${realmId}`)

  // The admin console (/manage) is a DIFFERENT first-party client
  // (admin-web-console) than the user account center the login flow mints a
  // token for. When the SPA lands on /manage the root loader's initializeAuth
  // fires POST /api/auth/browser-token/switch-client, which REVOKES the source
  // token family and returns a new admin token. That swap is asynchronous with
  // respect to the URL reaching /manage, so if a caller navigates to a realm
  // admin page (e.g. /manage/users → GET /api/users/{realmId}) immediately
  // after this helper returns, the in-flight request can ride on the now-
  // revoked source token and 401 out, collapsing the whole session (401 →
  // refresh 401 → kicked back to /auth/login). Capture the switch response up
  // front so we can await its completion before returning, guaranteeing the
  // admin token is persisted in the store (mirrors LoginPage.loginAsAdmin's
  // switchResponsePromise handling at login-page.ts:290-295).
  const switchClientResponsePromise = page
    .waitForResponse(
      (response) =>
        new URL(response.url()).pathname === '/api/auth/browser-token/switch-client'
        && response.request().method() === 'POST',
      { timeout: 15000 }
    )
    .catch(() => null)

  await loginAsAdmin(page, {
    realmId,
    forceRelogin: options?.forceRelogin ?? true,
    waitNavigation: false,
  })

  const reconsentView = page.locator(SELECTORS.legalConsent.loginReconsentView)
  const needsReconsent = await reconsentView.isVisible({ timeout: 3000 }).catch(() => false)

  if (needsReconsent) {
    console.log('[ConsentAwareLogin] Login-time re-consent view detected; agreeing...')
    const agreeButton = page.locator(SELECTORS.legalConsent.loginAgreeAndContinueButton)
    await Promise.all([
      page.waitForURL(`**/manage**`, { timeout: 15000 }),
      agreeButton.click(),
    ])
    console.log('[ConsentAwareLogin] Re-consent accepted; navigation completed')
  } else {
    console.log('[ConsentAwareLogin] No re-consent gate; already past consent')

    // loginAsAdmin was called with waitNavigation:false, so the SPA navigation
    // to the admin dashboard may still be in flight when we reach here. Wait
    // for the URL to settle on /manage (session-scoped, no realm prefix, per
    // DEFAULT_ADMIN_REDIRECT) before returning — otherwise the caller's next
    // step (e.g. clicking a sidebar entry) races the route loader and fails.
    try {
      await page.waitForURL('**/manage**', { timeout: 15000 })
    } catch {
      // Fallback: force-navigate to the realm-prefixed dashboard if the SPA
      // redirect did not land on /manage in time.
      await page.goto(`${BASE_URL}/${realmId}/manage`, { waitUntil: 'domcontentloaded' })
    }
  }

  // The first-login welcome dialog is modal: while it is open its overlay
  // intercepts every click on the console (sidebar entries, tabs, filters).
  // loginAsAdmin() skips its own dismissal here — with waitNavigation:false the
  // URL is not on /manage yet when it checks — so dismiss it once the console
  // URL has settled. Without this, a context whose completion marker was
  // cleared (e.g. localStorage.clear() between two logins) hands back a
  // blocked console and the caller's next click waits out the test timeout.
  await dismissOnboardingWelcomeDialogIfPresent(page)

  // Stabilize the admin console session before handing control back: the
  // caller's next action is typically a sidebar click to a realm admin page
  // whose data query (e.g. GET /api/users/{realmId}) requires the
  // admin-web-console token. Awaiting the client-switch completion (or a short
  // settle window when no switch fires — e.g. an already-admin session) ensures
  // the store holds the valid admin token instead of the revoked source token.
  const switchClientResponse = await switchClientResponsePromise
  if (switchClientResponse) {
    console.log(
      `[ConsentAwareLogin] Admin client switch resolved (${switchClientResponse.status()})`
    )
    if (!switchClientResponse.ok()) {
      // A failed switch leaves the session on the (still valid) source product
      // token, which will 403/redirect on /manage routes. Force a reload so the
      // root loader re-runs initializeAuth and either completes the switch or
      // redirects the caller to a safe authenticated page — rather than letting
      // a stale in-memory token silently break the next admin API call.
      console.warn(
        `[ConsentAwareLogin] Admin client switch failed (HTTP ${switchClientResponse.status()}); reloading /manage to resync`
      )
      await page.goto(`${BASE_URL}/${realmId}/manage`, { waitUntil: 'domcontentloaded' })
      await page.waitForLoadState('domcontentloaded')
    }
  } else {
    // No switch fired (the session was already admin-web-console). Still let the
    // route loader settle so any pending React Query fetches complete on the
    // authenticated token before the caller navigates further.
    await page.waitForLoadState('domcontentloaded')
  }
}
