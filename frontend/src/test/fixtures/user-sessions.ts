/**
 * Test fixtures for the user-sessions management feature.
 *
 * `UserSessionResponse` is the camelCase API contract returned by
 * `GET /api/users/{userId}/sessions` and consumed by the sessions
 * dialog. Centralizing the field set here keeps list-render assertions readable
 * and lets a test override a single field (e.g. drop `clientAppName` to `null`
 * to exercise the name-missing fallback) while keeping the rest valid.
 *
 * The generated type marks `clientAppName` / `clientIp` / `createdAt` /
 * `userAgent` as `?: string | null` — they are optional AND nullable on the
 * wire (absent or null for legacy families without the session metadata index).
 */

import type { UserSessionResponse } from '@/lib/api-generated'

/**
 * Build one `UserSessionResponse` with realistic defaults. `overrides` is
 * spread LAST so a test can null out any optional field to exercise the
 * dialog's null-fallback rendering.
 */
export function makeUserSession(overrides: Partial<UserSessionResponse> = {}): UserSessionResponse {
  return {
    familyId: 'fam-1',
    clientAppId: 'app-1',
    clientAppName: 'Web Console',
    clientIp: '203.0.113.10',
    userAgent: 'Mozilla/5.0 (Macintosh) Chrome/120',
    createdAt: '2026-07-01T09:00:00Z',
    credentialClass: 'first_party',
    absoluteExpiresAt: '2026-07-31T09:00:00Z',
    ...overrides,
  }
}

/**
 * Build `count` DISTINCT sessions, each with a unique `familyId` (`fam-${i}`)
 * and `clientAppId` (`app-${i}`) so per-row revoke testids
 * (`user-sessions-table-${index}-revoke-button`) are uniquely targetable. An
 * optional `overridesPerItem` array supplies per-item field overrides (matched
 * by index) — useful for the name-fallback test, where one row drops
 * `clientAppName` to null.
 */
export function makeUserSessions(
  count: number,
  overridesPerItem: Partial<UserSessionResponse>[] = []
): UserSessionResponse[] {
  return Array.from({ length: count }, (_, i) =>
    makeUserSession({
      familyId: `fam-${i}`,
      clientAppId: `app-${i}`,
      clientAppName: `Web Console ${i}`,
      ...overridesPerItem[i],
    })
  )
}
