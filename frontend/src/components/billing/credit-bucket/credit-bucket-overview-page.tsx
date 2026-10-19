import { useMemo, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { ChevronRight, Layers } from 'lucide-react'
import { Link } from '@tanstack/react-router'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Checkbox } from '@/components/ui/checkbox'
import { Skeleton } from '@/components/ui/skeleton'
import { Switch } from '@/components/ui/switch'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { PageHeader } from '@/components/shared'
import { creditBucketOverviewQueryOptions } from '@/data/query-options'
import { m } from '@/paraglide/messages'
import type { ByCreditTypeResponse, OverviewRowResponse } from '@/lib/api-generated'
import type {
  CreditTypeKey,
  OverviewSearch,
} from '@/routes/$realmId/manage/billing/credit-buckets/overview'
import { realmPath, useResolvedRealmContext } from '@/lib/realm-routing'

const CREDIT_TYPE_KEYS: ReadonlyArray<CreditTypeKey> = [
  'topup',
  'subscription',
  'registration',
  'freePeriodic',
  'granted',
]

function creditTypeLabel(key: CreditTypeKey): string {
  return m[`credit_buckets.overview.type_${key}`]()
}

interface CreditBucketOverviewPageProps {
  realmId: string
  search: OverviewSearch
  onSearchChange: (next: OverviewSearch) => void
}

/**
 * Bucket × Credit-type overview matrix audit page.
 *
 * Rows = buckets; columns = per-credit-type balance + `bucketTotal`. A final
 * dedicated row renders the top-level `grandTotal` (cross-bucket aggregate).
 * `grandTotal` is rendered as a separate row, NOT mixed into `rows`
 * (it's a distinct top-level field on `BucketOverviewResponse`).
 *
 * Filtering (creditTypes multi-select + enabledOnly) is client-side and driven
 * from URL search params — it never triggers a refetch. Disabled buckets render
 * at `opacity-50` with their residual amounts retained.
 */
export function CreditBucketOverviewPage({
  realmId,
  search,
  onSearchChange,
}: CreditBucketOverviewPageProps) {
  const { data, isLoading } = useQuery(creditBucketOverviewQueryOptions(realmId))

  const selectedTypes = useMemo<CreditTypeKey[]>(
    () => (search.creditTypes?.length ? search.creditTypes : [...CREDIT_TYPE_KEYS]),
    [search.creditTypes]
  )

  const enabledOnly = search.enabledOnly ?? false

  const rows = useMemo<OverviewRowResponse[]>(() => data?.rows ?? [], [data?.rows])
  const grandTotal = data?.grandTotal

  const visibleRows = useMemo<OverviewRowResponse[]>(() => {
    return rows.filter((row) => (enabledOnly ? row.enabled : true))
  }, [rows, enabledOnly])

  function toggleType(key: CreditTypeKey) {
    const set = new Set(selectedTypes)
    if (set.has(key)) {
      set.delete(key)
    } else {
      set.add(key)
    }
    onSearchChange({
      enabledOnly,
      creditTypes: [...set],
    })
  }

  function handleEnabledOnlyChange(next: boolean) {
    onSearchChange({ enabledOnly: next, creditTypes: selectedTypes })
  }

  const hasBuckets = rows.length > 0

  return (
    <div className="space-y-6" data-testid="credit-bucket-overview-page">
      <PageHeader
        title={m['credit_buckets.overview.page_title']()}
        subtitle={m['credit_buckets.overview.page_subtitle']()}
        headingTestId="credit-bucket-overview-heading"
      />

      {/* Toolbar: credit-type multi-select + enabled-only switch */}
      <div
        className="flex flex-wrap items-center gap-6"
        data-testid="credit-bucket-overview-toolbar"
      >
        <div className="flex flex-wrap items-center gap-4">
          <span className="text-sm font-medium text-muted-foreground">
            {m['credit_buckets.overview.filter_types']()}
          </span>
          {CREDIT_TYPE_KEYS.map((key) => (
            <label
              key={key}
              className="flex items-center gap-2 text-sm"
              data-testid={`credit-bucket-overview-type-${key}`}
            >
              <Checkbox
                checked={selectedTypes.includes(key)}
                onCheckedChange={() => toggleType(key)}
              />
              {creditTypeLabel(key)}
            </label>
          ))}
        </div>
        <label
          className="flex items-center gap-2 text-sm"
          data-testid="credit-bucket-overview-enabled-only"
        >
          <Switch checked={enabledOnly} onCheckedChange={handleEnabledOnlyChange} />
          {m['credit_buckets.overview.enabled_only']()}
        </label>
      </div>

      {isLoading ? (
        <LoadingSkeleton />
      ) : !hasBuckets ? (
        <EmptyState realmId={realmId} />
      ) : (
        <Card>
          <CardHeader>
            <CardTitle>{m['credit_buckets.overview.table_title']()}</CardTitle>
          </CardHeader>
          <CardContent>
            <OverviewMatrix
              rows={visibleRows}
              selectedTypes={selectedTypes}
              grandTotal={grandTotal}
            />
          </CardContent>
        </Card>
      )}
    </div>
  )
}

