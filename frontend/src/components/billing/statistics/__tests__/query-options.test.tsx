import { describe, it, expect, beforeEach } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { QueryClient, QueryClientProvider, useQuery } from '@tanstack/react-query'
import { server } from '@/test/mocks/server'
import {
  paymentStatsQueryOptions,
  pointsConsumptionStatsQueryOptions,
  queryKeys,
  type StatisticsWindow,
} from '@/data/query-options'
import { PaymentStatsPanel } from '@/components/billing/statistics/payment-stats-panel'
import { PointsConsumptionPanel } from '@/components/billing/statistics/points-consumption-panel'
import { QUERY_KEYS } from '@/lib/constants'
import type { PaymentStatsResponse, PointsConsumptionStatsResponse } from '@/lib/api-generated'

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

function makePaymentStats(overrides: Partial<PaymentStatsResponse> = {}): PaymentStatsResponse {
  return {
    windowDays: 7,
    succeededCount: 0,
    failedCount: 0,
    amountsByCurrency: [],
    providers: [],
    // Empty realms still receive a fully zero-filled series from the backend.
    paymentTrend: Array.from({ length: 7 }, (_, i) => ({
      date: `2026-09-0${i + 1}`,
      succeededCount: 0,
      failedCount: 0,
    })),
    ...overrides,
  }
}

function makePointsStats(
  overrides: Partial<PointsConsumptionStatsResponse> = {}
): PointsConsumptionStatsResponse {
  return {
    windowDays: 7,
    totalConsumedPoints: 0,
    consumingUsers: 0,
    buckets: [],
    consumptionTrend: Array.from({ length: 7 }, (_, i) => ({
      date: `2026-09-0${i + 1}`,
      consumedPoints: 0,
    })),
    ...overrides,
  }
}

// Minimal observer exercising the query options through React Query
function StatsObserver({
  realmId,
  days,
  testId,
}: {
  realmId: string
  days: StatisticsWindow
  testId: string
}) {
  const payment = useQuery({ ...paymentStatsQueryOptions(realmId, days), retry: false })
  const points = useQuery({ ...pointsConsumptionStatsQueryOptions(realmId, days), retry: false })

  if (payment.isLoading || points.isLoading) {
    return <div data-testid={`${testId}-loading`}>Loading...</div>
  }
  if (payment.error) return <div data-testid={`${testId}-error`}>{payment.error.message}</div>
  if (points.error) return <div data-testid={`${testId}-error`}>{points.error.message}</div>
  return (
    <div data-testid={testId}>
      {payment.data?.succeededCount}/{payment.data?.failedCount}·{points.data?.totalConsumedPoints}
    </div>
  )
}

describe('billing statistics query options', () => {
  beforeEach(() => {
    server.resetHandlers()
  })

  // ==================== Endpoint + days passthrough ====================

  it('hits both stats endpoints and passes the selected window as days', async () => {
    const capturedQueries: string[] = []
    server.use(
      http.get(`${API_BASE_URL}/api/bill/:realmId/stats/payments`, ({ request }) => {
        capturedQueries.push(`payments${new URL(request.url).search}`)
        return HttpResponse.json(makePaymentStats({ succeededCount: 12, failedCount: 3 }))
      }),
      http.get(`${API_BASE_URL}/api/points/:realmId/stats/consumption`, ({ request }) => {
        capturedQueries.push(`points${new URL(request.url).search}`)
        return HttpResponse.json(makePointsStats({ totalConsumedPoints: 25000 }))
      })
    )

    renderWithQueryClient(<StatsObserver realmId="realm-1" days={7} testId="stats" />)

    expect(await screen.findByTestId('stats')).toHaveTextContent('12/3·25000')
    expect(capturedQueries).toEqual(expect.arrayContaining(['payments?days=7', 'points?days=7']))
  })

  it('enters the error state when the stats API rejects', async () => {
    server.use(
      http.get(`${API_BASE_URL}/api/bill/:realmId/stats/payments`, () => {
        return HttpResponse.json({ message: 'Forbidden' }, { status: 403 })
      })
    )

    renderWithQueryClient(<StatsObserver realmId="forbidden-realm" days={7} testId="stats" />)

    expect(await screen.findByTestId('stats-error', undefined, { timeout: 5000 })).toBeVisible()
  })

  // ==================== Window key isolation ====================

  it('keeps the 7-day and 30-day windows in separate query cache entries', async () => {
    // Window switching is a query-key change; each window must resolve to its
    // own data, never a cached hit from the other window.
    server.use(
      http.get(`${API_BASE_URL}/api/bill/:realmId/stats/payments`, ({ request }) => {
        const days = new URL(request.url).searchParams.get('days')
        return HttpResponse.json(
          makePaymentStats({
            windowDays: days === '30' ? 30 : 7,
            succeededCount: days === '30' ? 20 : 5,
            failedCount: days === '30' ? 4 : 1,
          })
        )
      }),
      http.get(`${API_BASE_URL}/api/points/:realmId/stats/consumption`, () => {
        return HttpResponse.json(makePointsStats())
      })
    )

    const queryClient = createTestQueryClient()
    render(
      <QueryClientProvider client={queryClient}>
        <StatsObserver realmId="realm-1" days={7} testId="stats-7" />
        <StatsObserver realmId="realm-1" days={30} testId="stats-30" />
      </QueryClientProvider>
    )

    expect(await screen.findByTestId('stats-7')).toHaveTextContent('5/1')
    expect(await screen.findByTestId('stats-30')).toHaveTextContent('20/4')
  })

  it('derives query keys from the shared key constants and factory', () => {
    expect(paymentStatsQueryOptions('realm-1', 7).queryKey).toEqual(
      queryKeys.paymentStats('realm-1', 7)
    )
    expect(paymentStatsQueryOptions('realm-1', 7).queryKey).toEqual([
      QUERY_KEYS.PAYMENT_STATS,
      'realm-1',
      7,
    ])
    expect(pointsConsumptionStatsQueryOptions('realm-1', 30).queryKey).toEqual([
      QUERY_KEYS.POINTS_CONSUMPTION_STATS,
      'realm-1',
      30,
    ])
    expect(paymentStatsQueryOptions('realm-1', 7).queryKey).not.toEqual(
      paymentStatsQueryOptions('realm-1', 30).queryKey
    )
  })
})

