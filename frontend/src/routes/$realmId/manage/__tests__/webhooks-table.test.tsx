/**
 * WebhookSubscriptionTable three-state action mapping test.
 *
 * Pins the UI projection of the backend subscription state machine
 * (US-WEP-001 scenario 3 / US-WEP-003): active rows offer "disable",
 * system-paused rows offer "resume" plus the paused-reason entry, disabled
 * rows offer "enable". A wrong mapping would let admins fire transitions the
 * backend rejects with 400 (paused is system-only) or hide the recovery path
 * for auto-paused subscriptions.
 */
import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { WebhookSubscriptionTable } from '@/components/webhook-subscription-table'
import { makeWebhookSubscription } from '@/test/fixtures/webhook-subscriptions'

describe('WebhookSubscriptionTable status-to-action mapping', () => {
  const user = userEvent.setup({ delay: null })

  it.each([
    { status: 'active', expectedButton: 'webhook-disable-button' },
    { status: 'paused', expectedButton: 'webhook-resume-button' },
    { status: 'disabled', expectedButton: 'webhook-enable-button' },
  ] as const)(
    'renders only the $status row action for a $status subscription',
    ({ status, expectedButton }) => {
      const allButtons = [
        'webhook-disable-button',
        'webhook-resume-button',
        'webhook-enable-button',
      ]
      render(
        <WebhookSubscriptionTable
          data={[makeWebhookSubscription({ id: `sub-${status}`, status })]}
        />
      )

      expect(screen.getByTestId(expectedButton)).toBeInTheDocument()
      for (const button of allButtons.filter((b) => b !== expectedButton)) {
        expect(screen.queryByTestId(button)).not.toBeInTheDocument()
      }
    }
  )

  it('shows the paused-reason entry only on paused rows', () => {
    render(
      <WebhookSubscriptionTable
        data={[
          makeWebhookSubscription({ id: 'sub-active', status: 'active' }),
          makeWebhookSubscription({
            id: 'sub-paused',
            status: 'paused',
            consecutiveFailures: 16,
            lastError: 'HTTP 500',
            pausedAt: '2026-11-20T13:00:00Z',
          }),
          makeWebhookSubscription({ id: 'sub-disabled', status: 'disabled' }),
        ]}
      />
    )

    expect(screen.getAllByTestId('webhook-paused-reason-trigger')).toHaveLength(1)
  })

  it('sends the backend-compatible target status for each transition', async () => {
    const onSetStatus = vi.fn()
    render(
      <WebhookSubscriptionTable
        data={[
          makeWebhookSubscription({ id: 'sub-active', status: 'active' }),
          makeWebhookSubscription({ id: 'sub-paused', status: 'paused' }),
          makeWebhookSubscription({ id: 'sub-disabled', status: 'disabled' }),
        ]}
        onSetStatus={onSetStatus}
      />
    )

    await user.click(screen.getByTestId('webhook-disable-button'))
    expect(onSetStatus).toHaveBeenLastCalledWith(
      expect.objectContaining({ id: 'sub-active' }),
      'disabled'
    )

    await user.click(screen.getByTestId('webhook-resume-button'))
    expect(onSetStatus).toHaveBeenLastCalledWith(
      expect.objectContaining({ id: 'sub-paused' }),
      'active'
    )

    await user.click(screen.getByTestId('webhook-enable-button'))
    expect(onSetStatus).toHaveBeenLastCalledWith(
      expect.objectContaining({ id: 'sub-disabled' }),
      'active'
    )
  })

  it('disables every row action when the operator lacks webhooks.manage', () => {
    render(
      <WebhookSubscriptionTable
        data={[makeWebhookSubscription({ id: 'sub-active', status: 'active' })]}
        canManage={false}
      />
    )

    for (const testId of [
      'edit-webhook-button',
      'webhook-disable-button',
      'rotate-webhook-secret-button',
      'delete-webhook-button',
    ]) {
      expect(screen.getByTestId(testId)).toBeDisabled()
    }
  })
})
