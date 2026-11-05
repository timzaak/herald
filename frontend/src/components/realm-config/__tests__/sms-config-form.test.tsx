/**
 * SmsConfigForm tests: the phone-login switch's fail-closed gate and the
 * masked-secret save contract.
 *
 * WHY these two contracts:
 *  - The switch is the first user-facing line of defense against enabling
 *    phone login without a working SMS channel (US-PL-004 scene 3 UI
 *    surface). It must be disabled — with the reason attached — whenever the
 *    channel status is not explicitly configured; the server re-checks on
 *    PUT, but a clickable switch that always 400s is a broken affordance.
 *  - `buildSmsConfigRequest` must never round-trip the masked placeholder:
 *    resubmitting `••••••••` as a "new" secret would silently overwrite a
 *    stored credential with garbage (same MASKED_SECRET semantics as the
 *    email form).
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import React from 'react'
import { SmsConfigForm } from '../sms-config-form'
import { buildSmsConfigRequest } from '@/lib/realm-config-utils'
import { renderWithProviders } from '@/test/utils/render'
import type { SmsConfigForm as SmsConfigFormValues } from '@/lib/schemas/realm-config'

vi.mock('@/lib/api-generated/sdk.gen', () => ({
  smsTest: vi.fn(),
}))

vi.mock('@/lib/api-utils', () => ({
  handleApiResponse: vi.fn((response) => response.data),
}))

describe('SmsConfigForm', () => {
  const mockOnSave = vi.fn()
  const mockOnSavePhoneLogin = vi.fn()
  const defaultProps = {
    onSave: mockOnSave,
    onSavePhoneLogin: mockOnSavePhoneLogin,
    isLoading: false,
  }

  beforeEach(() => {
    mockOnSave.mockClear()
    mockOnSavePhoneLogin.mockClear()
  })

  const aliyunConfig: SmsConfigFormValues = {
    provider: 'aliyun',
    aliyunAccessKeyId: 'LTAI-test',
    aliyunAccessKeySecret: '••••••••',
    aliyunSignName: 'Herald',
    aliyunTemplateCode: 'SMS_123',
  }

  it('GIVEN the channel is configured WHEN rendering THEN the phone-login switch is enabled with no tooltip', () => {
    renderWithProviders(
      <SmsConfigForm
        {...defaultProps}
        initialConfig={aliyunConfig}
        smsStatus={{ configured: true, provider: 'aliyun', missingFields: [] }}
        phoneLoginInitialConfig={{ enabled: false, defaultCountryCode: 'CN' }}
      />
    )

    const switchEl = screen.getByTestId('phone-login-enabled-switch')
    expect(switchEl).not.toBeDisabled()
    expect(screen.queryByTestId('phone-login-enabled-switch-tooltip')).not.toBeInTheDocument()
  })

  it('GIVEN the channel is not configured WHEN rendering THEN the switch is disabled and the reason is attached', () => {
    renderWithProviders(
      <SmsConfigForm
        {...defaultProps}
        initialConfig={aliyunConfig}
        smsStatus={{ configured: false, provider: null, missingFields: ['access_key_id'] }}
        phoneLoginInitialConfig={{ enabled: false, defaultCountryCode: 'CN' }}
      />
    )

    const switchEl = screen.getByTestId('phone-login-enabled-switch')
    expect(switchEl).toBeDisabled()
    // The tooltip trigger is rendered alongside the disabled switch so the
    // admin can discover WHY (fail-closed with an explanation, not a dead end).
    expect(screen.getByTestId('phone-login-enabled-switch-tooltip-trigger')).toBeInTheDocument()
    // The missing fields are listed next to the badge.
    expect(screen.getByTestId('sms-missing-fields')).toHaveTextContent('access_key_id')
  })

  it('GIVEN no status yet WHEN rendering THEN the switch defaults to disabled (fail-closed on loading)', () => {
    renderWithProviders(
      <SmsConfigForm
        {...defaultProps}
        initialConfig={aliyunConfig}
        smsStatus={null}
        phoneLoginInitialConfig={{ enabled: false, defaultCountryCode: 'CN' }}
      />
    )

    expect(screen.getByTestId('phone-login-enabled-switch')).toBeDisabled()
  })

  it('GIVEN an unmasked secret edit WHEN saving THEN the new secret is submitted as a secret row', async () => {
    const user = userEvent.setup()
    mockOnSave.mockResolvedValue(undefined)
    renderWithProviders(
      <SmsConfigForm
        {...defaultProps}
        initialConfig={{ ...aliyunConfig, aliyunAccessKeySecret: 'brand-new-secret' }}
        smsStatus={{ configured: true, provider: 'aliyun', missingFields: [] }}
      />
    )

    await user.click(screen.getByTestId('sms-save-button'))

    await waitFor(() => {
      expect(mockOnSave).toHaveBeenCalledTimes(1)
    })
    const saved = mockOnSave.mock.calls[0][0] as SmsConfigFormValues
    expect(saved.aliyunAccessKeySecret).toBe('brand-new-secret')
  })

  it('GIVEN the masked placeholder WHEN building the request THEN unchanged secrets are omitted', () => {
    const rows = buildSmsConfigRequest(aliyunConfig)

    const keys = rows.map((row) => row.configKey)
    // The masked value never round-trips: server-side masking means "unchanged".
    expect(keys).not.toContain('aliyun_access_key_secret')
    // Non-secret rows (including empty strings) always submit so cleared
    // fields actually clear.
    expect(keys).toEqual(
      expect.arrayContaining([
        'provider',
        'aliyun_access_key_id',
        'aliyun_sign_name',
        'aliyun_template_code',
      ])
    )
    expect(rows.every((row) => row.configType === 'sms')).toBe(true)
  })

  it('GIVEN a fresh twilio secret WHEN building the request THEN the twilio secret row is included', () => {
    const rows = buildSmsConfigRequest({
      provider: 'twilio',
      twilioAccountSid: 'AC123',
      twilioAuthToken: 'fresh-token',
      twilioFrom: '+15017122661',
      twilioContentSid: 'HX123',
    })

    const secretRow = rows.find((row) => row.configKey === 'twilio_auth_token')
    expect(secretRow).toMatchObject({ configValue: 'fresh-token', isSecret: true })
  })

  it('GIVEN the provider radio WHEN switching providers THEN the visible field set follows', async () => {
    const user = userEvent.setup()
    renderWithProviders(
      <SmsConfigForm
        {...defaultProps}
        initialConfig={aliyunConfig}
        smsStatus={{ configured: true, provider: 'aliyun', missingFields: [] }}
      />
    )

    expect(screen.getByTestId('sms-aliyun-access-key-id-input')).toBeInTheDocument()
    expect(screen.queryByTestId('sms-twilio-account-sid-input')).not.toBeInTheDocument()

    await user.click(screen.getByRole('radio', { name: /twilio/i }))

    expect(screen.getByTestId('sms-twilio-account-sid-input')).toBeInTheDocument()
    expect(screen.queryByTestId('sms-aliyun-access-key-id-input')).not.toBeInTheDocument()
  })
})
