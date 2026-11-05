import { fireEvent, render, screen } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { MultiWindowQuotaEditor, type MultiWindowQuotaEditorProps } from '../MultiWindowQuotaEditor'
import type { QuotaWindowInput } from '@/lib/api-generated/types.gen'

/**
 * Factory for a single quota window. Centralised so test intent reads as
 * "an hour-long window with limit 100" rather than a magic `{...}` literal,
 * and so the payload shape (`{ windowSeconds, limit }`) is asserted in one
 * place rather than scattered as hardcoded fixtures.
 */
function windowSeconds(windowSeconds: number, limit: number): QuotaWindowInput {
  return { windowSeconds, limit }
}

/** Editor is a controlled, page-agnostic component; default props for the suite. */
function defaultProps(
  overrides: Partial<MultiWindowQuotaEditorProps> = {}
): MultiWindowQuotaEditorProps {
  return {
    value: [],
    onChange: vi.fn(),
    context: 'entitlement-mapping',
    ...overrides,
  }
}

/**
 * MultiWindowQuotaEditor is the single source of truth for assembling the
 * `QuotaWindowInput[]` payload consumed by both the entitlement-mapping and
 * realm-default pages. Its invariants matter because pages trust whatever
 * array it emits and persist it nearly verbatim — a wrong payload, a lost row,
 * or a silently-violated 8-window cap would corrupt stored quota config.
 */
