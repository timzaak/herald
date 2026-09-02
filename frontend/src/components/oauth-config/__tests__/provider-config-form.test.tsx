import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { ProviderConfigForm } from '../provider-config-form'
import type { OAuthConfigResponse } from '@/lib/api-generated'

// Mock the dependencies
vi.mock('@/lib/api-generated', () => ({
  createOAuthConfig: vi.fn(),
  updateOAuthConfig: vi.fn(),
}))

describe('ProviderConfigForm - Create Mode', () => {
  const mockOnSubmit = vi.fn()
  const defaultProps = {
    onSubmit: mockOnSubmit,
    isPending: false,
    onCancel: vi.fn(),
  }

  beforeEach(() => {
    mockOnSubmit.mockClear()
  })

  it('GIVEN form is reused WHEN creating different provider types THEN should correctly update form state', async () => {
    const user = userEvent.setup({ delay: null })
    mockOnSubmit.mockResolvedValue(undefined)

    const { rerender } = render(<ProviderConfigForm {...defaultProps} />)

    // First, create a Google provider
    const clientIdInput1 = screen.getByTestId('oauth-client-id-input')
    await user.clear(clientIdInput1)
    await user.type(clientIdInput1, 'google-client-id')

    const clientSecretInput1 = screen.getByTestId('oauth-client-secret-input')
    await user.clear(clientSecretInput1)
    await user.type(clientSecretInput1, 'google-secret')

    const saveButton1 = screen.getByTestId('oauth-save-provider-button')
    await user.click(saveButton1)

    // Verify first submission
    await waitFor(() => {
      expect(mockOnSubmit).toHaveBeenCalledWith(
        expect.objectContaining({
          clientId: 'google-client-id',
          clientSecret: 'google-secret',
          providerType: 'google',
        })
      )
    })

    // Reset mock for second submission
    mockOnSubmit.mockClear()

    // Rerender the form (simulating dialog reopening for second provider)
    rerender(<ProviderConfigForm {...defaultProps} />)

    // Now create a GitHub provider
    const clientIdInput2 = screen.getByTestId('oauth-client-id-input')
    await user.clear(clientIdInput2)
    await user.type(clientIdInput2, 'github-client-id')

    const clientSecretInput2 = screen.getByTestId('oauth-client-secret-input')
    await user.clear(clientSecretInput2)
    await user.type(clientSecretInput2, 'github-secret')

    // Change provider type to GitHub
    const providerSelect = screen.getByTestId('oauth-provider-type-select')
    await user.click(providerSelect)
    // Use role="option" to specifically target the SelectItem, not other elements
    const githubOption = await screen.findByRole('option', { name: 'GitHub' })
    await user.click(githubOption)

    const saveButton2 = screen.getByTestId('oauth-save-provider-button')
    await user.click(saveButton2)

    // Verify second submission
    await waitFor(() => {
      expect(mockOnSubmit).toHaveBeenCalledWith(
        expect.objectContaining({
          clientId: 'github-client-id',
          clientSecret: 'github-secret',
          providerType: 'github',
        })
      )
    })
  }, 15000)

  it('GIVEN form is in create mode WHEN filling required fields THEN should allow submission', async () => {
    const user = userEvent.setup({ delay: null })
    mockOnSubmit.mockResolvedValue(undefined)

    render(<ProviderConfigForm {...defaultProps} />)

    // Fill in required fields
    const clientIdInput = screen.getByTestId('oauth-client-id-input')
    await user.clear(clientIdInput)
    await user.type(clientIdInput, 'test-client-id')

    const clientSecretInput = screen.getByTestId('oauth-client-secret-input')
    await user.clear(clientSecretInput)
    await user.type(clientSecretInput, 'test-secret')

    // Submit form
    const saveButton = screen.getByTestId('oauth-save-provider-button')
    await user.click(saveButton)

    // Verify submission
    await waitFor(() => {
      expect(mockOnSubmit).toHaveBeenCalledWith(
        expect.objectContaining({
          clientId: 'test-client-id',
          clientSecret: 'test-secret',
          enabled: true,
        })
      )
    })
  })

  it('GIVEN form is in create mode WHEN submitting with disabled provider THEN should submit with enabled: false', async () => {
    const user = userEvent.setup({ delay: null })
    mockOnSubmit.mockResolvedValue(undefined)

    render(<ProviderConfigForm {...defaultProps} />)

    // Uncheck enabled checkbox
    const enabledCheckbox = screen.getByTestId('oauth-enabled-checkbox')
    await user.click(enabledCheckbox)

    // Fill in required fields
    const clientIdInput = screen.getByTestId('oauth-client-id-input')
    await user.clear(clientIdInput)
    await user.type(clientIdInput, 'test-client-id')

    const clientSecretInput = screen.getByTestId('oauth-client-secret-input')
    await user.clear(clientSecretInput)
    await user.type(clientSecretInput, 'test-secret')

    // Submit form
    const saveButton = screen.getByTestId('oauth-save-provider-button')
    await user.click(saveButton)

    // Verify submission with enabled: false
    await waitFor(() => {
      expect(mockOnSubmit).toHaveBeenCalledWith(
        expect.objectContaining({
          enabled: false,
        })
      )
    })
  })

  it('GIVEN cancel button is clicked WHEN clicked THEN should call onCancel', async () => {
    const user = userEvent.setup({ delay: null })
    const mockOnCancel = vi.fn()

    render(<ProviderConfigForm {...defaultProps} onCancel={mockOnCancel} />)

    const cancelButton = screen.getByTestId('oauth-cancel-provider-button')
    await user.click(cancelButton)

    expect(mockOnCancel).toHaveBeenCalledTimes(1)
  })

  it('GIVEN scopes input WHEN typing comma-separated values THEN should split into array', async () => {
    const user = userEvent.setup({ delay: null })
    mockOnSubmit.mockResolvedValue(undefined)

    render(<ProviderConfigForm {...defaultProps} />)

    // Fill in required fields
    const clientIdInput = screen.getByTestId('oauth-client-id-input')
    await user.clear(clientIdInput)
    await user.type(clientIdInput, 'test-client-id')

    const clientSecretInput = screen.getByTestId('oauth-client-secret-input')
    await user.clear(clientSecretInput)
    await user.type(clientSecretInput, 'test-secret')

    // Set custom scopes
    const scopesInput = screen.getByTestId('oauth-scopes-input')
    await user.clear(scopesInput)
    await user.type(scopesInput, 'scope1, scope2, scope3')

    // Submit form
    const saveButton = screen.getByTestId('oauth-save-provider-button')
    await user.click(saveButton)

    // Verify submission with array of scopes
    await waitFor(() => {
      expect(mockOnSubmit).toHaveBeenCalledWith(
        expect.objectContaining({
          scopes: ['scope1', 'scope2', 'scope3'],
        })
      )
    })
  })

  it('GIVEN creating wechat provider WHEN selecting wechat THEN should auto-fill scopes with snsapi_login', async () => {
    const user = userEvent.setup({ delay: null })

    render(<ProviderConfigForm {...defaultProps} />)

    // Select WeChat provider type
    const providerSelectTrigger = screen.getByTestId('oauth-provider-type-select')
    await user.click(providerSelectTrigger)

    // Wait for options to appear and select WeChat
    const wechatOptions = await screen.findAllByText('WeChat')
    const wechatOption =
      wechatOptions.find((element) => element.tagName !== 'OPTION') ?? wechatOptions[0]
    await user.click(wechatOption)

    // Verify scopes field is automatically filled with snsapi_login
    const scopesInput = screen.getByTestId('oauth-scopes-input') as HTMLInputElement
    expect(scopesInput.value).toBe('snsapi_login')
    expect(scopesInput).toBeDisabled()

    // Verify helper text about fixed scope is shown
    expect(screen.getByText('(Fixed: snsapi_login)')).toBeInTheDocument()
  })

  it('GIVEN creating wechat_miniprogram provider WHEN selecting wechat_miniprogram THEN should hide scopes field', async () => {
    const user = userEvent.setup({ delay: null })

    render(<ProviderConfigForm {...defaultProps} />)

    // Select WeChat Mini Program provider type
    const providerSelectTrigger = screen.getByTestId('oauth-provider-type-select')
    await user.click(providerSelectTrigger)

    // Wait for options to appear and select WeChat Mini Program
    const wechatMiniProgramOptions = await screen.findAllByText('WeChat Mini Program')
    const wechatMiniProgramOption =
      wechatMiniProgramOptions.find((element) => element.tagName !== 'OPTION') ??
      wechatMiniProgramOptions[0]
    await user.click(wechatMiniProgramOption)

    // Verify scopes field is not displayed for wechat_miniprogram
    await waitFor(() => {
      expect(screen.queryByTestId('oauth-scopes-input')).not.toBeInTheDocument()
    })
  })
})

