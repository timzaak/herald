import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { QueryClientProvider } from '@tanstack/react-query'
import { http, HttpResponse } from 'msw'
import { server } from '@/test/mocks/server'
import { createTestQueryClient } from '@/test/utils/render'
import { useAuthStore } from '@/stores/auth-store'
import { PERMISSION } from '@/lib/constants/auth-constants'
import { makePaymentStats, makePointsStats } from '@/test/fixtures/billing-stats'
import { BillingStatisticsPage, type StatisticsSearch } from '../billing-statistics-page'

const API_BASE_URL = 'http://localhost:3000'

const paymentStatsRequests: string[] = []
const pointsStatsRequests: string[] = []

function setPermissions(permissions: string[]) {
  useAuthStore.setState({ permissions })
}

function renderPage(search: StatisticsSearch = {}) {
  const onSearchChange = vi.fn()
  const queryClient = createTestQueryClient()
  render(
    <QueryClientProvider client={queryClient}>
      <BillingStatisticsPage realmId="realm-1" search={search} onSearchChange={onSearchChange} />
    </QueryClientProvider>
  )
  return { onSearchChange }
}

/**
 * The URL is the single source of truth for the statistics view: the page is
 * a controlled component that only renders what `search` says and only emits
 * patches — never local state. These tests pin that contract (refresh and
 * deep-link restore) plus the permission fallbacks that keep an
 * impermissible URL value from mounting a panel that would only 403.
 */
describe('billing statistics page URL-driven view', () => {
  beforeEach(() => {
    paymentStatsRequests.length = 0
    pointsStatsRequests.length = 0
    setPermissions([PERMISSION.BILLING_VIEW, PERMISSION.POINTS_VIEW])
    server.resetHandlers()
    server.use(
      http.get(`${API_BASE_URL}/api/bill/stats/payments`, ({ request }) => {
        paymentStatsRequests.push(request.url)
        return HttpResponse.json(makePaymentStats())
      }),
      http.get(`${API_BASE_URL}/api/points/stats/consumption`, ({ request }) => {
        pointsStatsRequests.push(request.url)
        return HttpResponse.json(makePointsStats())
      })
    )
  })

  it('defaults to the payment tab and a 7-day window, and leaves the inactive panel unmounted', async () => {
    renderPage()

    expect(screen.getByTestId('statistics-tab-payment')).toHaveAttribute('data-state', 'active')
    expect(screen.getByTestId('statistics-window-7-trigger')).toHaveAttribute(
      'data-state',
      'active'
    )

    await expect(screen.findByTestId('payment-stats-panel')).resolves.toBeVisible()
    expect(screen.queryByTestId('points-stats-panel')).not.toBeInTheDocument()

    // An unmounted tab never issues its query — the points endpoint must
    // stay silent until the visitor actually opens that tab.
    expect(paymentStatsRequests).toHaveLength(1)
    expect(pointsStatsRequests).toHaveLength(0)
  })

  it('restores the points tab and the 30-day window from the search props', async () => {
    renderPage({ tab: 'points', days: 30 })

    expect(screen.getByTestId('statistics-tab-points')).toHaveAttribute('data-state', 'active')
    expect(screen.getByTestId('statistics-window-30-trigger')).toHaveAttribute(
      'data-state',
      'active'
    )

    await expect(screen.findByTestId('points-stats-panel')).resolves.toBeVisible()
    expect(screen.queryByTestId('payment-stats-panel')).not.toBeInTheDocument()

    await vi.waitFor(() => {
      expect(pointsStatsRequests.some((url) => url.includes('days=30'))).toBe(true)
    })
    expect(paymentStatsRequests).toHaveLength(0)
  })

  it('emits a single-key patch per control edit instead of holding local state', async () => {
    const { onSearchChange } = renderPage({ tab: 'points' })
    const user = userEvent.setup()

    await user.click(screen.getByTestId('statistics-window-30-trigger'))
    expect(onSearchChange).toHaveBeenCalledWith({ days: 30 })

    await user.click(screen.getByTestId('statistics-tab-payment'))
    expect(onSearchChange).toHaveBeenCalledWith({ tab: 'payment' })

    // Each patch touches exactly the edited key — the route merges patches
    // into the current search, so switching windows must never reset the tab
    // or vice versa.
    for (const call of onSearchChange.mock.calls) {
      expect(Object.keys(call[0])).toHaveLength(1)
    }
  })

  it('falls back to the points tab when the URL asks for payment but the admin lacks billing.view', async () => {
    setPermissions([PERMISSION.POINTS_VIEW])
    renderPage({ tab: 'payment' })

    expect(screen.queryByTestId('statistics-tab-payment')).not.toBeInTheDocument()
    expect(screen.getByTestId('statistics-tab-points')).toHaveAttribute('data-state', 'active')

    await expect(screen.findByTestId('points-stats-panel')).resolves.toBeVisible()
    expect(screen.queryByTestId('payment-stats-panel')).not.toBeInTheDocument()
    expect(paymentStatsRequests).toHaveLength(0)
  })

  it('shows the no-permission state when neither stats permission is held', () => {
    setPermissions([])
    renderPage({ tab: 'payment' })

    expect(screen.getByTestId('statistics-no-permission')).toBeVisible()
    expect(screen.queryByTestId('payment-stats-panel')).not.toBeInTheDocument()
    expect(screen.queryByTestId('points-stats-panel')).not.toBeInTheDocument()
  })
})
