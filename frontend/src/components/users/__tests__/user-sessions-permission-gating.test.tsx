/**
 * Permission-gating behavior test for the user-sessions entry point.
 *
 * The gating contract has two halves:
 *   1. `UserTable` renders the inline "Sessions" button (testid
 *      `user-table-${row.index}-sessions-button`) ONLY when the parent passes
 *      `onManageSessions`.
 *   2. The route passes `onManageSessions` only when `canManage`
 *      (`hasPermission('users.manage')`).
 *
 * This test exercises HALF 1 directly — the component-contract half — by
 * rendering `UserTable` with a one-row fixture and toggling the
 * `onManageSessions` prop. This is the robust, low-mock expression of the gate:
 * it avoids mocking `@tanstack/react-router`, `usePermission`, and
 * `useDialogManager` (the heavier route-level pattern used by
 * `routes/$realmId/manage/api-keys/__tests__/permission-gating.test.tsx`).
 *
 * `UserTable` consumes already-resolved rows (no internal query), so a plain
 * `render` suffices — no `QueryClientProvider` needed. The router context is
 * only touched by `UserTableError`, which never mounts because we pass valid
 * non-empty `data`.
 */

import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { UserTable } from '../user-table'
import type { UserResponse } from '@/lib/api-generated'

// `UserTable` accepts `UserWithRoles` rows (`UserResponse & { roles? }`). Roles
// are optional and irrelevant to the sessions entry, so a minimal user row is
// enough — only `id`/`email`/`status`/`realmId`/`createdAt` are required by the
// type, and the actions cell only reads `row.original` when a button fires.
function makeRow(overrides: Partial<UserResponse> = {}): UserResponse {
  return {
    id: 'u-1',
    email: 'alice@example.com',
    nickname: 'Alice',
    status: 'normal',
    realmId: 'r1',
    createdAt: '2026-07-01T09:00:00Z',
    ...overrides,
  }
}

describe('UserTable sessions entry — permission gating via onManageSessions', () => {
  describe('manager (onManageSessions provided)', () => {
    it('renders the Sessions entry button and invokes the handler with the row user', async () => {
      const user = userEvent.setup()
      const onManageSessions = vi.fn()

      render(<UserTable data={[makeRow()]} onManageSessions={onManageSessions} />)

      // The entry exists — this is the manager-visible affordance.
      const sessionsButton = screen.getByTestId('user-table-0-sessions-button')
      expect(sessionsButton).toBeInTheDocument()

      // Clicking it reports the row's user to the parent — the bridge to the
      // sessions dialog. Assert the user object is forwarded, not just fired.
      await user.click(sessionsButton)
      expect(onManageSessions).toHaveBeenCalledTimes(1)
      expect(onManageSessions).toHaveBeenCalledWith(
        expect.objectContaining({ id: 'u-1', email: 'alice@example.com' })
      )
    })
  })

  describe('view-only (onManageSessions omitted)', () => {
    it('does not render the Sessions entry button', () => {
      // Omitting `onManageSessions` is how the route expresses "no
      // users.manage". The entry point must be entirely absent — not disabled,
      // not greyed out — so a non-manager has nothing to attempt.
      // (`onManageSessions={canManage ? fn : undefined}` is the same runtime
      // shape as omission, so one test covers both.)
      render(<UserTable data={[makeRow()]} />)

      expect(screen.queryByTestId('user-table-0-sessions-button')).not.toBeInTheDocument()
    })
  })
})
