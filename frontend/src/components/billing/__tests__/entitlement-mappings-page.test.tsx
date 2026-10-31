import { beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import type { EntitlementMappingResponse } from '@/lib/api-generated'

vi.mock('@/hooks/use-permission', () => ({
  usePermission: () => ({ hasPermission: () => true }),
}))

// The page's Create Mapping button navigates via TanStack Router. Mock
// useNavigate + realm routing so the component renders without a router
// provider (mirrors the client-app-form-page test).
vi.mock('@tanstack/react-router', async () => {
  const actual =
    await vi.importActual<typeof import('@tanstack/react-router')>('@tanstack/react-router')
  return {
    ...actual,
    useNavigate: () => vi.fn(),
  }
})

vi.mock('@/lib/realm-routing', () => ({
  realmPath: (_ctx: unknown, path: string) => path,
  useResolvedRealmContext: () => ({ realmId: 'admin', mode: 'default' }),
}))

vi.mock('@/data/query-options', () => ({
  queryKeys: {
    entitlementMappings: (realmId: string) => ['entitlement-mappings', realmId, {}],
  },
  entitlementMappingsQueryOptions: (realmId: string) => ({
    queryKey: ['entitlement-mappings', realmId, {}],
    queryFn: async () => ({ items: [], total: 0 }),
    staleTime: Number.POSITIVE_INFINITY,
  }),
  adminRolesQueryOptions: (realmId: string) => ({
    queryKey: ['roles', realmId],
    queryFn: async () => [],
  }),
  creditBucketsListQueryOptions: (realmId: string) => ({
    queryKey: ['credit-buckets', realmId],
    queryFn: async () => [
      {
        id: 'bucket-a',
        name: 'General',
        bucketKey: 'general',
        displayOrder: 0,
        enabled: true,
        coveredClientAppCount: 1,
        ruleReferenceCount: 1,
      },
      {
        id: 'bucket-b',
        name: 'Images',
        bucketKey: 'images',
        displayOrder: 1,
        enabled: true,
        coveredClientAppCount: 1,
        ruleReferenceCount: 1,
      },
    ],
  }),
}))

const { batchMutate, updateMutate } = vi.hoisted(() => ({
  batchMutate: vi.fn(),
  updateMutate: vi.fn(),
}))

vi.mock('@/data/entitlement-mapping-mutations', () => ({
  useBatchUpdateEntitlementMappings: () => ({ mutate: batchMutate, isPending: false }),
  useUpdateEntitlementMapping: () => ({ mutate: updateMutate, isPending: false }),
  useCreateEntitlementMapping: () => ({ mutate: vi.fn(), isPending: false }),
  isProtectedPriceError: () => false,
  extractActiveSubscriptions: () => null,
}))

vi.mock('@/components/billing/provider-sync-button', () => ({
  ProviderSyncButton: () => <div data-testid="provider-sync-button" />,
}))

vi.mock('@/components/shared/role-selector', () => ({
  RoleSelector: () => <div data-testid="role-selector" />,
}))

import { EntitlementMappingsPage } from '../entitlement-mappings-page'

const mapping: EntitlementMappingResponse = {
  id: 'mapping-1',
  createdAt: '2026-01-01T00:00:00Z',
  updatedAt: '2026-01-01T00:00:00Z',
  paymentProvider: 'stripe',
  externalProductId: 'prod-1',
  externalPriceId: 'price-1',
  entitlementKey: 'pro',
  enabled: true,
  billingType: 'recurring',
  billingPeriod: 'month',
  grantedRoleIds: [],
  pointRules: [
    {
      id: 'rule-1',
      bucketId: 'bucket-a',
      triggerSources: ['subscription_initial', 'subscription_renewal'],
      grantMode: 'fixed',
      pointsAmount: 100,
      validityDays: 30,
      enabled: true,
      displayOrder: 0,
    },
    {
      id: 'rule-2',
      bucketId: 'bucket-b',
      triggerSources: ['subscription_initial'],
      grantMode: 'quota',
      quotaWindows: [{ key: 'hour', windowSeconds: 3600, limit: 20 }],
      enabled: true,
      displayOrder: 1,
    },
  ],
}

function renderPage(mappingOverride?: EntitlementMappingResponse) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  client.setQueryData(['entitlement-mappings', 'realm-1', {}], {
    items: [mappingOverride ?? mapping],
    total: 1,
  })
  return render(
    <QueryClientProvider client={client}>
      <EntitlementMappingsPage realmId="realm-1" />
    </QueryClientProvider>
  )
}

