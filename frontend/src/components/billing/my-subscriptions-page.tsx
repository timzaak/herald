import { useMemo, useState } from 'react'
import { Link } from '@tanstack/react-router'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { PageHeader } from '@/components/shared/page-header'
import { ConfirmDialog } from '@/components/shared'
import { formatDate } from '@/lib/date-utils'
import { clientAppsQueryOptions, userSubscriptionsQueryOptions } from '@/data/query-options'
import {
  cancelSubscriptionForClientApp,
  getSubscriptionForClientApp,
  type ClientAppItem,
  type SubscriptionDetailResponse,
} from '@/lib/api-generated'
import { formatProviderName } from '@/components/billing/format-provider-name'
import { m } from '@/paraglide/messages'
import { toast } from 'sonner'
import { getErrorMessage } from '@/lib/error-utils'

interface MySubscriptionsPageProps {
  realmId: string
}

type SubscriptionWithClientApp = {
  clientApp: ClientAppItem
  subscription: SubscriptionDetailResponse
}

/**
 * Self-service cancel action for a single subscription.
 *
 * Provider routing:
 * - stripe / creem: call the provider cancel API; the local status is updated
 *   later by the provider webhook (the success message reflects this).
 * - apple / google: developer-initiated cancel is not supported by the platform
 *   APIs, so only a static hint to manage the subscription in the store is
 *   shown — no cancel button, no API call.
 */
function SubscriptionCancelAction({
  realmId,
  clientAppId,
  subscriptionId,
  paymentProvider,
  active,
}: {
  realmId: string
  clientAppId: string
  subscriptionId: string
  paymentProvider: string
  active: boolean
}) {
  const queryClient = useQueryClient()
  const [confirmOpen, setConfirmOpen] = useState(false)

  const cancelMutation = useMutation({
    mutationFn: async () => {
      const response = await cancelSubscriptionForClientApp({
        path: { clientAppId },
        body: { cancelAtPeriodEnd: false },
      })
      if (response.error) throw response.error
      return response.data
    },
    onSuccess: () => {
      toast.success(m['billing.subscription_canceled_success']())
      setConfirmOpen(false)
      // Local status updates via webhook; refetch to pick it up when it lands.
      queryClient.invalidateQueries({ queryKey: ['user-subscriptions', realmId] })
    },
    onError: (error: unknown) => {
      toast.error(m['billing.subscription_cancel_failed']({ message: getErrorMessage(error) }))
    },
  })

  // Apple / Google: no developer cancel API; show only the store hint.
  if (paymentProvider === 'apple' || paymentProvider === 'google') {
    return (
      <p
        className="text-sm text-muted-foreground"
        data-testid={`subscription-manage-hint-${subscriptionId}`}
      >
        {paymentProvider === 'apple'
          ? m['billing.subscription_manage_via_app_store']()
          : m['billing.subscription_manage_via_google_play']()}
      </p>
    )
  }

  // Other/unknown providers: also cannot self-cancel, generic hint.
  if (paymentProvider !== 'stripe' && paymentProvider !== 'creem') {
    return (
      <p className="text-sm text-muted-foreground">
        {m['billing.subscription_cancel_provider_unsupported']()}
      </p>
    )
  }

  if (!active) {
    return null
  }

  return (
    <>
      <Button
        variant="destructive"
        size="sm"
        className="w-full"
        onClick={() => setConfirmOpen(true)}
        disabled={cancelMutation.isPending}
        data-testid={`subscription-cancel-${subscriptionId}`}
      >
        {cancelMutation.isPending
          ? m['billing.subscription_canceling']()
          : m['billing.subscription_cancel_button']()}
      </Button>
      <ConfirmDialog
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        title={m['billing.subscription_cancel_confirm_title']()}
        description={m['billing.subscription_cancel_confirm_description']()}
        onConfirm={() => {
          cancelMutation.mutateAsync().catch(() => {})
        }}
        confirmLabel={m['billing.subscription_cancel_button']()}
        isPending={cancelMutation.isPending}
      />
    </>
  )
}