describe('MultiWindowQuotaEditor', () => {
  describe('rendering provided rows', () => {
    it('renders one row per provided window with stable index-based testid suffixes', () => {
      render(
        <MultiWindowQuotaEditor
          {...defaultProps({
            value: [windowSeconds(3600, 100), windowSeconds(86400, 50)],
          })}
        />
      )

      // Each row stamps the full set of index-suffixed testids downstream
      // tests (FE-T07 runner, page tests, accept items) and Playwright rely on.
      expect(screen.getByTestId('quota-window-row-0')).toBeInTheDocument()
      expect(screen.getByTestId('quota-window-length-row-0')).toBeInTheDocument()
      expect(screen.getByTestId('quota-window-unit-row-0')).toBeInTheDocument()
      expect(screen.getByTestId('quota-window-limit-row-0')).toBeInTheDocument()
      expect(screen.getByTestId('quota-window-delete-row-0')).toBeInTheDocument()

      expect(screen.getByTestId('quota-window-row-1')).toBeInTheDocument()
      expect(screen.getByTestId('quota-window-length-row-1')).toBeInTheDocument()
      expect(screen.getByTestId('quota-window-unit-row-1')).toBeInTheDocument()
      expect(screen.getByTestId('quota-window-limit-row-1')).toBeInTheDocument()
      expect(screen.getByTestId('quota-window-delete-row-1')).toBeInTheDocument()
    })

    it('shows the empty-state row and no window rows when value is empty', () => {
      render(<MultiWindowQuotaEditor {...defaultProps({ value: [] })} />)

      // Empty array is the "no free periodic quota configured" state — must
      // be visually distinct so operators don't mistake a blank table for a
      // load failure.
      expect(screen.getByTestId('quota-window-empty-row')).toBeInTheDocument()
      expect(screen.queryByTestId('quota-window-row-0')).not.toBeInTheDocument()
    })
  })

  describe('adding windows', () => {
    it('appends a new {windowSeconds:3600, limit:0} row and emits the full next array', async () => {
      const onChange = vi.fn()
      const user = userEvent.setup()
      render(
        <MultiWindowQuotaEditor
          {...defaultProps({
            value: [windowSeconds(7200, 10)],
            onChange,
          })}
        />
      )

      await user.click(screen.getByTestId('quota-window-add-button'))

      // Design step 4: new windows default to 1 hour, limit 0 — a safe,
      // valid baseline the operator can edit. The full array (not a delta)
      // is emitted so pages stay dumb sinks.
      expect(onChange).toHaveBeenCalledTimes(1)
      expect(onChange).toHaveBeenCalledWith([
        { windowSeconds: 7200, limit: 10 },
        { windowSeconds: 3600, limit: 0 },
      ])
    })
  })

  describe('removing windows', () => {
    it('deletes the targeted row and preserves the remaining rows verbatim', async () => {
      const onChange = vi.fn()
      const user = userEvent.setup()
      render(
        <MultiWindowQuotaEditor
          {...defaultProps({
            value: [windowSeconds(3600, 1), windowSeconds(7200, 2), windowSeconds(10800, 3)],
            onChange,
          })}
        />
      )

      await user.click(screen.getByTestId('quota-window-delete-row-1'))

      // Removing the middle row must keep both untouched windows byte-for-byte
      // — silently dropping/cloning a neighbouring window would corrupt quota.
      expect(onChange).toHaveBeenCalledWith([
        { windowSeconds: 3600, limit: 1 },
        { windowSeconds: 10800, limit: 3 },
      ])
    })
  })

  describe('field editing', () => {
    it('scales the length input by the active unit so windowSeconds stays absolute', () => {
      const onChange = vi.fn()
      render(
        <MultiWindowQuotaEditor
          {...defaultProps({
            // 3600s derives unit=hours, amount=1.
            value: [windowSeconds(3600, 0)],
            onChange,
          })}
        />
      )

      // Typing "5" in the length box means "5 of the current unit" (hours
      // here) → 5*3600=18000 seconds. This is why the unit selector exists:
      // operators think in human units, the wire format is always seconds.
      fireEvent.change(screen.getByTestId('quota-window-length-row-0'), {
        target: { value: '5' },
      })

      expect(onChange).toHaveBeenCalledWith([{ windowSeconds: 18000, limit: 0 }])
    })

    it('switching unit keeps the length number stable and recomputes windowSeconds in the new unit', async () => {
      let current: QuotaWindowInput[] = [windowSeconds(86400, 0)]
      const onChange = vi.fn((next: QuotaWindowInput[]) => {
        current = next
      })
      const user = userEvent.setup()
      const { rerender } = render(
        <MultiWindowQuotaEditor
          {...defaultProps({
            // 86400s derives unit=days, amount=1.
            value: current,
            onChange,
          })}
        />
      )

      // Open the Select and pick "hours": the displayed length number (1) must
      // stay stable while the absolute windowSeconds is recomputed in the new
      // unit (1 hour = 3600s). The unit selector is a true unit-conversion
      // control, not a presentation-only affordance. Radix Select needs real
      // pointer events, hence userEvent rather than fireEvent.
      await user.click(screen.getByTestId('quota-window-unit-row-0'))
      await user.click(await screen.findByRole('option', { name: 'hours' }))

      expect(onChange).toHaveBeenCalledWith([{ windowSeconds: 3600, limit: 0 }])
      // Apply the emitted value like a controlled parent would, then the length
      // input reflects 1 hour in the now-selected "hours" unit.
      rerender(<MultiWindowQuotaEditor {...defaultProps({ value: current, onChange })} />)
      expect(screen.getByTestId('quota-window-length-row-0')).toHaveValue(1)
    })

    it('emits limit as a non-negative integer', () => {
      const onChange = vi.fn()
      render(
        <MultiWindowQuotaEditor {...defaultProps({ value: [windowSeconds(3600, 5)], onChange })} />
      )

      fireEvent.change(screen.getByTestId('quota-window-limit-row-0'), {
        target: { value: '42' },
      })

      expect(onChange).toHaveBeenCalledWith([{ windowSeconds: 3600, limit: 42 }])
    })
  })

  describe('validation surfacing (error prop)', () => {
    // The editor stays page-agnostic: it does NOT own the save gate. Pages
    // run quotaWindowSchema/quotaWindowsSchema and pass per-row errors in;
    // the editor's job is to surface them so the operator can see WHICH row
    // is invalid. These tests pin that contract so a future refactor can't
    // silently swallow per-row errors (which would make schema failures
    // invisible to operators).

    it('marks the length input invalid and shows the inline message when error.windowSeconds is present', () => {
      render(
        <MultiWindowQuotaEditor
          {...defaultProps({
            value: [windowSeconds(0, 0)],
            error: { 0: { windowSeconds: 'Window length must be positive' } },
          })}
        />
      )

      const lengthInput = screen.getByTestId('quota-window-length-row-0')
      expect(lengthInput).toHaveAttribute('aria-invalid', 'true')
      // The inline message is what actually tells the operator the row is
      // bad — aria-invalid alone is invisible to sighted users. Query by the
      // message text because that's the operator-visible signal.
      expect(screen.getByText('Window length must be positive')).toBeInTheDocument()
    })

    it('marks the limit input invalid and shows the inline message when error.limit is present', () => {
      render(
        <MultiWindowQuotaEditor
          {...defaultProps({
            value: [windowSeconds(3600, -1)],
            error: { 0: { limit: 'Limit must be at least 0' } },
          })}
        />
      )

      const limitInput = screen.getByTestId('quota-window-limit-row-0')
      expect(limitInput).toHaveAttribute('aria-invalid', 'true')
      expect(screen.getByText('Limit must be at least 0')).toBeInTheDocument()
    })

    it('does not mark inputs invalid when no error is supplied for that row', () => {
      render(<MultiWindowQuotaEditor {...defaultProps({ value: [windowSeconds(3600, 5)] })} />)

      // The Input primitive always serializes aria-invalid (as "false" when
      // valid); what matters is that no per-row error message is surfaced to
      // the operator and the inputs read as valid. (The impact-alert `Alert`
      // banner is always present and is unrelated to per-row validation.)
      expect(screen.getByTestId('quota-window-length-row-0')).toHaveAttribute(
        'aria-invalid',
        'false'
      )
      expect(screen.getByTestId('quota-window-limit-row-0')).toHaveAttribute(
        'aria-invalid',
        'false'
      )
      expect(screen.queryByText('Window length must be positive')).not.toBeInTheDocument()
      expect(screen.queryByText('Limit must be at least 0')).not.toBeInTheDocument()
    })
  })

  describe('8-window ceiling', () => {
    // PRD §4 hard-caps quota config at 8 windows. quotaWindowsSchema enforces
    // the same cap on the wire, so the editor's add-button MUST disable at
    // exactly the same threshold — otherwise operators could assemble a
    // 9-window array that the schema then rejects on save, a confusing
    // mismatch between UI and persistence.

    function eightWindows(): QuotaWindowInput[] {
      return Array.from({ length: 8 }, (_, i) => windowSeconds(3600 * (i + 1), i * 10))
    }

    it('disables the add button and shows the window-cap badge once 8 windows exist', () => {
      render(<MultiWindowQuotaEditor {...defaultProps({ value: eightWindows() })} />)

      expect(screen.getByTestId('quota-window-add-button')).toBeDisabled()
      // The badge is the operator-facing signal that the cap is a deliberate
      // limit, not a UI bug.
      expect(screen.getByTestId('quota-window-window-cap')).toBeInTheDocument()
    })

    it('re-enables add after a row is removed below the cap', () => {
      const eight = eightWindows()
      const onChange = vi.fn()
      const { rerender } = render(
        <MultiWindowQuotaEditor {...defaultProps({ value: eight, onChange })} />
      )
      expect(screen.getByTestId('quota-window-add-button')).toBeDisabled()

      // Simulate the page applying the remove-row onChange: re-render the
      // SAME mounted editor with 7 windows. Two separate render() calls would
      // leave both editors in the DOM and collide on shared testids.
      const nextValue = eight.slice(0, 7)
      rerender(<MultiWindowQuotaEditor {...defaultProps({ value: nextValue, onChange })} />)

      expect(screen.getByTestId('quota-window-add-button')).toBeEnabled()
      expect(screen.queryByTestId('quota-window-window-cap')).not.toBeInTheDocument()
    })
  })

  describe('context and testIdPrefix', () => {
    it('renders the entitlement-mapping impact message by default', async () => {
      const user = userEvent.setup()
      render(
        <MultiWindowQuotaEditor
          {...defaultProps({
            value: [],
            context: 'entitlement-mapping',
          })}
        />
      )

      // The impact tooltip is the only context-driven difference and is the
      // operator's guarantee that editing here won't retroactively change
      // already-issued entitlements.
      await user.hover(screen.getByTestId('quota-window-impact-tooltip'))
      expect(await screen.findByTestId('quota-window-impact-tooltip-content')).toHaveTextContent(
        /future granted quota/i
      )
    })

    it('renders the realm-default impact message and a custom testid prefix', async () => {
      const user = userEvent.setup()
      render(
        <MultiWindowQuotaEditor
          {...defaultProps({
            value: [windowSeconds(3600, 1)],
            context: 'realm-default',
            testIdPrefix: 'default-quota',
          })}
        />
      )

      // Custom prefix lets a page embed two editors without testid clashes.
      expect(screen.getByTestId('default-quota-editor')).toBeInTheDocument()
      expect(screen.getByTestId('default-quota-length-row-0')).toBeInTheDocument()
      expect(screen.getByTestId('default-quota-add-button')).toBeInTheDocument()
      await user.hover(screen.getByTestId('default-quota-impact-tooltip'))
      expect(await screen.findByTestId('default-quota-impact-tooltip-content')).toHaveTextContent(
        /new registered users/i
      )
    })
  })

  describe('payload shape', () => {
    it('every emitted onChange entry is exactly {windowSeconds:number, limit:number}', async () => {
      const onChange = vi.fn()
      const user = userEvent.setup()
      render(<MultiWindowQuotaEditor {...defaultProps({ value: [], onChange })} />)

      await user.click(screen.getByTestId('quota-window-add-button'))

      const emitted = onChange.mock.calls[0][0] as QuotaWindowInput[]
      expect(emitted).toHaveLength(1)
      // Pin the wire shape callers depend on: no stray keys, both fields
      // present, both numbers.
      expect(Object.keys(emitted[0]).sort()).toEqual(['limit', 'windowSeconds'])
      expect(typeof emitted[0].windowSeconds).toBe('number')
      expect(typeof emitted[0].limit).toBe('number')
    })
  })
})