function OverviewMatrix({
  rows,
  selectedTypes,
  grandTotal,
}: {
  rows: OverviewRowResponse[]
  selectedTypes: CreditTypeKey[]
  grandTotal?: ByCreditTypeResponse
}) {
  return (
    <Table data-testid="credit-bucket-overview-table">
      <TableHeader>
        <TableRow>
          <TableHead>{m['credit_buckets.overview.col_bucket']()}</TableHead>
          {selectedTypes.map((key) => (
            <TableHead key={key} className="text-right">
              <div className="flex flex-col items-end">
                <span>{creditTypeLabel(key)}</span>
                {grandTotal && (
                  <span
                    className="text-xs font-normal text-muted-foreground"
                    data-testid={`credit-bucket-overview-col-total-${key}`}
                  >
                    {formatAmount(grandTotal[key])}
                  </span>
                )}
              </div>
            </TableHead>
          ))}
          <TableHead className="text-right">{m['credit_buckets.overview.col_total']()}</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {rows.map((row) => (
          <OverviewRow key={row.bucketId} row={row} selectedTypes={selectedTypes} />
        ))}
        {grandTotal && (
          <TableRow
            className="border-t-2 border-t-border font-semibold hover:bg-transparent"
            data-testid="credit-bucket-overview-grandtotal"
          >
            <TableCell>{m['credit_buckets.overview.grand_total_label']()}</TableCell>
            {selectedTypes.map((key) => (
              <TableCell
                key={key}
                className="text-right tabular-nums"
                data-testid={`credit-bucket-overview-grandtotal-${key}`}
              >
                {formatAmount(grandTotal[key])}
              </TableCell>
            ))}
            <TableCell className="text-right tabular-nums">
              {formatAmount(sumByCreditType(grandTotal))}
            </TableCell>
          </TableRow>
        )}
      </TableBody>
    </Table>
  )
}

function OverviewRow({
  row,
  selectedTypes,
}: {
  row: OverviewRowResponse
  selectedTypes: CreditTypeKey[]
}) {
  const [expanded, setExpanded] = useState(false)
  const rowTotal = row.bucketTotal

  return (
    <>
      <TableRow
        className={row.enabled ? '' : 'opacity-50'}
        onClick={() => setExpanded((v) => !v)}
        data-testid={`credit-bucket-overview-row-${row.bucketId}`}
      >
        <TableCell>
          <div className="flex items-center gap-2">
            <ChevronRight
              className={`h-4 w-4 text-muted-foreground transition-transform ${
                expanded ? 'rotate-90' : ''
              }`}
            />
            <span className="max-w-xs truncate font-medium" title={row.name}>
              {row.name}
            </span>
            {!row.enabled && (
              <Badge
                variant="secondary"
                data-testid={`credit-bucket-overview-disabled-${row.bucketId}`}
              >
                {m['credit_buckets.disabled_badge']()}
              </Badge>
            )}
            {row.byCreditType.registration > 0 && (
              <Badge
                variant="outline"
                data-testid={`credit-bucket-overview-registration-${row.bucketId}`}
              >
                {m['credit_buckets.registration_pool_badge']()}
              </Badge>
            )}
          </div>
        </TableCell>
        {selectedTypes.map((key) => (
          <TableCell
            key={key}
            className="text-right tabular-nums"
            data-testid={`credit-bucket-overview-cell-${row.bucketId}-${key}`}
          >
            {formatAmount(row.byCreditType[key])}
          </TableCell>
        ))}
        <TableCell className="text-right tabular-nums font-medium">
          {formatAmount(rowTotal)}
        </TableCell>
      </TableRow>
      {expanded && (
        <TableRow
          className="bg-muted/40"
          data-testid={`credit-bucket-overview-detail-${row.bucketId}`}
        >
          <TableCell colSpan={selectedTypes.length + 2}>
            <div className="flex flex-wrap gap-x-6 gap-y-1 py-1 text-xs text-muted-foreground">
              {selectedTypes.map((key) => (
                <span key={key}>
                  {creditTypeLabel(key)}: {formatAmount(row.byCreditType[key])}
                  {rowTotal > 0 && (
                    <span className="ml-1">
                      ({Math.round((row.byCreditType[key] / rowTotal) * 100)}%)
                    </span>
                  )}
                </span>
              ))}
            </div>
          </TableCell>
        </TableRow>
      )}
    </>
  )
}

function EmptyState({ realmId }: { realmId: string }) {
  const realmContext = useResolvedRealmContext()
  return (
    <Card className="border-dashed" data-testid="credit-bucket-overview-empty-state">
      <CardContent className="flex flex-col items-center justify-center py-12">
        <Layers className="mb-4 h-12 w-12 text-muted-foreground" />
        <p className="mb-4 text-center text-sm text-muted-foreground">
          {m['credit_buckets.overview.empty_realm']()}
        </p>
        <Link
          to={realmPath({ ...realmContext, realmId }, '/manage/billing/credit-buckets')}
          className="text-sm font-medium text-primary underline-offset-4 hover:underline"
          data-testid="credit-bucket-overview-empty-cta"
        >
          {m['credit_buckets.overview.empty_cta']()}
        </Link>
      </CardContent>
    </Card>
  )
}

function LoadingSkeleton() {
  return (
    <Card>
      <CardHeader>
        <Skeleton className="h-6 w-40" />
      </CardHeader>
      <CardContent className="space-y-4">
        {Array.from({ length: 5 }).map((_, i) => (
          <Skeleton key={i} className="h-10 w-full" />
        ))}
      </CardContent>
    </Card>
  )
}

function formatAmount(value: number): string {
  return new Intl.NumberFormat().format(value)
}

function sumByCreditType(totals: ByCreditTypeResponse): number {
  return (
    totals.topup + totals.subscription + totals.registration + totals.freePeriodic + totals.granted
  )
}
