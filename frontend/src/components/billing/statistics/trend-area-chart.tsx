import { Area, AreaChart, CartesianGrid, XAxis, YAxis } from 'recharts'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  ChartLegend,
  ChartLegendContent,
  type ChartConfig,
} from '@/components/ui/chart'
import { m } from '@/paraglide/messages'

export interface TrendSeries {
  dataKey: string
  label: string
  color: string
  dashed?: boolean
}

interface TrendAreaChartProps<TData extends object> {
  title: string
  description: string
  data: TData[]
  series: TrendSeries[]
  testId?: string
}

function formatShortDate(dateStr: string): string {
  const date = new Date(dateStr + 'T00:00:00')
  return date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' })
}

export function TrendAreaChart<TData extends object>({
  title,
  description,
  data,
  series,
  testId,
}: TrendAreaChartProps<TData>) {
  const chartConfig: ChartConfig = {}
  for (const { dataKey, label, color } of series) {
    chartConfig[dataKey] = { label, color }
  }

  return (
    <Card data-testid={testId}>
      <CardHeader>
        <CardTitle>{title}</CardTitle>
        <CardDescription>{description}</CardDescription>
      </CardHeader>
      <CardContent>
        {data.length === 0 ? (
          <div className="flex h-[250px] items-center justify-center text-muted-foreground">
            {m['common.no_data']()}
          </div>
        ) : (
          <ChartContainer config={chartConfig} className="h-[250px] w-full">
            <AreaChart data={data} margin={{ top: 10, right: 10, left: 0, bottom: 0 }}>
              <CartesianGrid vertical={false} />
              <XAxis
                dataKey="date"
                tickLine={false}
                axisLine={false}
                tickMargin={8}
                tickFormatter={formatShortDate}
              />
              <YAxis tickLine={false} axisLine={false} tickMargin={8} allowDecimals={false} />
              <ChartTooltip
                content={
                  <ChartTooltipContent
                    labelFormatter={(_, payload) => {
                      if (payload?.[0]?.payload?.date) {
                        return formatShortDate(payload[0].payload.date)
                      }
                      return ''
                    }}
                  />
                }
              />
              {series.length > 1 && <ChartLegend content={<ChartLegendContent />} />}
              {series.map((s) => (
                <Area
                  key={s.dataKey}
                  type="monotone"
                  dataKey={s.dataKey}
                  stroke={`var(--color-${s.dataKey})`}
                  fill={`var(--color-${s.dataKey})`}
                  fillOpacity={0.2}
                  strokeWidth={2}
                  strokeDasharray={s.dashed ? '5 5' : undefined}
                />
              ))}
            </AreaChart>
          </ChartContainer>
        )}
      </CardContent>
    </Card>
  )
}
