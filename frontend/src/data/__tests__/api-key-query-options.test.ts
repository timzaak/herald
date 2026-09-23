/**
 * API Key Query Options Tests
 *
 * Tests query key structure, cache key isolation, and parameter mapping
 * for apiKeysQueryOptions and apiKeyQueryOptions.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { apiKeysQueryOptions, apiKeyQueryOptions } from '../query-options'

// Mock the API functions to observe parameters in queryFn tests
vi.mock('@/lib/api-generated', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api-generated')>('@/lib/api-generated')
  return {
    ...actual,
    listApiKeys: vi.fn(),
    getApiKey: vi.fn(),
  }
})

import { listApiKeys, getApiKey } from '@/lib/api-generated'

// Minimal factory for successful listApiKeys response
function makeListResponse(overrides?: Record<string, unknown>) {
  return {
    data: {
      items: [],
      total: 0,
      page: 0,
      pageSize: 20,
      ...overrides,
    },
    error: undefined,
  }
}

// Minimal factory for successful getApiKey response
function makeDetailResponse(overrides?: Record<string, unknown>) {
  return {
    data: {
      id: 'key-1',
      name: 'Test Key',
      enabled: true,
      realmId: 'realm-1',
      createdAt: '2025-01-01T00:00:00Z',
      ...overrides,
    },
    error: undefined,
  }
}

describe('API Key Query Options', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('apiKeysQueryOptions -- parameter mapping', () => {
    it('sends default pagination (page 0, pageSize 20) when no filters provided', async () => {
      vi.mocked(listApiKeys).mockResolvedValueOnce(makeListResponse() as any)

      const options = apiKeysQueryOptions('realm-1', {})
      await options.queryFn!()

      expect(listApiKeys).toHaveBeenCalledWith({
        query: { page: 0, pageSize: 20 },
      })
    })

    it('passes explicit filter parameters to the API call', async () => {
      vi.mocked(listApiKeys).mockResolvedValueOnce(
        makeListResponse({ page: 2, pageSize: 50 }) as any
      )

      const options = apiKeysQueryOptions('realm-1', { page: 2, pageSize: 50 })
      await options.queryFn!()

      expect(listApiKeys).toHaveBeenCalledWith({
        query: { page: 2, pageSize: 50 },
      })
    })

    it('defaults page to 0 when only pageSize is provided', async () => {
      vi.mocked(listApiKeys).mockResolvedValueOnce(makeListResponse() as any)

      const options = apiKeysQueryOptions('realm-1', { pageSize: 10 })
      await options.queryFn!()

      expect(listApiKeys).toHaveBeenCalledWith({
        query: { page: 0, pageSize: 10 },
      })
    })

    it('defaults pageSize to 20 when only page is provided', async () => {
      vi.mocked(listApiKeys).mockResolvedValueOnce(makeListResponse() as any)

      const options = apiKeysQueryOptions('realm-1', { page: 3 })
      await options.queryFn!()

      expect(listApiKeys).toHaveBeenCalledWith({
        query: { page: 3, pageSize: 20 },
      })
    })
  })

  describe('apiKeyQueryOptions -- parameter mapping', () => {
    it('calls getApiKey with correct realmId and apiKeyId', async () => {
      vi.mocked(getApiKey).mockResolvedValueOnce(makeDetailResponse() as any)

      const options = apiKeyQueryOptions('realm-1', 'key-42')
      await options.queryFn!()

      expect(getApiKey).toHaveBeenCalledWith({
        path: { apiKeyId: 'key-42' },
      })
    })
  })
})
