import { useState, useCallback, useMemo } from 'react'
import { Link } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { type ColumnDef } from '@tanstack/react-table'
import {
  MoreHorizontal,
  Eye,
  Pencil,
  Send,
  Ban,
  CheckCircle,
  Download,
  Settings,
  Plus,
  Search,
  Shield,
  ExternalLink,
} from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { DataTable } from '@/components/shared/data-table'
import { PageHeader } from '@/components/shared'
import { InvoiceStatusBadge } from '@/components/billing/invoices/invoice-status-badge'
import { RefundAmountChip } from '@/components/billing/invoices/refund-amount-chip'
import { ListPagination } from '@/components/shared'
import type { InvoiceResponse } from '@/lib/api-generated'
import { formatDate } from '@/lib/date-utils'
import { invoiceListQueryOptions } from '@/data/invoice-query-options'
import { featureAvailabilityQueryOptions } from '@/data/query-options'
import {
  INVOICE_PAGE_SIZE,
  getInvoiceSourceLabel,
  formatInvoiceAmount,
  downloadInvoicePdf,
  getAvailableActions,
  getProviderLabel,
  isExternalInvoice,
  getViewInProviderUrl,
} from '@/lib/invoice-utils'
import { m } from '@/paraglide/messages'
import { realmPath, useResolvedRealmContext } from '@/lib/realm-routing'

interface InvoiceFilters {
  status?: string
  source?: string
  dateFrom?: string
  dateTo?: string
  search?: string
  provider?: string
  attribution?: string
}

interface InvoiceAdminPageProps {
  realmId: string
  onViewInvoice?: (invoice: InvoiceResponse) => void
  onEditInvoice?: (invoice: InvoiceResponse) => void
  onCreateInvoice?: () => void
  onOpenSellerConfig?: () => void
  onOpenPolicyConfig?: () => void
  onIssueInvoice?: (invoice: InvoiceResponse) => void
  onVoidInvoice?: (invoice: InvoiceResponse) => void
  onMarkPaidInvoice?: (invoice: InvoiceResponse) => void
}

