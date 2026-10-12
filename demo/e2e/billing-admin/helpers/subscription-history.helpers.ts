/**
 * Subscription History Page Helpers
 *
 * Shared helper functions for subscription history tests
 */

import { Page, expect } from '@playwright/test'
import { SELECTORS } from '../../selectors'

/**
 * Event type constants for subscription history
 */
export const EVENT_TYPES = {
  created: 'created',
  upgraded: 'upgraded',
  downgraded: 'downgraded',
  canceled: 'canceled',
  renewed: 'renewed',
  reactivated: 'reactivated',
  expired: 'expired',
} as const

/**
 * Event display type constants (for UI display)
 */
export const EVENT_DISPLAY_TYPES = {
  created: 'Created',
  upgraded: 'Upgraded',
  downgraded: 'Downgraded',
  canceled: 'Canceled',
  renewed: 'Renewed',
  reactivated: 'Reactivated',
  expired: 'Expired',
} as const

/**
 * Helper function to navigate to subscription history page
 *
 * Robustness notes:
 * - A full `page.goto('/manage/subscription-history')` reload is NOT used. In
 *   the demo env the root loader's reload-time auth restore (refresh-token
 *   exchange + first-party client switch) is flaky and frequently 401s,
 *   bouncing the page back to /auth/login (observed across runs a1/a2 and fix
 *   attempts 1-4 — even with a fully-populated auth-storage). Instead we use
 *   an IN-APP navigation via the TanStack Router exposed at `window.router`
 *   (dev only), which preserves the in-memory access token established at
 *   login and renders the page deterministically.
 * - `loginPage.loginAsAdmin` returns as soon as the login API responds; the
 *   post-login SPA navigation to /manage (or /user/profile) happens
 *   asynchronously and can take a couple of seconds. Its `waitForURL` check
 *   can match the still-on-login URL, so this helper explicitly waits for the
 *   URL to leave /auth/login AND settle on an authenticated route before
 *   issuing the in-app navigation. Navigating too early (while the login
 *   mutation's onSuccess redirect is still pending) leaves the session
 *   half-established and the subsequent route bounces to /auth/login.
 * - Readiness is asserted via the shipped `data-testid` anchors
 *   (`subscription-history-page` / `-filter` / `-list`) rather than ambiguous
 *   text. `getByText('History Events')` resolves to TWO elements when the list
 *   is empty (the card title AND the "No history events found" cell), causing a
 *   strict-mode violation; the testid is unambiguous.
 * - The selector constants in `selectors.ts` are FULL CSS attribute selectors
 *   (e.g. `'[data-testid="subscription-history-page"]'`), so they MUST be used
 *   with `page.locator(...)`, NOT `page.getByTestId(...)` (which expects the
 *   bare testid value and would otherwise build a broken locator).
 */
export async function navigateToSubscriptionHistory(page: Page, realmId: string): Promise<void> {
  const targetPath = `/manage/subscription-history`
  const pageLocator = page.locator(SELECTORS.subscriptionHistory.page)

  // 1) Ensure the post-login SPA navigation has finished. The login POM returns
  //    promptly after the login API call; the redirect to /manage or
  //    /user/profile is still in flight. Wait until we are definitively OFF the
  //    login page and on an authenticated session-scoped route. (Ignore the
  //    return value — we only need the wait; an already-settled page resolves
  //    immediately.)
  await waitForAuthenticatedLanding(page, realmId)

  // 2) In-app navigation (no document reload) preserves the in-memory access
  //    token, sidestepping the flaky reload-time auth restore. Fall back to a
  //    full goto only if the router is unavailable.
  const navigatedViaSpa = await navigateViaRouter(page, targetPath)
  if (!navigatedViaSpa) {
    await page.goto(targetPath)
  }

  // Wait for the loading state to finish if it's present
  await page
    .waitForSelector('[data-testid="page-loading"]', { state: 'detached', timeout: 5000 })
    .catch(() => {
      // If the loading state was never present or already finished, that's fine
    })

  // Wait for the page container (more reliable than the heading text)
  await expect(pageLocator).toBeVisible({ timeout: 15000 })
  // Wait for the filter section to be rendered
  await expect(page.locator(SELECTORS.subscriptionHistory.filterContainer)).toBeVisible({ timeout: 10000 })
  // Wait for the history list container to be rendered (covers both the
  // populated table and the empty-state row)
  await expect(page.locator(SELECTORS.subscriptionHistory.listContainer)).toBeVisible({ timeout: 10000 })
}

/**
 * Wait for the post-login SPA navigation to settle on an authenticated route.
 *
 * `loginPage.loginAsAdmin` resolves as soon as the login API responds; the
 * redirect to the top-level session-scoped routes (`/manage` or `/user/...`)
 * happens asynchronously in the login mutation's `onSuccess`. Issuing the next
 * navigation before that redirect completes leaves the session
 * half-established. This waits until the URL is no longer on an auth page AND
 * is on one of the session-scoped authenticated routes.
 */
async function waitForAuthenticatedLanding(page: Page, realmId: string): Promise<void> {
  // First, wait for the URL to leave /auth/login (post-login redirect).
  await page
    .waitForURL(
      (url) => !url.pathname.includes('/auth/login'),
      { timeout: 15000 }
    )
    .catch(() => {
      // If we time out, proceed anyway — the assertions below will fail loudly.
    })
  // Then ensure we've landed on an authenticated route. The admin lands on
  // /manage; a regular user lands on /user/.... Accept the legacy realm root
  // /{realmId} too for backward-compat.
  await page
    .waitForURL(
      (url) => {
        const p = url.pathname
        return p.startsWith('/manage') || p.startsWith('/user') || p === `/${realmId}`
      },
      { timeout: 15000 }
    )
    .catch(() => {
      // Best-effort; the page-container assertion below surfaces real failures.
    })
}

