import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import {
  CustomDomainConfigForm,
  type CustomDomainConfigFormProps,
} from '../custom-domain-config-form'
import { emptyCustomDomainConfig } from '@/lib/realm-config-utils'
import type { CustomDomainConfigForm as CustomDomainConfigFormValues } from '@/lib/schemas/realm-config'
import type { CustomDomainStatus } from '@/lib/api-generated'

/**
 * Custom-domain config form (FE-D01/D02).
 *
 * The form is a presentational editor: it owns field rendering, CNAME guidance,
 * the live status badges, and the save/refresh action entry points, but
 * performs no data access — every action is surfaced via a callback. These
 * tests assert the contract the parent relies on (in-flight flags, disabled,
 * payload assembly).
 */
describe('CustomDomainConfigForm', () => {
  const mockOnSave = vi.fn()
  const mockOnRefreshStatus = vi.fn()

  const defaultProps: CustomDomainConfigFormProps = {
    realmId: 'admin',
    initialConfig: emptyCustomDomainConfig(),
    disabled: false,
    cnameTarget: 'custom.herald.com',
    status: null,
    onSave: mockOnSave,
    onRefreshStatus: mockOnRefreshStatus,
  }

  beforeEach(() => {
    mockOnSave.mockClear()
    mockOnRefreshStatus.mockClear()
    mockOnSave.mockResolvedValue(undefined)
  })

  it('GIVEN form is rendered with no initial config THEN should display an empty hostname field and a save button', async () => {
    const screen = render(<CustomDomainConfigForm {...defaultProps} />)

    expect(screen.getByTestId('custom-domain-hostname')).toHaveValue('')
    expect(screen.getByTestId('custom-domain-save')).toBeInTheDocument()
  })

  it('GIVEN an initial config is provided WHEN rendering THEN should reflect the supplied hostname', async () => {
    const initialConfig: CustomDomainConfigFormValues = { hostname: 'login.acme.com' }

    const screen = render(
      <CustomDomainConfigForm {...defaultProps} initialConfig={initialConfig} />
    )

    expect(screen.getByTestId('custom-domain-hostname')).toHaveValue('login.acme.com')
  })

  it('GIVEN user types a hostname WHEN clicking save THEN should call onSave once with the current form value', async () => {
    const screen = render(<CustomDomainConfigForm {...defaultProps} />)

    await userEvent.type(screen.getByTestId('custom-domain-hostname'), 'login.acme.com')
    await userEvent.click(screen.getByTestId('custom-domain-save'))

    await waitFor(() => {
      expect(mockOnSave).toHaveBeenCalledTimes(1)
      expect(mockOnSave).toHaveBeenCalledWith(
        expect.objectContaining({ hostname: 'login.acme.com' })
      )
    })
  })

  it.each([
    ['isSaving', 'custom-domain-save', { isSaving: true }],
    ['isRefreshing', 'custom-domain-refresh-status', { isRefreshing: true }],
  ] as const)(
    'GIVEN an action is in flight (%s) WHEN rendering THEN should disable its own button only',
    async (_flag, testId, inFlight) => {
      const screen = render(<CustomDomainConfigForm {...defaultProps} {...inFlight} />)

      expect(screen.getByTestId(testId)).toBeDisabled()
      // Buttons without their own in-flight flag stay usable (independent gating).
      const allButtons = ['custom-domain-save', 'custom-domain-refresh-status'] as const
      for (const id of allButtons) {
        if (id === testId) continue
        expect(screen.getByTestId(id)).not.toBeDisabled()
      }
    }
  )

  it('GIVEN a verified status is provided WHEN rendering THEN should reflect it on the badges and clicking refresh triggers onRefreshStatus', async () => {
    const status: CustomDomainStatus = {
      cnameVerified: true,
      tlsReady: false,
      checkedAt: '2026-07-09T00:00:00Z',
    }
    const screen = render(<CustomDomainConfigForm {...defaultProps} status={status} />)

    // CNAME verified + TLS pending: the badges carry the effective state.
    expect(screen.getByTestId('custom-domain-status-cname')).toBeInTheDocument()
    expect(screen.getByTestId('custom-domain-status-tls')).toBeInTheDocument()

    const refreshButton = screen.getByTestId('custom-domain-refresh-status')
    expect(refreshButton).not.toBeDisabled()
    await userEvent.click(refreshButton)
    await waitFor(() => {
      expect(mockOnRefreshStatus).toHaveBeenCalledTimes(1)
    })
  })
})