function createInvoiceColumns(
  _realmId: string,
  handlers: {
    onView?: (invoice: InvoiceResponse) => void
    onEdit?: (invoice: InvoiceResponse) => void
    onIssue?: (invoice: InvoiceResponse) => void
    onVoid?: (invoice: InvoiceResponse) => void
    onMarkPaid?: (invoice: InvoiceResponse) => void
  },
  page: number,
  pageSize: number
): ColumnDef<InvoiceResponse>[] {
  return [
    {
      id: 'rowNumber',
      header: '#',
      cell: ({ row }) => (
        <span className="text-muted-foreground text-sm">{page * pageSize + row.index + 1}</span>
      ),
    },
    {
      accessorKey: 'invoiceNumber',
      header: m['billing.invoice_number'](),
      cell: ({ row }) => (
        <span className="font-mono text-sm font-medium">{row.getValue('invoiceNumber')}</span>
      ),
    },
    {
      accessorKey: 'billingName',
      header: m['billing.invoice_buyer'](),
      cell: ({ row }) => <span className="text-sm">{row.getValue('billingName')}</span>,
    },
    {
      accessorKey: 'source',
      header: m['billing.invoice_source'](),
      cell: ({ row }) => {
        const source = row.getValue('source') as string
        return <Badge variant="outline">{getInvoiceSourceLabel(source)}</Badge>
      },
    },
    {
      accessorKey: 'provider',
      header: m['billing.invoice_provider'](),
      cell: ({ row }) => {
        const provider = row.original.provider
        const label = getProviderLabel(provider)
        if (provider === 'manual') {
          return <Badge variant="outline">{label}</Badge>
        }
        const isUnattributed = !row.original.subscriptionId && !row.original.paymentAttemptId
        return (
          <div className="flex items-center gap-1.5">
            <Badge variant="secondary" className="bg-info/10 text-info border-info/20">
              {label}
            </Badge>
            {isUnattributed && (
              <Badge
                variant="outline"
                className="bg-warning/10 text-warning border-warning/20"
                data-testid={`invoice-unattributed-badge-${row.original.id}`}
              >
                {m['billing.invoice_attribution_unattributed_badge']()}
              </Badge>
            )}
          </div>
        )
      },
    },
    {
      accessorKey: 'status',
      header: m['common.status'](),
      cell: ({ row }) => <InvoiceStatusBadge status={row.getValue('status') as string} />,
    },
    {
      accessorKey: 'total',
      header: m['billing.invoice_total'](),
      cell: ({ row }) => {
        const total = row.getValue('total') as number
        const currency = row.original.currency
        return <span className="font-mono text-sm">{formatInvoiceAmount(total, currency)}</span>
      },
    },
    {
      accessorKey: 'amountRefunded',
      header: m['billing.invoice_refunded_column'](),
      cell: ({ row }) => <RefundAmountChip invoice={row.original} />,
    },
    {
      accessorKey: 'dueDate',
      header: m['billing.invoice_due_date'](),
      cell: ({ row }) => {
        const dueDate = row.getValue('dueDate') as string
        return <span className="text-sm">{formatDate(dueDate)}</span>
      },
    },
    {
      accessorKey: 'createdAt',
      header: m['billing.invoice_created_at'](),
      cell: ({ row }) => {
        const createdAt = row.getValue('createdAt') as string
        return <span className="text-sm">{formatDate(createdAt)}</span>
      },
    },
    {
      id: 'actions',
      header: m['billing.invoice_actions'](),
      cell: ({ row }) => {
        const invoice = row.original
        const availableActions = getAvailableActions(invoice.status, invoice.provider)
        return (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                className="h-8 w-8 p-0"
                data-testid={`invoice-actions-menu-${invoice.id}`}
              >
                <span className="sr-only">{m['billing.invoice_open_menu']()}</span>
                <MoreHorizontal className="h-4 w-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              {availableActions.includes('view') && (
                <DropdownMenuItem
                  onClick={() => handlers.onView?.(invoice)}
                  data-testid={`invoice-view-${invoice.id}`}
                >
                  <Eye className="mr-2 h-4 w-4" />
                  {m['billing.invoice_view']()}
                </DropdownMenuItem>
              )}
              {availableActions.includes('edit') && (
                <DropdownMenuItem
                  onClick={() => handlers.onEdit?.(invoice)}
                  data-testid={`invoice-edit-${invoice.id}`}
                >
                  <Pencil className="mr-2 h-4 w-4" />
                  {m['billing.invoice_edit']()}
                </DropdownMenuItem>
              )}
              {availableActions.includes('issue') && (
                <DropdownMenuItem
                  onClick={() => handlers.onIssue?.(invoice)}
                  data-testid={`invoice-issue-${invoice.id}`}
                >
                  <Send className="mr-2 h-4 w-4" />
                  {m['billing.invoice_issue']()}
                </DropdownMenuItem>
              )}
              {availableActions.includes('void') && (
                <>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem
                    onClick={() => handlers.onVoid?.(invoice)}
                    className="text-destructive"
                    data-testid={`invoice-void-${invoice.id}`}
                  >
                    <Ban className="mr-2 h-4 w-4" />
                    {m['billing.invoice_void']()}
                  </DropdownMenuItem>
                </>
              )}
              {availableActions.includes('markPaid') && (
                <DropdownMenuItem
                  onClick={() => handlers.onMarkPaid?.(invoice)}
                  data-testid={`invoice-mark-paid-${invoice.id}`}
                >
                  <CheckCircle className="mr-2 h-4 w-4" />
                  {m['billing.invoice_mark_paid']()}
                </DropdownMenuItem>
              )}
              {availableActions.includes('downloadPdf') && (
                <>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem
                    onClick={() =>
                      downloadInvoicePdf(
                        `/api/bill/invoices/${invoice.id}/pdf`,
                        `${invoice.invoiceNumber}.pdf`
                      )
                    }
                    data-testid={`invoice-download-pdf-${invoice.id}`}
                  >
                    <Download className="mr-2 h-4 w-4" />
                    {m['billing.invoice_download_pdf']()}
                  </DropdownMenuItem>
                </>
              )}
              {isExternalInvoice(invoice.provider) && getViewInProviderUrl(invoice) && (
                <>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem
                    onClick={() => {
                      window.open(getViewInProviderUrl(invoice)!, '_blank', 'noopener,noreferrer')
                    }}
                    data-testid={`invoice-view-provider-${invoice.id}`}
                  >
                    <ExternalLink className="mr-2 h-4 w-4" />
                    {m['billing.invoice_view_in_provider']({
                      provider: getProviderLabel(invoice.provider),
                    })}
                  </DropdownMenuItem>
                </>
              )}
            </DropdownMenuContent>
          </DropdownMenu>
        )
      },
    },
  ]
}

