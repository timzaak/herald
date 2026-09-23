import { http, HttpResponse } from 'msw'
import type {
  SingleSubscriptionHistoryResponse,
  GlobalSubscriptionHistoryResponse,
  SubscriptionHistoryEvent,
  SubscriptionHistoryEventWithUser,
} from '@/types/billing'
import { stripeHandlers } from './handlers/stripe'
import { pointsHandlers } from './handlers/points'
import { unifiedPurchaseHandlers } from './handlers/unified-purchase'
import { auditHandlers } from './handlers/audit'
import { deviceHandlers } from './handlers/device'
import { dashboardHandlers } from './handlers/dashboard'
import { entitlementMappingHandlers } from './handlers/entitlement-mappings'

// Add API handlers here and override with `server.use(...)` in specific tests when needed.
export const handlers = [
  // Stripe handlers
  ...stripeHandlers,

  // Points handlers
  ...pointsHandlers,

  // Entitlement-mappings (price-granularity billing) handlers.
  // Default GET returns an EMPTY list; feature tests override via server.use.
  ...entitlementMappingHandlers,

  // Unified Purchase handlers
  ...unifiedPurchaseHandlers,

  // Audit handlers
  ...auditHandlers,

  // Device code handlers
  ...deviceHandlers,

  // Dashboard handlers
  ...dashboardHandlers,
  http.get('/__msw_health__', () => {
    return new Response(null, { status: 204 })
  }),

  // Subscription History Handlers
  http.get('/api/bill/subscriptions/:subscriptionId/history', ({ params }) => {
    const { realmId, subscriptionId } = params

    const mockEvents: SubscriptionHistoryEvent[] = [
      {
        id: 'evt-1',
        subscriptionId: subscriptionId as string,
        eventType: 'created',
        timestamp: '2025-01-01T09:00:00Z',
        actor: 'user@example.com',
        previousState: {
          id: subscriptionId as string,
          realmId: realmId as string,
          status: 'active',
          entitlementKey: 'basic',
          paymentProvider: 'stripe',
          externalPriceId: 'price_basic',
          clientAppId: 'app-1',
          cancelAtPeriodEnd: false,
        },
      },
      {
        id: 'evt-2',
        subscriptionId: subscriptionId as string,
        eventType: 'upgraded',
        timestamp: '2025-01-20T10:30:00Z',
        actor: 'user@example.com',
        changes: {
          changedFields: ['entitlementKey'],
          previousEntitlementKey: 'basic',
          newEntitlementKey: 'pro',
        },
        previousState: {
          id: subscriptionId as string,
          realmId: realmId as string,
          status: 'active',
          entitlementKey: 'basic',
          paymentProvider: 'stripe',
          externalPriceId: 'price_basic',
          clientAppId: 'app-1',
          cancelAtPeriodEnd: false,
        },
        newState: {
          id: subscriptionId as string,
          realmId: realmId as string,
          status: 'active',
          entitlementKey: 'pro',
          paymentProvider: 'stripe',
          externalPriceId: 'price_pro',
          clientAppId: 'app-1',
          cancelAtPeriodEnd: false,
        },
      },
      {
        id: 'evt-3',
        subscriptionId: subscriptionId as string,
        eventType: 'canceled',
        timestamp: '2025-02-15T14:00:00Z',
        actor: 'user@example.com',
        changes: {
          changedFields: ['status'],
          previousStatus: 'active',
          newStatus: 'canceled',
        },
        previousState: {
          id: subscriptionId as string,
          realmId: realmId as string,
          status: 'active',
          entitlementKey: 'pro',
          paymentProvider: 'stripe',
          externalPriceId: 'price_pro',
          clientAppId: 'app-1',
          cancelAtPeriodEnd: false,
        },
        newState: {
          id: subscriptionId as string,
          realmId: realmId as string,
          status: 'canceled',
          entitlementKey: 'pro',
          paymentProvider: 'stripe',
          externalPriceId: 'price_pro',
          clientAppId: 'app-1',
          cancelAtPeriodEnd: true,
          cancelAt: '2025-03-15T00:00:00Z',
        },
      },
    ]

    const response: SingleSubscriptionHistoryResponse = {
      subscriptionId: subscriptionId as string,
      events: mockEvents,
      total: mockEvents.length,
    }

    return HttpResponse.json(response)
  }),

  http.get('/api/bill/subscriptions/history', ({ request, params }) => {
    const url = new URL(request.url)
    const eventType = url.searchParams.get('eventType') as any
    const userId = url.searchParams.get('userId')
    const entitlementKey = url.searchParams.get('entitlementKey')
    const subscriptionStatus = url.searchParams.get('subscriptionStatus')
    const page = parseInt(url.searchParams.get('page') || '1')
    const pageSize = parseInt(url.searchParams.get('pageSize') || '20')

    const mockEvents: SubscriptionHistoryEventWithUser[] = [
      {
        id: 'evt-1',
        subscriptionId: 'sub-1',
        eventType: eventType || 'created',
        timestamp: '2025-01-01T09:00:00Z',
        actor: 'user@example.com',
        user: {
          id: 'user-1',
          email: 'user@example.com',
        },
        subscription: {
          id: 'sub-1',
          status: 'active',
          entitlementKey: 'basic',
        },
      },
      {
        id: 'evt-2',
        subscriptionId: 'sub-2',
        eventType: 'upgraded',
        timestamp: '2025-01-20T10:30:00Z',
        actor: 'user2@example.com',
        user: {
          id: 'user-2',
          email: 'user2@example.com',
        },
        subscription: {
          id: 'sub-2',
          status: 'active',
          entitlementKey: 'pro',
        },
      },
    ]

    // Apply filters
    let filteredEvents = mockEvents
    if (eventType) {
      filteredEvents = filteredEvents.filter((e) => e.eventType === eventType)
    }
    if (userId) {
      filteredEvents = filteredEvents.filter((e) => e.user?.id === userId)
    }
    if (entitlementKey) {
      filteredEvents = filteredEvents.filter(
        (e) => e.subscription.entitlementKey === entitlementKey
      )
    }
    if (subscriptionStatus) {
      filteredEvents = filteredEvents.filter((e) => e.subscription.status === subscriptionStatus)
    }

    const response: GlobalSubscriptionHistoryResponse = {
      events: filteredEvents,
      pagination: {
        page,
        pageSize,
        total: filteredEvents.length,
        totalPages: Math.ceil(filteredEvents.length / pageSize),
      },
    }

    return HttpResponse.json(response)
  }),
]
