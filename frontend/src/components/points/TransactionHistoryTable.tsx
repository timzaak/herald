import { useMemo } from 'react'
import { type ColumnDef, flexRender, getCoreRowModel, useReactTable } from '@tanstack/react-table'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Badge } from '@/components/ui/badge'
import { ArrowUpRight, ArrowDownRight, Clock, ExternalLink } from 'lucide-react'
import type { PointsTransactionResponse } from '@/lib/api-generated'
import type { TransactionFilters } from '@/lib/schemas/points-forms'
import { useActiveFilters } from '@/hooks/use-active-filters'
import { m } from '@/paraglide/messages'
import { formatDateTimeShort } from '@/lib/date-utils'

interface TransactionHistoryTableProps {
  transactions: PointsTransactionResponse[]
  loading?: boolean
  filters: TransactionFilters
  admin?: boolean
  clientApps?: Array<{ id: string; name: string }>
  /**
   * Credit Bucket lookup source for the Bucket column. When
   * provided the column renders; rows whose `bucketId` has no entry fall back
   * to the first 8 chars of `bucketId` (mirrors the client-app fallback).
   */
  buckets?: Array<{ id: string; name: string }>
}

export function TransactionHistoryTable({
  transactions,
  loading = false,
  filters,
  admin = false,
  clientApps,
  buckets,
}: TransactionHistoryTableProps) {
  const hasActiveFilters = useActiveFilters(filters)

  // Memoized client app map for O(1) lookup
  const clientAppsMap = useMemo(
    () => new Map(clientApps?.map((app) => [app.id, app])),
    [clientApps]
  )

  // Memoized bucket map for O(1) lookup by id.
  const bucketsMap = useMemo(
    () => new Map(buckets?.map((bucket) => [bucket.id, bucket])),
    [buckets]
  )

  const columns = useMemo<ColumnDef<PointsTransactionResponse>[]>(
    () => [
      {
        id: 'createdAt',
        accessorKey: 'createdAt',
        header: m['points.transaction_col_time'](),
        cell: ({ row }) => {
          const dateStr = row.getValue('createdAt') as string
          return (
            <div className="flex items-center gap-2" data-testid={`transaction-time-${row.index}`}>
              <Clock className="h-3 w-3 text-muted-foreground" />
              <span className="text-sm font-mono">{formatDateTimeShort(dateStr)}</span>
            </div>
          )
        },
      },
      {
        accessorKey: 'transactionType',
        header: m['points.transaction_col_type'](),
        cell: ({ row }) => {
          // Since API uses 'type' field, we need to determine from amount sign
          const amount = row.getValue('amount') as number
          const type = amount >= 0 ? 'recharge' : 'consume'
          return (
            <div className="flex items-center gap-2" data-testid={`transaction-type-${row.index}`}>
              {type === 'recharge' ? (
                <ArrowUpRight className="h-4 w-4 text-success" />
              ) : (
                <ArrowDownRight className="h-4 w-4 text-destructive" />
              )}
              <Badge variant={type === 'recharge' ? 'default' : 'secondary'}>
                {type === 'recharge'
                  ? m['points.transaction_type_recharge']()
                  : m['points.transaction_type_consume']()}
              </Badge>
            </div>
          )
        },
      },
      {
        accessorKey: 'amount',
        header: m['points.transaction_col_amount'](),
        cell: ({ row }) => {
          const amount = row.getValue('amount') as number
          return (
            <div
              className={`font-semibold ${amount >= 0 ? 'text-success' : 'text-destructive'}`}
              data-testid={`transaction-amount-${row.index}`}
            >
              {amount >= 0 ? '+' : ''}
              {amount.toLocaleString()}
            </div>
          )
        },
      },
      // Bucket dimension. Column renders whenever the caller
      // supplies a bucket lookup; rows without a resolvable bucket fall back
      // to the first 8 chars of `bucketId`, matching the client-app fallback.
      ...(buckets
        ? [
            {
              id: 'bucket',
              accessorKey: 'bucketId' as const,
              header: m['points.transaction_bucket_column'](),
              meta: { hiddenOnMobile: true },
              cell: ({ row }: { row: { original: PointsTransactionResponse; index: number } }) => {
                // Read bucketId from `row.original` rather than `row.getValue('bucketId')`:
                // this column sets an explicit `id: 'bucket'`, which overrides the
                // accessor-derived id in TanStack Table v8, so `getValue('bucketId')`
                // resolves to no column and logs "Column with id 'bucketId' does not exist".
                const bucketId = row.original.bucketId
                const bucket = bucketId ? bucketsMap.get(bucketId) : undefined
                const label = bucket ? bucket.name : bucketId ? String(bucketId).slice(0, 8) : '-'
                return (
                  <div data-testid={`transaction-bucket-${row.index}`}>
                    <Badge variant="outline">{label}</Badge>
                  </div>
                )
              },
            },
          ]
        : []),
      {
        accessorKey: 'balanceAfter',
        header: m['points.transaction_col_balance_after'](),
        meta: { hiddenOnMobile: true },
        cell: ({ row }) => (
          <div className="font-mono" data-testid={`transaction-balance-${row.index}`}>
            {(row.getValue('balanceAfter') as number).toLocaleString()}
          </div>
        ),
      },
      {
        accessorKey: 'description',
        header: m['points.transaction_col_description'](),
        meta: { hiddenOnMobile: true },
        cell: ({ row }) => {
          const description = row.getValue('description') as string | null
          const subscriptionId = row.original.subscriptionId
          return (
            <div className="max-w-xs truncate" data-testid={`transaction-description-${row.index}`}>
              {description || '-'}
              {subscriptionId && (
                <div className="text-xs text-muted-foreground">
                  Sub: {String(subscriptionId).slice(0, 8)}...
                </div>
              )}
            </div>
          )
        },
      },
      ...(admin
        ? [
            {
              accessorKey: 'clientAppId',
              header: m['points.transaction_col_source'](),
              meta: { hiddenOnMobile: true },
              cell: ({ row }: { row: { getValue: (key: string) => unknown; index: number } }) => {
                const clientAppId = row.getValue('clientAppId') as string | null
                const clientApp = clientAppsMap.get(clientAppId ?? '')
                return (
                  <div data-testid={`transaction-client-${row.index}`}>
                    {clientApp
                      ? clientApp.name
                      : clientAppId
                        ? String(clientAppId).slice(0, 8) + '...'
                        : '-'}
                  </div>
                )
              },
            },
          ]
        : []),
      ...(admin
        ? [
            {
              accessorKey: 'externalRefId',
              header: m['points.transaction_col_ref_id'](),
              meta: { hiddenOnMobile: true },
              cell: ({ row }: { row: { getValue: (key: string) => unknown; index: number } }) => {
                const externalRefId = row.getValue('externalRefId') as string | null
                return (
                  <div
                    className="text-xs text-muted-foreground"
                    data-testid={`transaction-ref-${row.index}`}
                  >
                    {externalRefId ? (
                      <div className="flex items-center gap-1">
                        <ExternalLink className="h-3 w-3" />
                        <span className="font-mono">{String(externalRefId).slice(0, 12)}...</span>
                      </div>
                    ) : (
                      '-'
                    )}
                  </div>
                )
              },
            },
          ]
        : []),
    ],
    [admin, clientAppsMap, buckets, bucketsMap]
  )

  const table = useReactTable({
    data: transactions,
    columns,
    getCoreRowModel: getCoreRowModel(),
  })

  if (loading) {
    return (
      <div className="space-y-4">
        <div className="animate-pulse">
          <div className="h-12 bg-muted rounded mb-4" />
          {[...Array(5)].map((_, i) => (
            <div key={i} className="h-12 bg-muted/50 rounded mb-2" />
          ))}
        </div>
      </div>
    )
  }

  if (transactions.length === 0) {
    return (
      <div className="text-center py-12 text-muted-foreground" data-testid="no-transactions">
        <Clock className="h-12 w-12 mx-auto mb-4 opacity-50" />
        <p>{m['points.transaction_empty']()}</p>
        {hasActiveFilters && (
          <p className="text-sm mt-2">{m['points.transaction_empty_filter_hint']()}</p>
        )}
      </div>
    )
  }

  return (
    <div className="space-y-4" data-testid="transaction-history-table">
      <Table>
        <TableHeader>
          {table.getHeaderGroups().map((headerGroup) => (
            <TableRow key={headerGroup.id}>
              {headerGroup.headers.map((header) => (
                <TableHead
                  key={header.id}
                  className={
                    header.column.columnDef.meta?.hiddenOnMobile
                      ? 'hidden md:table-cell'
                      : undefined
                  }
                >
                  {header.isPlaceholder
                    ? null
                    : flexRender(header.column.columnDef.header, header.getContext())}
                </TableHead>
              ))}
            </TableRow>
          ))}
        </TableHeader>
        <TableBody>
          {table.getRowModel().rows.map((row) => (
            <TableRow key={row.id} data-testid={`transaction-row-${row.index}`}>
              {row.getVisibleCells().map((cell) => (
                <TableCell
                  key={cell.id}
                  className={
                    cell.column.columnDef.meta?.hiddenOnMobile ? 'hidden md:table-cell' : undefined
                  }
                >
                  {flexRender(cell.column.columnDef.cell, cell.getContext())}
                </TableCell>
              ))}
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
