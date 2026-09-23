import { http, HttpResponse } from 'msw'
import type { DashboardStatsResponse } from '@/lib/api-generated'

const API_BASE_URL = 'http://localhost:3000'

export function makeDashboardStats(
  overrides?: Partial<DashboardStatsResponse>
): DashboardStatsResponse {
  return {
    userStats: {
      totalUsers: 100,
      newUsers: 5,
      activeUsers: 20,
    },
    authTrend: [
      { date: '2026-05-01', successCount: 42, failureCount: 3 },
      { date: '2026-05-02', successCount: 38, failureCount: 1 },
    ],
    ...overrides,
  }
}

export const getDashboardStatsHandler = http.get(
  `${API_BASE_URL}/api/dashboard/stats`,
  ({ params }) => {
    const { realmId } = params
    if (realmId === 'forbidden-realm') {
      return HttpResponse.json({ message: 'Forbidden' }, { status: 403 })
    }
    if (realmId === 'not-found-realm') {
      return HttpResponse.json({ message: 'Realm not found' }, { status: 404 })
    }
    return HttpResponse.json(makeDashboardStats())
  }
)

export const dashboardHandlers = [getDashboardStatsHandler]