/**
 * Navigate to `path` using the in-app TanStack Router (exposed at
 * `window.router` in dev). Returns true on success, false if the router is
 * unavailable or the navigation rejected (caller should fall back to goto).
 *
 * Keeping navigation client-side preserves the in-memory access token so the
 * destination route's loader does not need to re-authenticate — sidestepping
 * the flaky reload-time auth restore.
 */
async function navigateViaRouter(page: Page, path: string): Promise<boolean> {
  try {
    const ok = await page.evaluate(async (target) => {
      const w = window as unknown as { router?: { navigate: (opts: { to: string }) => Promise<unknown> | unknown } }
      if (!w.router || typeof w.router.navigate !== 'function') {
        return false
      }
      try {
        await w.router.navigate({ to: target })
        return true
      } catch {
        return false
      }
    }, path)
    return ok === true
  } catch {
    return false
  }
}

/**
 * Helper function to navigate to subscription detail history page
 */
export async function navigateToSubscriptionDetailHistory(page: Page, realmId: string, subscriptionId: string): Promise<void> {
  await page.goto(`/${realmId}/user/my-subscriptions/${subscriptionId}/history`)
  await expect(page.locator(SELECTORS.subscriptionDetailHistory.page)).toBeVisible()
}

/**
 * Helper function to set event type filter
 */
export async function setEventTypeFilter(page: Page, eventType: string): Promise<void> {
  // Use label-based selector instead of data-testid
  await page.getByLabel('Event Type').click()
  // Wait for the dropdown to be visible (check first option)
  await expect(page.getByRole('option').first()).toBeVisible()
  await page.getByRole('option', { name: eventType }).click()
}

/**
 * Helper function to set plan filter
 */
export async function setPlanFilter(page: Page, planId: string): Promise<void> {
  // Use label-based selector instead of data-testid
  await page.getByTestId('subscription-history-filter').getByLabel('Plan').click()
  // Wait for the dropdown to be visible (check first option)
  await expect(page.getByRole('option').first()).toBeVisible()
  await page.getByRole('option', { name: planId }).click()
}

/**
 * Helper function to set user ID filter
 */
export async function setUserIdFilter(page: Page, userId: string): Promise<void> {
  const filterInput = page.locator(SELECTORS.subscriptionHistory.filterUserId)
  await filterInput.clear()
  await filterInput.fill(userId)
}

/**
 * Helper function to set date range filter
 */
export async function setDateRangeFilter(page: Page, fromDate: string, toDate: string): Promise<void> {
  // Use label-based selector instead of data-testid
  const fromInput = page.getByLabel('From Date')
  await fromInput.clear()
  await fromInput.fill(fromDate)

  const toInput = page.getByLabel('To Date')
  await toInput.clear()
  await toInput.fill(toDate)
}

/**
 * Helper function to apply filters
 */
export async function applyFilters(page: Page): Promise<void> {
  // Use text-based selector instead of data-testid
  await page.getByRole('button', { name: 'Apply Filters' }).click()
  // Wait for filters to apply (wait for network to settle)
  await page.waitForLoadState('networkidle')
}

/**
 * Helper function to reset filters
 */
export async function resetFilters(page: Page): Promise<void> {
  // Use text-based selector instead of data-testid
  await page.getByRole('button', { name: 'Reset' }).click()
}

/**
 * Helper function to click timeline event details
 */
export async function viewTimelineEventDetails(page: Page, eventIndex: number): Promise<void> {
  await page.getByTestId(SELECTORS.subscriptionDetailHistory.viewEventDetailsButton(eventIndex)).click()
}

/**
 * Helper function to close event detail dialog
 */
export async function closeEventDetailDialog(page: Page): Promise<void> {
  await page.getByRole('button', { name: 'Close' }).click()
  await expect(page.getByTestId(SELECTORS.subscriptionDetailHistory.eventDetailDialog)).not.toBeVisible()
}

/**
 * Helper function to verify event badge is visible
 */
export async function verifyEventBadge(page: Page, eventType: keyof typeof EVENT_TYPES): Promise<void> {
  const badgeSelector = SELECTORS.subscriptionDetailHistory.eventBadge(eventType)
  await expect(page.locator(badgeSelector)).toBeVisible()
}

/**
 * Helper function to check if timeline is empty
 */
export async function isTimelineEmpty(page: Page): Promise<boolean> {
  const isEmpty = await page.getByTestId(SELECTORS.subscriptionDetailHistory.timelineEmpty).isVisible().catch(() => false)
  return isEmpty
}

/**
 * Helper function to check if timeline is loading
 */
export async function isTimelineLoading(page: Page): Promise<boolean> {
  const isLoading = await page.getByTestId(SELECTORS.subscriptionDetailHistory.timelineLoading).isVisible().catch(() => false)
  return isLoading
}

/**
 * Helper function to wait for timeline to load
 */
export async function waitForTimelineToLoad(page: Page): Promise<void> {
  // Wait for loading indicator to disappear using Playwright's built-in waiting
  await page.waitForFunction(
    async () => {
      // Check if loading indicator is not visible or not present
      const loadingIndicator = document.querySelector('[data-testid="timeline-loading"]')
      return !loadingIndicator || loadingIndicator === null
    },
    { timeout: 10000 }
  )
}

/**
 * Helper function to click back button
 */
export async function clickBackButton(page: Page): Promise<void> {
  // Use text-based selector instead of data-testid
  await page.getByRole('button', { name: 'Back' }).click()
}
