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

    // agreementsUsingDefault=false: this test exercises the in-flight save
    // state, not the enable-registration reminder that would intercept it.
    const screen = render(
      <RegistrationConfigForm {...defaultProps} agreementsUsingDefault={false} />
    )

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

  // 无法开启的原因通过 tooltip 告知（而非静态提示文字）：
  // 悬停被禁用的开关时应说明"邮箱未配置"这一前置条件缺失。
  it('GIVEN email not configured WHEN hovering the disabled switch THEN should explain the reason in a tooltip', async () => {
    const user = userEvent.setup()
    const screen = render(<RegistrationConfigForm {...defaultProps} emailConfigured={false} />)

    expect(screen.queryByTestId('email-config-required-hint')).not.toBeInTheDocument()

    await user.hover(screen.getByTestId('reg-require-email-switch-tooltip-trigger'))
    expect(await screen.findByTestId('reg-require-email-switch-tooltip')).toHaveTextContent(
      'Email verification requires email configuration'
    )
  })

  it('GIVEN email configured WHEN rendering THEN should enable requireEmailVerification switch', async () => {
    const screen = render(<RegistrationConfigForm {...defaultProps} emailConfigured={true} />)

    const requireEmailSwitch = screen.getByTestId('reg-require-email-switch')
    expect(requireEmailSwitch).not.toBeDisabled()
    expect(screen.queryByTestId('reg-require-email-switch-tooltip-trigger')).not.toBeInTheDocument()
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

  // 开启注册 = 用户开始对本域协议作出同意。协议仍为平台默认模板（占位内容）
  // 时直接放行会让用户同意一个无人审阅的模板 —— 保存必须先过提醒弹窗。
  describe('agreements reminder dialog', () => {
    it('GIVEN enabling registration while agreements still use platform defaults WHEN saving THEN intercepts with the reminder and saves only after confirmation', async () => {
      mockOnSave.mockResolvedValue(undefined)
      const screen = render(
        <RegistrationConfigForm
          {...defaultProps}
          initialConfig={{ enabled: false, requireEmailVerification: true }}
          agreementsUsingDefault
          onGoToLegal={() => {}}
        />
      )
      const user = userEvent.setup()

      await user.click(screen.getByTestId('reg-enabled-switch'))
      await user.click(screen.getByTestId('reg-save-button'))

      // The save is parked behind the dialog, not fired.
      expect(mockOnSave).not.toHaveBeenCalled()
      expect(await screen.findByTestId('reg-agreement-dialog-title')).toBeInTheDocument()

      await user.click(screen.getByTestId('reg-agreement-dialog-confirm'))
      await waitFor(() => {
        expect(mockOnSave).toHaveBeenCalledWith({
          enabled: true,
          requireEmailVerification: true,
        })
      })
    })

    it('GIVEN the reminder dialog WHEN choosing go-to-agreements THEN navigates without saving', async () => {
      const onGoToLegal = vi.fn()
      const screen = render(
        <RegistrationConfigForm
          {...defaultProps}
          initialConfig={{ enabled: false, requireEmailVerification: true }}
          agreementsUsingDefault
          onGoToLegal={onGoToLegal}
        />
      )
      const user = userEvent.setup()

      await user.click(screen.getByTestId('reg-enabled-switch'))
      await user.click(screen.getByTestId('reg-save-button'))
      await user.click(await screen.findByTestId('reg-agreement-dialog-go-legal'))

      // The admin is routed to configure agreements; nothing is persisted.
      expect(onGoToLegal).toHaveBeenCalledTimes(1)
      expect(mockOnSave).not.toHaveBeenCalled()
    })

    it('GIVEN agreements already customized WHEN enabling registration THEN saves without the reminder', async () => {
      mockOnSave.mockResolvedValue(undefined)
      const screen = render(
        <RegistrationConfigForm
          {...defaultProps}
          initialConfig={{ enabled: false, requireEmailVerification: true }}
          agreementsUsingDefault={false}
          onGoToLegal={() => {}}
        />
      )
      const user = userEvent.setup()

      await user.click(screen.getByTestId('reg-enabled-switch'))
      await user.click(screen.getByTestId('reg-save-button'))

      // Both agreements have custom versions — the reminder has nothing to say.
      await waitFor(() => {
        expect(mockOnSave).toHaveBeenCalledWith({
          enabled: true,
          requireEmailVerification: true,
        })
      })
      expect(screen.queryByTestId('reg-agreement-dialog-title')).not.toBeInTheDocument()
    })

    it('GIVEN registration already enabled WHEN saving other changes THEN does not nag with the reminder', async () => {
      mockOnSave.mockResolvedValue(undefined)
      const screen = render(
        <RegistrationConfigForm
          {...defaultProps}
          initialConfig={{ enabled: true, requireEmailVerification: true }}
          agreementsUsingDefault
          onGoToLegal={() => {}}
        />
      )
      const user = userEvent.setup()

      // Unrelated tweak: turn off email verification on an already-open realm.
      await user.click(screen.getByTestId('reg-require-email-switch'))
      await user.click(screen.getByTestId('reg-save-button'))

      await waitFor(() => {
        expect(mockOnSave).toHaveBeenCalledWith({
          enabled: true,
          requireEmailVerification: false,
        })
      })
      expect(screen.queryByTestId('reg-agreement-dialog-title')).not.toBeInTheDocument()
    })
  })
})