describe('payment statistics panel all-zero window', () => {
  beforeEach(() => {
    server.resetHandlers()
    server.use(
      http.get(`${API_BASE_URL}/api/bill/:realmId/stats/payments`, () => {
        return HttpResponse.json(makePaymentStats())
      })
    )
  })

  it('shows zero counters, "—" success rate and "no data" placeholders instead of a flat zero line', async () => {
    renderWithQueryClient(<PaymentStatsPanel realmId="empty-realm" days={7} />)

    const rateCard = await screen.findByTestId('payment-success-rate-card')
    expect(within(rateCard).getByText('—')).toBeVisible()
    expect(within(screen.getByTestId('payment-success-count-card')).getByText('0')).toBeVisible()
    expect(within(screen.getByTestId('payment-failed-count-card')).getByText('0')).toBeVisible()

    // An all-zero window must read as "no data" — not a zero-filled chart.
    expect(within(screen.getByTestId('payment-trend-chart')).getByText('No data')).toBeVisible()
    expect(
      within(screen.getByTestId('payment-amount-by-currency-list')).getByText('No data')
    ).toBeVisible()
    expect(within(screen.getByTestId('payment-provider-table')).getByText('No data')).toBeVisible()
  })
})

describe('payment statistics panel with completed attempts', () => {
  beforeEach(() => {
    server.resetHandlers()
  })

  it('derives the success rate from succeeded/(succeeded+failed)', async () => {
    server.use(
      http.get(`${API_BASE_URL}/api/bill/:realmId/stats/payments`, () => {
        return HttpResponse.json(makePaymentStats({ succeededCount: 12, failedCount: 3 }))
      })
    )

    renderWithQueryClient(<PaymentStatsPanel realmId="realm-1" days={7} />)

    const rateCard = await screen.findByTestId('payment-success-rate-card')
    expect(within(rateCard).getByText('80%')).toBeVisible()
  })
})

describe('points consumption panel all-zero window', () => {
  beforeEach(() => {
    server.resetHandlers()
    server.use(
      http.get(`${API_BASE_URL}/api/points/:realmId/stats/consumption`, () => {
        return HttpResponse.json(makePointsStats())
      })
    )
  })

  it('shows zero totals and "no data" placeholders without erroring', async () => {
    renderWithQueryClient(<PointsConsumptionPanel realmId="empty-realm" days={7} />)

    expect(
      within(await screen.findByTestId('points-total-consumed-card')).getByText('0')
    ).toBeVisible()
    expect(within(screen.getByTestId('points-consuming-users-card')).getByText('0')).toBeVisible()
    expect(within(screen.getByTestId('points-bucket-table')).getByText('No data')).toBeVisible()
    expect(within(screen.getByTestId('points-trend-chart')).getByText('No data')).toBeVisible()
  })
})
