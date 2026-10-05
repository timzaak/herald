import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { PageHeader } from '@/components/shared/page-header'
import { AccessDenied } from '@/components/shared/access-denied'
import { usePermission } from '@/hooks/use-permission'
import { PERMISSION } from '@/lib/constants/auth-constants'
import type { StatisticsWindow } from '@/data/query-options'
import { PaymentStatsPanel } from './payment-stats-panel'
import { PointsConsumptionPanel } from './points-consumption-panel'
import { m } from '@/paraglide/messages'

export type StatisticsSection = 'payment' | 'points'

export interface StatisticsSearch {
  tab?: StatisticsSection
  days?: StatisticsWindow
}

interface BillingStatisticsPageProps {
  realmId: string
  // The URL is the single source of truth for the section tab and the stats
  // window: controls bind to `search` and every edit is a patch the route
  // navigates with, so a refresh (or a shared deep link) restores the exact
  // view the visitor had selected.
  search: StatisticsSearch
  onSearchChange: (patch: Partial<StatisticsSearch>) => void
}

export function BillingStatisticsPage({
  realmId,
  search,
  onSearchChange,
}: BillingStatisticsPageProps) {
  const { hasPermission } = usePermission()
  const hasPayment = hasPermission(PERMISSION.BILLING_VIEW)
  const hasPoints = hasPermission(PERMISSION.POINTS_VIEW)

  // The URL values are advisory; the permission set decides which section can
  // actually render, so a missing/forbidden tab or window falls back to a
  // permitted value instead of mounting a panel that would only 403.
  const tab: StatisticsSection =
    search.tab === 'points' && hasPoints ? 'points' : hasPayment ? 'payment' : 'points'
  const days: StatisticsWindow = search.days === 30 ? 30 : 7

  return (
    <div className="container mx-auto py-6 space-y-6">
      <PageHeader title={m['billing.statistics_title']()} headingTestId="statistics-heading" />

      {!hasPayment && !hasPoints ? (
        <div data-testid="statistics-no-permission">
          <AccessDenied message={m['billing.statistics_no_permission']()} />
        </div>
      ) : (
        <Tabs
          value={tab}
          onValueChange={(value) => onSearchChange({ tab: value as StatisticsSection })}
        >
          <div className="flex flex-wrap items-center justify-between gap-2">
            <TabsList data-testid="statistics-section-tabs">
              {hasPayment && (
                <TabsTrigger value="payment" data-testid="statistics-tab-payment">
                  {m['billing.statistics_payment_title']()}
                </TabsTrigger>
              )}
              {hasPoints && (
                <TabsTrigger value="points" data-testid="statistics-tab-points">
                  {m['billing.statistics_points_title']()}
                </TabsTrigger>
              )}
            </TabsList>

            <Tabs
              value={String(days)}
              onValueChange={(value) => onSearchChange({ days: value === '30' ? 30 : 7 })}
            >
              <TabsList data-testid="statistics-window-tabs">
                <TabsTrigger value="7" data-testid="statistics-window-7-trigger">
                  {m['billing.statistics_window_7']()}
                </TabsTrigger>
                <TabsTrigger value="30" data-testid="statistics-window-30-trigger">
                  {m['billing.statistics_window_30']()}
                </TabsTrigger>
              </TabsList>
            </Tabs>
          </div>

          {/* A panel mounts only while its tab is active — an unmounted
              panel never issues its query. The `tab` derivation above keeps
              the active tab inside the permission set. */}
          <TabsContent value="payment" className="mt-4">
            <PaymentStatsPanel realmId={realmId} days={days} />
          </TabsContent>
          <TabsContent value="points" className="mt-4">
            <PointsConsumptionPanel realmId={realmId} days={days} />
          </TabsContent>
        </Tabs>
      )}
    </div>
  )
}
