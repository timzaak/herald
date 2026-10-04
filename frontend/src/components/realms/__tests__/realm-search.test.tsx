/**
 * @vitest-environment jsdom
 */
import { describe, it, expect, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { RealmSearch } from '../realm-search'

/**
 * Wiring of the realms search box onto the shared useUrlSyncedInput
 * mechanism. Before the migration RealmSearch seeded its state once and
 * never followed later URL-driven realmId changes, so deep links like
 * /manage/realms?realmId=… and back/forward left the box showing a stale
 * filter while the list filtered correctly. These tests pin that the box
 * follows the URL; the lead-while-typing semantics live in the hook's own
 * spec.
 */

// 6x the 500ms debounce in useUrlSyncedInput — headroom for CI jitter.
const DEBOUNCE_WAIT_MS = 3000

describe('RealmSearch URL sync', () => {
  it('seeds the box from the URL realmId', () => {
    render(<RealmSearch realmId="admin" onSearchChange={vi.fn()} />)
    expect(screen.getByTestId('realms-search-input')).toHaveValue('admin')
  })

  it('follows URL-driven realmId changes', async () => {
    const onSearchChange = vi.fn()
    const view = render(<RealmSearch realmId="admin" onSearchChange={onSearchChange} />)
    expect(screen.getByTestId('realms-search-input')).toHaveValue('admin')

    // deep link / back-forward changes the URL realmId; the box must follow
    view.rerender(<RealmSearch realmId="billing" onSearchChange={onSearchChange} />)
    await waitFor(() => {
      expect(screen.getByTestId('realms-search-input')).toHaveValue('billing')
    })
  })

  it('reports debounced input as the new filter', async () => {
    const user = userEvent.setup()
    const onSearchChange = vi.fn()
    render(<RealmSearch onSearchChange={onSearchChange} />)
    await user.type(screen.getByTestId('realms-search-input'), 'bill')
    await waitFor(() => expect(onSearchChange).toHaveBeenCalledWith('bill'), {
      timeout: DEBOUNCE_WAIT_MS,
    })
  })
})
