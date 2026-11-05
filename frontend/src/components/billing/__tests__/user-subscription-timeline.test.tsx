import { describe, it, expect, vi } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { UserSubscriptionTimeline } from '../user-subscription-timeline'
import type { SubscriptionHistoryEvent } from '@/types/billing'

describe('UserSubscriptionTimeline - Event Sorting', () => {
  const mockEvents: SubscriptionHistoryEvent[] = [
    {
      id: 'evt-1',
      subscriptionId: 'sub-1',
      eventType: 'created',
      timestamp: '2025-01-15T09:00:00Z',
      actor: 'user@example.com',
      previousState: {
        id: 'sub-1',
        realmId: 'realm-1',
        status: 'active',
        entitlementKey: 'basic',
        cancelAtPeriodEnd: false,
      },
    },
    {
      id: 'evt-2',
      subscriptionId: 'sub-1',
      eventType: 'upgraded',
      timestamp: '2025-01-20T10:30:00Z',
      actor: 'admin@example.com',
      changes: {
        changedFields: ['entitlementKey'],
        previousEntitlementKey: 'basic-plan',
        newEntitlementKey: 'pro-plan',
      },
      newState: {
        id: 'sub-1',
        realmId: 'realm-1',
        status: 'active',
        entitlementKey: 'pro',
        cancelAtPeriodEnd: false,
      },
    },
    {
      id: 'evt-3',
      subscriptionId: 'sub-1',
      eventType: 'canceled',
      timestamp: '2025-02-01T14:00:00Z',
      actor: 'user@example.com',
      changes: {
        changedFields: ['status'],
        previousStatus: 'active',
        newStatus: 'canceled',
      },
      newState: {
        id: 'sub-1',
        realmId: 'realm-1',
        status: 'canceled',
        entitlementKey: 'pro',
        cancelAtPeriodEnd: true,
      },
    },
  ]

  it('should display events in reverse chronological order (newest first)', () => {
    render(<UserSubscriptionTimeline events={mockEvents} />)

    const eventElements = screen.getAllByTestId(/timeline-event-/)
    expect(eventElements).toHaveLength(3)

    // First event should be canceled (newest: 2025-02-01)
    expect(eventElements[0]).toHaveAttribute('data-testid', 'timeline-event-evt-3')
    // Second event should be upgraded (middle: 2025-01-20)
    expect(eventElements[1]).toHaveAttribute('data-testid', 'timeline-event-evt-2')
    // Third event should be created (oldest: 2025-01-15)
    expect(eventElements[2]).toHaveAttribute('data-testid', 'timeline-event-evt-1')
  })
})