function getStatusOptions() {
  return [
    { value: 'all', label: m['billing.invoice_status_all']() },
    { value: 'draft', label: m['billing.invoice_status_draft']() },
    { value: 'issued', label: m['billing.invoice_status_issued']() },
    { value: 'paid', label: m['billing.invoice_status_paid']() },
    { value: 'overdue', label: m['billing.invoice_status_overdue']() },
    { value: 'void', label: m['billing.invoice_status_void']() },
  ]
}

function getSourceOptions() {
  return [
    { value: 'all', label: m['billing.invoice_source_all']() },
    { value: 'admin_manual', label: m['billing.invoice_source_manual']() },
    { value: 'user_application', label: m['billing.invoice_source_application']() },
  ]
}

function getProviderOptions() {
  return [
    { value: 'all', label: m['billing.invoice_provider_all']() },
    { value: 'manual', label: getProviderLabel('manual') },
    { value: 'stripe', label: getProviderLabel('stripe') },
    { value: 'creem', label: getProviderLabel('creem') },
  ]
}

function getAttributionOptions() {
  return [
    { value: 'all', label: m['billing.invoice_attribution_all']() },
    { value: 'missing', label: m['billing.invoice_attribution_missing']() },
  ]
}

function FilterBar({
  filters,
  onFiltersChange,
}: {
  filters: InvoiceFilters
  onFiltersChange: (filters: InvoiceFilters) => void
}) {
  return (
    <div className="flex flex-wrap items-center gap-3" data-testid="invoice-filter-bar">
      <Select
        value={filters.status ?? 'all'}
        onValueChange={(value) =>
          onFiltersChange({ ...filters, status: value === 'all' ? undefined : value })
        }
      >
        <SelectTrigger className="w-[160px]" data-testid="invoice-status-filter">
          <SelectValue placeholder={m['billing.invoice_status_all']()} />
        </SelectTrigger>
        <SelectContent>
          {getStatusOptions().map((opt) => (
            <SelectItem key={opt.value} value={opt.value}>
              {opt.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Select
        value={filters.source ?? 'all'}
        onValueChange={(value) =>
          onFiltersChange({ ...filters, source: value === 'all' ? undefined : value })
        }
      >
        <SelectTrigger className="w-[160px]" data-testid="invoice-source-filter">
          <SelectValue placeholder={m['billing.invoice_source_all']()} />
        </SelectTrigger>
        <SelectContent>
          {getSourceOptions().map((opt) => (
            <SelectItem key={opt.value} value={opt.value}>
              {opt.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Select
        value={filters.provider ?? 'all'}
        onValueChange={(value) =>
          onFiltersChange({ ...filters, provider: value === 'all' ? undefined : value })
        }
      >
        <SelectTrigger className="w-[160px]" data-testid="invoice-provider-filter">
          <SelectValue placeholder={m['billing.invoice_provider_all']()} />
        </SelectTrigger>
        <SelectContent>
          {getProviderOptions().map((opt) => (
            <SelectItem key={opt.value} value={opt.value}>
              {opt.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Select
        value={filters.attribution ?? 'all'}
        onValueChange={(value) =>
          onFiltersChange({ ...filters, attribution: value === 'all' ? undefined : value })
        }
      >
        <SelectTrigger className="w-[160px]" data-testid="invoice-attribution-filter">
          <SelectValue placeholder={m['billing.invoice_attribution_all']()} />
        </SelectTrigger>
        <SelectContent>
          {getAttributionOptions().map((opt) => (
            <SelectItem key={opt.value} value={opt.value}>
              {opt.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Input
        type="date"
        className="w-[150px]"
        value={filters.dateFrom ?? ''}
        onChange={(e) => onFiltersChange({ ...filters, dateFrom: e.target.value || undefined })}
        data-testid="invoice-date-from-filter"
      />
      <Input
        type="date"
        className="w-[150px]"
        value={filters.dateTo ?? ''}
        onChange={(e) => onFiltersChange({ ...filters, dateTo: e.target.value || undefined })}
        data-testid="invoice-date-to-filter"
      />

      <div className="relative">
        <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
        <Input
          placeholder={m['billing.invoice_search_placeholder']()}
          className="w-[220px] pl-9"
          value={filters.search ?? ''}
          onChange={(e) => onFiltersChange({ ...filters, search: e.target.value || undefined })}
          data-testid="invoice-search-input"
        />
      </div>
    </div>
  )
}

export function InvoiceAdminPage({
  realmId,
  onViewInvoice,
  onEditInvoice,
  onCreateInvoice,
  onOpenSellerConfig,
  onOpenPolicyConfig,
  onIssueInvoice,
  onVoidInvoice,
  onMarkPaidInvoice,
}: InvoiceAdminPageProps) {
  const realmContext = useResolvedRealmContext()
  const [filters, setFilters] = useState<InvoiceFilters>({})
  const [page, setPage] = useState(0)

  const queryFilters = {
    ...filters,
    page,
    pageSize: INVOICE_PAGE_SIZE,
  }

  const { data, isLoading, error } = useQuery(invoiceListQueryOptions(realmId, queryFilters))
  const { data: features } = useQuery(featureAvailabilityQueryOptions(realmId))
  const eligibility = features?.invoiceEligibility

  const invoices = data?.data ?? []
  const total = data?.total ?? 0

  // Pre-gate Create Invoice on realm-level eligibility returned by
  // feature-availability (policy + seller config), so users do not fill the
  // form only to be rejected by the backend on submit.
  const noPolicy = eligibility?.policy === 'none' || eligibility?.canCreateManualInvoice === false
  const noSeller = eligibility?.hasSellerConfig === false
  const createDisabled = !!onCreateInvoice && !!eligibility && (noPolicy || noSeller)
  const sellerConfigRoute = realmPath({ ...realmContext, realmId }, '/manage/billing/invoices')

  const handleFiltersChange = useCallback((newFilters: InvoiceFilters) => {
    setFilters(newFilters)
    setPage(0)
  }, [])

  const actionHandlers = useMemo(
    () => ({
      onView: onViewInvoice,
      onEdit: onEditInvoice,
      onIssue: onIssueInvoice,
      onVoid: onVoidInvoice,
      onMarkPaid: onMarkPaidInvoice,
    }),
    [onViewInvoice, onEditInvoice, onIssueInvoice, onVoidInvoice, onMarkPaidInvoice]
  )

  const columns = useMemo(
    () => createInvoiceColumns(realmId, actionHandlers, page, INVOICE_PAGE_SIZE),
    [realmId, actionHandlers, page]
  )

  return (
    <div className="space-y-6" data-testid="invoice-admin-page">
      <div className="flex items-start justify-between gap-4">
        <PageHeader title={m['billing.invoice_page_title']()} headingTestId="invoice-heading" />
        <div className="flex gap-2">
          {onOpenSellerConfig && (
            <Button
              variant="outline"
              onClick={onOpenSellerConfig}
              data-testid="seller-config-button"
            >
              <Settings className="mr-2 h-4 w-4" />
              {m['billing.invoice_seller_config']()}
            </Button>
          )}
          {onOpenPolicyConfig && (
            <Button
              variant="outline"
              onClick={onOpenPolicyConfig}
              data-testid="policy-config-button"
            >
              <Shield className="mr-2 h-4 w-4" />
              {m['billing.invoice_policy_title']()}
            </Button>
          )}
          {onCreateInvoice && (
            <Button
              onClick={onCreateInvoice}
              data-testid="create-invoice-button"
              disabled={createDisabled}
            >
              <Plus className="mr-2 h-4 w-4" />
              {m['billing.invoice_create_button']()}
            </Button>
          )}
        </div>
      </div>

      {onCreateInvoice && createDisabled && (
        <div
          className="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm text-muted-foreground"
          data-testid="create-invoice-disabled-reason"
        >
          {noPolicy ? (
            <span>{m['billing.invoice_no_policy_disabled']()}</span>
          ) : (
            <>
              <span>{m['billing.invoice_create_disabled_no_seller']()}</span>
              <Link
                to={sellerConfigRoute}
                search={{ open: 'seller' }}
                className="text-primary underline-offset-4 hover:underline"
                data-testid="create-invoice-configure-link"
              >
                {m['billing.invoice_create_configure_link']()}
              </Link>
            </>
          )}
        </div>
      )}

      <FilterBar filters={filters} onFiltersChange={handleFiltersChange} />

      <Card>
        <CardHeader>
          <CardTitle>{m['billing.invoice_list_title']()}</CardTitle>
        </CardHeader>
        <CardContent>
          <DataTable
            columns={columns}
            data={invoices}
            isLoading={isLoading}
            error={error instanceof Error ? error : error ? new Error(String(error)) : undefined}
            loadingMessage={m['billing.invoice_loading']()}
            emptyMessage={m['billing.invoice_empty']()}
            data-testid="invoice-table"
          />
        </CardContent>
      </Card>

      {total > 0 && (
        <ListPagination
          page={page}
          pageSize={INVOICE_PAGE_SIZE}
          total={total}
          onPageChange={setPage}
          testIdPrefix="invoice-pagination"
        />
      )}
    </div>
  )
}
