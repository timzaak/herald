import { useQuery } from '@tanstack/react-query'
import { format } from 'date-fns'
import { CreditCard } from 'lucide-react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Skeleton } from '@/components/ui/skeleton'
import { PageHeader, ListPagination } from '@/components/shared'
import { subscriptionsQueryOptions } from '@/data/query-options'
import { useUrlSyncedInput } from '@/hooks/use-url-synced-input'
import { formatProviderName } from '@/components/billing/format-provider-name'
import { m } from '@/paraglide/messages'
import type { SubscriptionListItemResponse, SubscriptionListResponse } from '@/lib/api-generated'

const PAGE_SIZE = 20

const STATUS_FILTER_OPTIONS = ['all', 'active', 'past_due', 'canceled', 'expired'] as const

const PROVIDER_FILTER_OPTIONS = ['all', 'stripe', 'creem'] as const

function getStatusBadgeClass(status: string): string {
  switch (status) {
    case 'active':
      return 'bg-success/10 text-success'
    case 'past_due':
      return 'bg-warning/10 text-warning'
    case 'canceled':
      return 'bg-muted text-muted-foreground'
    case 'expired':
      return 'bg-destructive/10 text-destructive'
    default:
      return 'bg-info/10 text-info'
  }
}

function formatStatusLabel(status: string): string {
  const labels: Record<string, string> = {
    active: m['billing.subscription_status_label_active'](),
    past_due: m['billing.subscription_status_label_past_due'](),
    canceled: m['billing.subscription_status_label_canceled'](),
    expired: m['billing.subscription_status_label_expired'](),
    trialing: m['billing.subscription_status_label_trialing'](),
    incomplete: m['billing.subscription_status_label_incomplete'](),
    paused: m['billing.subscription_status_label_paused'](),
    disputed: m['billing.subscription_status_label_disputed'](),
  }
  return labels[status] ?? status
}

function formatBillingTypeLabel(billingType: string): string {
  switch (billingType) {
    case 'recurring':
      return m['billing.billing_type_recurring']()
    case 'non_renewing':
      return m['billing.billing_type_non_renewing']()
    default:
      return billingType
  }
}

interface SubscriptionListSearch {
  page?: number
  pageSize?: number
  entitlementKey?: string
  status?: string
  paymentProvider?: string
}

interface AdminSubscriptionListPageProps {
  realmId: string
  search: SubscriptionListSearch
  // The URL is the single source of truth for these filters: controls bind
  // to `search` and every edit is a patch the route navigates with, so deep
  // links/back-forward always match what the controls show.
  onSearchChange: (patch: Partial<SubscriptionListSearch>) => void
}

