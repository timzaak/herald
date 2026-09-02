import { describe, it, expect, vi, beforeEach } from 'vitest'
import {
  purchaseHistoryQueryOptions,
  paymentAttemptStatusQueryOptions,
  paymentProvidersQueryOptions,
} from '@/data/query-options'
import type { PurchaseHistoryItem } from '@/lib/api-generated'

// Mock SDK functions used by query options under test
vi.mock('@/lib/api-generated/sdk.gen', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/lib/api-generated/sdk.gen')>()
  return {
    ...original,
    getPurchaseHistory: vi.fn(),
    getPaymentAttemptStatus: vi.fn(),
    listPaymentProviders: vi.fn(),
  }
})

import {
  getPurchaseHistory,
  getPaymentAttemptStatus,
  listPaymentProviders,
} from '@/lib/api-generated/sdk.gen'

// ==================== Factory functions ====================

function makePurchaseHistoryItem(overrides?: Partial<PurchaseHistoryItem>): PurchaseHistoryItem {
  return {
    attemptId: 'attempt-001',
    targetMappingId: 'mapping-001',
    productName: 'Premium Access',
    points: 1000,
    amount: 9.99,
    currency: 'USD',
    paymentProvider: 'stripe',
    status: 'Succeeded',
    completedAt: '2025-01-15T10:30:00Z',
    createdAt: '2025-01-15T10:00:00Z',
    ...overrides,
  }
}

function makePurchaseHistoryResponse(
  overrides?: Partial<{ items: PurchaseHistoryItem[]; total: number }>
) {
  return {
    items: [makePurchaseHistoryItem()],
    total: 1,
    ...overrides,
  }
}

// ==================== purchaseHistoryQueryOptions ====================

describe('purchaseHistoryQueryOptions', () => {
  beforeEach(() => {
    vi.mocked(getPurchaseHistory).mockResolvedValue({
      data: makePurchaseHistoryResponse(),
      error: undefined,
    })
  })

  it('calls getPurchaseHistory with empty query when no filters provided', async () => {
    const options = purchaseHistoryQueryOptions('realm-1')
    await options.queryFn()

    expect(getPurchaseHistory).toHaveBeenCalledWith({
      query: {},
    })
  })

  it('passes paymentProvider filter as snake_case query param', async () => {
    const options = purchaseHistoryQueryOptions('realm-1', {
      paymentProvider: 'stripe',
    })
    await options.queryFn()

    const callArgs = vi.mocked(getPurchaseHistory).mock.calls[0][0]
    expect(callArgs.query).toEqual({
      payment_provider: 'stripe',
    })
  })

  it('passes startDate and endDate as snake_case query params', async () => {
    const options = purchaseHistoryQueryOptions('realm-1', {
      startDate: '2025-01-01',
      endDate: '2025-06-30',
    })
    await options.queryFn()

    const callArgs = vi.mocked(getPurchaseHistory).mock.calls[0][0]
    expect(callArgs.query).toEqual({
      start_date: '2025-01-01',
      end_date: '2025-06-30',
    })
  })

  it('passes page and pageSize as snake_case query params', async () => {
    const options = purchaseHistoryQueryOptions('realm-1', {
      page: 2,
      pageSize: 50,
    })
    await options.queryFn()

    const callArgs = vi.mocked(getPurchaseHistory).mock.calls[0][0]
    expect(callArgs.query).toEqual({
      page: 2,
      page_size: 50,
    })
  })

  it('passes all filters combined', async () => {
    const options = purchaseHistoryQueryOptions('realm-1', {
      page: 1,
      pageSize: 25,
      paymentProvider: 'creem',
      startDate: '2025-01-01',
      endDate: '2025-12-31',
    })
    await options.queryFn()

    const callArgs = vi.mocked(getPurchaseHistory).mock.calls[0][0]
    expect(callArgs.query).toEqual({
      page: 1,
      page_size: 25,
      payment_provider: 'creem',
      start_date: '2025-01-01',
      end_date: '2025-12-31',
    })
  })

  it('returns response with items and total', async () => {
    const items = [
      makePurchaseHistoryItem({ attemptId: 'a-1' }),
      makePurchaseHistoryItem({ attemptId: 'a-2' }),
    ]
    vi.mocked(getPurchaseHistory).mockResolvedValue({
      data: makePurchaseHistoryResponse({ items, total: 2 }),
      error: undefined,
    })

    const options = purchaseHistoryQueryOptions('realm-1')
    const result = await options.queryFn()

    expect(result.items).toEqual(items)
    expect(result.total).toBe(2)
  })

  it('throws when API returns error', async () => {
    vi.mocked(getPurchaseHistory).mockResolvedValue({
      data: undefined,
      error: { message: 'Server error', status: 500 },
    })

    const options = purchaseHistoryQueryOptions('realm-1')

    await expect(options.queryFn()).rejects.toEqual({
      message: 'Server error',
      status: 500,
    })
  })
})

