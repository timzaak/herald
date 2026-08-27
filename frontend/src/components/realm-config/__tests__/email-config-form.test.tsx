import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import React from 'react'
import { EmailConfigForm } from '../email-config-form'
import { renderWithProviders } from '@/test/utils/render'
import type { EmailConfigForm as EmailConfigFormValues } from '@/lib/schemas/realm-config'

// Mock the API generated SDK
vi.mock('@/lib/api-generated/sdk.gen', () => ({
  emailTest: vi.fn(),
}))

// Mock the api-utils
vi.mock('@/lib/api-utils', () => ({
  handleApiResponse: vi.fn((response) => response.data),
}))

import { emailTest } from '@/lib/api-generated/sdk.gen'
import { handleApiResponse } from '@/lib/api-utils'

describe('EmailConfigForm', () => {
  const mockOnSave = vi.fn()
  const defaultProps = {
    realmId: 'test-realm',
    onSave: mockOnSave,
    isLoading: false,
  }

  beforeEach(() => {
    mockOnSave.mockClear()
    vi.mocked(emailTest).mockReset()
    vi.mocked(handleApiResponse).mockReset()
  })

  it('GIVEN form is rendered WHEN no initial config THEN should display form with default values', () => {
    renderWithProviders(<EmailConfigForm {...defaultProps} />)

    expect(screen.getByTestId('email-provider-resend')).toBeInTheDocument()
    expect(screen.getByTestId('email-provider-smtp')).toBeInTheDocument()
    expect(screen.getByTestId('email-from-address-input')).toBeInTheDocument()
    expect(screen.getByTestId('email-resend-api-key-input')).toBeInTheDocument()
    expect(screen.getByTestId('email-save-button')).toBeInTheDocument()
    expect(screen.getByTestId('email-test-button')).toBeInTheDocument()
  })

  it('GIVEN initial resend config WHEN rendering THEN should display resend fields', () => {
    const initialConfig: EmailConfigFormValues = {
      provider: 'resend',
      fromAddress: 'noreply@example.com',
      resendApiKey: 're_test_key',
      smtpPort: '587',
      smtpEncryption: 'starttls',
    }

    renderWithProviders(<EmailConfigForm {...defaultProps} initialConfig={initialConfig} />)

    // Provider radio should reflect resend
    const resendRadio = screen.getByTestId('email-provider-resend')
    expect(resendRadio).toBeChecked()

    // From address should be populated
    const fromInput = screen.getByTestId('email-from-address-input') as HTMLInputElement
    expect(fromInput.value).toBe('noreply@example.com')

    // Resend API key field should be visible
    expect(screen.getByTestId('email-resend-api-key-input')).toBeInTheDocument()

    // SMTP fields should NOT be visible
    expect(screen.queryByTestId('email-smtp-host-input')).not.toBeInTheDocument()
  })

  it('GIVEN initial smtp config WHEN rendering THEN should display smtp fields', () => {
    const initialConfig: EmailConfigFormValues = {
      provider: 'smtp',
      fromAddress: 'noreply@example.com',
      smtpHost: 'smtp.example.com',
      smtpPort: '465',
      smtpUsername: 'user@example.com',
      smtpPassword: 'password123',
      smtpEncryption: 'ssl',
    }

    renderWithProviders(<EmailConfigForm {...defaultProps} initialConfig={initialConfig} />)

    // Provider radio should reflect smtp
    const smtpRadio = screen.getByTestId('email-provider-smtp')
    expect(smtpRadio).toBeChecked()

    // SMTP fields should be visible
    expect(screen.getByTestId('email-smtp-host-input')).toBeInTheDocument()
    expect(screen.getByTestId('email-smtp-port-input')).toBeInTheDocument()
    expect(screen.getByTestId('email-smtp-encryption-select')).toBeInTheDocument()
    expect(screen.getByTestId('email-smtp-username-input')).toBeInTheDocument()
    expect(screen.getByTestId('email-smtp-password-input')).toBeInTheDocument()

    // Resend API key field should NOT be visible
    expect(screen.queryByTestId('email-resend-api-key-input')).not.toBeInTheDocument()
  })

  it('GIVEN provider is resend WHEN switching to smtp THEN should show smtp fields and hide resend fields', async () => {
    renderWithProviders(<EmailConfigForm {...defaultProps} />)

    // Initially resend is selected (default)
    expect(screen.getByTestId('email-resend-api-key-input')).toBeInTheDocument()
    expect(screen.queryByTestId('email-smtp-host-input')).not.toBeInTheDocument()

    // Switch to SMTP
    await userEvent.click(screen.getByTestId('email-provider-smtp'))

    // SMTP fields should now be visible
    await waitFor(() => {
      expect(screen.getByTestId('email-smtp-host-input')).toBeInTheDocument()
      expect(screen.getByTestId('email-smtp-port-input')).toBeInTheDocument()
      expect(screen.getByTestId('email-smtp-encryption-select')).toBeInTheDocument()
      expect(screen.getByTestId('email-smtp-username-input')).toBeInTheDocument()
      expect(screen.getByTestId('email-smtp-password-input')).toBeInTheDocument()
    })

    // Resend field should be hidden
    expect(screen.queryByTestId('email-resend-api-key-input')).not.toBeInTheDocument()
  })

  it('GIVEN provider is smtp WHEN switching to resend THEN should show resend fields and hide smtp fields', async () => {
    const initialConfig: EmailConfigFormValues = {
      provider: 'smtp',
      fromAddress: 'test@example.com',
      smtpHost: 'smtp.example.com',
      smtpPort: '587',
      smtpEncryption: 'starttls',
    }

    renderWithProviders(<EmailConfigForm {...defaultProps} initialConfig={initialConfig} />)

    // Initially smtp is selected
    expect(screen.getByTestId('email-smtp-host-input')).toBeInTheDocument()
    expect(screen.queryByTestId('email-resend-api-key-input')).not.toBeInTheDocument()

    // Switch to Resend
    await userEvent.click(screen.getByTestId('email-provider-resend'))

    // Resend field should now be visible
    await waitFor(() => {
      expect(screen.getByTestId('email-resend-api-key-input')).toBeInTheDocument()
    })

    // SMTP fields should be hidden
    expect(screen.queryByTestId('email-smtp-host-input')).not.toBeInTheDocument()
  })

  it('GIVEN valid form WHEN clicking save THEN should call onSave with form values', async () => {
    mockOnSave.mockResolvedValue(undefined)
    renderWithProviders(<EmailConfigForm {...defaultProps} />)

    // Fill in from_address
    await userEvent.type(screen.getByTestId('email-from-address-input'), 'noreply@example.com')

    // Click save
    await userEvent.click(screen.getByTestId('email-save-button'))

    await waitFor(() => {
      expect(mockOnSave).toHaveBeenCalledWith(
        expect.objectContaining({
          provider: 'resend',
          fromAddress: 'noreply@example.com',
        })
      )
    })
  })

  it('GIVEN form is disabled WHEN rendering THEN should disable save button and all fields', () => {
    renderWithProviders(<EmailConfigForm {...defaultProps} disabled={true} />)

    expect(screen.getByTestId('email-save-button')).toBeDisabled()
    expect(screen.getByTestId('email-test-button')).toBeDisabled()
    expect(screen.getByTestId('email-from-address-input')).toBeDisabled()
    expect(screen.getByTestId('email-provider-resend')).toBeDisabled()
    expect(screen.getByTestId('email-provider-smtp')).toBeDisabled()
    expect(screen.getByTestId('email-resend-api-key-input')).toBeDisabled()
  })

  it('GIVEN save is in progress WHEN submitting THEN should disable save button', async () => {
    // The pending window must span the assertion deterministically: a fixed
    // setTimeout races against form state updates under CPU contention and the
    // button can already be re-enabled by the time waitFor polls.
    let finishSave!: () => void
    mockOnSave.mockImplementation(() => new Promise<void>((resolve) => (finishSave = resolve)))

    renderWithProviders(<EmailConfigForm {...defaultProps} />)

    const saveButton = screen.getByTestId('email-save-button')
    await userEvent.click(saveButton)

    await waitFor(() => {
      expect(saveButton).toBeDisabled()
    })
    finishSave()
  })

  it('GIVEN emailStatus is configured WHEN rendering THEN should show green badge', () => {
    renderWithProviders(
      <EmailConfigForm {...defaultProps} emailStatus={{ configured: true, missingFields: [] }} />
    )

    const badge = screen.getByTestId('email-config-status-badge')
    expect(badge).toBeInTheDocument()
    expect(badge.textContent).toBe('Email is configured')
  })

  it('GIVEN emailStatus is not configured WHEN rendering THEN should show amber badge', () => {
    renderWithProviders(
      <EmailConfigForm
        {...defaultProps}
        emailStatus={{ configured: false, missingFields: ['provider', 'from_address'] }}
      />
    )

    const badge = screen.getByTestId('email-config-status-badge')
    expect(badge).toBeInTheDocument()
    expect(badge.textContent).toBe('Email is not configured')
  })

  it('GIVEN no emailStatus WHEN rendering THEN should not show badge', () => {
    renderWithProviders(<EmailConfigForm {...defaultProps} />)

    expect(screen.queryByTestId('email-config-status-badge')).not.toBeInTheDocument()
  })

  it('GIVEN test email succeeds WHEN clicking test button THEN should show success message', async () => {
    vi.mocked(emailTest).mockResolvedValue({
      data: { success: true, message: 'Test email sent' },
      error: undefined,
    } as never)
    vi.mocked(handleApiResponse).mockReturnValue({ success: true, message: 'Test email sent' })

    renderWithProviders(<EmailConfigForm {...defaultProps} />)

    // Type recipient
    await userEvent.type(screen.getByTestId('email-test-recipient-input'), 'test@example.com')

    // Click test button
    await userEvent.click(screen.getByTestId('email-test-button'))

    await waitFor(() => {
      expect(screen.getByTestId('email-test-success')).toBeInTheDocument()
    })
  })

  it('GIVEN test email fails WHEN clicking test button THEN should show error message', async () => {
    vi.mocked(handleApiResponse).mockImplementation(() => {
      throw new Error('Failed to send test email')
    })

    renderWithProviders(<EmailConfigForm {...defaultProps} />)

    // Type recipient
    await userEvent.type(screen.getByTestId('email-test-recipient-input'), 'test@example.com')

    // Click test button
    await userEvent.click(screen.getByTestId('email-test-button'))

    await waitFor(() => {
      expect(screen.getByTestId('email-test-error')).toBeInTheDocument()
    })
  })

  // --- Email-OTP integration (merged into the email card) -------------------
  // WHY: OTP login can only send codes if the email channel is configured, so
  // the OTP switches must be gated on `emailStatus.configured`. These tests
  // pin that dependency so a regression that lets admins enable OTP without
  // email (and silently break login) is caught.
  describe('Email-OTP integration', () => {
    const mockOnSaveOtp = vi.fn()
    const propsWithOtp = {
      ...defaultProps,
      onSaveEmailOtp: mockOnSaveOtp,
    }

    beforeEach(() => {
      mockOnSaveOtp.mockClear()
    })

    it('GIVEN email not configured WHEN rendering THEN should disable OTP switches and show hint', () => {
      renderWithProviders(
        <EmailConfigForm
          {...propsWithOtp}
          emailStatus={{ configured: false, missingFields: ['provider'] }}
        />
      )

      expect(screen.getByTestId('email-otp-section')).toBeInTheDocument()
      expect(screen.getByTestId('email-otp-enabled-switch')).toBeDisabled()
      expect(screen.getByTestId('email-otp-auto-register-switch')).toBeDisabled()
      expect(screen.getByTestId('email-otp-save-button')).toBeDisabled()
      expect(screen.getByTestId('email-otp-email-required-hint')).toBeInTheDocument()
    })

    it('GIVEN email configured WHEN rendering THEN should enable OTP switches and hide hint', () => {
      renderWithProviders(
        <EmailConfigForm {...propsWithOtp} emailStatus={{ configured: true, missingFields: [] }} />
      )

      expect(screen.getByTestId('email-otp-enabled-switch')).not.toBeDisabled()
      expect(screen.getByTestId('email-otp-auto-register-switch')).not.toBeDisabled()
      expect(screen.getByTestId('email-otp-save-button')).not.toBeDisabled()
      expect(screen.queryByTestId('email-otp-email-required-hint')).not.toBeInTheDocument()
    })

    it('GIVEN no onSaveEmailOtp provided WHEN rendering THEN should not render the OTP section', () => {
      // Without the OTP save handler the merged OTP block is hidden, keeping
      // the email-only usage (and the legacy tests above) intact.
      renderWithProviders(<EmailConfigForm {...defaultProps} />)

      expect(screen.queryByTestId('email-otp-section')).not.toBeInTheDocument()
    })

    it('GIVEN email configured WHEN toggling enabled and saving THEN should call onSaveEmailOtp with the OTP values', async () => {
      mockOnSaveOtp.mockResolvedValue(undefined)
      renderWithProviders(
        <EmailConfigForm {...propsWithOtp} emailStatus={{ configured: true, missingFields: [] }} />
      )

      await userEvent.click(screen.getByTestId('email-otp-enabled-switch'))
      await userEvent.click(screen.getByTestId('email-otp-save-button'))

      await waitFor(() => {
        expect(mockOnSaveOtp).toHaveBeenCalledWith({
          enabled: true,
          autoRegister: false,
        })
      })
    })
  })
})
