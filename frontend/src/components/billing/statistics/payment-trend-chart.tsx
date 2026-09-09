import { m } from '@/paraglide/messages'
import { TrendAreaChart } from './trend-area-chart'

interface PaymentTrendChartProps {
  data: Array<{
    date: string
    succeededCount: number
    failedCount: number
  }>
  days: number
  testId?: string
}

export function PaymentTrendChart({ data, days, testId }: PaymentTrendChartProps) {
  return (
    <TrendAreaChart
      title={m['billing.statistics_payment_trend']()}
      description={m['billing.statistics_window_days']({ days })}
      data={data}
      testId={testId}
      series={[
        {
          dataKey: 'succeededCount',
          label: m['billing.statistics_succeeded'](),
          color: 'var(--chart-1)',
        },
        {
          dataKey: 'failedCount',
          label: m['billing.statistics_failed'](),
          color: 'var(--destructive)',
          dashed: true,
        },
      ]}
    />
  )
}
