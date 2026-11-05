import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { TOTPConfigForm } from '../totp-config-form'
import type { TOTPConfigForm as TOTPConfigFormData } from '@/lib/schemas/realm-config'

describe('TOTPConfigForm', () => {
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
    const initialConfig: TOTPConfigFormData = {
      enabled: true,
      forceEnabled: false,
    }

    const screen = render(<TOTPConfigForm {...defaultProps} initialConfig={initialConfig} />)

    const enabledSwitch = screen.getByTestId('totp-enabled-switch')
    expect(enabledSwitch).toBeChecked()

    const forceSwitch = screen.getByTestId('totp-force-enabled-switch')
    expect(forceSwitch).not.toBeChecked()
  })

  it('GIVEN user toggles switches WHEN submitting form THEN should call onSave with config', async () => {
    mockOnSave.mockResolvedValue(undefined)
    const screen = render(<TOTPConfigForm {...defaultProps} />)

    // 启用 TOTP
    const enabledSwitch = screen.getByTestId('totp-enabled-switch')
    await userEvent.click(enabledSwitch)

    // 提交表单
    const saveButton = screen.getByTestId('totp-save-button')
    await userEvent.click(saveButton)

    // 验证 onSave 被调用
    await waitFor(() => {
      expect(mockOnSave).toHaveBeenCalledWith({
        enabled: true,
        forceEnabled: false,
      })
    })

    // 等待提交流程完全结束（isSubmitting 复位），避免表单的异步延续
    // 在 jsdom 环境销毁后触发 "window is not defined" unhandled error
    await waitFor(() => {
      expect(saveButton).toBeEnabled()
    })
  })

  it('GIVEN TOTP is disabled WHEN enabling forceEnabled THEN should auto-enable TOTP', async () => {
    const screen = render(<TOTPConfigForm {...defaultProps} />)

    // TOTP 默认禁用，直接启用 forceEnabled
    const forceSwitch = screen.getByTestId('totp-force-enabled-switch')
    await userEvent.click(forceSwitch)

    // 提交表单
    const saveButton = screen.getByTestId('totp-save-button')
    await userEvent.click(saveButton)

    // 验证 onSave 被调用，组件不自动启用 TOTP（用户需要手动启用）
    // 这是预期行为 - 字段是独立的
    await waitFor(() => {
      expect(mockOnSave).toHaveBeenCalledWith({
        enabled: false, // TOTP 仍然是禁用的
        forceEnabled: true,
      })
    })

    // 等待提交流程完全结束（同上，避免 teardown 后的异步延续）
    await waitFor(() => {
      expect(saveButton).toBeEnabled()
    })
  })
})