describe('UserSubscriptionTimeline - Expand/Collapse Details', () => {
  const mockEventWithState: SubscriptionHistoryEvent[] = [
    {
      id: 'evt-1',
      subscriptionId: 'sub-1',
      eventType: 'upgraded',
      timestamp: '2025-01-20T10:30:00Z',
      actor: 'admin@example.com',
      changes: {
        changedFields: ['entitlementKey'],
        previousEntitlementKey: 'basic-plan',
        newEntitlementKey: 'pro-plan',
      },
      previousState: {
        id: 'sub-1',
        realmId: 'realm-1',
        status: 'active',
        entitlementKey: 'basic',
        cancelAtPeriodEnd: false,
      },
      newState: {
        id: 'sub-1',
        realmId: 'realm-1',
        status: 'active',
        entitlementKey: 'pro',
        cancelAtPeriodEnd: false,
      },
    },
  ]

  it('should show "Show Details" button when event has state', () => {
    render(<UserSubscriptionTimeline events={mockEventWithState} />)

    const showButton = screen.getByTestId('toggle-event-details-evt-1')
    expect(showButton).toBeInTheDocument()
    expect(showButton).toHaveTextContent('Show Details')
  })

  it('should not show toggle button when event has no state', () => {
    const eventWithoutState: SubscriptionHistoryEvent[] = [
      {
        id: 'evt-1',
        subscriptionId: 'sub-1',
        eventType: 'created',
        timestamp: '2025-01-20T10:30:00Z',
        actor: 'user@example.com',
      },
    ]

    render(<UserSubscriptionTimeline events={eventWithoutState} />)

    expect(screen.queryByTestId('toggle-event-details-evt-1')).not.toBeInTheDocument()
  })

  it('should expand event details when toggle button is clicked', async () => {
    const user = userEvent.setup()
    render(<UserSubscriptionTimeline events={mockEventWithState} />)

    // Initially, details should not be visible
    expect(screen.queryByText(/Previous State/i)).not.toBeInTheDocument()
    expect(screen.queryByText(/New State/i)).not.toBeInTheDocument()

    const showButton = screen.getByTestId('toggle-event-details-evt-1')
    await user.click(showButton)

    // After clicking, details should be visible
    expect(screen.getByText(/Previous State/i)).toBeInTheDocument()
    expect(screen.getByText(/New State/i)).toBeInTheDocument()

    // Button should now say "Hide Details"
    expect(showButton).toHaveTextContent('Hide Details')
  })

  it('should collapse event details when clicked again', async () => {
    const user = userEvent.setup()
    render(<UserSubscriptionTimeline events={mockEventWithState} />)

    const showButton = screen.getByTestId('toggle-event-details-evt-1')

    // Expand
    await user.click(showButton)
    expect(screen.getByText(/Previous State/i)).toBeInTheDocument()

    // Collapse
    await user.click(showButton)
    expect(screen.queryByText(/Previous State/i)).not.toBeInTheDocument()

    // Button should say "Show Details" again
    expect(showButton).toHaveTextContent('Show Details')
  })

  it('should maintain expanded state independently for each event', async () => {
    const user = userEvent.setup()
    const events: SubscriptionHistoryEvent[] = [
      {
        id: 'evt-1',
        subscriptionId: 'sub-1',
        eventType: 'upgraded',
        timestamp: '2025-01-20T10:30:00Z',
        actor: 'admin@example.com',
        newState: {
          id: 'sub-1',
          realmId: 'realm-1',
          status: 'active',
          entitlementKey: 'pro',
          cancelAtPeriodEnd: false,
        },
      },
      {
        id: 'evt-2',
        subscriptionId: 'sub-1',
        eventType: 'downgraded',
        timestamp: '2025-02-01T14:00:00Z',
        actor: 'user@example.com',
        newState: {
          id: 'sub-1',
          realmId: 'realm-1',
          status: 'active',
          entitlementKey: 'basic',
          cancelAtPeriodEnd: false,
        },
      },
    ]

    render(<UserSubscriptionTimeline events={events} />)

    const button1 = screen.getByTestId('toggle-event-details-evt-1')
    const button2 = screen.getByTestId('toggle-event-details-evt-2')

    // Expand first event
    await user.click(button1)
    expect(screen.getByText(/New State/i)).toBeInTheDocument()

    // Second event should still be collapsed
    expect(button2).toHaveTextContent('Show Details')

    // Expand second event
    await user.click(button2)
    expect(button2).toHaveTextContent('Hide Details')
    expect(button1).toHaveTextContent('Hide Details')

    // Collapse first event
    await user.click(button1)
    expect(button1).toHaveTextContent('Show Details')
    expect(button2).toHaveTextContent('Hide Details')
  })
})

