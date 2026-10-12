import { createFileRoute } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { ArrowLeft, Info } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { SubscriptionTimeline } from '@/components/billing/subscription-timeline'
import { queryKeys, subscriptionHistoryQueryOptions } from '@/data/query-options'
import { toast } from 'sonner'
import { m } from '@/paraglide/messages'
import { formatDateTimeShort } from '@/lib/date-utils'

export const Route = createFileRoute('/$realmId/user/my-subscriptions/$subscriptionId/history')({
  component: SubscriptionDetailHistoryRoute,
})

function SubscriptionDetailHistoryRoute() {
  const { realmId, subscriptionId } = Route.useParams()

  // Query subscription history
  const {
    data: historyData,
    isLoading: historyLoading,
    error: historyError,
  } = useQuery(subscriptionHistoryQueryOptions(realmId, subscriptionId))

  // Query subscription details
  const { data: subscription, isLoading: subscriptionLoading } = useQuery({
    queryKey: queryKeys.subscriptionDetails(realmId, subscriptionId),
    queryFn: async () => {
      // For now, we'll use the history data to get subscription info
      // In the future, this could call a dedicated subscription detail endpoint
      return historyData?.events?.[0]?.newState
    },
    enabled: !!historyData?.events?.length,
  })

  if (historyError) {
    return (
      <div className="container mx-auto px-4 py-8">
        <Card className="border-destructive">
          <CardContent className="p-6">
            <p className="text-destructive">
              {m['billing.subscription_history_failed_load']({
                error: historyError instanceof Error ? historyError.message : 'Unknown error',
              })}
            </p>
            <Button variant="outline" className="mt-4" onClick={() => window.location.reload()}>
              {m['common.retry']()}
            </Button>
          </CardContent>
        </Card>
      </div>
    )
  }

  return (
    <div
      className="container mx-auto space-y-6 px-4 py-8"
      data-testid="subscription-detail-history-page"
    >
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-4">
          <Button
            variant="ghost"
            size="icon"
            data-testid="back-button"
            onClick={() => window.history.back()}
          >
            <ArrowLeft className="h-5 w-5" />
            <span className="sr-only">{m['common.back']()}</span>
          </Button>
          <h1 className="text-xl font-semibold">
            {m['billing.subscription_detail_history_title']()}
          </h1>
        </div>
      </div>

      {/* Subscription Info */}
      {!subscriptionLoading && subscription && (
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Info className="h-5 w-5" />
              {m['billing.subscription_current_status']()}
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
              <div>
                <p className="text-sm font-medium text-muted-foreground">
                  {m['billing.subscription_status']()}
                </p>
                <p className="text-lg font-semibold">{subscription.status}</p>
              </div>
              {subscription.entitlementKey && (
                <div>
                  <p className="text-sm font-medium text-muted-foreground">
                    {m['billing.subscription_plan']()}
                  </p>
                  <p className="text-lg font-semibold">{subscription.entitlementKey}</p>
                </div>
              )}
            </div>
          </CardContent>
        </Card>
      )}

      {/* Timeline */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Info className="h-5 w-5" />
            {m['billing.subscription_change_timeline']()}
          </CardTitle>
        </CardHeader>
        <CardContent>
          <SubscriptionTimeline
            events={historyData?.events || []}
            loading={historyLoading}
            onEventClick={(event) => {
              toast.info(`Event: ${event.eventType}`, {
                description: `Timestamp: ${formatDateTimeShort(event.timestamp)}`,
              })
            }}
          />
        </CardContent>
      </Card>
    </div>
  )
}
