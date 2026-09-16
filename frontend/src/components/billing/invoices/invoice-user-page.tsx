import { useState, useMemo, type ReactNode } from 'react'
import { useQuery } from '@tanstack/react-query'
import { type ColumnDef } from '@tanstack/react-table'
import { Download, ExternalLink, Eye } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { DataTable } from '@/components/shared/data-table'
import { PageHeader } from '@/components/shared'
import { InvoiceStatusBadge } from '@/components/billing/invoices/invoice-status-badge'
import { ListPagination } from '@/components/shared'
import type { InvoiceResponse } from '@/lib/api-generated'
import { formatDate } from '@/lib/date-utils'
import { myInvoiceListQueryOptions } from '@/data/invoice-query-options'
import {
  formatInvoiceAmount,
  downloadInvoicePdf,
  getProviderLabel,
  isExternalInvoice,
  shouldRenderRefundDimension,
  INVOICE_PAGE_SIZE,
  PDF_DOWNLOADABLE_STATUSES,
} from '@/lib/invoice-utils'
import { m } from '@/paraglide/messages'

function createInvoiceColumns(
  page: number,
  pageSize: number,
  onViewInvoice?: (invoice: InvoiceResponse) => void
): ColumnDef<InvoiceResponse>[] {
  return [
    {
      id: 'rowNumber',
      header: '#',
      meta: { hiddenOnMobile: true },
      cell: ({ row }) => (
        <span className="text-muted-foreground text-sm">{page * pageSize + row.index + 1}</span>
      ),
    },
    {
      accessorKey: 'invoiceNumber',
      header: m['billing.invoice_number'](),
      cell: ({ row }) => (
        <span className="font-mono text-sm font-medium break-all">
          {row.getValue('invoiceNumber')}
        </span>
      ),
    },
    {
      accessorKey: 'provider',
      header: m['billing.invoice_provider'](),
      meta: { hiddenOnMobile: true },
      cell: ({ row }) => {
        const provider = row.original.provider
        if (provider === 'manual') return null
        return (
          <Badge variant="secondary" className="bg-info/10 text-info border-info/20 text-xs">
            {getProviderLabel(provider)}
          </Badge>
        )
      },
    },
    {
      accessorKey: 'total',
      header: m['billing.invoice_total'](),
      cell: ({ row }) => {
        const total = row.getValue('total') as number
        const currency = row.original.currency
        const provider = row.original.provider
        const amountRefunded = row.original.amountRefunded
        const showPill = shouldRenderRefundDimension(provider, amountRefunded)
        return (
          <div className="flex flex-col">
            <span className="font-mono text-sm">{formatInvoiceAmount(total, currency)}</span>
            {showPill && (
              <span
                className="mt-0.5 inline-flex w-fit items-center rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground"
                data-testid={`invoice-refund-summary-${row.original.id}`}
              >
                {m['billing.refund_summary_label']()}{' '}
                {formatInvoiceAmount(amountRefunded, currency)}/
                {formatInvoiceAmount(total, currency)}
              </span>
            )}
          </div>
        )
      },
    },
    {
      accessorKey: 'status',
      header: m['common.status'](),
      cell: ({ row }) => (
        <InvoiceStatusBadge
          status={row.getValue('status') as string}
          provider={row.original.provider}
        />
      ),
    },
    {
      accessorKey: 'dueDate',
      header: m['billing.invoice_due_date'](),
      meta: { hiddenOnMobile: true },
      cell: ({ row }) => {
        const dueDate = row.getValue('dueDate') as string
        return <span className="text-sm">{formatDate(dueDate)}</span>
      },
    },
    {
      id: 'actions',
      header: m['billing.invoice_actions'](),
      cell: ({ row }) => {
        const invoice = row.original
        const provider = invoice.provider
        const isExternal = isExternalInvoice(provider)
        const externalPdfUrl = invoice.externalPdfUrl ?? null
        const externalHostedUrl = invoice.externalHostedUrl ?? null

        const viewButton = onViewInvoice ? (
          <Button
            variant="ghost"
            size="sm"
            onClick={() => onViewInvoice(invoice)}
            data-testid={`invoice-view-${invoice.id}`}
          >
            <Eye className="mr-1 h-4 w-4" />
            {m['billing.invoice_view']()}
          </Button>
        ) : null

        let actionContent: ReactNode = null

        if (isExternal && externalPdfUrl) {
          actionContent = (
            <Button
              variant="ghost"
              size="sm"
              asChild
              data-testid={`invoice-download-pdf-${invoice.id}`}
            >
              <a href={externalPdfUrl} target="_blank" rel="noopener noreferrer">
                <Download className="mr-1 h-4 w-4" />
                PDF
              </a>
            </Button>
          )
        } else if (isExternal && externalHostedUrl) {
          actionContent = (
            <Button
              variant="ghost"
              size="sm"
              asChild
              data-testid={`invoice-view-provider-${invoice.id}`}
            >
              <a href={externalHostedUrl} target="_blank" rel="noopener noreferrer">
                <ExternalLink className="mr-1 h-4 w-4" />
                {getProviderLabel(provider)}
              </a>
            </Button>
          )
        } else if (isExternal) {
          actionContent = (
            <span
              className="text-xs text-muted-foreground"
              data-testid={`invoice-managed-external-${invoice.id}`}
            >
              {m['billing.invoice_external_link_pending']({
                provider: getProviderLabel(provider),
              })}
            </span>
          )
        } else {
          const canDownload = PDF_DOWNLOADABLE_STATUSES.has(invoice.status)
          if (canDownload) {
            actionContent = (
              <Button
                variant="ghost"
                size="sm"
                onClick={() =>
                  downloadInvoicePdf(
                    `/api/user/bill/invoices/${invoice.id}/pdf`,
                    `${invoice.invoiceNumber}.pdf`
                  )
                }
                data-testid={`invoice-download-pdf-${invoice.id}`}
              >
                <Download className="mr-1 h-4 w-4" />
                PDF
              </Button>
            )
          }
        }

        if (!viewButton && !actionContent) return null

        return (
          <div className="flex items-center gap-1">
            {viewButton}
            {actionContent}
          </div>
        )
      },
    },
  ]
}

