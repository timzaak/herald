import { describe, it, expect, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { SubscriptionSelector } from '../subscription-selector'
import type { ClientAppItem, SubscriptionDetailResponse } from '@/lib/api-generated'
import { server } from '@/test/mocks/server'
import { renderWithProviders } from '@/test/utils/render'

const REALM_ID = 'realm-1'
const BASE_URL = 'http://localhost:3000'
const SUB_ID = 'sub1'

describe('SubscriptionSelector - Rendering', () => {
  it('should display empty state when no subscriptions', () => {
    const mockOnSelect = vi.fn()

    render(<SubscriptionSelector subscriptions={[]} onSelect={mockOnSelect} />)

    expect(screen.getByTestId('subscription-selector-empty')).toBeInTheDocument()
    expect(screen.getByText('No subscriptions found')).toBeInTheDocument()
    expect(screen.queryByTestId('subscription-selector')).not.toBeInTheDocument()
  })

  it('should render subscription without subscription object', () => {
    const mockOnSelect = vi.fn()
    const subscriptionsWithoutSubscription = [
      {
        clientApp: {
          id: 'app1',
          name: 'App 1',
          description: 'Description 1',
          clientId: 'client-1',
          clientSecret: 'secret-1',
          redirectUris: ['http://localhost:3000/callback'],
          postLogoutRedirectUris: ['http://localhost:3000/logout'],
          scopes: ['openid', 'profile'],
          grantTypes: ['authorization_code'],
          realmId: 'realm-1',
          createdAt: '2025-01-01T00:00:00Z',
          updatedAt: '2025-01-01T00:00:00Z',
        },
        subscription: null,
      },
    ]

    render(
      <SubscriptionSelector
        subscriptions={subscriptionsWithoutSubscription}
        onSelect={mockOnSelect}
      />
    )

    expect(screen.getByTestId('subscription-card-app1')).toBeInTheDocument()
    expect(screen.getByText('No subscription')).toBeInTheDocument()
    expect(screen.queryByText('Plan:')).not.toBeInTheDocument()
  })
})

describe('SubscriptionSelector - Selection Logic', () => {
  const mockSubscriptions: Array<{
    clientApp: ClientAppItem
    subscription: SubscriptionDetailResponse | null
  }> = [
    {
      clientApp: {
        id: 'app1',
        name: 'App 1',
        description: 'Description 1',
        clientId: 'client-1',
        clientSecret: 'secret-1',
        redirectUris: ['http://localhost:3000/callback'],
        postLogoutRedirectUris: ['http://localhost:3000/logout'],
        scopes: ['openid', 'profile'],
        grantTypes: ['authorization_code'],
        realmId: 'realm-1',
        createdAt: '2025-01-01T00:00:00Z',
        updatedAt: '2025-01-01T00:00:00Z',
      },
      subscription: {
        id: 'sub1',
        status: 'active',
        entitlementKey: 'basic',
        paymentProvider: 'stripe',
        externalPriceId: 'price_basic',
        currentPeriodStart: '2025-01-01T00:00:00Z',
        currentPeriodEnd: '2025-02-01T00:00:00Z',
        cancelAtPeriodEnd: false,
      } as SubscriptionDetailResponse,
    },
  ]

  it('should call onSelect with subscription ID when subscription card is clicked', async () => {
    const mockOnSelect = vi.fn()
    const user = userEvent.setup()

    render(<SubscriptionSelector subscriptions={mockSubscriptions} onSelect={mockOnSelect} />)

    const card = screen.getByTestId('subscription-card-app1')
    await user.click(card)

    expect(mockOnSelect).toHaveBeenCalledTimes(1)
    expect(mockOnSelect).toHaveBeenCalledWith('sub1')
  })

  it('should call onApplyInvoice with subscription ID without selecting card', async () => {
    // The Invoice button is eligibility-gated: the test must stub a
    // manual_fallback eligibility verdict for the button to be enabled.
    server.use(
      http.get(`${BASE_URL}/api/user/bill/invoices/apply-eligibility`, () => {
        return HttpResponse.json({
          referenceType: 'subscription',
          referenceId: SUB_ID,
          canApply: true,
          route: 'manual_fallback',
          provider: null,
          reason: null,
        })
      })
    )
    const mockOnSelect = vi.fn()
    const mockOnApplyInvoice = vi.fn()
    const user = userEvent.setup()

    renderWithProviders(
      <SubscriptionSelector
        subscriptions={mockSubscriptions}
        onSelect={mockOnSelect}
        realmId={REALM_ID}
        onApplyInvoice={mockOnApplyInvoice}
      />
    )

    const button = await screen.findByTestId(`subscription-invoice-button-${SUB_ID}`)
    await waitFor(() => {
      expect(button).toBeEnabled()
    })
    await user.click(button)

    expect(mockOnApplyInvoice).toHaveBeenCalledTimes(1)
    expect(mockOnApplyInvoice).toHaveBeenCalledWith(SUB_ID)
    // Clicking the eligibility button must not bubble to card selection.
    expect(mockOnSelect).not.toHaveBeenCalled()
  })

  it('should call onSelect with client app ID when subscription is null', async () => {
    const mockOnSelect = vi.fn()
    const user = userEvent.setup()
    const subscriptionsWithoutSubscription = [
      {
        clientApp: {
          id: 'app1',
          name: 'App 1',
          description: 'Description 1',
          clientId: 'client-1',
          clientSecret: 'secret-1',
          redirectUris: ['http://localhost:3000/callback'],
          postLogoutRedirectUris: ['http://localhost:3000/logout'],
          scopes: ['openid', 'profile'],
          grantTypes: ['authorization_code'],
          realmId: 'realm-1',
          createdAt: '2025-01-01T00:00:00Z',
          updatedAt: '2025-01-01T00:00:00Z',
        },
        subscription: null,
      },
    ]

    render(
      <SubscriptionSelector
        subscriptions={subscriptionsWithoutSubscription}
        onSelect={mockOnSelect}
      />
    )

    const card = screen.getByTestId('subscription-card-app1')
    await user.click(card)

    expect(mockOnSelect).toHaveBeenCalledTimes(1)
    expect(mockOnSelect).toHaveBeenCalledWith('app1')
  })

  it('should highlight selected subscription card', () => {
    const mockOnSelect = vi.fn()

    render(
      <SubscriptionSelector
        subscriptions={mockSubscriptions}
        selectedId="sub1"
        onSelect={mockOnSelect}
      />
    )

    const card = screen.getByTestId('subscription-card-app1')
    expect(card).toHaveClass('border-primary', 'ring-2', 'ring-primary', 'ring-offset-2')
  })

  it('should not highlight unselected subscription cards', () => {
    const mockOnSelect = vi.fn()

    render(
      <SubscriptionSelector
        subscriptions={mockSubscriptions}
        selectedId="sub2"
        onSelect={mockOnSelect}
      />
    )

    const card = screen.getByTestId('subscription-card-app1')
    expect(card).toHaveClass('border-border')
    expect(card).not.toHaveClass('border-primary', 'ring-2', 'ring-primary', 'ring-offset-2')
  })
})

describe('SubscriptionSelector - Date Formatting', () => {
  it('should format currentPeriodEnd date correctly', () => {
    const mockOnSelect = vi.fn()

    const mockSubscription: Array<{
      clientApp: ClientAppItem
      subscription: SubscriptionDetailResponse | null
    }> = [
      {
        clientApp: {
          id: 'app1',
          name: 'App 1',
          description: 'Description 1',
          clientId: 'client-1',
          clientSecret: 'secret-1',
          redirectUris: ['http://localhost:3000/callback'],
          postLogoutRedirectUris: ['http://localhost:3000/logout'],
          scopes: ['openid', 'profile'],
          grantTypes: ['authorization_code'],
          realmId: 'realm-1',
          createdAt: '2025-01-01T00:00:00Z',
          updatedAt: '2025-01-01T00:00:00Z',
        },
        subscription: {
          id: 'sub1',
          status: 'active',
          entitlementKey: 'basic',
          paymentProvider: 'stripe',
          externalPriceId: 'price_basic',
          currentPeriodStart: '2025-01-01T00:00:00Z',
          currentPeriodEnd: '2025-02-15T00:00:00Z',
          cancelAtPeriodEnd: false,
        } as SubscriptionDetailResponse,
      },
    ]

    render(<SubscriptionSelector subscriptions={mockSubscription} onSelect={mockOnSelect} />)

    // The date should be formatted using toLocaleDateString()
    // We just check that "Expires:" text is present
    expect(screen.getByText(/Expires:/i)).toBeInTheDocument()
  })

  it('should not show expiry date when currentPeriodEnd is missing', () => {
    const mockOnSelect = vi.fn()

    const mockSubscription: Array<{
      clientApp: ClientAppItem
      subscription: SubscriptionDetailResponse | null
    }> = [
      {
        clientApp: {
          id: 'app1',
          name: 'App 1',
          description: 'Description 1',
          clientId: 'client-1',
          clientSecret: 'secret-1',
          redirectUris: ['http://localhost:3000/callback'],
          postLogoutRedirectUris: ['http://localhost:3000/logout'],
          scopes: ['openid', 'profile'],
          grantTypes: ['authorization_code'],
          realmId: 'realm-1',
          createdAt: '2025-01-01T00:00:00Z',
          updatedAt: '2025-01-01T00:00:00Z',
        },
        subscription: {
          id: 'sub1',
          status: 'active',
          entitlementKey: 'basic',
          paymentProvider: 'stripe',
          externalPriceId: 'price_basic',
          currentPeriodStart: '2025-01-01T00:00:00Z',
          cancelAtPeriodEnd: false,
        } as SubscriptionDetailResponse,
      },
    ]

    render(<SubscriptionSelector subscriptions={mockSubscription} onSelect={mockOnSelect} />)

    expect(screen.queryByText(/Expires:/i)).not.toBeInTheDocument()
  })
})