// ==================== paymentAttemptStatusQueryOptions (retained) ====================

describe('paymentAttemptStatusQueryOptions', () => {
  const mockAttemptResponse = {
    id: 'attempt-001',
    status: 'Pending',
    targetType: 'entitlement_mapping',
    targetId: 'mapping-001',
    amount: 9.99,
    currency: 'USD',
    createdAt: '2025-01-15T10:00:00Z',
    expiresAt: '2025-01-15T12:00:00Z',
    completedAt: null,
    fulfillment: null,
    providerStatus: null,
  }

  beforeEach(() => {
    vi.mocked(getPaymentAttemptStatus).mockResolvedValue({
      data: mockAttemptResponse,
      error: undefined,
    })
  })

  it('calls getPaymentAttemptStatus with correct path params', async () => {
    const options = paymentAttemptStatusQueryOptions('realm-1', 'attempt-001')
    await options.queryFn()

    expect(getPaymentAttemptStatus).toHaveBeenCalledWith({
      path: { realmId: 'realm-1', attemptId: 'attempt-001' },
    })
  })

  it('resolves with attempt status data', async () => {
    const options = paymentAttemptStatusQueryOptions('realm-1', 'attempt-001')
    const result = await options.queryFn()

    expect(result).toEqual(mockAttemptResponse)
  })

  it('throws when attemptId is empty string', async () => {
    const options = paymentAttemptStatusQueryOptions('realm-1', '')

    await expect(options.queryFn()).rejects.toThrow('attemptId is required')
  })

  it('throws when API returns error', async () => {
    vi.mocked(getPaymentAttemptStatus).mockResolvedValue({
      data: undefined,
      error: { message: 'Not found', status: 404 },
    })

    const options = paymentAttemptStatusQueryOptions('realm-1', 'attempt-001')

    await expect(options.queryFn()).rejects.toEqual({
      message: 'Not found',
      status: 404,
    })
  })

  it('has refetchInterval that polls for pending status', () => {
    const options = paymentAttemptStatusQueryOptions('realm-1', 'attempt-001')

    // Simulate pending state
    const result = options.refetchInterval!({
      state: { data: { ...mockAttemptResponse, status: 'Pending' } },
    } as any)

    expect(result).toBe(60000) // ONE_MINUTE
  })

  it('has refetchInterval that stops polling for succeeded status', () => {
    const options = paymentAttemptStatusQueryOptions('realm-1', 'attempt-001')

    const result = options.refetchInterval!({
      state: { data: { ...mockAttemptResponse, status: 'Succeeded' } },
    } as any)

    expect(result).toBe(false)
  })

  it('has refetchInterval that stops polling for failed status', () => {
    const options = paymentAttemptStatusQueryOptions('realm-1', 'attempt-001')

    const result = options.refetchInterval!({
      state: { data: { ...mockAttemptResponse, status: 'Failed' } },
    } as any)

    expect(result).toBe(false)
  })

  it('has refetchInterval that returns false for undefined query state', () => {
    const options = paymentAttemptStatusQueryOptions('realm-1', 'attempt-001')

    const result = options.refetchInterval!(undefined as any)

    expect(result).toBe(false)
  })
})

// ==================== paymentProvidersQueryOptions (retained) ====================

describe('paymentProvidersQueryOptions', () => {
  const mockProviders = [
    { provider: 'stripe', name: 'Stripe', enabled: true },
    { provider: 'creem', name: 'Creem', enabled: true },
  ]

  beforeEach(() => {
    vi.mocked(listPaymentProviders).mockResolvedValue({
      data: { providers: mockProviders },
      error: undefined,
    } as any)
  })

  it('calls listPaymentProviders with correct path param', async () => {
    const options = paymentProvidersQueryOptions('realm-1')
    await options.queryFn()

    expect(listPaymentProviders).toHaveBeenCalledWith({
      path: { realmId: 'realm-1' },
    })
  })

  it('returns providers array from response', async () => {
    const options = paymentProvidersQueryOptions('realm-1')
    const result = await options.queryFn()

    expect(result).toEqual(mockProviders)
  })

  it('returns empty array when providers is undefined', async () => {
    vi.mocked(listPaymentProviders).mockResolvedValue({
      data: { providers: undefined } as any,
      error: undefined,
    } as any)

    const options = paymentProvidersQueryOptions('realm-1')
    const result = await options.queryFn()

    expect(result).toEqual([])
  })

  it('throws when API returns error', async () => {
    vi.mocked(listPaymentProviders).mockResolvedValue({
      data: undefined,
      error: { message: 'Unauthorized', status: 401 },
    })

    const options = paymentProvidersQueryOptions('realm-1')

    await expect(options.queryFn()).rejects.toEqual({
      message: 'Unauthorized',
      status: 401,
    })
  })
})
