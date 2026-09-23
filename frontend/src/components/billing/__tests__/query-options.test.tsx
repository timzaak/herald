import { describe, it, expect, vi, beforeEach } from 'vitest'
import {
  featureAvailabilityQueryOptions,
  userFeatureAvailabilityQueryOptions,
  subscriptionHistoryQueryOptions,
  globalSubscriptionHistoryQueryOptions,
  getSubscriptionHistory,
  getGlobalSubscriptionHistory,
} from '@/data/query-options'
import type { HistoryFilters } from '@/types/billing'
import { QUERY_KEYS } from '@/lib/constants'
import {
  getFeatureAvailability,
  getUserFeatureAvailability,
  getSubscriptionHistory as getSubscriptionHistoryApi,
  listSubscriptionHistory,
} from '@/lib/api-generated'

vi.mock('@/lib/api-generated', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/lib/api-generated')>()),
  getFeatureAvailability: vi.fn(),
  getUserFeatureAvailability: vi.fn(),
  getSubscriptionHistory: vi.fn(),
  listSubscriptionHistory: vi.fn(),
}))

beforeEach(() => {
  vi.clearAllMocks()
})

describe('featureAvailabilityQueryOptions', () => {
  it('should load protected feature availability through the generated SDK', async () => {
    vi.mocked(getFeatureAvailability).mockResolvedValue({
      data: {},
      error: undefined,
    } as never)

    const options = featureAvailabilityQueryOptions('realm-1')
    await options.queryFn?.({} as never)

    expect(getFeatureAvailability).toHaveBeenCalledWith({})
  })
})

describe('userFeatureAvailabilityQueryOptions', () => {
  it('loads user-facing feature availability without using the admin realm endpoint', async () => {
    vi.mocked(getUserFeatureAvailability).mockResolvedValue({
      data: {},
      error: undefined,
    } as never)

    await userFeatureAvailabilityQueryOptions.queryFn?.({} as never)

    expect(getUserFeatureAvailability).toHaveBeenCalledOnce()
    expect(getFeatureAvailability).not.toHaveBeenCalled()
  })
})

describe('subscriptionHistoryQueryOptions', () => {
  it('should create correct query key for subscription history', () => {
    const options = subscriptionHistoryQueryOptions('realm-1', 'sub-1')

    expect(options.queryKey).toEqual([QUERY_KEYS.SUBSCRIPTION_HISTORY, 'realm-1', 'sub-1'])
  })

  it('should load protected subscription history through the generated SDK', async () => {
    vi.mocked(getSubscriptionHistoryApi).mockResolvedValue({
      data: { subscriptionId: 'sub-1', events: [], total: 0 },
      error: undefined,
    } as never)

    await getSubscriptionHistory('realm-1', 'sub-1')

    expect(getSubscriptionHistoryApi).toHaveBeenCalledWith({
      path: { subscriptionId: 'sub-1' },
    })
  })
})

describe('globalSubscriptionHistoryQueryOptions', () => {
  it('should create correct query key for global history', () => {
    const filters: HistoryFilters = {
      eventType: 'upgraded',
      userId: 'user-1',
    }

    const options = globalSubscriptionHistoryQueryOptions('realm-1', filters, 1, 20)

    expect(options.queryKey).toEqual([
      QUERY_KEYS.GLOBAL_SUBSCRIPTION_HISTORY,
      'realm-1',
      filters,
      1,
      20,
    ])
  })
})

describe('Filter Parameter Handling', () => {
  it('should use the generated SDK with camelCase backend query params', async () => {
    vi.mocked(listSubscriptionHistory).mockResolvedValue({
      data: {
        events: [],
        pagination: { page: 1, pageSize: 50, totalCount: 0, totalPages: 0 },
      },
      error: undefined,
    } as never)

    await getGlobalSubscriptionHistory(
      'realm-1',
      {
        userId: 'user-1',
        entitlementKey: 'basic',
        eventType: 'upgraded',
        subscriptionStatus: 'active',
        fromDate: '2025-01-01T00:00:00.000Z',
        toDate: '2025-01-31T23:59:59.999Z',
        sortBy: 'timestamp',
        sortOrder: 'desc',
      },
      2,
      50
    )

    expect(listSubscriptionHistory).toHaveBeenCalledWith({
      query: {
        userId: 'user-1',
        entitlementKey: 'basic',
        eventType: 'upgraded',
        subscriptionStatus: 'active',
        fromDate: '2025-01-01T00:00:00.000Z',
        toDate: '2025-01-31T23:59:59.999Z',
        sortBy: 'timestamp',
        sortOrder: 'desc',
        page: 2,
        pageSize: 50,
      },
    })
  })
})
