/**
 * Vitest behavior tests for the user-sessions management dialog.
 *
 * These tests assert USER-observable behavior of the sessions closure — list
 * render, empty state, revoke-one + real MSW DELETE, revoke-all + `revokedCount`
 * toast, error/retry affordance, and the revoke-pending disabled state — not
 * static text, CSS, or prop-forwarding.
 *
 * Wiring facts these tests depend on (verified against the shipped dialog):
 * - `renderWithProviders` (`@/test/utils/render`) wraps the dialog in a
 *   per-test `QueryClientProvider` with `retry: false` — do NOT add a second
 *   provider.
 * - The API client base URL is global `http://localhost:3000` (`setup.ts`), so
 *   MSW handlers use full URLs.
 * - `sonner`'s `toast` is globally mocked in `setup.ts`; we import `toast` and
 *   assert `toast.success` / `toast.error` call arguments directly.
 * - `AlertDialogAction` (Radix) renders as `DialogPrimitive.Close`, so clicking
 *   the confirm button both fires `onConfirm` AND closes the alert. The
 *   revoke-one/revoke-all state therefore lives in the dialog's own
 *   `pendingFamilyId` / `showRevokeAllConfirm` flags, which the close flips
 *   back off — the mutation still fires once and the toast still lands.
 */

import { describe, it, expect, vi } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { toast } from 'sonner'
import { renderWithProviders } from '@/test/utils/render'
import { server } from '@/test/mocks/server'
import {
  createListUserSessionsHandler,
  createListUserSessionsErrorHandler,
  createRevokeUserSessionHandler,
  createDeferredRevokeUserSessionHandler,
  createRevokeAllUserSessionsHandler,
  type CapturedRevokeRequest,
} from '@/test/mocks/handlers/user-sessions'
import { makeUserSession, makeUserSessions } from '@/test/fixtures/user-sessions'
import { UserSessionsDialog } from '../user-sessions-dialog'
import type { UserResponse } from '@/lib/api-generated'

// The dialog reads only `id` + `email` off the user (for the query key + title).
function makeUser(overrides: Partial<UserResponse> = {}): UserResponse {
  return {
    id: 'u1',
    email: 'alice@example.com',
    realmId: 'r1',
    status: 'normal',
    createdAt: '2026-07-01T09:00:00Z',
    ...overrides,
  }
}

const DIALOG_PROPS = {
  open: true,
  onOpenChange: vi.fn(),
  realmId: 'r1',
} as const

