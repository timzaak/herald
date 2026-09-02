import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { RegistrationConfigForm } from '../registration-config-form'
import type { RegistrationConfigForm as RegistrationConfigFormData } from '@/lib/schemas/realm-config'

describe('RegistrationConfigForm', () => {
  const mockOnSave = vi.fn()
  const defaultProps = {
    realmId: 'admin',
    onSave: mockOnSave,
    isLoading: false,
  }

  beforeEach(() => {
    mockOnSave.mockClear()
  })

  it('GIVEN initial config provided WHEN rendering THEN should display configuration values', async () => {
    const initialConfig: RegistrationConfigFormData = {
      enabled: false,
      requireEmailVerification: true,
    }

    const screen = render(
      <RegistrationConfigForm {...defaultProps} initialConfig={initialConfig} />
    )

    const enabledSwitch = screen.getByTestId('reg-enabled-switch')
    expect(enabledSwitch).not.toBeChecked()

    const requireEmailSwitch = screen.getByTestId('reg-require-email-switch')
    expect(requireEmailSwitch).toBeChecked()
  })

  it('GIVEN user toggles switches WHEN submitting form THEN should call onSave with config', async () => {
    mockOnSave.mockResolvedValue(undefined)
    const screen = render(<RegistrationConfigForm {...defaultProps} />)

    // 禁用注册
    const enabledSwitch = screen.getByTestId('reg-enabled-switch')
    await userEvent.click(enabledSwitch)

    // 提交表单
    const saveButton = screen.getByTestId('reg-save-button')
    await userEvent.click(saveButton)

    // 验证 onSave 被调用
    await waitFor(() => {
      expect(mockOnSave).toHaveBeenCalledWith({
        enabled: false,
        requireEmailVerification: true,
      })
    })
  })

  it('GIVEN form is disabled WHEN user interacts THEN should not allow changes', async () => {
    const screen = render(<RegistrationConfigForm {...defaultProps} disabled={true} />)

    // 验证开关被禁用
    const enabledSwitch = screen.getByTestId('reg-enabled-switch')
    expect(enabledSwitch).toBeDisabled()

    const requireEmailSwitch = screen.getByTestId('reg-require-email-switch')
    expect(requireEmailSwitch).toBeDisabled()

    // 验证保存按钮被禁用
    const saveButton = screen.getByTestId('reg-save-button')
    expect(saveButton).toBeDisabled()
  })

  it('GIVEN isLoading prop is true WHEN rendering THEN should disable save button', async () => {
    const screen = render(<RegistrationConfigForm {...defaultProps} isLoading={true} />)

    const saveButton = screen.getByTestId('reg-save-button')
    expect(saveButton).toBeDisabled()
  })

  it('GIVEN form is submitting WHEN save is in progress THEN should disable save button', async () => {
    mockOnSave.mockImplementation(() => new Promise((resolve) => setTimeout(resolve, 100)))

    const screen = render(<RegistrationConfigForm {...defaultProps} />)

    const saveButton = screen.getByTestId('reg-save-button')
    await userEvent.click(saveButton)

    // 验证按钮被禁用（使用 waitFor 等待状态更新）
    await waitFor(() => {
      expect(saveButton).toBeDisabled()
    })
  })

  // 互补分支：enabled:false + requireEmail:true 已由上面的
  // "initial config provided" 测试覆盖，这里测 enabled:true 分支
  it('GIVEN initial config allows registration without email verification WHEN rendering THEN should display switches correctly', async () => {
    const initialConfig: RegistrationConfigFormData = {
      enabled: true,
      requireEmailVerification: false,
    }

    const screen = render(
      <RegistrationConfigForm {...defaultProps} initialConfig={initialConfig} />
    )

    const enabledSwitch = screen.getByTestId('reg-enabled-switch')
    expect(enabledSwitch).toBeChecked()

    const requireEmailSwitch = screen.getByTestId('reg-require-email-switch')
    expect(requireEmailSwitch).not.toBeChecked()
  })

  // Email configuration gating tests
  it('GIVEN email not configured WHEN rendering THEN should disable requireEmailVerification switch', async () => {
    const screen = render(<RegistrationConfigForm {...defaultProps} emailConfigured={false} />)

    const requireEmailSwitch = screen.getByTestId('reg-require-email-switch')
    expect(requireEmailSwitch).toBeDisabled()
  })

  it('GIVEN email not configured WHEN rendering THEN should show email config required hint', async () => {
    const screen = render(<RegistrationConfigForm {...defaultProps} emailConfigured={false} />)

    expect(screen.getByTestId('email-config-required-hint')).toBeInTheDocument()
    expect(screen.getByTestId('email-config-required-hint')).toHaveTextContent(
      'Email verification requires email configuration'
    )
  })

  it('GIVEN email configured WHEN rendering THEN should enable requireEmailVerification switch', async () => {
    const screen = render(<RegistrationConfigForm {...defaultProps} emailConfigured={true} />)

    const requireEmailSwitch = screen.getByTestId('reg-require-email-switch')
    expect(requireEmailSwitch).not.toBeDisabled()
  })

  // email 失效时开关视觉应反映"实际生效状态"(OFF),而非 DB 存储值(ON)。
  // 后端 is_email_verification_required 已对 email 未配置做 fail-safe 降级,
  // UI 必须与之同态:emailConfigured=false 时,即使 DB require_email_verification=true,
  // 开关也应显示 OFF(且禁用),避免"ON+灰色禁用"的歧义视觉。
  it('GIVEN email not configured and DB requireEmailVerification=true WHEN rendering THEN should show switch OFF (not ON+disabled)', async () => {
    const initialConfig: RegistrationConfigFormData = {
      enabled: true,
      requireEmailVerification: true,
    }

    const screen = render(
      <RegistrationConfigForm
        {...defaultProps}
        initialConfig={initialConfig}
        emailConfigured={false}
      />
    )

    const requireEmailSwitch = screen.getByTestId('reg-require-email-switch')
    expect(requireEmailSwitch).toBeDisabled()
    expect(requireEmailSwitch).not.toBeChecked()
  })
})
