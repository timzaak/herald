import { describe, it, expect, vi } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import { http, HttpResponse } from 'msw'

// TanStack Router's <Link> needs a router context; mock it to a plain <a> so
// the entry CTAs (Change plan / Browse plans) render in jsdom. The mock
// forwards the literal `to`, which is enough for the "links to
// purchase-points" assertion.
vi.mock('@tanstack/react-router', () => ({
  Link: ({
    children,
    to,
    ...rest
  }: {
    children: React.ReactNode
    to: string
    params?: Record<string, string>
    [key: string]: unknown
  }) => (
    <a href={to} {...rest}>
      {children}
    </a>
  ),
}))

import { MySubscriptionsPage } from '../my-subscriptions-page'
import type { ClientAppItem, SubscriptionDetailResponse } from '@/lib/api-generated'
import { server } from '@/test/mocks/server'
import { renderWithProviders } from '@/test/utils/render'

const REALM_ID = 'realm-1'

const clientApp: ClientAppItem = {
  id: 'app-1',
  name: 'Demo App',
  description: '',
  clientId: 'client-1',
  clientSecret: '',
  redirectUris: [],
  postLogoutRedirectUris: [],
  scopes: ['openid'],
  grantTypes: ['authorization_code'],
  realmId: REALM_ID,
  createdAt: '2025-01-01T00:00:00Z',
  updatedAt: '2025-01-01T00:00:00Z',
}

function stubClientApps(apps: ClientAppItem[]) {
  server.use(
    http.get('/api/client', () =>
      HttpResponse.json({ items: apps, total: apps.length, page: 0, pageSize: 100 })
    )
  )
}

/**
 * Stub the per-client-app subscription lookup consumed by
 * `userSubscriptionsQueryOptions`. The page issues one request per client app
 * via `getSubscriptionForClientApp`; we answer with the provided subscription
 * (or 404 to signal "no subscription for this app").
 */
function stubSubscriptionsByApp(map: Record<string, SubscriptionDetailResponse | 404>) {
  server.use(
    http.get('/api/bill/client/:clientAppId/subscription', ({ params }) => {
      const sub = map[params.clientAppId as string]
      if (sub === 404) return new HttpResponse(null, { status: 404 })
      return HttpResponse.json(sub)
    })
  )
}

const baseSubscription = (overrides: Partial<SubscriptionDetailResponse>) =>
  ({
    id: 'sub-1',
    entitlementKey: 'pro-plan',
    status: 'active',
    paymentProvider: 'stripe',
    currentPeriodStart: '2026-01-01T00:00:00Z',
    currentPeriodEnd: '2026-02-01T00:00:00Z',
    ...overrides,
  }) as SubscriptionDetailResponse

describe('MySubscriptionsPage — Change plan / Browse plans entry', () => {
  it('renders a Change plan entry on an ACTIVE subscription (links to purchase-points)', async () => {
    stubClientApps([clientApp])
    stubSubscriptionsByApp({ 'app-1': baseSubscription({ id: 'sub-active', status: 'active' }) })

    renderWithProviders(<MySubscriptionsPage realmId={REALM_ID} />)

    // Change plan is gated on active status; assert it links to purchase-points.
    await waitFor(() =>
      expect(screen.queryByTestId('subscription-change-plan-sub-active')).toBeInTheDocument()
    )
    const changePlan = screen.getByTestId('subscription-change-plan-sub-active')
    expect(changePlan.getAttribute('href')).toContain('purchase-points')
  })

  it('does NOT render a Change plan entry on a non-active subscription', async () => {
    stubClientApps([clientApp])
    stubSubscriptionsByApp({
      'app-1': baseSubscription({ id: 'sub-cancelled', status: 'canceled' }),
    })

    renderWithProviders(<MySubscriptionsPage realmId={REALM_ID} />)

    // Card renders (history link present) but Change plan is absent.
    await waitFor(() => expect(screen.getByTestId('my-subscriptions-page')).toBeInTheDocument())
    expect(screen.queryByTestId('subscription-change-plan-sub-cancelled')).not.toBeInTheDocument()
  })

  it('renders a Browse plans CTA in the empty state (links to purchase-points)', async () => {
    // One client app, but no subscription for it → subscriptions list is empty.
    stubClientApps([clientApp])
    stubSubscriptionsByApp({ 'app-1': 404 })

    renderWithProviders(<MySubscriptionsPage realmId={REALM_ID} />)

    await waitFor(() =>
      expect(screen.getByTestId('my-subscriptions-browse-plans')).toBeInTheDocument()
    )
    const browse = screen.getByTestId('my-subscriptions-browse-plans')
    expect(browse.getAttribute('href')).toContain('purchase-points')
  })
})
