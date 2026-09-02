import { describe, it, expect, vi } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { SubscriptionTimeline } from '../subscription-timeline'
import type { SubscriptionHistoryEvent } from '@/types/billing'
import { mockSubscriptionHistoryEvent } from './utils'

describe('SubscriptionTimeline - Empty State', () => {
  it('should display empty state message when no events', () => {
    render(<SubscriptionTimeline events={[]} />)

    expect(screen.getByTestId('timeline-empty')).toBeInTheDocument()
    expect(screen.getByText(/no history events found/i)).toBeInTheDocument()
  })

  it('should hide timeline when loading is true and events is empty', () => {
    render(<SubscriptionTimeline events={[]} loading={true} />)

    expect(screen.queryByTestId('subscription-timeline')).not.toBeInTheDocument()
    expect(screen.getByTestId('timeline-loading')).toBeInTheDocument()
    expect(screen.getByText(/loading history/i)).toBeInTheDocument()
  })

  it('should display loading state when loading is true', () => {
    const mockEvents = [
      mockSubscriptionHistoryEvent({
        id: '1',
        eventType: 'upgraded',
      }),
    ]

    render(<SubscriptionTimeline events={mockEvents} loading={true} />)

    expect(screen.getByTestId('timeline-loading')).toBeInTheDocument()
    expect(screen.getByText(/loading history/i)).toBeInTheDocument()
    // Loading state should not show events
    expect(screen.queryByTestId('subscription-timeline')).not.toBeInTheDocument()
  })
})

describe('SubscriptionTimeline - Event Rendering', () => {
  const mockEvents: SubscriptionHistoryEvent[] = [
    {
      ...mockSubscriptionHistoryEvent({
        id: '1',
        eventType: 'upgraded',
        actor: 'admin@example.com',
        changes: {
          changedFields: ['entitlementKey'],
          previousEntitlementKey: 'basic-plan',
          newEntitlementKey: 'pro-plan',
        },
      }),
    },
  ]

  it('should display change summary when changes are present', () => {
    render(<SubscriptionTimeline events={mockEvents} />)

    const eventContainer = screen.getByTestId('timeline-event-0')
    // Verify i18n template parts rendered
    expect(within(eventContainer).getByText(/plan changed from/i)).toBeInTheDocument()
    expect(within(eventContainer).getByText(/basic-plan/)).toBeInTheDocument()
    expect(within(eventContainer).getByText(/pro-plan/)).toBeInTheDocument()
    // Verify plan names are in styled spans (font-medium)
    const basicPlanEl = within(eventContainer).getByText(/basic-plan/)
    expect(basicPlanEl.className).toContain('font-medium')
    const proPlanEl = within(eventContainer).getByText(/pro-plan/)
    expect(proPlanEl.className).toContain('font-medium')
  })
})

describe('SubscriptionTimeline - Event Interaction', () => {
  const mockEvents: SubscriptionHistoryEvent[] = [
    mockSubscriptionHistoryEvent({
      id: '1',
      eventType: 'upgraded',
    }),
  ]

  it('should call onEventClick when event details button is clicked', async () => {
    const onEventClick = vi.fn()

    render(<SubscriptionTimeline events={mockEvents} onEventClick={onEventClick} />)

    const viewButton = screen.getByTestId('view-event-details-0')
    await userEvent.click(viewButton)

    expect(onEventClick).toHaveBeenCalledTimes(1)
    expect(onEventClick).toHaveBeenCalledWith(mockEvents[0])
  })

  it('should open event detail dialog when view details is clicked', async () => {
    render(<SubscriptionTimeline events={mockEvents} />)

    expect(screen.queryByTestId('event-detail-dialog')).not.toBeInTheDocument()

    const viewButton = screen.getByTestId('view-event-details-0')
    await userEvent.click(viewButton)

    expect(screen.getByTestId('event-detail-dialog')).toBeInTheDocument()
  })

  it('should close event detail dialog when close button is clicked', async () => {
    render(<SubscriptionTimeline events={mockEvents} />)

    const viewButton = screen.getByTestId('view-event-details-0')
    await userEvent.click(viewButton)

    const closeButton = screen.getByRole('button', { name: /close/i })
    await userEvent.click(closeButton)

    expect(screen.queryByTestId('event-detail-dialog')).not.toBeInTheDocument()
  })
})

