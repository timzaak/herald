import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { PointDistributionRuleEditor } from '../point-distribution-rule-editor'
import { pointDistributionRulesSchema } from '@/lib/schemas/billing-forms'

const buckets = [
  {
    id: 'bucket-a',
    name: 'General',
    bucketKey: 'general',
    displayOrder: 0,
    enabled: true,
    coveredClientAppCount: 1,
    ruleReferenceCount: 0,
  },
]

describe('PointDistributionRuleEditor', () => {
  it('removes unsaved rules but explicitly disables persisted rules', async () => {
    const user = userEvent.setup()
    const onChange = vi.fn()
    const { rerender } = render(
      <PointDistributionRuleEditor
        value={[
          {
            id: 'rule-1',
            bucketId: 'bucket-a',
            triggerSources: ['topup'],
            grantMode: 'fixed',
            pointsAmount: 100,
            validityDays: 0,
            quotaWindows: null,
            enabled: true,
          },
        ]}
        onChange={onChange}
        buckets={buckets}
        triggers={[{ value: 'topup', label: 'Top-up' }]}
        allowQuota={false}
      />
    )

    await user.click(screen.getByTestId('point-rule-remove-rule-1'))
    expect(onChange).toHaveBeenLastCalledWith([
      expect.objectContaining({ id: 'rule-1', enabled: false }),
    ])

    rerender(
      <PointDistributionRuleEditor
        value={[
          {
            bucketId: 'bucket-a',
            triggerSources: ['topup'],
            grantMode: 'fixed',
            pointsAmount: 100,
            validityDays: 0,
            quotaWindows: null,
            enabled: true,
          },
        ]}
        onChange={onChange}
        buckets={buckets}
        triggers={[{ value: 'topup', label: 'Top-up' }]}
        allowQuota={false}
      />
    )
    await user.click(screen.getByTestId('point-rule-remove-new-0'))
    expect(onChange).toHaveBeenLastCalledWith([])
  })

  it('rejects fixed and quota rules with missing policy data', () => {
    expect(
      pointDistributionRulesSchema.safeParse([
        {
          bucketId: 'bucket-a',
          triggerSources: ['topup'],
          grantMode: 'fixed',
          pointsAmount: 0,
        },
      ]).success
    ).toBe(false)
    expect(
      pointDistributionRulesSchema.safeParse([
        {
          bucketId: 'bucket-a',
          triggerSources: ['subscription_initial'],
          grantMode: 'quota',
          quotaWindows: [],
        },
      ]).success
    ).toBe(false)
  })

  it('seeds the grant period for new rules in period-capable contexts', async () => {
    const user = userEvent.setup()
    const onChange = vi.fn()
    render(
      <PointDistributionRuleEditor
        value={[]}
        onChange={onChange}
        buckets={buckets}
        triggers={[{ value: 'free_periodic_grant', label: 'Free periodic' }]}
        allowQuota
        allowPeriod
      />
    )

    await user.click(screen.getByTestId('point-rule-add'))
    // A free-periodic fixed rule without a period is rejected server-side
    // (FreePeriodicRequiresGrantPeriod) while the select still displays a
    // default — the seeded value must match what the user sees.
    expect(onChange).toHaveBeenLastCalledWith([
      expect.objectContaining({ grantPeriodType: 'once' }),
    ])
  })

  it('seeds the grant period when a loaded quota rule switches to fixed', async () => {
    const user = userEvent.setup()
    const onChange = vi.fn()
    render(
      <PointDistributionRuleEditor
        value={[
          {
            id: 'rule-quota',
            bucketId: 'bucket-a',
            triggerSources: ['free_periodic_grant'],
            grantMode: 'quota',
            pointsAmount: null,
            validityDays: null,
            quotaWindows: [{ windowSeconds: 3600, limit: 5 }],
            // The API returns grant_period_type only for fixed rules, so a
            // loaded quota rule has none to preserve.
            grantPeriodType: null,
            enabled: true,
          },
        ]}
        onChange={onChange}
        buckets={buckets}
        triggers={[{ value: 'free_periodic_grant', label: 'Free periodic' }]}
        allowQuota
        allowPeriod
      />
    )

    await user.click(screen.getByTestId('point-rule-mode'))
    await user.click(await screen.findByRole('option', { name: 'Fixed points' }))

    // The period Select displays "Once" for a null period, so switching to
    // fixed must seed 'once' — otherwise the payload stays null while the
    // admin sees "Once" and the backend rejects the whole save
    // (FreePeriodicRequiresGrantPeriod).
    expect(onChange).toHaveBeenLastCalledWith([
      expect.objectContaining({ grantMode: 'fixed', grantPeriodType: 'once' }),
    ])
  })

  it('hides the validity input when grants must be permanent', () => {
    render(
      <PointDistributionRuleEditor
        value={[
          {
            id: 'rule-1',
            bucketId: 'bucket-a',
            triggerSources: ['registration'],
            grantMode: 'fixed',
            pointsAmount: 50,
            validityDays: 0,
            quotaWindows: null,
            enabled: true,
          },
        ]}
        onChange={() => {}}
        buckets={buckets}
        triggers={[{ value: 'registration', label: 'Registration' }]}
        allowQuota={false}
        requirePermanent
      />
    )

    // Registration grants must stay permanent (RegistrationMustBePermanent):
    // the editor must not offer an expiry the backend would reject.
    expect(screen.queryByTestId('point-rule-validity-rule-1')).not.toBeInTheDocument()
  })
})
