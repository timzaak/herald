import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { PlatformSignupConfigForm } from '../platform-signup-config-form'
import type { PlatformSignupConfigForm as PlatformSignupConfigFormData } from '@/lib/schemas/realm-config'

describe('PlatformSignupConfigForm', () => {
  const mockOnSave = vi.fn()
  const defaultProps = {
    onSave: mockOnSave,
    isLoading: false,
  }

  beforeEach(() => {
    mockOnSave.mockClear()
  })

  it('GIVEN no initial config WHEN rendering THEN defaults the toggle to off (fail-closed, DEC-013)', async () => {
    render(<PlatformSignupConfigForm {...defaultProps} />)

    expect(screen.getByTestId('platform-signup-switch')).not.toBeChecked()
    expect(screen.getByTestId('platform-signup-save-button')).toBeInTheDocument()
  })

  it('GIVEN initial config enabled WHEN rendering THEN reflects the stored value', async () => {
    const initialConfig: PlatformSignupConfigFormData = { enabled: true }
    const screen = render(
      <PlatformSignupConfigForm {...defaultProps} initialConfig={initialConfig} />
    )

    expect(screen.getByTestId('platform-signup-switch')).toBeChecked()
  })

  it('GIVEN the toggle is flipped on WHEN submitting THEN calls onSave with enabled:true', async () => {
    mockOnSave.mockResolvedValue(undefined)
    const screen = render(<PlatformSignupConfigForm {...defaultProps} />)

    await userEvent.click(screen.getByTestId('platform-signup-switch'))
    await userEvent.click(screen.getByTestId('platform-signup-save-button'))

    await waitFor(() => {
      expect(mockOnSave).toHaveBeenCalledWith({ enabled: true })
    })
  })
})
