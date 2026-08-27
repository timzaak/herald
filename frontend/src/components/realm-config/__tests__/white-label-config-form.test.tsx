import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, within, fireEvent } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { WhiteLabelConfigForm, type WhiteLabelConfigFormProps } from '../white-label-config-form'
import { emptyWhiteLabelConfig } from '@/lib/realm-config-utils'
import type { WhiteLabelConfigForm as WhiteLabelConfigFormValues } from '@/lib/schemas/realm-config'

/**
 * White-label config form (FE-D04).
 *
 * The form is a presentational editor: it owns field rendering, the live
 * preview, the contrast warning, and the draft/publish/discard/restore
 * action entry points, but performs no data access — every action is surfaced
 * via a callback so FE-D05 can wire up the queries/mutations. These tests
 * assert the contract FE-D05 will rely on.
 */
describe('WhiteLabelConfigForm', () => {
  const mockOnSaveDraft = vi.fn()
  const mockOnPublish = vi.fn()
  const mockOnDiscardDraft = vi.fn()
  const mockOnRestore = vi.fn()

  const defaultProps: WhiteLabelConfigFormProps = {
    realmId: 'admin',
    initialConfig: emptyWhiteLabelConfig(),
    hasDraft: false,
    hasPrevious: false,
    disabled: false,
    onSaveDraft: mockOnSaveDraft,
    onPublish: mockOnPublish,
    onDiscardDraft: mockOnDiscardDraft,
    onRestore: mockOnRestore,
  }

  beforeEach(() => {
    mockOnSaveDraft.mockClear()
    mockOnPublish.mockClear()
    mockOnDiscardDraft.mockClear()
    mockOnRestore.mockClear()
    mockOnSaveDraft.mockResolvedValue(undefined)
    mockOnPublish.mockResolvedValue(undefined)
    mockOnDiscardDraft.mockResolvedValue(undefined)
    mockOnRestore.mockResolvedValue(undefined)
  })

  it('GIVEN form is rendered WHEN no initial config THEN should display all brand fields with empty values', async () => {
    const screen = render(<WhiteLabelConfigForm {...defaultProps} />)

    // Core brand fields
    expect(screen.getByTestId('white-label-logo-url')).toHaveValue('')
    expect(screen.getByTestId('white-label-accent-color')).toHaveValue('')
    expect(screen.getByTestId('white-label-background-type')).toBeInTheDocument()
    expect(screen.getByTestId('white-label-footer-text')).toHaveValue('')

    // Login copy
    expect(screen.getByTestId('white-label-login-title')).toHaveValue('')
    expect(screen.getByTestId('white-label-login-subtitle')).toHaveValue('')

    // Register copy
    expect(screen.getByTestId('white-label-register-title')).toHaveValue('')
    expect(screen.getByTestId('white-label-register-subtitle')).toHaveValue('')

    // Action buttons
    expect(screen.getByTestId('white-label-save-draft')).toBeInTheDocument()
    expect(screen.getByTestId('white-label-publish')).toBeInTheDocument()
    expect(screen.getByTestId('white-label-discard-draft')).toBeInTheDocument()
    expect(screen.getByTestId('white-label-restore')).toBeInTheDocument()
  })

  it('GIVEN initial config provided WHEN rendering THEN should reflect the supplied values', async () => {
    const initialConfig: WhiteLabelConfigFormValues = {
      logoUrl: 'https://cdn.example.com/logo.svg',
      accentColor: '#2563eb',
      background: { type: 'gradient', value: 'linear-gradient(135deg, #000, #fff)' },
      footerText: '© Example Inc.',
      loginTitle: 'Sign in to Example',
      loginSubtitle: 'Use your Example account',
      registerTitle: 'Create your Example account',
      registerSubtitle: 'Start with Example',
    }

    const screen = render(<WhiteLabelConfigForm {...defaultProps} initialConfig={initialConfig} />)

    expect(screen.getByTestId('white-label-logo-url')).toHaveValue(
      'https://cdn.example.com/logo.svg'
    )
    expect(screen.getByTestId('white-label-accent-color')).toHaveValue('#2563eb')
    expect(screen.getByTestId('white-label-footer-text')).toHaveValue('© Example Inc.')
    expect(screen.getByTestId('white-label-login-title')).toHaveValue('Sign in to Example')
    expect(screen.getByTestId('white-label-login-subtitle')).toHaveValue('Use your Example account')
    expect(screen.getByTestId('white-label-register-title')).toHaveValue(
      'Create your Example account'
    )
    expect(screen.getByTestId('white-label-register-subtitle')).toHaveValue('Start with Example')
  })

  it('GIVEN user types into fields WHEN editing THEN should call onSaveDraft with the new values on submit', async () => {
    const screen = render(<WhiteLabelConfigForm {...defaultProps} />)

    // Long values go in as single change events: per-keystroke typing against
    // this live-preview form drops characters when the worker is CPU-starved,
    // which scrambles the asserted payload.
    fireEvent.change(screen.getByTestId('white-label-logo-url'), {
      target: { value: 'https://cdn.example.com/logo.svg' },
    })
    fireEvent.change(screen.getByTestId('white-label-footer-text'), {
      target: { value: '© Example Inc.' },
    })
    fireEvent.change(screen.getByTestId('white-label-login-title'), {
      target: { value: 'Sign in to Example' },
    })

    await userEvent.click(screen.getByTestId('white-label-save-draft'))

    await waitFor(() => {
      expect(mockOnSaveDraft).toHaveBeenCalledTimes(1)
      expect(mockOnSaveDraft).toHaveBeenCalledWith(
        expect.objectContaining({
          logoUrl: 'https://cdn.example.com/logo.svg',
          footerText: '© Example Inc.',
          loginTitle: 'Sign in to Example',
        })
      )
    })
  })

  it('GIVEN accent color has low contrast WHEN rendering THEN should show warning and keep save/publish enabled', async () => {
    // #777777 vs white ≈ 4.48 — just below the 4.5 WCAG AA threshold.
    const initialConfig: WhiteLabelConfigFormValues = {
      ...emptyWhiteLabelConfig(),
      accentColor: '#777777',
    }

    const screen = render(<WhiteLabelConfigForm {...defaultProps} initialConfig={initialConfig} />)

    expect(screen.getByTestId('white-label-accent-warning')).toBeInTheDocument()
    // The warning must NOT block the action buttons.
    expect(screen.getByTestId('white-label-save-draft')).not.toBeDisabled()
    expect(screen.getByTestId('white-label-publish')).not.toBeDisabled()
  })

  it('GIVEN accent color meets contrast WHEN rendering THEN should not show the warning', async () => {
    // #000000 vs white = 21 — well above threshold.
    const initialConfig: WhiteLabelConfigFormValues = {
      ...emptyWhiteLabelConfig(),
      accentColor: '#000000',
    }

    const screen = render(<WhiteLabelConfigForm {...defaultProps} initialConfig={initialConfig} />)

    expect(screen.queryByTestId('white-label-accent-warning')).not.toBeInTheDocument()
  })

  it('GIVEN accent color is invalid WHEN rendering THEN should not show the contrast warning', async () => {
    const initialConfig: WhiteLabelConfigFormValues = {
      ...emptyWhiteLabelConfig(),
      accentColor: 'not-a-color',
    }

    const screen = render(<WhiteLabelConfigForm {...defaultProps} initialConfig={initialConfig} />)

    expect(screen.queryByTestId('white-label-accent-warning')).not.toBeInTheDocument()
  })

  it('GIVEN user clicks publish WHEN not disabled THEN should call onPublish with current values', async () => {
    const initialConfig: WhiteLabelConfigFormValues = {
      ...emptyWhiteLabelConfig(),
      accentColor: '#000000',
    }

    const screen = render(<WhiteLabelConfigForm {...defaultProps} initialConfig={initialConfig} />)

    // Single change event — see the draft test above for why long values are
    // not typed keystroke-by-keystroke.
    fireEvent.change(screen.getByTestId('white-label-login-title'), {
      target: { value: 'New Title' },
    })
    await userEvent.click(screen.getByTestId('white-label-publish'))

    await waitFor(() => {
      expect(mockOnPublish).toHaveBeenCalledTimes(1)
      expect(mockOnPublish).toHaveBeenCalledWith(
        expect.objectContaining({ loginTitle: 'New Title', accentColor: '#000000' })
      )
    })
  })

  it('GIVEN hasDraft is true WHEN rendering THEN should enable discard and call onDiscardDraft on click', async () => {
    const screen = render(<WhiteLabelConfigForm {...defaultProps} hasDraft={true} />)

    const discardButton = screen.getByTestId('white-label-discard-draft')
    expect(discardButton).not.toBeDisabled()

    await userEvent.click(discardButton)
    await waitFor(() => {
      expect(mockOnDiscardDraft).toHaveBeenCalledTimes(1)
    })
  })

  it('GIVEN no draft exists WHEN rendering THEN should disable the discard button', async () => {
    const screen = render(<WhiteLabelConfigForm {...defaultProps} hasDraft={false} />)
    expect(screen.getByTestId('white-label-discard-draft')).toBeDisabled()
  })

  it('GIVEN hasPrevious is true WHEN clicking restore THEN should open confirm dialog and call onRestore on confirm', async () => {
    const screen = render(<WhiteLabelConfigForm {...defaultProps} hasPrevious={true} />)

    const restoreButton = screen.getByTestId('white-label-restore')
    expect(restoreButton).not.toBeDisabled()

    await userEvent.click(restoreButton)

    const dialog = await screen.findByTestId('white-label-restore-dialog')
    expect(dialog).toBeInTheDocument()

    // The restore callback must NOT fire until the user confirms.
    expect(mockOnRestore).not.toHaveBeenCalled()

    const confirmButton = within(dialog).getByTestId('white-label-restore-confirm')
    await userEvent.click(confirmButton)

    await waitFor(() => {
      expect(mockOnRestore).toHaveBeenCalledTimes(1)
    })
  })

  it('GIVEN no previous version exists WHEN rendering THEN should disable the restore button', async () => {
    const screen = render(<WhiteLabelConfigForm {...defaultProps} hasPrevious={false} />)
    expect(screen.getByTestId('white-label-restore')).toBeDisabled()
  })

  it('GIVEN form is disabled WHEN rendering THEN should disable all inputs and action buttons', async () => {
    const screen = render(<WhiteLabelConfigForm {...defaultProps} disabled={true} />)

    expect(screen.getByTestId('white-label-logo-url')).toBeDisabled()
    expect(screen.getByTestId('white-label-accent-color')).toBeDisabled()
    expect(screen.getByTestId('white-label-background-type')).toBeDisabled()
    expect(screen.getByTestId('white-label-footer-text')).toBeDisabled()
    expect(screen.getByTestId('white-label-save-draft')).toBeDisabled()
    expect(screen.getByTestId('white-label-publish')).toBeDisabled()
    expect(screen.getByTestId('white-label-restore')).toBeDisabled()
  })

  it('GIVEN hasDraft or form is dirty WHEN rendering THEN should show the draft notice', async () => {
    // hasDraft flag alone surfaces the notice.
    const screenA = render(<WhiteLabelConfigForm {...defaultProps} hasDraft={true} />)
    expect(screenA.getByTestId('white-label-draft-notice')).toBeInTheDocument()
    screenA.unmount()

    // Editing a field also surfaces the notice (dirty form).
    const screenB = render(<WhiteLabelConfigForm {...defaultProps} hasDraft={false} />)
    expect(screenB.queryByTestId('white-label-draft-notice')).not.toBeInTheDocument()
    await userEvent.type(screenB.getByTestId('white-label-footer-text'), 'Footer')
    expect(screenB.getByTestId('white-label-draft-notice')).toBeInTheDocument()
  })

  it('GIVEN preview tabs exist WHEN rendering THEN should show login and register preview tabs', async () => {
    const screen = render(<WhiteLabelConfigForm {...defaultProps} />)

    expect(screen.getByTestId('white-label-preview-login')).toBeInTheDocument()
    expect(screen.getByTestId('white-label-preview-register')).toBeInTheDocument()

    // The login preview panel is visible by default; the register panel is
    // rendered once its tab is selected.
    expect(screen.getByTestId('white-label-preview-login-panel')).toBeInTheDocument()

    await userEvent.click(screen.getByTestId('white-label-preview-register'))
    await waitFor(() => {
      expect(screen.getByTestId('white-label-preview-register-panel')).toBeInTheDocument()
    })
  })

  it('GIVEN initial config has an image background WHEN rendering THEN should show the background value field', async () => {
    const initialConfig: WhiteLabelConfigFormValues = {
      ...emptyWhiteLabelConfig(),
      background: { type: 'image', value: 'https://cdn.example.com/bg.jpg' },
    }

    const screen = render(<WhiteLabelConfigForm {...defaultProps} initialConfig={initialConfig} />)

    // A configured background surfaces the value editor with its value.
    const valueField = screen.getByTestId('white-label-background-value')
    expect(valueField).toBeInTheDocument()
    expect(valueField).toHaveValue('https://cdn.example.com/bg.jpg')
  })

  it('GIVEN no background is configured WHEN rendering THEN should hide the background value field', async () => {
    const screen = render(<WhiteLabelConfigForm {...defaultProps} />)
    expect(screen.queryByTestId('white-label-background-value')).not.toBeInTheDocument()
  })

  it('GIVEN initial config has a gradient background WHEN rendering THEN should show the gradient-labelled value field', async () => {
    const initialConfig: WhiteLabelConfigFormValues = {
      ...emptyWhiteLabelConfig(),
      background: { type: 'gradient', value: 'linear-gradient(135deg, #000, #fff)' },
    }

    const screen = render(<WhiteLabelConfigForm {...defaultProps} initialConfig={initialConfig} />)

    const valueField = screen.getByTestId('white-label-background-value')
    expect(valueField).toBeInTheDocument()
    expect(valueField).toHaveValue('linear-gradient(135deg, #000, #fff)')
  })

  it('GIVEN an action is pending WHEN rendering THEN should disable its button', async () => {
    const screen = render(<WhiteLabelConfigForm {...defaultProps} isPublishing={true} />)

    expect(screen.getByTestId('white-label-publish')).toBeDisabled()
    // Other actions remain usable unless their own flag is set.
    expect(screen.getByTestId('white-label-save-draft')).not.toBeDisabled()
  })
})