// WeChat rows price by hand (no hosted catalog to sync from): the stored
// manual price lives in the same provider_product_info JSONB keys sync writes.
const wechatMapping: EntitlementMappingResponse = {
  ...mapping,
  id: 'mapping-wechat',
  paymentProvider: 'wechat',
  externalProductId: 'wx_prod_1',
  externalPriceId: null,
  entitlementKey: 'wechat-pro',
  billingType: 'non_renewing',
  billingPeriod: null,
  serviceDurationDays: 30,
  providerProductInfo: { price: 1990, currency: 'CNY', name: 'wechat-pro' },
  pointRules: [],
}

describe('EntitlementMappingsPage distribution rules', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders each fixed and quota rule without collapsing them into one amount', async () => {
    renderPage()
    expect(await screen.findByTestId('point-rule-rule-1')).toBeInTheDocument()
    expect(screen.getByTestId('point-rule-rule-2')).toBeInTheDocument()
    expect(screen.getAllByTestId('point-rule-trigger-subscription_initial')).toHaveLength(2)
    expect(screen.getByTestId('point-rule-quota-rule-2-editor')).toBeInTheDocument()
  })

  it('turns removal of a persisted rule into an explicit disabled upsert', async () => {
    const user = userEvent.setup()
    renderPage()

    await user.click(await screen.findByTestId('point-rule-remove-rule-1'))
    await user.click(screen.getByTestId('save-mapping-button'))

    expect(batchMutate).toHaveBeenCalledOnce()
    const request = batchMutate.mock.calls[0]?.[0]
    expect(request.updates[0].pointRules).toEqual(
      expect.arrayContaining([expect.objectContaining({ id: 'rule-1', enabled: false })])
    )
    expect(request.updates[0]).not.toHaveProperty('pointsPerPeriod')
    expect(request.updates[0]).not.toHaveProperty('bucketId')
  })
})

describe('EntitlementMappingsPage header tooltip', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  // "Entitlement" is billing-domain jargon; the header must let an admin who
  // has never worked with Stripe/RevenueCat learn what the term means here.
  it('defines what an entitlement is next to the page title', async () => {
    const user = userEvent.setup()
    renderPage()

    await user.hover(screen.getByTestId('page-header-title-tooltip'))
    expect(await screen.findByTestId('page-header-title-tooltip-content')).toHaveTextContent(
      /An entitlement is the benefit a user unlocks/
    )
  })
})

describe('EntitlementMappingsPage WeChat manual price', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('edits the stored price in major units and PUTs integer minor units on blur', async () => {
    const user = userEvent.setup()
    renderPage(wechatMapping)

    const priceInput = await screen.findByTestId('price-manual-price-mapping-wechat')
    expect(priceInput).toHaveValue('19.90')

    await user.clear(priceInput)
    await user.type(priceInput, '29.9')
    await user.tab()

    expect(updateMutate).toHaveBeenCalledWith({ price: 2990 })
  })

  it('PUTs the currency when it changes on blur', async () => {
    const user = userEvent.setup()
    renderPage(wechatMapping)

    const currencyInput = await screen.findByTestId('price-manual-currency-mapping-wechat')
    expect(currencyInput).toHaveValue('CNY')

    await user.clear(currencyInput)
    await user.type(currencyInput, 'USD')
    await user.tab()

    expect(updateMutate).toHaveBeenCalledWith({ currency: 'USD' })
  })

  it('does not fire a price PUT when the value is unchanged or invalid', async () => {
    const user = userEvent.setup()
    renderPage(wechatMapping)

    const priceInput = await screen.findByTestId('price-manual-price-mapping-wechat')
    // Unchanged value on blur → no request.
    await user.click(priceInput)
    await user.tab()
    expect(updateMutate).not.toHaveBeenCalled()

    // Malformed input on blur → no request (the editor keeps the text; the
    // stored value is untouched until a valid price replaces it).
    await user.clear(priceInput)
    await user.type(priceInput, 'abc')
    await user.tab()
    expect(updateMutate).not.toHaveBeenCalled()
  })
})