describe('UserSubscriptionTimeline - State Display', () => {
  const mockEventWithFullState: SubscriptionHistoryEvent[] = [
    {
      id: 'evt-1',
      subscriptionId: 'sub-1',
      eventType: 'upgraded',
      timestamp: '2025-01-20T10:30:00Z',
      actor: 'admin@example.com',
      changes: {
        changedFields: ['entitlementKey'],
        previousEntitlementKey: 'basic-plan',
        newEntitlementKey: 'pro-plan',
      },
      previousState: {
        id: 'sub-1',
        realmId: 'realm-1',
        status: 'active',
        entitlementKey: 'basic-plan',
        cancelAtPeriodEnd: false,
        currentPeriodStart: '2025-01-01T00:00:00Z',
        currentPeriodEnd: '2025-02-01T00:00:00Z',
      },
      newState: {
        id: 'sub-1',
        realmId: 'realm-1',
        status: 'active',
        entitlementKey: 'pro-plan',
        cancelAtPeriodEnd: false,
        currentPeriodStart: '2025-01-01T00:00:00Z',
        currentPeriodEnd: '2025-02-01T00:00:00Z',
      },
    },
  ]

  it('should display previous state when expanded', async () => {
    const user = userEvent.setup()
    render(<UserSubscriptionTimeline events={mockEventWithFullState} />)

    const showButton = screen.getByTestId('toggle-event-details-evt-1')
    await user.click(showButton)

    expect(screen.getByText(/Previous State/i)).toBeInTheDocument()
    // Check that we have multiple Status: labels (both previous and new state)
    expect(screen.getAllByText(/Status:/i).length).toBeGreaterThan(0)
    expect(screen.getAllByText('active').length).toBeGreaterThan(0)
    expect(screen.getAllByText(/Entitlement:/i).length).toBeGreaterThan(0)
    expect(screen.getAllByText('basic-plan').length).toBeGreaterThan(0)
  })

  it('should display new state when expanded', async () => {
    const user = userEvent.setup()
    render(<UserSubscriptionTimeline events={mockEventWithFullState} />)

    const showButton = screen.getByTestId('toggle-event-details-evt-1')
    await user.click(showButton)

    expect(screen.getByText(/New State/i)).toBeInTheDocument()
    expect(screen.getByText('pro-plan')).toBeInTheDocument()
  })

  it('should display billing period when both dates are present', async () => {
    const user = userEvent.setup()
    render(<UserSubscriptionTimeline events={mockEventWithFullState} />)

    const showButton = screen.getByTestId('toggle-event-details-evt-1')
    await user.click(showButton)

    // Check that we have multiple Period: labels (both previous and new state)
    expect(screen.getAllByText(/Period:/i).length).toBeGreaterThan(0)
  })

  it('should show plan change indicator for upgrade events', async () => {
    const user = userEvent.setup()
    render(<UserSubscriptionTimeline events={mockEventWithFullState} />)

    const showButton = screen.getByTestId('toggle-event-details-evt-1')
    await user.click(showButton)

    expect(screen.getByText('Plan changed')).toBeInTheDocument()
  })

  it('should show plan assigned indicator when only new plan exists', async () => {
    const eventWithNewPlanOnly: SubscriptionHistoryEvent[] = [
      {
        id: 'evt-1',
        subscriptionId: 'sub-1',
        eventType: 'created',
        timestamp: '2025-01-20T10:30:00Z',
        actor: 'system',
        changes: {
          changedFields: ['entitlementKey'],
          newEntitlementKey: 'basic-plan',
        },
        newState: {
          id: 'sub-1',
          realmId: 'realm-1',
          status: 'active',
          entitlementKey: 'basic-plan',
          cancelAtPeriodEnd: false,
        },
      },
    ]

    const user = userEvent.setup()
    render(<UserSubscriptionTimeline events={eventWithNewPlanOnly} />)

    const showButton = screen.getByTestId('toggle-event-details-evt-1')
    await user.click(showButton)

    expect(screen.getByText('Plan assigned')).toBeInTheDocument()
  })
})

describe('UserSubscriptionTimeline - Actor and Changes Display', () => {
  it('should not display actor when not available', () => {
    const mockEvent: SubscriptionHistoryEvent[] = [
      {
        id: 'evt-1',
        subscriptionId: 'sub-1',
        eventType: 'created',
        timestamp: '2025-01-20T10:30:00Z',
      },
    ]

    render(<UserSubscriptionTimeline events={mockEvent} />)

    expect(screen.queryByText(/Actor:/i)).not.toBeInTheDocument()
  })

  it('should not display changed fields when not available', () => {
    const mockEvent: SubscriptionHistoryEvent[] = [
      {
        id: 'evt-1',
        subscriptionId: 'sub-1',
        eventType: 'created',
        timestamp: '2025-01-20T10:30:00Z',
        actor: 'user@example.com',
      },
    ]

    render(<UserSubscriptionTimeline events={mockEvent} />)

    expect(screen.queryByText(/Changed:/i)).not.toBeInTheDocument()
  })
})

describe('UserSubscriptionTimeline - Loading State', () => {
  it('should display loading state when loading prop is true', () => {
    render(<UserSubscriptionTimeline events={[]} loading={true} />)

    expect(screen.getByTestId('subscription-timeline-loading')).toBeInTheDocument()
    expect(screen.getByText(/Loading history\.\.\./i)).toBeInTheDocument()
  })
})

