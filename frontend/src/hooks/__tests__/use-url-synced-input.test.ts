import { renderHook, act, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { useUrlSyncedInput } from '@/hooks/use-url-synced-input'

/**
 * Spec of the shared URL-synced filter-input mechanism (used by UserSearch
 * and RealmSearch).
 *
 * The box LEADS the URL only while the user types (debounced commit) and
 * must FOLLOW URL-driven value changes otherwise (deep links, back/forward,
 * programmatic navigation) — but never clobber uncommitted keystrokes when
 * a debounced round-trip lands mid-edit. Losing either side breaks the
 * bookmark/filter contract every list page relies on; before the hook
 * existed, RealmSearch shipped with only the lead half.
 */

// 6x the 500ms debounce inside the hook — headroom for CI jitter.
const DEBOUNCE_WAIT_MS = 3000

function renderInput(initialUrlValue = '') {
  const onCommit = vi.fn()
  const view = renderHook(({ urlValue }) => useUrlSyncedInput(urlValue, onCommit), {
    initialProps: { urlValue: initialUrlValue },
  })
  return { onCommit, view }
}

describe('useUrlSyncedInput', () => {
  it('seeds the input from the URL value and stays silent while in sync', () => {
    const { onCommit, view } = renderInput('admin@fornetcode.com')
    expect(view.result.current[0]).toBe('admin@fornetcode.com')
    expect(onCommit).not.toHaveBeenCalled()
  })

  it('follows URL-driven value changes while the input is pristine', () => {
    const { view } = renderInput('a@example.com')
    act(() => view.rerender({ urlValue: 'b@example.com' }))
    expect(view.result.current[0]).toBe('b@example.com')
  })

  it('commits debounced edits and maps empty to undefined', async () => {
    const { onCommit, view } = renderInput('old')
    act(() => view.result.current[1]('ty'))
    await waitFor(() => expect(onCommit).toHaveBeenCalledWith('ty'), { timeout: DEBOUNCE_WAIT_MS })

    // clearing the box must remove the filter, not commit an empty string
    act(() => view.result.current[1](''))
    await waitFor(() => expect(onCommit).toHaveBeenCalledWith(undefined), {
      timeout: DEBOUNCE_WAIT_MS,
    })
  })

  it('keeps uncommitted keystrokes when the debounced round-trip lands late', async () => {
    const { onCommit, view } = renderInput()

    act(() => view.result.current[1]('ty'))
    // debounce (500ms) fires → parent navigates → URL value becomes 'ty'
    await waitFor(() => expect(onCommit).toHaveBeenCalledWith('ty'), { timeout: DEBOUNCE_WAIT_MS })

    // user keeps typing before the URL update is applied
    act(() => view.result.current[1]('typ'))
    act(() => view.rerender({ urlValue: 'ty' }))

    expect(view.result.current[0]).toBe('typ')
  })
})