describe('SubscriptionTimeline - Event Detail Dialog', () => {
  const mockEvents: SubscriptionHistoryEvent[] = [
    {
      ...mockSubscriptionHistoryEvent({
        id: 'evt-1',
        eventType: 'upgraded',
        actor: 'admin@example.com',
        previousState: {
          id: 'sub-1',
          realmId: 'realm-1',
          status: 'active',
          entitlementKey: 'basic-plan',
          paymentProvider: 'stripe',
          cancelAtPeriodEnd: false,
        },
        newState: {
          id: 'sub-1',
          realmId: 'realm-1',
          status: 'active',
          entitlementKey: 'pro-plan',
          paymentProvider: 'stripe',
          cancelAtPeriodEnd: false,
        },
      }),
    },
  ]

  it('should display event ID in detail dialog', async () => {
    render(<SubscriptionTimeline events={mockEvents} />)

    const viewButton = screen.getByTestId('view-event-details-0')
    await userEvent.click(viewButton)

    // Check that dialog contains event ID using a function matcher
    expect(
      screen.getByText((content, element) => {
        return content.includes('evt-1') && element?.tagName.toLowerCase() === 'p'
      })
    ).toBeInTheDocument()
  })

  it('should display previous state in detail dialog', async () => {
    render(<SubscriptionTimeline events={mockEvents} />)

    const viewButton = screen.getByTestId('view-event-details-0')
    await userEvent.click(viewButton)

    // Check that dialog contains previous state section
    expect(screen.getByText(/previous state/i)).toBeInTheDocument()
    // Check that status and entitlement are mentioned (using getAllByText since there are multiple)
    expect(screen.getAllByText('Status:').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Entitlement:').length).toBeGreaterThan(0)
    expect(screen.getAllByText('basic-plan').length).toBeGreaterThan(0)
  })

  it('should display new state in detail dialog', async () => {
    render(<SubscriptionTimeline events={mockEvents} />)

    const viewButton = screen.getByTestId('view-event-details-0')
    await userEvent.click(viewButton)

    // Check that dialog contains new state section
    expect(screen.getByText(/new state/i)).toBeInTheDocument()
    // Check that status and entitlement are mentioned (using getAllByText since there are multiple)
    expect(screen.getAllByText('Status:').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Entitlement:').length).toBeGreaterThan(0)
    expect(screen.getAllByText('pro-plan').length).toBeGreaterThan(0)
  })

  it('should display changes with changed fields when present', async () => {
    const mockEventsWithChanges: SubscriptionHistoryEvent[] = [
      {
        ...mockSubscriptionHistoryEvent({
          id: '1',
          eventType: 'upgraded',
          changes: {
            changedFields: ['entitlementKey', 'paymentProvider'],
          },
        }),
      },
    ]

    render(<SubscriptionTimeline events={mockEventsWithChanges} />)

    const viewButton = screen.getByTestId('view-event-details-0')
    await userEvent.click(viewButton)

    expect(screen.getByText(/changes/i)).toBeInTheDocument()
    expect(screen.getByText(/changed fields:/i)).toBeInTheDocument()
    expect(screen.getByText('entitlementKey')).toBeInTheDocument()
    expect(screen.getByText('paymentProvider')).toBeInTheDocument()
  })
})

describe('SubscriptionTimeline - Multiple Events', () => {
  const mockEvents: SubscriptionHistoryEvent[] = [
    mockSubscriptionHistoryEvent({ id: '1', eventType: 'created' }),
    mockSubscriptionHistoryEvent({ id: '2', eventType: 'upgraded' }),
    mockSubscriptionHistoryEvent({ id: '3', eventType: 'downgraded' }),
  ]

  it('should display different event types with different badges', () => {
    render(<SubscriptionTimeline events={mockEvents} />)

    expect(screen.getByTestId('event-badge-created')).toBeInTheDocument()
    expect(screen.getByTestId('event-badge-upgraded')).toBeInTheDocument()
    expect(screen.getByTestId('event-badge-downgraded')).toBeInTheDocument()
  })
})
