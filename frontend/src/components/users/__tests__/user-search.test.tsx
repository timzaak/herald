/**
 * @vitest-environment jsdom
 */
import { describe, it, expect, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { UserSearch } from '../user-search'

/**
 * URL-sync semantics of the users email filter box.
 *
 * `email` is the URL-driven value (deep links like /manage/users?email=…,
 * back/forward, programmatic navigation); the box LEADS the URL only while
 * the user types (debounced onSearchChange → navigate). The box must follow
 * URL changes when pristine, but never clobber uncommitted keystrokes when a
 * debounced round-trip lands mid-edit — losing either side breaks the
 * bookmark/filter contract the admin console relies on.
 */

// 6x the 500ms debounce in useUrlSyncedInput — headroom for CI jitter.
const DEBOUNCE_WAIT_MS = 3000

function setup(email?: string) {
  const onSearchChange = vi.fn()
  const view = render(<UserSearch email={email} onSearchChange={onSearchChange} />)
  return { onSearchChange, view }
}

describe('UserSearch URL sync', () => {
  it('seeds the box from the URL email', () => {
    setup('admin@fornetcode.com')
    expect(screen.getByTestId('users-search-input')).toHaveValue('admin@fornetcode.com')
  })

  it('follows URL-driven email changes while the box is pristine', async () => {
    const { view } = setup('a@example.com')
    expect(screen.getByTestId('users-search-input')).toHaveValue('a@example.com')

    // Deep link / back-forward changes the URL email; the box must follow.
    view.rerender(<UserSearch email="b@example.com" onSearchChange={vi.fn()} />)
    await waitFor(() => {
      expect(screen.getByTestId('users-search-input')).toHaveValue('b@example.com')
    })
  })

  it('keeps uncommitted keystrokes when the debounced round-trip lands late', async () => {
    const user = userEvent.setup()
    const { onSearchChange, view } = setup()

    const input = screen.getByTestId('users-search-input')
    await user.type(input, 'ty')
    // debounce (500ms) fires → parent navigates → URL email becomes 'ty'
    await waitFor(() => expect(onSearchChange).toHaveBeenCalledWith('ty'), {
      timeout: DEBOUNCE_WAIT_MS,
    })

    // user keeps typing before the URL update is applied
    await user.type(input, 'p')
    view.rerender(<UserSearch email="ty" onSearchChange={onSearchChange} />)

    expect(screen.getByTestId('users-search-input')).toHaveValue('typ')
  })

  it('reports debounced input as the new filter', async () => {
    const user = userEvent.setup()
    const { onSearchChange } = setup()
    await user.type(screen.getByTestId('users-search-input'), 'x@y.z')
    await waitFor(() => expect(onSearchChange).toHaveBeenCalledWith('x@y.z'), {
      timeout: DEBOUNCE_WAIT_MS,
    })
  })
})