export function AdminSubscriptionListPage({
  realmId,
  search,
  onSearchChange,
}: AdminSubscriptionListPageProps) {
  const [entitlementKeyFilter, setEntitlementKeyFilter] = useUrlSyncedInput(
    search.entitlementKey ?? '',
    (value) => onSearchChange({ entitlementKey: value, page: 0 })
  )

  const filters = {
    entitlementKey: search.entitlementKey || undefined,
    status: search.status,
    paymentProvider: search.paymentProvider,
    page: search.page ?? 0,
    pageSize: search.pageSize ?? PAGE_SIZE,
  }

  const { data, isLoading } = useQuery({
    ...subscriptionsQueryOptions(realmId, filters),
    select: (rawData) => rawData as SubscriptionListResponse | undefined,
  })

  const subscriptions = data?.items ?? []
  const total = data?.total ?? 0

  const hasFilters =
    (search.entitlementKey ?? '') !== '' ||
    search.status !== undefined ||
    search.paymentProvider !== undefined

  return (
    <div className="space-y-6" data-testid="admin-subscription-list-page">
      <PageHeader
        title={m['billing.subscription_list_title']()}
        headingTestId="admin-subscription-list-heading"
      />

      {/* Filter bar */}
      <div className="flex flex-wrap items-center gap-4">
        <Input
          placeholder={m['billing.subscription_filter_entitlement_key_placeholder']()}
          value={entitlementKeyFilter}
          onChange={(e) => setEntitlementKeyFilter(e.target.value)}
          className="w-[220px]"
          data-testid="entitlement-key-filter-input"
        />

        <Select
          value={search.status ?? 'all'}
          onValueChange={(value) =>
            onSearchChange({ status: value === 'all' ? undefined : value, page: 0 })
          }
        >
          <SelectTrigger className="w-[160px]" data-testid="status-filter-select">
            <SelectValue placeholder={m['billing.subscription_filter_all_statuses']()} />
          </SelectTrigger>
          <SelectContent>
            {STATUS_FILTER_OPTIONS.map((value) => (
              <SelectItem key={value} value={value}>
                {value === 'all'
                  ? m['billing.subscription_filter_all_statuses']()
                  : formatStatusLabel(value)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Select
          value={search.paymentProvider ?? 'all'}
          onValueChange={(value) =>
            onSearchChange({ paymentProvider: value === 'all' ? undefined : value, page: 0 })
          }
        >
          <SelectTrigger className="w-[160px]" data-testid="payment-provider-filter-select">
            <SelectValue placeholder={m['billing.subscription_filter_all_providers']()} />
          </SelectTrigger>
          <SelectContent>
            {PROVIDER_FILTER_OPTIONS.map((value) => (
              <SelectItem key={value} value={value}>
                {value === 'all'
                  ? m['billing.subscription_filter_all_providers']()
                  : formatProviderName(value)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {/* Table or empty state */}
      {isLoading ? (
        <LoadingSkeleton />
      ) : subscriptions.length === 0 ? (
        <Card className="border-dashed" data-testid="admin-subscription-list-empty-state">
          <CardContent className="flex flex-col items-center justify-center py-12">
            <CreditCard className="h-12 w-12 text-muted-foreground mb-4" />
            <p className="text-sm text-muted-foreground text-center">
              {hasFilters
                ? m['billing.subscription_list_no_match']()
                : m['billing.subscription_list_empty']()}
            </p>
          </CardContent>
        </Card>
      ) : (
        <>
          <Card>
            <CardHeader>
              <CardTitle>{m['billing.subscription_list_title']()}</CardTitle>
            </CardHeader>
            <CardContent>
              <Table data-testid="admin-subscription-list-table">
                <TableHeader>
                  <TableRow>
                    <TableHead>{m['billing.subscription_entitlement_key']()}</TableHead>
                    <TableHead>{m['billing.subscription_payment_provider']()}</TableHead>
                    <TableHead>{m['billing.subscription_external_price_id']()}</TableHead>
                    <TableHead>{m['billing.subscription_synced_at']()}</TableHead>
                    <TableHead>{m['billing.subscription_billing_type']()}</TableHead>
                    <TableHead>{m['billing.subscription_service_period_end']()}</TableHead>
                    <TableHead>{m['common.status']()}</TableHead>
                    <TableHead>Client App</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {subscriptions.map((sub) => (
                    <SubscriptionRow key={sub.id} subscription={sub} />
                  ))}
                </TableBody>
              </Table>
            </CardContent>
          </Card>

          {total > 0 && (
            <ListPagination
              page={search.page ?? 0}
              pageSize={search.pageSize ?? PAGE_SIZE}
              total={total}
              onPageChange={(page) => onSearchChange({ page })}
              testIdPrefix="admin-subscription-list-pagination"
            />
          )}
        </>
      )}
    </div>
  )
}

function SubscriptionRow({ subscription }: { subscription: SubscriptionListItemResponse }) {
  return (
    <TableRow data-testid={`subscription-row-${subscription.id}`}>
      <TableCell className="font-mono text-sm">{subscription.entitlementKey}</TableCell>
      <TableCell className="font-medium">
        {formatProviderName(subscription.paymentProvider)}
      </TableCell>
      <TableCell className="font-mono text-sm">{subscription.externalPriceId ?? '---'}</TableCell>
      <TableCell className="text-sm">
        {subscription.syncedAt ? format(new Date(subscription.syncedAt), 'PP') : '---'}
      </TableCell>
      <TableCell className="text-sm" data-testid={`billing-type-${subscription.id}`}>
        {formatBillingTypeLabel(subscription.billingType)}
      </TableCell>
      <TableCell className="text-sm" data-testid={`service-period-end-${subscription.id}`}>
        {subscription.billingType === 'non_renewing' && subscription.currentPeriodEnd
          ? format(new Date(subscription.currentPeriodEnd), 'PP')
          : '---'}
      </TableCell>
      <TableCell>
        <span
          className={`inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium ${getStatusBadgeClass(subscription.status)}`}
        >
          {formatStatusLabel(subscription.status)}
        </span>
      </TableCell>
      <TableCell className="text-sm">{subscription.clientAppId ?? '---'}</TableCell>
    </TableRow>
  )
}

function LoadingSkeleton() {
  return (
    <Card>
      <CardHeader>
        <Skeleton className="h-6 w-32" />
      </CardHeader>
      <CardContent className="space-y-4">
        {Array.from({ length: 5 }).map((_, i) => (
          <Skeleton key={i} className="h-10 w-full" />
        ))}
      </CardContent>
    </Card>
  )
}