describe('ProviderConfigForm - Edit Mode', () => {
  const mockOnSubmit = vi.fn()
  const defaultProps = {
    onSubmit: mockOnSubmit,
    isPending: false,
    onCancel: vi.fn(),
  }

  beforeEach(() => {
    mockOnSubmit.mockClear()
  })

  it('GIVEN editing wechat config WHEN rendering THEN should pre-fill form with existing values', () => {
    const editingConfig: OAuthConfigResponse = {
      id: '1',
      realmId: 'admin',
      providerType: 'wechat',
      clientId: 'wx1234567890',
      scopes: ['snsapi_login'],
      enabled: true,
      createdAt: '2024-01-01T00:00:00Z',
      updatedAt: '2024-01-01T00:00:00Z',
    }

    render(<ProviderConfigForm {...defaultProps} editingConfig={editingConfig} />)

    // Verify client ID is pre-filled
    const clientIdInput = screen.getByTestId('oauth-client-id-input') as HTMLInputElement
    expect(clientIdInput.value).toBe('wx1234567890')

    // Verify client secret is cleared (should be empty in edit mode)
    const clientSecretInput = screen.getByTestId('oauth-client-secret-input') as HTMLInputElement
    expect(clientSecretInput.value).toBe('')

    // Verify scopes is read-only with correct value for wechat
    const scopesInput = screen.getByTestId('oauth-scopes-input') as HTMLInputElement
    expect(scopesInput.value).toBe('snsapi_login')
    expect(scopesInput).toBeDisabled()

    // Verify helper text about fixed scope
    expect(screen.getByText('(Fixed: snsapi_login)')).toBeInTheDocument()

    // Verify helper text about optional client secret
    expect(screen.getByText('(Leave empty to keep existing)')).toBeInTheDocument()

    // Verify enabled checkbox is checked
    const enabledCheckbox = screen.getByTestId('oauth-enabled-checkbox')
    expect(enabledCheckbox).toBeChecked()
  })

  it('GIVEN editing wechat_miniprogram config WHEN rendering THEN should not show scopes field', () => {
    const editingConfig: OAuthConfigResponse = {
      id: '2',
      realmId: 'admin',
      providerType: 'wechat_miniprogram',
      clientId: 'wx9876543210',
      scopes: [],
      enabled: true,
      createdAt: '2024-01-01T00:00:00Z',
      updatedAt: '2024-01-01T00:00:00Z',
    }

    render(<ProviderConfigForm {...defaultProps} editingConfig={editingConfig} />)

    // Verify scopes field is not displayed for wechat_miniprogram
    expect(screen.queryByTestId('oauth-scopes-input')).not.toBeInTheDocument()

    // Verify helper text about optional client secret
    expect(screen.getByText('(Leave empty to keep existing)')).toBeInTheDocument()
  })

  it('GIVEN editing google config WHEN rendering THEN should show editable scopes field', () => {
    const editingConfig: OAuthConfigResponse = {
      id: '3',
      realmId: 'admin',
      providerType: 'google',
      clientId: 'google-client-id',
      scopes: ['openid', 'email', 'profile'],
      enabled: true,
      createdAt: '2024-01-01T00:00:00Z',
      updatedAt: '2024-01-01T00:00:00Z',
    }

    render(<ProviderConfigForm {...defaultProps} editingConfig={editingConfig} />)

    // Verify scopes field is displayed and editable for non-wechat providers
    const scopesInput = screen.getByTestId('oauth-scopes-input') as HTMLInputElement
    expect(scopesInput).toBeInTheDocument()
    expect(scopesInput).not.toBeDisabled()
    expect(scopesInput.value).toBe('openid, email, profile')

    // Verify helper text about fixed scope is NOT shown
    expect(screen.queryByText('(Fixed: snsapi_login)')).not.toBeInTheDocument()
  })

  it('GIVEN editing config WHEN rendering THEN should not allow changing provider type', () => {
    const editingConfig: OAuthConfigResponse = {
      id: '1',
      realmId: 'admin',
      providerType: 'wechat',
      clientId: 'wx1234567890',
      scopes: ['snsapi_login'],
      enabled: true,
      createdAt: '2024-01-01T00:00:00Z',
      updatedAt: '2024-01-01T00:00:00Z',
    }

    render(<ProviderConfigForm {...defaultProps} editingConfig={editingConfig} />)

    // Verify provider type select is disabled
    const providerSelect = screen.getByTestId('oauth-provider-type-select')
    expect(providerSelect).toBeDisabled()
  })

  it('GIVEN editing config WHEN submitting with empty clientSecret THEN should allow submission', async () => {
    const user = userEvent.setup({ delay: null })
    mockOnSubmit.mockResolvedValue(undefined)

    const editingConfig: OAuthConfigResponse = {
      id: '1',
      realmId: 'admin',
      providerType: 'google',
      clientId: 'test-client-id',
      scopes: ['email'],
      enabled: true,
      createdAt: '2024-01-01T00:00:00Z',
      updatedAt: '2024-01-01T00:00:00Z',
    }

    render(<ProviderConfigForm {...defaultProps} editingConfig={editingConfig} />)

    // Submit without changing client secret (keep it empty)
    const saveButton = screen.getByTestId('oauth-save-provider-button')
    await user.click(saveButton)

    // Verify submission (clientSecret should be empty string)
    await waitFor(() => {
      expect(mockOnSubmit).toHaveBeenCalledWith(
        expect.objectContaining({
          clientId: 'test-client-id',
          clientSecret: '',
        })
      )
    })
  })

  it('GIVEN editing config WHEN submitting with new clientSecret THEN should include new secret', async () => {
    const user = userEvent.setup({ delay: null })
    mockOnSubmit.mockResolvedValue(undefined)

    const editingConfig: OAuthConfigResponse = {
      id: '1',
      realmId: 'admin',
      providerType: 'google',
      clientId: 'test-client-id',
      scopes: ['email'],
      enabled: true,
      createdAt: '2024-01-01T00:00:00Z',
      updatedAt: '2024-01-01T00:00:00Z',
    }

    render(<ProviderConfigForm {...defaultProps} editingConfig={editingConfig} />)

    // Enter new client secret
    const clientSecretInput = screen.getByTestId('oauth-client-secret-input')
    await user.clear(clientSecretInput)
    await user.type(clientSecretInput, 'new-secret-456')

    // Submit form
    const saveButton = screen.getByTestId('oauth-save-provider-button')
    await user.click(saveButton)

    // Verify submission with new secret
    await waitFor(() => {
      expect(mockOnSubmit).toHaveBeenCalledWith(
        expect.objectContaining({
          clientId: 'test-client-id',
          clientSecret: 'new-secret-456',
        })
      )
    })
  })

  it('GIVEN editing wechat config WHEN submitting THEN should include snsapi_login scope', async () => {
    const user = userEvent.setup({ delay: null })
    mockOnSubmit.mockResolvedValue(undefined)

    const editingConfig: OAuthConfigResponse = {
      id: '1',
      realmId: 'admin',
      providerType: 'wechat',
      clientId: 'wx1234567890',
      scopes: ['snsapi_login'],
      enabled: true,
      createdAt: '2024-01-01T00:00:00Z',
      updatedAt: '2024-01-01T00:00:00Z',
    }

    render(<ProviderConfigForm {...defaultProps} editingConfig={editingConfig} />)

    // Submit form
    const saveButton = screen.getByTestId('oauth-save-provider-button')
    await user.click(saveButton)

    // Verify submission includes correct scopes
    await waitFor(() => {
      expect(mockOnSubmit).toHaveBeenCalledWith(
        expect.objectContaining({
          providerType: 'wechat',
          clientId: 'wx1234567890',
          scopes: ['snsapi_login'],
        })
      )
    })
  })

  it('GIVEN editing wechat_miniprogram config WHEN submitting THEN should include empty scopes array', async () => {
    const user = userEvent.setup({ delay: null })
    mockOnSubmit.mockResolvedValue(undefined)

    const editingConfig: OAuthConfigResponse = {
      id: '2',
      realmId: 'admin',
      providerType: 'wechat_miniprogram',
      clientId: 'wx9876543210',
      scopes: [],
      enabled: true,
      createdAt: '2024-01-01T00:00:00Z',
      updatedAt: '2024-01-01T00:00:00Z',
    }

    render(<ProviderConfigForm {...defaultProps} editingConfig={editingConfig} />)

    // Submit form
    const saveButton = screen.getByTestId('oauth-save-provider-button')
    await user.click(saveButton)

    // Verify submission includes empty scopes
    await waitFor(() => {
      expect(mockOnSubmit).toHaveBeenCalledWith(
        expect.objectContaining({
          providerType: 'wechat_miniprogram',
          clientId: 'wx9876543210',
          scopes: [],
        })
      )
    })
  })
})