export function MySubscriptionsPage({ realmId }: MySubscriptionsPageProps) {
  const { data: clientAppsResponse, isLoading: isLoadingApps } = useQuery(
    clientAppsQueryOptions(realmId, { page: 0, pageSize: 100 })
  )

  const clientApps = useMemo(() => clientAppsResponse?.items ?? [], [clientAppsResponse?.items])

  const clientAppIds = useMemo(
    () =>
      clientApps
        .map((app) => app.id)
        .sort()
        .join(','),
    [clientApps]
  )

  // Query for subscriptions
  const { data: subscriptions = [], isLoading: isLoadingSubscriptions } = useQuery({
    ...userSubscriptionsQueryOptions<SubscriptionWithClientApp[]>(
      realmId,
      clientAppIds,
      async () => {
        if (clientApps.length === 0) {
          return []
        }

        const results = await Promise.all(
          clientApps.map(async (clientApp) => {
            try {
              const response = await getSubscriptionForClientApp({
                path: { clientAppId: clientApp.id },
              })

              if (response.error || !response.data) {
                return null
              }

              return {
                clientApp,
                subscription: response.data,
              }
            } catch {
              return null
            }
          })
        )

        return results.filter((result): result is SubscriptionWithClientApp => result !== null)
      }
    ),
    enabled: clientApps.length > 0,
  })

  const isLoading = isLoadingApps || isLoadingSubscriptions

  const getStatusBadge = (status: string) => {
    switch (status.toLowerCase()) {
      case 'active':
        return (
          <Badge className="bg-success/10 text-success">
            {m['billing.subscription_status_label_active']()}
          </Badge>
        )
      case 'past_due':
        return (
          <Badge variant="destructive">{m['billing.subscription_status_label_past_due']()}</Badge>
        )
      case 'canceled':
        return (
          <Badge variant="secondary">{m['billing.subscription_status_label_canceled']()}</Badge>
        )
      case 'scheduled_cancel':
        return (
          <Badge className="bg-warning/10 text-warning">
            {m['billing.subscription_status_label_scheduled_cancel']()}
          </Badge>
        )
      default:
        return <Badge variant="outline">{status}</Badge>
    }
  }

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="text-muted-foreground">{m['billing.my_subscriptions_loading']()}</div>
      </div>
    )
  }

  return (
    <div className="space-y-6" data-testid="my-subscriptions-page">
      <PageHeader title={m['billing.my_subscriptions_title']()} />

      {subscriptions.length === 0 ? (
        <Card className="border-dashed">
          <CardContent className="flex flex-col items-center justify-center py-12">
            <div className="text-4xl mb-4">📦</div>
            <h3 className="text-lg font-semibold mb-2">
              {m['billing.my_subscriptions_empty_title']()}
            </h3>
            <p className="text-sm text-muted-foreground text-center">
              {m['billing.my_subscriptions_empty_description']()}
            </p>
            <Button asChild className="mt-4" data-testid="my-subscriptions-browse-plans">
              <Link to="/$realmId/user/purchase-points" params={{ realmId }}>
                {m['billing.my_subscriptions_browse_plans']()}
              </Link>
            </Button>
          </CardContent>
        </Card>
      ) : (
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3" data-testid="subscription-list">
          {subscriptions.map(({ clientApp, subscription }) => (
            <Card key={subscription.id}>
              <CardHeader>
                <div className="flex items-start justify-between">
                  <div>
                    <CardTitle className="text-lg">
                      {subscription.entitlementKey ?? 'Subscription'}
                    </CardTitle>
                    <CardDescription className="mt-1">{clientApp.name}</CardDescription>
                  </div>
                  {getStatusBadge(subscription.status)}
                </div>
              </CardHeader>
              <CardContent className="space-y-4">
                <div>
                  <div className="text-sm text-muted-foreground">
                    {m['billing.my_subscriptions_current_period']()}
                  </div>
                  <div className="text-sm font-medium">
                    {subscription.currentPeriodStart && subscription.currentPeriodEnd
                      ? `${formatDate(subscription.currentPeriodStart)} - ${formatDate(subscription.currentPeriodEnd)}`
                      : m['billing.my_subscriptions_not_available']()}
                  </div>
                </div>

                <div>
                  <div className="text-sm text-muted-foreground">
                    {m['billing.subscription_payment_provider']()}
                  </div>
                  <div className="text-2xl font-bold">
                    {formatProviderName(subscription.paymentProvider ?? '')}
                  </div>
                </div>

                {subscription.status.toLowerCase() === 'active' && (
                  <Button
                    asChild
                    variant="secondary"
                    size="sm"
                    className="w-full"
                    data-testid={`subscription-change-plan-${subscription.id}`}
                  >
                    <Link to="/$realmId/user/purchase-points" params={{ realmId }}>
                      {m['billing.my_subscriptions_change_plan']()}
                    </Link>
                  </Button>
                )}

                <SubscriptionCancelAction
                  realmId={realmId}
                  clientAppId={clientApp.id}
                  subscriptionId={subscription.id}
                  paymentProvider={subscription.paymentProvider ?? ''}
                  active={subscription.status.toLowerCase() === 'active'}
                />

                <Button asChild variant="outline" size="sm" className="w-full">
                  <Link
                    to="/$realmId/subscription/$subscriptionId/history"
                    params={{ realmId, subscriptionId: subscription.id }}
                  >
                    {m['billing.my_subscriptions_view_history']()}
                  </Link>
                </Button>
              </CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  )
}
