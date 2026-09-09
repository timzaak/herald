import { m } from '@/paraglide/messages'
import { TrendAreaChart } from './trend-area-chart'

interface ConsumptionTrendChartProps {
  data: Array<{
    date: string
    consumedPoints: number
  }>
  days: number
  testId?: string
}

export function ConsumptionTrendChart({ data, days, testId }: ConsumptionTrendChartProps) {
  return (
    <TrendAreaChart
      title={m['billing.statistics_consumption_trend']()}
      description={m['billing.statistics_window_days']({ days })}
      data={data}
      testId={testId}
      series={[
        {
          dataKey: 'consumedPoints',
          label: m['billing.statistics_consumed'](),
          color: 'var(--chart-1)',
        },
      ]}
    />
  )
}