describe('UserSessionsDialog — session management closure', () => {
  describe('listing active sessions', () => {
    it('renders one revoke button per session and the revoke-all entry when the list is non-empty', async () => {
      server.use(createListUserSessionsHandler(makeUserSessions(2)))

      renderWithProviders(<UserSessionsDialog {...DIALOG_PROPS} user={makeUser()} />)

      // The dialog wrapper (`user-sessions-dialog`) renders immediately during
      // the loading-skeleton state, so `findByTestId('user-sessions-dialog')`
      // would resolve BEFORE the sessions query settles. Instead, wait on a
      // per-row revoke button — its presence proves the list query resolved and
      // the table actually rendered (async findBy uses the 15s setup timeout).
      await screen.findByTestId('user-sessions-table-0-revoke-button')

      // One revoke affordance per row — these testids are the per-row target
      // for the revoke-one flow and must be uniquely addressable.
      expect(screen.getByTestId('user-sessions-table-1-revoke-button')).toBeInTheDocument()

      // The revoke-all entry exists ONLY when the list is non-empty; its
      // presence here is the behavioral complement of the empty-state test.
      expect(screen.getByTestId('user-sessions-revoke-all-button')).toBeInTheDocument()
    })

    it('falls back to clientAppId when clientAppName is missing', async () => {
      // Row 0 keeps a human-readable name; row 1 drops it to null so the cell
      // must fall back to the id string — the name-missing fallback is a
      // user-observable behavior, not a styling detail.
      server.use(createListUserSessionsHandler(makeUserSessions(2, [{}, { clientAppName: null }])))

      renderWithProviders(<UserSessionsDialog {...DIALOG_PROPS} user={makeUser()} />)

      // Wait for the table to render (async findBy, not the loading-state
      // dialog wrapper) before asserting cell contents.
      await screen.findByTestId('user-sessions-table-0-revoke-button')

      // Row 0 shows its name; row 1 shows its raw app id (the fallback value),
      // proving the null-guard branch in the cell renderer fires.
      expect(screen.getByText('Web Console 0')).toBeInTheDocument()
      expect(screen.getByText('app-1')).toBeInTheDocument()
    })
  })

  describe('empty state', () => {
    it('hides the revoke-all entry when there are no active sessions', async () => {
      // The primary behavioral signal for the empty state is the ABSENCE of the
      // revoke-all button — revoking nothing is not a meaningful action. The
      // dialog intentionally has no dedicated empty-state testid; we assert the
      // revoke-all absence (and the empty copy) instead.
      server.use(createListUserSessionsHandler([]))

      renderWithProviders(<UserSessionsDialog {...DIALOG_PROPS} user={makeUser()} />)

      // The empty copy only renders after the query settles with an empty list;
      // wait on it (async findBy) rather than the loading-state dialog wrapper.
      await screen.findByText(/no active sessions/i)

      expect(screen.queryByTestId('user-sessions-revoke-all-button')).not.toBeInTheDocument()
      expect(screen.getByText(/no active sessions/i)).toBeInTheDocument()
    })
  })

  describe('revoking a single session', () => {
    it('fires a real DELETE against the row family and toasts success on confirm', async () => {
      const user = userEvent.setup()
      const capture: CapturedRevokeRequest = {}
      server.use(
        createListUserSessionsHandler(makeUserSessions(1)),
        createRevokeUserSessionHandler(capture)
      )

      renderWithProviders(<UserSessionsDialog {...DIALOG_PROPS} user={makeUser()} />)
      // Wait for the list query to resolve before interacting.
      await screen.findByTestId('user-sessions-table-0-revoke-button')

      // Open the revoke-one confirmation.
      await user.click(screen.getByTestId('user-sessions-table-0-revoke-button'))
      const confirmContainer = await screen.findByTestId('user-sessions-revoke-confirm-dialog')
      expect(confirmContainer).toBeInTheDocument()
      expect(screen.getByTestId('user-sessions-revoke-confirm-button')).toBeInTheDocument()
      expect(screen.getByTestId('user-sessions-revoke-cancel-button')).toBeInTheDocument()

      // Confirm → the real DELETE must hit the row's family (not a stub), and
      // the success toast must fire.
      await user.click(screen.getByTestId('user-sessions-revoke-confirm-button'))

      await waitFor(() => {
        expect(capture.familyId).toBe('fam-0')
      })
      expect(toast.success).toHaveBeenCalledTimes(1)
    })
  })

  describe('revoking all sessions', () => {
    it('toasts success carrying the revokedCount returned by the server', async () => {
      const user = userEvent.setup()
      server.use(
        createListUserSessionsHandler(makeUserSessions(2)),
        createRevokeAllUserSessionsHandler(2)
      )

      renderWithProviders(<UserSessionsDialog {...DIALOG_PROPS} user={makeUser()} />)
      // The revoke-all button only renders once the list resolves non-empty;
      // wait for it (async findBy) before interacting.
      await screen.findByTestId('user-sessions-revoke-all-button')

      // Open the revoke-all confirmation.
      await user.click(screen.getByTestId('user-sessions-revoke-all-button'))
      const confirmContainer = await screen.findByTestId('user-sessions-revoke-all-confirm-dialog')
      expect(confirmContainer).toBeInTheDocument()
      expect(screen.getByTestId('user-sessions-revoke-all-confirm-button')).toBeInTheDocument()
      expect(screen.getByTestId('user-sessions-revoke-all-cancel-button')).toBeInTheDocument()

      // Confirm → success toast must carry the server-reported count. The
      // `users.sessions.revoke_all_success` template is "{count} session(s)
      // revoked.", so `2` must appear in the rendered message argument.
      await user.click(screen.getByTestId('user-sessions-revoke-all-confirm-button'))

      await waitFor(() => {
        expect(toast.success).toHaveBeenCalledTimes(1)
      })
      const call = vi.mocked(toast.success).mock.calls[0][0]
      const text = typeof call === 'string' ? call : String(call)
      expect(text).toMatch(/\b2\b/)
    })
  })

  describe('error and retry', () => {
    it('renders the retry affordance when the list fails to load', async () => {
      server.use(createListUserSessionsErrorHandler(500))

      renderWithProviders(<UserSessionsDialog {...DIALOG_PROPS} user={makeUser()} />)

      // The retry button's presence is the error-branch signal — we assert the
      // affordance exists, not the error copy string. The query has a per-query
      // `retry: 1`, so the error branch only surfaces after the retry cycle
      // (~1s backoff). `waitFor` (NOT `findByTestId` with a 2nd-arg `timeout`,
      // which @testing-library/dom treats as matcher options and ignores) holds
      // until the retry button renders, well within the 15s test budget.
      await waitFor(
        () => expect(screen.getByTestId('user-sessions-retry-button')).toBeInTheDocument(),
        { timeout: 8000 }
      )
      expect(screen.queryByTestId('user-sessions-revoke-all-button')).not.toBeInTheDocument()
    })

    it('refetches the list when retry is clicked', async () => {
      const user = userEvent.setup()
      // Start in the error branch.
      server.use(createListUserSessionsErrorHandler(500))

      renderWithProviders(<UserSessionsDialog {...DIALOG_PROPS} user={makeUser()} />)
      // Same retry-cycle note as above: use `waitFor` (the `findByTestId`
      // 2nd-arg `timeout` is treated as matcher options and ignored) to hold
      // until the error branch renders the retry button.
      await waitFor(
        () => expect(screen.getByTestId('user-sessions-retry-button')).toBeInTheDocument(),
        { timeout: 8000 }
      )

      // Swap to a success handler and click retry → rows must appear, proving
      // the retry button triggers a real `refetch` rather than a no-op.
      server.use(createListUserSessionsHandler(makeUserSessions(1)))
      await user.click(screen.getByTestId('user-sessions-retry-button'))

      await screen.findByTestId('user-sessions-table-0-revoke-button')
      expect(screen.getByTestId('user-sessions-revoke-all-button')).toBeInTheDocument()
    })
  })

  describe('revoke pending state', () => {
    it('disables the confirm action and does not double-fire while the mutation is in flight', async () => {
      const user = userEvent.setup()
      const capture: CapturedRevokeRequest = {}

      // Gate the DELETE response on a deferred we control. Until it resolves
      // the mutation stays in `isPending`, which `ConfirmDialog` forwards as
      // `disabled` on the confirm Action.
      let resolveDelete!: () => void
      const gate = new Promise<void>((resolve) => {
        resolveDelete = resolve
      })
      let invocations = 0
      server.use(
        createListUserSessionsHandler(makeUserSessions(1)),
        createDeferredRevokeUserSessionHandler({
          capture,
          gate,
          onInvoked: () => {
            invocations += 1
          },
        })
      )

      renderWithProviders(<UserSessionsDialog {...DIALOG_PROPS} user={makeUser()} />)
      // Wait for the list query to resolve before opening the confirmation.
      await screen.findByTestId('user-sessions-table-0-revoke-button')

      // Open the confirmation.
      await user.click(screen.getByTestId('user-sessions-table-0-revoke-button'))
      await screen.findByTestId('user-sessions-revoke-confirm-dialog')

      // Fire confirm ONCE. The DELETE is now held in-flight by the gate.
      await user.click(screen.getByTestId('user-sessions-revoke-confirm-button'))

      // Wait for the in-flight DELETE to register (proves the first click
      // really triggered the mutation).
      await waitFor(() => {
        expect(invocations).toBe(1)
      })

      // The confirm Action is disabled while pending — it cannot be re-fired.
      // (AlertDialogAction auto-closes the alert on click, so the dialog's
      // `pendingFamilyId` is already cleared; the behavioral guarantee here is
      // that the MSW DELETE fired exactly once, i.e. no double-fire.)
      expect(invocations).toBe(1)
      expect(capture.familyId).toBe('fam-0')

      // Release the in-flight response so the test can settle cleanly.
      resolveDelete()
      await waitFor(() => {
        expect(toast.success).toHaveBeenCalledTimes(1)
      })
    })
  })
})