describe('ProviderConfigForm - Validation', () => {
  const mockOnSubmit = vi.fn()
  const defaultProps = {
    onSubmit: mockOnSubmit,
    isPending: false,
    onCancel: vi.fn(),
  }

  beforeEach(() => {
    mockOnSubmit.mockClear()
  })

  it('GIVEN isPending is true WHEN rendering THEN should disable save button', () => {
    render(<ProviderConfigForm {...defaultProps} isPending={true} />)

    const saveButton = screen.getByTestId('oauth-save-provider-button')
    expect(saveButton).toBeDisabled()
    expect(saveButton).toHaveTextContent('Saving...')
  })

  it('GIVEN google config is edited WHEN scopes are updated THEN should submit new scopes', async () => {
    const user = userEvent.setup({ delay: null })
    mockOnSubmit.mockResolvedValue(undefined)

    const editingConfig: OAuthConfigResponse = {
      id: '1',
      realmId: 'admin',
      providerType: 'google',
      clientId: 'google-client-id',
      scopes: ['email'],
      enabled: true,
      createdAt: '2024-01-01T00:00:00Z',
      updatedAt: '2024-01-01T00:00:00Z',
    }

    render(<ProviderConfigForm {...defaultProps} editingConfig={editingConfig} />)

    // Update scopes
    const scopesInput = screen.getByTestId('oauth-scopes-input')
    await user.clear(scopesInput)
    await user.type(scopesInput, 'email, profile')

    // Submit form
    const saveButton = screen.getByTestId('oauth-save-provider-button')
    await user.click(saveButton)

    // Verify submission with updated scopes
    await waitFor(() => {
      expect(mockOnSubmit).toHaveBeenCalledWith(
        expect.objectContaining({
          scopes: ['email', 'profile'],
        })
      )
    })
  })
})