export function InvoiceUserPage({
  realmId,
  onViewInvoice,
}: {
  realmId: string
  onViewInvoice?: (invoice: InvoiceResponse) => void
}) {
  const [page, setPage] = useState(0)

  const { data, isLoading, error } = useQuery(
    myInvoiceListQueryOptions(realmId, {
      page,
      pageSize: INVOICE_PAGE_SIZE,
    })
  )

  const invoices = data?.data ?? []
  const total = data?.total ?? 0

  const columns = useMemo(
    () => createInvoiceColumns(page, INVOICE_PAGE_SIZE, onViewInvoice),
    [page, onViewInvoice]
  )

  return (
    <div className="space-y-6" data-testid="invoice-user-page">
      <PageHeader title={m['billing.invoice_my_title']()} headingTestId="invoice-user-heading" />

      <section>
        <h2 className="text-base font-semibold">{m['billing.invoice_user_list_title']()}</h2>
        <div className="mt-4 border-t border-border pt-4">
          <DataTable
            columns={columns}
            data={invoices}
            isLoading={isLoading}
            error={error instanceof Error ? error : error ? new Error(String(error)) : undefined}
            loadingMessage={m['billing.invoice_loading']()}
            emptyMessage={m['billing.invoice_empty']()}
            data-testid="invoice-user-table"
          />
        </div>
      </section>

      {total > 0 && (
        <ListPagination
          page={page}
          pageSize={INVOICE_PAGE_SIZE}
          total={total}
          onPageChange={setPage}
          testIdPrefix="invoice-user-pagination"
        />
      )}
    </div>
  )
}
