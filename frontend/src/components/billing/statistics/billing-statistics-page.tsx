import { useState } from 'react'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { PageHeader } from '@/components/shared/page-header'
import { AccessDenied } from '@/components/shared/access-denied'
import { usePermission } from '@/hooks/use-permission'
import { PERMISSION } from '@/lib/constants/auth-constants'
import type { StatisticsWindow } from '@/data/query-options'
import { PaymentStatsPanel } from './payment-stats-panel'
import { PointsConsumptionPanel } from './points-consumption-panel'
import { m } from '@/paraglide/messages'

interface BillingStatisticsPageProps {
  realmId: string
}

export function BillingStatisticsPage({ realmId }: BillingStatisticsPageProps) {
  const [days, setDays] = useState<StatisticsWindow>(7)
  const { hasPermission } = usePermission()
  const hasPayment = hasPermission(PERMISSION.BILLING_VIEW)
  const hasPoints = hasPermission(PERMISSION.POINTS_VIEW)

  return (
    <div className="container mx-auto py-6 space-y-6">
      <PageHeader title={m['billing.statistics_title']()} headingTestId="statistics-heading" />

      {!hasPayment && !hasPoints ? (
        <div data-testid="statistics-no-permission">
          <AccessDenied message={m['billing.statistics_no_permission']()} />
        </div>
      ) : (
        <>
          <Tabs value={String(days)} onValueChange={(value) => setDays(value === '30' ? 30 : 7)}>
            <TabsList data-testid="statistics-window-tabs">
              <TabsTrigger value="7" data-testid="statistics-window-7-trigger">
                {m['billing.statistics_window_7']()}
              </TabsTrigger>
              <TabsTrigger value="30" data-testid="statistics-window-30-trigger">
                {m['billing.statistics_window_30']()}
              </TabsTrigger>
            </TabsList>
          </Tabs>

          {/* Panels mount only with their permission — an unmounted panel
              never issues its query. */}
          {hasPayment && <PaymentStatsPanel realmId={realmId} days={days} />}
          {hasPoints && <PointsConsumptionPanel realmId={realmId} days={days} />}
        </>
      )}
    </div>
  )
}
