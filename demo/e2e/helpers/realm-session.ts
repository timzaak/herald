/**
 * Realm-scoped session persistence helpers.
 *
 * After the auth-rewrite the access token is in-memory only and the Zustand
 * `auth-storage` snapshot no longer contains the token family (its partialize
 * excludes it — frontend/src/stores/auth-store.ts). The one persisted session
 * artifact is the refresh token the Herald SDK client stores per realm under
 * `herald.refreshToken:{realmId}` (frontend/src/lib/herald-client.ts
 * heraldRefreshTokenStorageKey — one slot per realm so a login in one realm
 * never evicts another realm's session). The token lands asynchronously after
 * the post-login PKCE exchange and clears asynchronously on logout, so tests
 * must poll localStorage instead of sampling it once.
 */

import { expect, type Page } from '@playwright/test'

/** localStorage slot holding the realm's persisted refresh token. */
export function realmRefreshTokenKey(realmId: string): string {
  return `herald.refreshToken:${realmId}`
}

/** Poll until the realm's refresh-token slot holds a token (login completed). */
export async function waitForRealmSessionPersisted(page: Page, realmId: string): Promise<void> {
  await expect
    .poll(() =>
      page.evaluate((key) => Boolean(localStorage.getItem(key)), realmRefreshTokenKey(realmId)),
    )
    .toBe(true)
}

/** Poll until the realm's refresh-token slot is empty (logout completed). */
export async function waitForRealmSessionCleared(page: Page, realmId: string): Promise<void> {
  await expect
    .poll(() =>
      page.evaluate((key) => localStorage.getItem(key) === null, realmRefreshTokenKey(realmId)),
    )
    .toBe(true)
}
