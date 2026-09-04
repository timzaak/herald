import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { PasskeyConfigForm } from '../passkey-config-form'
import type { PasskeyConfigForm as PasskeyConfigFormValues } from '@/lib/schemas/realm-config'

/**
 * Passkey realm configuration form (FE-D02).
 *
 * Mirrors `totp-config-form.test.tsx`: the form is a presentational editor
 * driven by an `onSave` callback. We assert the switches reflect their initial
 * config, that toggling composes the right payload, and the submission guard /
 * disabled / error states behave. The P1 fields (`userVerification`,
 * `crossPlatformAuthenticator`) are exercised alongside the P0 booleans.
 */
describe('PasskeyConfigForm', () => {
  const mockOnSave = vi.fn()
  const defaultProps = {
    realmId: 'admin',
    onSave: mockOnSave,
    isLoading: false,
  }

  beforeEach(() => {
    mockOnSave.mockClear()
  })

  it('GIVEN form is rendered WHEN no initial config THEN should display all controls with default values', async () => {
    const screen = render(<PasskeyConfigForm {...defaultProps} />)

    expect(screen.getByTestId('passkey-enabled-switch')).toBeInTheDocument()
    expect(screen.getByTestId('passkey-cross-platform-switch')).toBeInTheDocument()
    expect(screen.getByTestId('passkey-user-verification-select')).toBeInTheDocument()
    expect(screen.getByTestId('passkey-save-button')).toBeInTheDocument()

    expect(screen.getByTestId('passkey-enabled-switch')).not.toBeChecked()
    expect(screen.getByTestId('passkey-cross-platform-switch')).toBeChecked()
  })

  it('GIVEN initial config provided WHEN rendering THEN should reflect the supplied values', async () => {
    const initialConfig: PasskeyConfigFormValues = {
      enabled: true,
      userVerification: 'required',
      crossPlatformAuthenticator: false,
    }

    const screen = render(<PasskeyConfigForm {...defaultProps} initialConfig={initialConfig} />)

    expect(screen.getByTestId('passkey-enabled-switch')).toBeChecked()
    expect(screen.getByTestId('passkey-cross-platform-switch')).not.toBeChecked()
  })

  it('GIVEN user toggles enabled switch WHEN submitting THEN should call onSave with enabled true and defaults preserved', async () => {
    mockOnSave.mockResolvedValue(undefined)
    const screen = render(<PasskeyConfigForm {...defaultProps} />)

    await userEvent.click(screen.getByTestId('passkey-enabled-switch'))
    await userEvent.click(screen.getByTestId('passkey-save-button'))

    await waitFor(() => {
      expect(mockOnSave).toHaveBeenCalledWith({
        enabled: true,
        forceEnabled: false,
        userVerification: 'preferred',
        crossPlatformAuthenticator: true,
      })
    })
  })

  it('GIVEN isLoading prop is true WHEN rendering THEN should disable save button', async () => {
    const screen = render(<PasskeyConfigForm {...defaultProps} isLoading={true} />)
    expect(screen.getByTestId('passkey-save-button')).toBeDisabled()
  })

  it('GIVEN form is disabled WHEN rendering THEN should disable all controls and the save button', async () => {
    const screen = render(<PasskeyConfigForm {...defaultProps} disabled={true} />)

    expect(screen.getByTestId('passkey-enabled-switch')).toBeDisabled()
    expect(screen.getByTestId('passkey-cross-platform-switch')).toBeDisabled()
    expect(screen.getByTestId('passkey-save-button')).toBeDisabled()
  })

  it('GIVEN form is submitting WHEN save is in progress THEN should disable the save button', async () => {
    // The pending window must span the assertion deterministically: a fixed
    // setTimeout races against form state updates under CPU contention and the
    // button can already be re-enabled by the time waitFor polls.
    let finishSave!: () => void
    mockOnSave.mockImplementation(() => new Promise<void>((resolve) => (finishSave = resolve)))

    const screen = render(<PasskeyConfigForm {...defaultProps} />)
    const saveButton = screen.getByTestId('passkey-save-button')
    await userEvent.click(saveButton)

    await waitFor(() => {
      expect(saveButton).toBeDisabled()
    })
    finishSave()
  })
})
