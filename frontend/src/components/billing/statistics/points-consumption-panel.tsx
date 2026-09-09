import { useQuery } from '@tanstack/react-query'
import { Coins, Users } from 'lucide-react'
import { pointsConsumptionStatsQueryOptions, type StatisticsWindow } from '@/data/query-options'
import { StatsCard } from '@/components/dashboard/stats-card'
import { ConsumptionTrendChart } from './consumption-trend-chart'
import { StatsErrorState } from './stats-error-state'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Skeleton } from '@/components/ui/skeleton'
import { m } from '@/paraglide/messages'

interface PointsConsumptionPanelProps {
  realmId: string
  days: StatisticsWindow
}

export function PointsConsumptionPanel({ realmId, days }: PointsConsumptionPanelProps) {
  const { data, isLoading, isError, error, refetch } = useQuery(
    pointsConsumptionStatsQueryOptions(realmId, days)
  )

  const totalConsumedPoints = data?.totalConsumedPoints ?? 0
  // Revoked/refunded points never offset these figures — the backend counts
  // consume transactions only, so zero here means no consumption in-window.
  const hasConsumption = totalConsumedPoints > 0
  const effectiveDays = data?.windowDays ?? days

  return (
    <section
      className="space-y-4"
      data-testid="points-stats-panel"
      aria-labelledby="points-stats-title"
    >
      <div>
        <h2 id="points-stats-title" className="text-lg font-semibold tracking-tight">
          {m['billing.statistics_points_title']()}
        </h2>
        <p className="text-xs text-muted-foreground">
          {m['billing.statistics_window_days']({ days: effectiveDays })}
        </p>
      </div>

      {isError ? (
        <StatsErrorState error={error} onRetry={() => refetch()} testId="points-stats-error" />
      ) : isLoading ? (
        <>
          <div className="grid gap-4 md:grid-cols-2">
            <Skeleton className="h-[120px] rounded-xl" />
            <Skeleton className="h-[120px] rounded-xl" />
          </div>
          <Skeleton className="h-[200px] rounded-xl" />
          <Skeleton className="h-[350px] rounded-xl" />
        </>
      ) : data ? (
        <>
          <div className="grid gap-4 md:grid-cols-2">
            <StatsCard
              title={m['billing.statistics_total_consumed']()}
              value={totalConsumedPoints.toLocaleString()}
              description={m['billing.statistics_window_days']({ days: effectiveDays })}
              icon={Coins}
              testId="points-total-consumed-card"
            />
            <StatsCard
              title={m['billing.statistics_consuming_users']()}
              value={data?.consumingUsers ?? 0}
              description={m['billing.statistics_window_days']({ days: effectiveDays })}
              icon={Users}
              testId="points-consuming-users-card"
            />
          </div>

          <Card data-testid="points-bucket-table">
            <CardHeader>
              <CardTitle>{m['billing.statistics_by_bucket']()}</CardTitle>
            </CardHeader>
            <CardContent>
              {!hasConsumption || data.buckets.length === 0 ? (
                <p className="py-6 text-center text-sm text-muted-foreground">
                  {m['common.no_data']()}
                </p>
              ) : (
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>{m['billing.statistics_bucket']()}</TableHead>
                      <TableHead className="text-right">
                        {m['billing.statistics_consumed']()}
                      </TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {data.buckets.map((bucket) => (
                      <TableRow key={bucket.bucketId}>
                        <TableCell>
                          <span className="font-medium">{bucket.bucketName}</span>
                          <span className="ml-2 text-xs text-muted-foreground">
                            {bucket.bucketKey}
                          </span>
                        </TableCell>
                        <TableCell className="text-right tabular-nums">
                          {bucket.consumedPoints.toLocaleString()}
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              )}
            </CardContent>
          </Card>

          <ConsumptionTrendChart
            data={hasConsumption ? data.consumptionTrend : []}
            days={effectiveDays}
            testId="points-trend-chart"
          />
        </>
      ) : null}
    </section>
  )
}