describe('UserSubscriptionTimeline - Empty State', () => {
  it('should display empty state when no events', () => {
    render(<UserSubscriptionTimeline events={[]} />)

    expect(screen.getByTestId('subscription-timeline-empty')).toBeInTheDocument()
    expect(screen.getByText(/No history available/i)).toBeInTheDocument()
  })
})

describe('UserSubscriptionTimeline - Edge Cases', () => {
  it('should handle events with only previous state', async () => {
    const user = userEvent.setup()
    const eventWithPreviousOnly: SubscriptionHistoryEvent[] = [
      {
        id: 'evt-1',
        subscriptionId: 'sub-1',
        eventType: 'canceled',
        timestamp: '2025-01-20T10:30:00Z',
        actor: 'user@example.com',
        previousState: {
          id: 'sub-1',
          realmId: 'realm-1',
          status: 'active',
          entitlementKey: 'basic',
          cancelAtPeriodEnd: false,
        },
      },
    ]

    render(<UserSubscriptionTimeline events={eventWithPreviousOnly} />)

    const showButton = screen.getByTestId('toggle-event-details-evt-1')
    await user.click(showButton)

    expect(screen.getByText(/Previous State/i)).toBeInTheDocument()
    expect(screen.queryByText(/New State/i)).not.toBeInTheDocument()
  })

  it('should handle events with only new state', async () => {
    const user = userEvent.setup()
    const eventWithNewOnly: SubscriptionHistoryEvent[] = [
      {
        id: 'evt-1',
        subscriptionId: 'sub-1',
        eventType: 'created',
        timestamp: '2025-01-20T10:30:00Z',
        actor: 'system',
        newState: {
          id: 'sub-1',
          realmId: 'realm-1',
          status: 'active',
          entitlementKey: 'basic',
          cancelAtPeriodEnd: false,
        },
      },
    ]

    render(<UserSubscriptionTimeline events={eventWithNewOnly} />)

    const showButton = screen.getByTestId('toggle-event-details-evt-1')
    await user.click(showButton)

    expect(screen.queryByText(/Previous State/i)).not.toBeInTheDocument()
    expect(screen.getByText(/New State/i)).toBeInTheDocument()
  })

  it('should handle events without period dates', async () => {
    const user = userEvent.setup()
    const eventWithoutPeriod: SubscriptionHistoryEvent[] = [
      {
        id: 'evt-1',
        subscriptionId: 'sub-1',
        eventType: 'upgraded',
        timestamp: '2025-01-20T10:30:00Z',
        actor: 'admin@example.com',
        newState: {
          id: 'sub-1',
          realmId: 'realm-1',
          status: 'active',
          entitlementKey: 'pro',
          cancelAtPeriodEnd: false,
        },
      },
    ]

    render(<UserSubscriptionTimeline events={eventWithoutPeriod} />)

    const showButton = screen.getByTestId('toggle-event-details-evt-1')
    await user.click(showButton)

    // Period should not be displayed if dates are missing
    const eventContainer = screen.getByTestId('timeline-event-evt-1')
    const periodTexts = within(eventContainer).queryAllByText(/Period:/i)
    expect(periodTexts.length).toBe(0)
  })

  it('should handle events with changes but no plan changes', async () => {
    const user = userEvent.setup()
    const eventWithOtherChanges: SubscriptionHistoryEvent[] = [
      {
        id: 'evt-1',
        subscriptionId: 'sub-1',
        eventType: 'billing_period_changed',
        timestamp: '2025-01-20T10:30:00Z',
        actor: 'admin@example.com',
        changes: {
          changedFields: ['status'],
          previousStatus: 'active',
          newStatus: 'past_due',
        },
        newState: {
          id: 'sub-1',
          realmId: 'realm-1',
          status: 'past_due',
          entitlementKey: 'basic',
          cancelAtPeriodEnd: false,
        },
      },
    ]

    render(<UserSubscriptionTimeline events={eventWithOtherChanges} />)

    const showButton = screen.getByTestId('toggle-event-details-evt-1')
    await user.click(showButton)

    // Should not show plan changed/assigned indicator
    expect(screen.queryByText('Plan changed')).not.toBeInTheDocument()
    expect(screen.queryByText('Plan assigned')).not.toBeInTheDocument()
  })
})
