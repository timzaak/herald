import { describe, it, expect, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { QueryClient, QueryClientProvider, useQuery } from '@tanstack/react-query'
import { server } from '@/test/mocks/server'
import { dashboardStatsQueryOptions, queryKeys } from '@/data/query-options'
import { QUERY_KEYS } from '@/lib/constants'

const API_BASE_URL = 'http://localhost:3000'

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  })
}

function renderWithQueryClient(ui: React.ReactNode) {
  const queryClient = createTestQueryClient()
  return render(<QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>)
}

// Minimal component to exercise query options through React Query
function DashboardStatsTestComponent({ realmId }: { realmId: string }) {
  const { data, isLoading, error } = useQuery({
    ...dashboardStatsQueryOptions(realmId),
    retry: false,
  })

  if (isLoading) return <div data-testid="loading">Loading...</div>
  if (error) return <div data-testid="dashboard-error">{error.message}</div>
  if (data) return <div data-testid="dashboard-data">{data.userStats.totalUsers} users</div>
  return null
}

describe('dashboardStatsQueryOptions', () => {
  // ==================== Query Key Isolation ====================

  describe('query key isolation', () => {
    it('should produce a query key containing the dashboard key constant', () => {
      const options = dashboardStatsQueryOptions('realm-1')
      expect(options.queryKey).toEqual([QUERY_KEYS.DASHBOARD_STATS, 'realm-1'])
    })

    it('should produce different query keys for different realmIds', () => {
      const options1 = dashboardStatsQueryOptions('realm-1')
      const options2 = dashboardStatsQueryOptions('realm-2')

      expect(options1.queryKey).not.toEqual(options2.queryKey)
    })

    it('should use queryKeys.dashboardStats helper consistently', () => {
      const realmId = 'test-realm'
      const options = dashboardStatsQueryOptions(realmId)

      expect(options.queryKey).toEqual(queryKeys.dashboardStats(realmId))
    })
  })

  // ==================== Error State Tests ====================

  describe('error states', () => {
    beforeEach(() => {
      server.resetHandlers()
    })

    it('should enter error state on 403 Forbidden', async () => {
      server.use(
        http.get(`${API_BASE_URL}/api/dashboard/:realmId/stats`, () => {
          return HttpResponse.json({ message: 'Forbidden' }, { status: 403 })
        })
      )

      renderWithQueryClient(<DashboardStatsTestComponent realmId="forbidden-realm" />)

      expect(screen.getByTestId('loading')).toBeInTheDocument()

      const errorElement = await screen.findByTestId('dashboard-error', undefined, {
        timeout: 5000,
      })
      expect(errorElement).toBeInTheDocument()
    })
    // 500/network-error variants hit the identical throw→error-state branch
    // (retry disabled, one error testid), so one representative pins it.
  })
})
