import { useQuery } from '@tanstack/react-query'
import { CheckCircle2, XCircle, Percent } from 'lucide-react'
import { paymentStatsQueryOptions, type StatisticsWindow } from '@/data/query-options'
import { StatsCard } from '@/components/dashboard/stats-card'
import { PaymentTrendChart } from './payment-trend-chart'
import { StatsErrorState } from './stats-error-state'
import { formatProviderName } from '@/components/billing/format-provider-name'
import { formatInvoiceAmount } from '@/lib/invoice-utils'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
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

interface PaymentStatsPanelProps {
  realmId: string
  days: StatisticsWindow
}

export function PaymentStatsPanel({ realmId, days }: PaymentStatsPanelProps) {
  const { data, isLoading, isError, error, refetch } = useQuery(
    paymentStatsQueryOptions(realmId, days)
  )

  const succeededCount = data?.succeededCount ?? 0
  const failedCount = data?.failedCount ?? 0
  const completedCount = succeededCount + failedCount
  // Derived on the client per contract — the backend deliberately returns only
  // the raw counts so there is no second source of truth for the ratio.
  const successRate =
    completedCount === 0 ? '—' : `${Math.round((succeededCount / completedCount) * 100)}%`
  // The backend zero-fills the trend; an all-zero window must read as "no
  // data" instead of a flat zero line.
  const hasAnyPayments = completedCount > 0
  const effectiveDays = data?.windowDays ?? days

  return (
    <section
      className="space-y-4"
      data-testid="payment-stats-panel"
      aria-labelledby="payment-stats-title"
    >
      <div>
        <h2 id="payment-stats-title" className="text-lg font-semibold tracking-tight">
          {m['billing.statistics_payment_title']()}
        </h2>
        <p className="text-xs text-muted-foreground">
          {m['billing.statistics_window_days']({ days: effectiveDays })}
        </p>
      </div>

      {isError ? (
        <StatsErrorState error={error} onRetry={() => refetch()} testId="payment-stats-error" />
      ) : isLoading ? (
        <>
          <div className="grid gap-4 md:grid-cols-3">
            <Skeleton className="h-[120px] rounded-xl" />
            <Skeleton className="h-[120px] rounded-xl" />
            <Skeleton className="h-[120px] rounded-xl" />
          </div>
          <Skeleton className="h-[100px] rounded-xl" />
          <Skeleton className="h-[200px] rounded-xl" />
          <Skeleton className="h-[350px] rounded-xl" />
        </>
      ) : data ? (
        <>
          <div className="grid gap-4 md:grid-cols-3">
            <StatsCard
              title={m['billing.statistics_succeeded_count']()}
              value={succeededCount}
              description={m['billing.statistics_window_days']({ days: effectiveDays })}
              icon={CheckCircle2}
              testId="payment-success-count-card"
            />
            <StatsCard
              title={m['billing.statistics_failed_count']()}
              value={failedCount}
              description={m['billing.statistics_window_days']({ days: effectiveDays })}
              icon={XCircle}
              testId="payment-failed-count-card"
            />
            <StatsCard
              title={m['billing.statistics_success_rate']()}
              value={successRate}
              description={m['billing.statistics_window_days']({ days: effectiveDays })}
              icon={Percent}
              testId="payment-success-rate-card"
            />
          </div>

          <Card data-testid="payment-amount-by-currency-list">
            <CardHeader>
              <CardTitle>{m['billing.statistics_amount_by_currency']()}</CardTitle>
              <CardDescription>{m['billing.statistics_amount_by_currency_hint']()}</CardDescription>
            </CardHeader>
            <CardContent>
              {!hasAnyPayments || data.amountsByCurrency.length === 0 ? (
                <p className="py-6 text-center text-sm text-muted-foreground">
                  {m['common.no_data']()}
                </p>
              ) : (
                <ul className="divide-y">
                  {data.amountsByCurrency.map((entry) => (
                    <li key={entry.currency} className="flex items-center justify-between py-2">
                      <span className="text-sm font-medium">{entry.currency}</span>
                      <span className="text-sm font-semibold tabular-nums">
                        {formatInvoiceAmount(entry.amount, entry.currency)}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </CardContent>
          </Card>

          <Card data-testid="payment-provider-table">
            <CardHeader>
              <CardTitle>{m['billing.statistics_by_provider']()}</CardTitle>
            </CardHeader>
            <CardContent>
              {!hasAnyPayments || data.providers.length === 0 ? (
                <p className="py-6 text-center text-sm text-muted-foreground">
                  {m['common.no_data']()}
                </p>
              ) : (
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>{m['billing.statistics_provider']()}</TableHead>
                      <TableHead className="text-right">
                        {m['billing.statistics_succeeded']()}
                      </TableHead>
                      <TableHead className="text-right">
                        {m['billing.statistics_failed']()}
                      </TableHead>
                      <TableHead className="text-right">
                        {m['billing.statistics_amount']()}
                      </TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {data.providers.map((provider) => (
                      <TableRow key={provider.paymentProvider}>
                        <TableCell className="font-medium">
                          {formatProviderName(provider.paymentProvider)}
                        </TableCell>
                        <TableCell className="text-right tabular-nums">
                          {provider.succeededCount}
                        </TableCell>
                        <TableCell className="text-right tabular-nums">
                          {provider.failedCount}
                        </TableCell>
                        <TableCell className="text-right">
                          {/* Amounts stay grouped per currency — never summed across currencies. */}
                          {provider.amountsByCurrency.length === 0 ? (
                            '—'
                          ) : (
                            <span className="flex flex-col items-end">
                              {provider.amountsByCurrency.map((entry) => (
                                <span key={entry.currency} className="tabular-nums">
                                  {formatInvoiceAmount(entry.amount, entry.currency)}
                                </span>
                              ))}
                            </span>
                          )}
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              )}
            </CardContent>
          </Card>

          <PaymentTrendChart
            data={hasAnyPayments ? data.paymentTrend : []}
            days={effectiveDays}
            testId="payment-trend-chart"
          />
        </>
      ) : null}
    </section>
  )
}
