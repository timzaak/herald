import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { CodeInput } from '../code-input'

describe('CodeInput', () => {
  it('auto-formats input to XXXX-XXXX after 4 characters', async () => {
    render(<CodeInput onSubmit={vi.fn()} />)

    const input = screen.getByTestId('device-code-input')
    await userEvent.type(input, 'BCDFGHJK')

    expect(input).toHaveValue('BCDF-GHJK')
  })

  it('does not insert hyphen when 4 or fewer characters', async () => {
    render(<CodeInput onSubmit={vi.fn()} />)

    const input = screen.getByTestId('device-code-input')
    await userEvent.type(input, 'BCDF')

    expect(input).toHaveValue('BCDF')
  })

  it('filters invalid characters (only BCDFGHJKMNPQRSTVWXYZ allowed)', async () => {
    render(<CodeInput onSubmit={vi.fn()} />)

    const input = screen.getByTestId('device-code-input')
    // A, E, I, O, U are invalid; digits and special chars too
    await userEvent.type(input, 'AB1C!D')

    // Only B, C, D should survive
    expect(input).toHaveValue('BCD')
  })

  it('uppercases input automatically', async () => {
    render(<CodeInput onSubmit={vi.fn()} />)

    const input = screen.getByTestId('device-code-input')
    await userEvent.type(input, 'bcdfghjk')

    expect(input).toHaveValue('BCDF-GHJK')
  })

  it('limits input to 8 valid characters', async () => {
    render(<CodeInput onSubmit={vi.fn()} />)

    const input = screen.getByTestId('device-code-input')
    await userEvent.type(input, 'BCDFGHJKMNPQ')

    // Only first 8 valid chars kept
    expect(input).toHaveValue('BCDF-GHJK')
  })

  it('disables submit button when fewer than 8 valid characters', () => {
    render(<CodeInput onSubmit={vi.fn()} />)

    const submitButton = screen.getByTestId('device-code-submit')
    expect(submitButton).toBeDisabled()
  })

  it('enables submit button when 8 valid characters entered', async () => {
    render(<CodeInput onSubmit={vi.fn()} />)

    const input = screen.getByTestId('device-code-input')
    await userEvent.type(input, 'BCDFGHJK')

    const submitButton = screen.getByTestId('device-code-submit')
    expect(submitButton).toBeEnabled()
  })

  it('calls onSubmit with formatted code (XXXX-XXXX) on valid submit', async () => {
    const onSubmit = vi.fn()
    render(<CodeInput onSubmit={onSubmit} />)

    const input = screen.getByTestId('device-code-input')
    await userEvent.type(input, 'BCDFGHJK')

    const submitButton = screen.getByTestId('device-code-submit')
    await userEvent.click(submitButton)

    expect(onSubmit).toHaveBeenCalledWith('BCDF-GHJK')
  })

  it('does not call onSubmit with empty input', async () => {
    const onSubmit = vi.fn()
    render(<CodeInput onSubmit={onSubmit} />)

    // Try to submit empty form via pressing Enter
    const input = screen.getByTestId('device-code-input')
    await userEvent.type(input, '{Enter}')

    expect(onSubmit).not.toHaveBeenCalled()
  })

  it('does not call onSubmit with partial input', async () => {
    const onSubmit = vi.fn()
    render(<CodeInput onSubmit={onSubmit} />)

    const input = screen.getByTestId('device-code-input')
    await userEvent.type(input, 'BCDF{Enter}')

    expect(onSubmit).not.toHaveBeenCalled()
  })

  it('uses defaultValue when provided', () => {
    render(<CodeInput onSubmit={vi.fn()} defaultValue="BCDFGHJK" />)

    const input = screen.getByTestId('device-code-input')
    expect(input).toHaveValue('BCDF-GHJK')
  })
})
