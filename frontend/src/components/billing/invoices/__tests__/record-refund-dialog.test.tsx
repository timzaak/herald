/**
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { InvoiceDetailDialog } from '../invoice-detail-dialog'
import { RecordRefundDialog } from '../record-refund-dialog'
import type { InvoiceDetailResponse, CreditNoteResponse } from '@/lib/api-generated'
import { server } from '@/test/mocks/server'
import { usePermission } from '@/hooks/use-permission'

// Button uses Radix Slot when asChild=true. Slot.Children.only(null) throws
// in dev mode when Button renders {isLoading && <Spinner />}{children} because
// the boolean `false` counts as a child alongside the <a> element.
// Mock Button to avoid the Slot crash while keeping functional behavior.
vi.mock('@/components/ui/button', () => {
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const React = require('react')

  function Button(props: any) {
    const {
      children,
      asChild,
      className,
      variant,
      size,
      loading,
      disabled,
      'data-testid': dataTestId,
      ...rest
    } = props

    if (asChild && React.isValidElement(children)) {
      return React.cloneElement(children, {
        className: [className, children.props.className].filter(Boolean).join(' '),
        'data-testid': dataTestId ?? children.props['data-testid'],
        ...rest,
      })
    }

    return React.createElement(
      'button',
      {
        className,
        'data-testid': dataTestId,
        disabled: disabled || loading,
        ...rest,
      },
      loading && React.createElement('span', null, 'Loading...'),
      children
    )
  }

  return { Button, buttonVariants: () => '' }
})

// The dialog imports Link from @tanstack/react-router. Link needs a router
// context that JSDOM does not provide; render a plain <a> so the
// attribution-link testid stays queryable. Mirrors invoice-detail-dialog.test.tsx.
vi.mock('@tanstack/react-router', () => ({
  Link: ({
    to,
    params,
    search,
    children,
    ...props
  }: {
    to?: string
    params?: Record<string, unknown>
    search?: Record<string, unknown>
    children?: React.ReactNode
  }) => {
    let href = typeof to === 'string' ? to : ''
    if (params) {
      for (const [k, v] of Object.entries(params)) {
        href = href.replace(`$${k}`, String(v))
      }
    }
    if (search && Object.keys(search).length) {
      href += '?' + new URLSearchParams(search as Record<string, string>).toString()
    }
    return (
      <a href={href} {...props}>
        {children}
      </a>
    )
  },
}))

// Control billing.manage permission for the record-refund button gate.
vi.mock('@/hooks/use-permission', () => ({
  usePermission: vi.fn(),
}))

const REALM_ID = 'test-realm'
const INVOICE_ID = 'inv-1'
const BASE_URL = 'http://localhost:3000'

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false },
    },
  })
}

function renderWithProviders(ui: React.ReactElement, queryClient?: QueryClient) {
  const qc = queryClient ?? createTestQueryClient()
  return render(<QueryClientProvider client={qc}>{ui}</QueryClientProvider>)
}

function makeInvoiceForRefund(
  overrides: Partial<InvoiceDetailResponse> = {}
): InvoiceDetailResponse {
  return {
    id: INVOICE_ID,
    invoiceNumber: 'INV-001',
    accountId: 'acc-1',
    applicantUserId: null,
    billingName: 'Test Buyer',
    billingEmail: 'buyer@test.com',
    billingAddress: '123 Buyer St',
    billingPhone: '111-222-3333',
    billingTaxId: 'TAX123',
    sellerName: 'Test Seller',
    sellerEmail: 'seller@test.com',
    sellerAddress: '456 Seller Ave',
    sellerPhone: '444-555-6666',
    sellerTaxId: 'TAX456',
    currency: 'CNY',
    source: 'admin_manual',
    status: 'paid',
    subtotal: 20000,
    discountAmount: 1000,
    discountMode: null,
    discountValue: null,
    taxAmount: 1500,
    taxMode: null,
    taxValue: null,
    shippingAmount: 500,
    shippingMode: null,
    shippingValue: null,
    total: 21000,
    dueDate: '2025-07-01T00:00:00Z',
    paymentTerms: 'Net 30',
    notes: 'Test notes',
    issueDate: '2025-06-01T00:00:00Z',
    issuedAt: null,
    createdAt: '2025-05-01T00:00:00Z',
    updatedAt: '2025-06-01T00:00:00Z',
    lineItems: [
      {
        id: 'li-1',
        invoiceId: INVOICE_ID,
        name: 'Service A',
        description: 'Service A description',
        quantity: '2',
        unitPrice: 5000,
        subtotal: 10000,
        sortOrder: 0,
      },
    ],
    history: [
      {
        id: 'hist-1',
        invoiceId: INVOICE_ID,
        eventType: 'created',
        actorType: 'user',
        actorUserId: 'user-1',
        changes: null,
        createdAt: '2025-05-01T10:00:00Z',
      },
    ],
    subscriptionId: null,
    paymentAttemptId: null,
    paidAt: '2025-06-15T00:00:00Z',
    voidReason: null,
    voidedAt: null,
    realmId: REALM_ID,
    amountRefunded: 0,
    amountRemaining: 21000,
    creditNotes: [],
    provider: 'manual',
    ...overrides,
  }
}

function makeCreditNote(overrides: Partial<CreditNoteResponse> = {}): CreditNoteResponse {
  return {
    id: 'cn-1',
    amount: 5000,
    currency: 'CNY',
    source: 'manual',
    status: 'active',
    createdAt: '2025-06-01T00:00:00Z',
    ...overrides,
  }
}

function mockHasPermission(granted: boolean) {
  vi.mocked(usePermission).mockReturnValue({
    hasPermission: () => granted,
    hasAnyPermission: vi.fn(() => granted),
    hasAllPermissions: vi.fn(() => granted),
    hasRole: vi.fn(() => false),
    hasAnyRole: vi.fn(() => false),
    hasAdminPermission: granted,
    permissions: granted ? ['billing.manage'] : [],
    roles: [],
    isLoading: false,
  } as ReturnType<typeof usePermission>)
}

const defaultOnOpenChange = vi.fn()

function setupDetailDialog(
  invoiceOverrides: Partial<InvoiceDetailResponse> = {},
  props: { open?: boolean; invoiceId?: string | null; variant?: 'admin' | 'user' } = {}
) {
  const invoice = makeInvoiceForRefund(invoiceOverrides)
  const variant = props.variant ?? 'admin'
  const endpoint =
    variant === 'user'
      ? `${BASE_URL}/api/user/bill/invoices/${invoice.id}`
      : `${BASE_URL}/api/bill/invoices/${invoice.id}`

  server.use(
    http.get(endpoint, () => {
      return HttpResponse.json(invoice)
    })
  )

  return renderWithProviders(
    <InvoiceDetailDialog
      open={props.open ?? true}
      onOpenChange={defaultOnOpenChange}
      realmId={REALM_ID}
      invoiceId={props.invoiceId ?? invoice.id}
      variant={variant}
    />
  )
}

// ==================== Tests ====================

describe('record refund button visibility gate', () => {
  beforeEach(() => {
    defaultOnOpenChange.mockClear()
  })

  // Manual credit notes are irreversible (no UI undo), so the trigger button
  // is strictly gated rather than disabled. Any unmet condition hides it.
  it.each([
    {
      name: 'manual+paid+billing.manage renders button (irreversible action needs strict gate)',
      provider: 'manual',
      status: 'paid',
      canManage: true,
      expectButton: true,
      amountRefunded: 0,
    },
    {
      name: 'stripe provider hides button even when paid and manager (manual only)',
      provider: 'stripe',
      status: 'paid',
      canManage: true,
      expectButton: false,
      amountRefunded: 5000,
    },
    {
      name: 'creem provider hides button (MoR refunds live outside Herald)',
      provider: 'creem',
      status: 'paid',
      canManage: true,
      expectButton: false,
      amountRefunded: 5000,
    },
    {
      name: 'manual but not paid hides button (only paid invoices can be refunded)',
      provider: 'manual',
      status: 'issued',
      canManage: true,
      expectButton: false,
      amountRefunded: 5000,
    },
    {
      name: 'manual+paid but no billing.manage hides button (permission gate)',
      provider: 'manual',
      status: 'paid',
      canManage: false,
      expectButton: false,
      amountRefunded: 5000,
      amountRemaining: 16000,
    },
    {
      name: 'manual+paid but fully refunded hides button (no refundable amount remains)',
      provider: 'manual',
      status: 'paid',
      canManage: true,
      expectButton: false,
      amountRefunded: 21000,
      amountRemaining: 0,
    },
  ])(
    '$name',
    async ({
      provider,
      status,
      canManage,
      expectButton,
      amountRefunded,
      amountRemaining = 16000,
    }) => {
      mockHasPermission(canManage)

      setupDetailDialog({
        provider,
        status,
        amountRefunded,
        amountRemaining,
        total: 21000,
      })

      await waitFor(() => {
        expect(screen.getByTestId('invoice-line-items-section')).toBeInTheDocument()
      })

      const button = screen.queryByTestId('record-refund-button')
      if (expectButton) {
        expect(button).toBeInTheDocument()
      } else {
        expect(button).not.toBeInTheDocument()
      }
    }
  )
})

describe('record refund dialog submission', () => {
  beforeEach(() => {
    defaultOnOpenChange.mockClear()
  })

  it('closes dialog and invalidates detail on successful submit', async () => {
    const user = userEvent.setup()
    let requestBody: unknown = null
    let postCalled = false

    server.use(
      http.post(`${BASE_URL}/api/bill/invoices/${INVOICE_ID}/credit-notes`, async ({ request }) => {
        postCalled = true
        requestBody = await request.json()
        return HttpResponse.json(makeCreditNote(), { status: 201 })
      })
    )

    const queryClient = createTestQueryClient()
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries')

    renderWithProviders(
      <RecordRefundDialog
        open
        onOpenChange={defaultOnOpenChange}
        realmId={REALM_ID}
        invoice={makeInvoiceForRefund({ amountRemaining: 21000 })}
      />,
      queryClient
    )

    await user.type(screen.getByTestId('record-refund-amount-input'), '50.00')
    await user.type(screen.getByTestId('record-refund-reason-input'), 'Partial refund')
    await user.click(screen.getByTestId('record-refund-submit-button'))

    await waitFor(() => {
      expect(postCalled).toBe(true)
    })

    // Amount is sent in cents; display value 50.00 -> 5000.
    expect(requestBody).toEqual({ amount: 5000, memo: 'Partial refund' })
    expect(defaultOnOpenChange).toHaveBeenCalledWith(false)

    // Success invalidates the detail query so breakdown + list refresh automatically.
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ['invoices', REALM_ID, 'detail', INVOICE_ID],
    })
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ['invoices', REALM_ID] })
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ['invoices', REALM_ID, 'my', 'detail', INVOICE_ID],
    })
  })

  it('keeps dialog open and shows inline error when API returns 400 over-remaining', async () => {
    const user = userEvent.setup()

    server.use(
      http.post(`${BASE_URL}/api/bill/invoices/${INVOICE_ID}/credit-notes`, () => {
        return HttpResponse.json(
          { message: 'Refund amount exceeds remaining payable' },
          { status: 400 }
        )
      })
    )

    renderWithProviders(
      <RecordRefundDialog
        open
        onOpenChange={defaultOnOpenChange}
        realmId={REALM_ID}
        invoice={makeInvoiceForRefund({ amountRemaining: 21000 })}
      />
    )

    await user.type(screen.getByTestId('record-refund-amount-input'), '50.00')
    await user.type(screen.getByTestId('record-refund-reason-input'), 'Partial refund')
    await user.click(screen.getByTestId('record-refund-submit-button'))

    const error = await screen.findByTestId('record-refund-error-message')
    expect(error).toHaveTextContent('Refund amount exceeds remaining payable')
    expect(defaultOnOpenChange).not.toHaveBeenCalled()
  })

  it('keeps dialog open and shows inline error when API returns 403 provider mismatch', async () => {
    const user = userEvent.setup()

    server.use(
      http.post(`${BASE_URL}/api/bill/invoices/${INVOICE_ID}/credit-notes`, () => {
        return HttpResponse.json(
          { message: 'Only manual invoices support credit notes' },
          { status: 403 }
        )
      })
    )

    renderWithProviders(
      <RecordRefundDialog
        open
        onOpenChange={defaultOnOpenChange}
        realmId={REALM_ID}
        invoice={makeInvoiceForRefund({ amountRemaining: 21000 })}
      />
    )

    await user.type(screen.getByTestId('record-refund-amount-input'), '50.00')
    await user.type(screen.getByTestId('record-refund-reason-input'), 'Partial refund')
    await user.click(screen.getByTestId('record-refund-submit-button'))

    const error = await screen.findByTestId('record-refund-error-message')
    expect(error).toHaveTextContent('Only manual invoices support credit notes')
    expect(defaultOnOpenChange).not.toHaveBeenCalled()
  })

  it('keeps dialog open and shows inline error when API returns 400 not paid', async () => {
    const user = userEvent.setup()

    server.use(
      http.post(`${BASE_URL}/api/bill/invoices/${INVOICE_ID}/credit-notes`, () => {
        return HttpResponse.json({ message: 'Invoice is not paid' }, { status: 400 })
      })
    )

    renderWithProviders(
      <RecordRefundDialog
        open
        onOpenChange={defaultOnOpenChange}
        realmId={REALM_ID}
        invoice={makeInvoiceForRefund({ amountRemaining: 21000 })}
      />
    )

    await user.type(screen.getByTestId('record-refund-amount-input'), '50.00')
    await user.type(screen.getByTestId('record-refund-reason-input'), 'Partial refund')
    await user.click(screen.getByTestId('record-refund-submit-button'))

    const error = await screen.findByTestId('record-refund-error-message')
    expect(error).toHaveTextContent('Invoice is not paid')
    expect(defaultOnOpenChange).not.toHaveBeenCalled()
  })

  it('blocks submit when amount exceeds remaining payable (frontend guard)', async () => {
    const user = userEvent.setup()
    let postCalled = false

    server.use(
      http.post(`${BASE_URL}/api/bill/invoices/${INVOICE_ID}/credit-notes`, async () => {
        postCalled = true
        return HttpResponse.json(makeCreditNote(), { status: 201 })
      })
    )

    renderWithProviders(
      <RecordRefundDialog
        open
        onOpenChange={defaultOnOpenChange}
        realmId={REALM_ID}
        invoice={makeInvoiceForRefund({ amountRemaining: 5000 })}
      />
    )

    // amountRemaining is 5000 cents = 50.00 display; 100.00 exceeds it.
    await user.type(screen.getByTestId('record-refund-amount-input'), '100.00')
    await user.type(screen.getByTestId('record-refund-reason-input'), 'Over refund')
    await user.click(screen.getByTestId('record-refund-submit-button'))

    // The input max attribute (and the JS fallback guard) prevent submission;
    // no network request is sent and the dialog stays open.
    expect(postCalled).toBe(false)
    expect(defaultOnOpenChange).not.toHaveBeenCalled()
  })

  it('cancel button closes dialog without submission', async () => {
    const user = userEvent.setup()
    let postCalled = false

    server.use(
      http.post(`${BASE_URL}/api/bill/invoices/${INVOICE_ID}/credit-notes`, async () => {
        postCalled = true
        return HttpResponse.json(makeCreditNote(), { status: 201 })
      })
    )

    renderWithProviders(
      <RecordRefundDialog
        open
        onOpenChange={defaultOnOpenChange}
        realmId={REALM_ID}
        invoice={makeInvoiceForRefund()}
      />
    )

    await user.click(screen.getByTestId('record-refund-cancel-button'))

    expect(defaultOnOpenChange).toHaveBeenCalledWith(false)
    expect(postCalled).toBe(false)
  })
})
