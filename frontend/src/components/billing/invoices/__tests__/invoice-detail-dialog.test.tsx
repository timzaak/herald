/**
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, within } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { InvoiceDetailDialog } from '../invoice-detail-dialog'
import type { InvoiceDetailResponse, CreditNoteResponse } from '@/lib/api-generated'
import { server } from '@/test/mocks/server'

// ==================== Mocks ====================

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
// attribution-link testid stays queryable. Mirrors invoice-admin-page.test.tsx.
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

// ==================== Test Helpers ====================

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

function makeInvoiceDetail(overrides: Partial<InvoiceDetailResponse> = {}): InvoiceDetailResponse {
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
    status: 'draft',
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
      {
        id: 'li-2',
        invoiceId: INVOICE_ID,
        name: 'Service B',
        description: null,
        quantity: '1',
        unitPrice: 10000,
        subtotal: 10000,
        sortOrder: 1,
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
      {
        id: 'hist-2',
        invoiceId: INVOICE_ID,
        eventType: 'updated',
        actorType: 'user',
        actorUserId: 'user-1',
        changes: { field: 'status', from: 'draft', to: 'review' },
        createdAt: '2025-05-15T14:00:00Z',
      },
      {
        id: 'hist-3',
        invoiceId: INVOICE_ID,
        eventType: 'issued',
        actorType: 'system',
        actorUserId: null,
        changes: null,
        createdAt: '2025-06-01T12:00:00Z',
      },
    ],
    subscriptionId: null,
    paymentAttemptId: null,
    paidAt: null,
    voidReason: null,
    voidedAt: null,
    realmId: REALM_ID,
    amountRefunded: 0,
    amountRemaining: 0,
    creditNotes: [],
    provider: 'manual',
    ...overrides,
  }
}

const defaultOnOpenChange = vi.fn()

function setupDetailDialog(
  invoiceOverrides: Partial<InvoiceDetailResponse> = {},
  props: { open?: boolean; invoiceId?: string | null; variant?: 'admin' | 'user' } = {}
) {
  const invoice = makeInvoiceDetail(invoiceOverrides)
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

// ==================== Tests ====================

describe('InvoiceDetailDialog', () => {
  beforeEach(() => {
    defaultOnOpenChange.mockClear()
  })

  // ==================== Attribution Block (by field presence) ====================

  describe('attribution block by field presence', () => {
    // The dialog's useQuery gates InvoiceContent (and thus the Attribution
    // sub-component) behind isLoading. invoice-detail-dialog is present even
    // during the skeleton phase, so we must wait on a post-load element
    // (line-items section) before asserting attribution presence/absence.
    const waitForInvoiceLoaded = () =>
      waitFor(() => {
        expect(screen.getByTestId('invoice-line-items-section')).toBeInTheDocument()
      })

    it('renders section, subscription link, and payment attempt when both fields present', async () => {
      setupDetailDialog({
        subscriptionId: 'sub-1',
        paymentAttemptId: 'pay-1',
      })

      await waitForInvoiceLoaded()

      expect(screen.getByTestId('invoice-attribution-section')).toBeInTheDocument()
      expect(screen.getByTestId('invoice-attribution-subscription-link')).toBeInTheDocument()
      expect(screen.getByTestId('invoice-attribution-payment-attempt')).toBeInTheDocument()
    })

    it('hides the section when both subscriptionId and paymentAttemptId are null', async () => {
      setupDetailDialog()

      await waitForInvoiceLoaded()

      expect(screen.queryByTestId('invoice-attribution-section')).not.toBeInTheDocument()
    })

    it('renders only the subscription link when subscriptionId is set and paymentAttemptId is null', async () => {
      setupDetailDialog({ subscriptionId: 'sub-1' })

      await waitForInvoiceLoaded()

      expect(screen.getByTestId('invoice-attribution-subscription-link')).toBeInTheDocument()
      expect(screen.queryByTestId('invoice-attribution-payment-attempt')).not.toBeInTheDocument()
    })

    it('renders only the payment attempt when paymentAttemptId is set and subscriptionId is null', async () => {
      setupDetailDialog({ paymentAttemptId: 'pay-1' })

      await waitForInvoiceLoaded()

      expect(screen.getByTestId('invoice-attribution-payment-attempt')).toBeInTheDocument()
      expect(screen.queryByTestId('invoice-attribution-subscription-link')).not.toBeInTheDocument()
    })
  })

  // ==================== Status History ====================

  describe('status history', () => {
    it('renders chronological events with timestamp and event type', async () => {
      setupDetailDialog()

      await waitFor(() => {
        expect(screen.getByTestId('invoice-status-history')).toBeInTheDocument()
      })

      expect(screen.getByText('Status History')).toBeInTheDocument()

      // Events sorted oldest-first: created, updated, issued
      expect(screen.getByText('Created')).toBeInTheDocument()
      expect(screen.getByText('Updated')).toBeInTheDocument()
      expect(screen.getByText('Issued')).toBeInTheDocument()

      // Actor labels: "User" for created and updated, "System" for issued
      const userLabels = screen.getAllByText('User')
      expect(userLabels).toHaveLength(2)
      expect(screen.getByText('System')).toBeInTheDocument()
    })

    it('renders history with change description when changes have field/from/to', async () => {
      setupDetailDialog({
        history: [
          {
            id: 'hist-change',
            invoiceId: INVOICE_ID,
            eventType: 'updated',
            actorType: 'user',
            actorUserId: 'user-1',
            changes: { field: 'status', from: 'draft', to: 'issued' },
            createdAt: '2025-06-01T11:00:00Z',
          },
        ],
      })

      await waitFor(() => {
        expect(screen.getByTestId('invoice-status-history')).toBeInTheDocument()
      })

      expect(screen.getByText('Updated')).toBeInTheDocument()
      expect(screen.getByText('status: draft -> issued')).toBeInTheDocument()
    })

    it('renders "No history events" when history is empty', async () => {
      setupDetailDialog({ history: [] })

      await waitFor(() => {
        expect(screen.getByTestId('invoice-status-history')).toBeInTheDocument()
      })

      expect(screen.getByText('No history events')).toBeInTheDocument()
    })
  })

  // ==================== PDF Download Button Visibility ====================

  describe('PDF download button per status', () => {
    it('visible for issued status', async () => {
      setupDetailDialog({ status: 'issued' })

      await waitFor(() => {
        expect(screen.getByTestId('invoice-download-pdf-button')).toBeInTheDocument()
      })

      expect(screen.getByText('Download PDF')).toBeInTheDocument()
    })

    it('visible for paid status', async () => {
      setupDetailDialog({ status: 'paid' })

      await waitFor(() => {
        expect(screen.getByTestId('invoice-download-pdf-button')).toBeInTheDocument()
      })
    })

    it('visible for overdue status', async () => {
      setupDetailDialog({ status: 'overdue' })

      await waitFor(() => {
        expect(screen.getByTestId('invoice-download-pdf-button')).toBeInTheDocument()
      })
    })

    it('NOT visible for draft status', async () => {
      setupDetailDialog({ status: 'draft' })

      await waitFor(() => {
        expect(screen.getByTestId('invoice-detail-dialog')).toBeInTheDocument()
      })

      expect(screen.queryByTestId('invoice-download-pdf-button')).not.toBeInTheDocument()
    })

    it('NOT visible for void status', async () => {
      setupDetailDialog({ status: 'void' })

      await waitFor(() => {
        expect(screen.getByTestId('invoice-detail-dialog')).toBeInTheDocument()
      })

      expect(screen.queryByTestId('invoice-download-pdf-button')).not.toBeInTheDocument()
    })

    it('PDF link href contains correct path', async () => {
      setupDetailDialog({ status: 'issued' })

      const pdfButton = await screen.findByTestId('invoice-download-pdf-button')
      const link = pdfButton.closest('a')
      expect(link?.getAttribute('href')).toBe(`/api/bill/invoices/${INVOICE_ID}/pdf`)
    })
  })

  // ==================== Additional Info ====================

  describe('additional info section', () => {
    it('renders issue date, due date, payment terms, and notes', async () => {
      setupDetailDialog()

      await waitFor(() => {
        expect(screen.getByTestId('invoice-additional-info')).toBeInTheDocument()
      })

      const section = within(screen.getByTestId('invoice-additional-info'))

      // Labels
      expect(section.getByText('Issue Date')).toBeInTheDocument()
      expect(section.getByText('Due Date')).toBeInTheDocument()
      expect(section.getByText('Payment Terms')).toBeInTheDocument()
      expect(section.getByText('Additional Information')).toBeInTheDocument()

      // Values
      expect(section.getByText('Net 30')).toBeInTheDocument()
      expect(section.getByText('Test notes')).toBeInTheDocument()
    })
  })

  // ==================== Loading State ====================

  describe('loading state', () => {
    it('shows skeleton placeholders while fetching detail', async () => {
      server.use(
        http.get(`${BASE_URL}/api/bill/invoices/${INVOICE_ID}`, async () => {
          await new Promise((resolve) => setTimeout(resolve, 500))
          return HttpResponse.json(makeInvoiceDetail())
        })
      )

      renderWithProviders(
        <InvoiceDetailDialog
          open
          onOpenChange={defaultOnOpenChange}
          realmId={REALM_ID}
          invoiceId={INVOICE_ID}
        />
      )

      // Dialog should be present while loading
      expect(screen.getByTestId('invoice-detail-dialog')).toBeInTheDocument()

      // Skeleton placeholders in header
      const skeletons = document.querySelectorAll('[data-slot="skeleton"]')
      expect(skeletons.length).toBeGreaterThan(0)

      // Wait for loading to complete (seller info only appears after data loads)
      await waitFor(() => {
        expect(screen.getByTestId('invoice-seller-info')).toBeInTheDocument()
      })
    })
  })

  // ==================== Refund Summary & Credit Note List Rendering Gates ====================

  describe('refund summary and credit note list rendering gates', () => {
    // The refund dimension only makes sense for providers where Herald keeps
    // refund evidence (manual / stripe). Creem acts as Merchant of Record and
    // stores refunds outside Herald, so the breakdown/list must be omitted.
    const waitForInvoiceLoaded = () =>
      waitFor(() => {
        expect(screen.getByTestId('invoice-line-items-section')).toBeInTheDocument()
      })

    it.each([
      { provider: 'stripe', amountRefunded: 5000, amountRemaining: 16000, label: 'stripe' },
      { provider: 'manual', amountRefunded: 3000, amountRemaining: 18000, label: 'manual' },
    ])(
      'renders refunded/remaining breakdown when provider is $label with refund',
      async ({ provider, amountRefunded, amountRemaining }) => {
        setupDetailDialog({
          provider,
          amountRefunded,
          amountRemaining,
          total: 21000,
        })

        expect(await screen.findByTestId('invoice-refund-summary')).toBeInTheDocument()
        expect(screen.getByTestId('invoice-refunded-amount')).toBeInTheDocument()
        expect(screen.getByTestId('invoice-remaining-amount')).toBeInTheDocument()
      }
    )

    it('does NOT render breakdown when provider is creem (MoR excludes refund dimension)', async () => {
      setupDetailDialog({
        provider: 'creem',
        amountRefunded: 5000,
        amountRemaining: 16000,
      })

      await waitForInvoiceLoaded()

      expect(screen.queryByTestId('invoice-refund-summary')).not.toBeInTheDocument()
    })

    it('does NOT render breakdown when amountRefunded is 0', async () => {
      setupDetailDialog({
        provider: 'stripe',
        amountRefunded: 0,
        amountRemaining: 21000,
      })

      await waitForInvoiceLoaded()

      expect(screen.queryByTestId('invoice-refund-summary')).not.toBeInTheDocument()
    })

    it('renders manual and stripe tracks separated by source', async () => {
      const manualNote = makeCreditNote({ id: 'cn-manual-1', source: 'manual' })
      const stripeNote = makeCreditNote({ id: 'cn-stripe-1', source: 'stripe' })

      setupDetailDialog({
        provider: 'manual',
        amountRefunded: 8000,
        amountRemaining: 13000,
        creditNotes: [manualNote, stripeNote],
      })

      expect(await screen.findByTestId('credit-note-list')).toBeInTheDocument()
      expect(screen.getByTestId('credit-note-list-manual')).toBeInTheDocument()
      expect(screen.getByTestId('credit-note-list-stripe')).toBeInTheDocument()
      expect(screen.getByTestId(`credit-note-row-${manualNote.id}`)).toBeInTheDocument()
      expect(screen.getByTestId(`credit-note-row-${stripeNote.id}`)).toBeInTheDocument()
    })

    it('marks voided credit note row with voided testid inline (audit retention)', async () => {
      // Voided Stripe credit notes remain visible as audit evidence rather than
      // being removed from the list; the testid is the stable contract.
      const voidedNote = makeCreditNote({
        id: 'cn-voided-1',
        source: 'stripe',
        status: 'voided',
      })

      setupDetailDialog({
        provider: 'stripe',
        amountRefunded: 0,
        amountRemaining: 21000,
        creditNotes: [voidedNote],
      })

      await waitForInvoiceLoaded()
      expect(screen.queryByTestId('invoice-refund-summary')).not.toBeInTheDocument()
      expect(await screen.findByTestId(`credit-note-voided-${voidedNote.id}`)).toBeInTheDocument()
    })

    it('renders over-total alert when amountRefunded exceeds total (stripe anomaly)', async () => {
      // Cumulative refunds exceeding the total can only happen as a Stripe-side
      // anomaly; manual creation rejects over-remaining refunds upfront.
      setupDetailDialog({
        provider: 'stripe',
        amountRefunded: 25000,
        total: 21000,
        amountRemaining: 0,
      })

      expect(await screen.findByTestId('invoice-refund-over-total-alert')).toBeInTheDocument()
    })

    it('does NOT render over-total alert when within total', async () => {
      setupDetailDialog({
        provider: 'stripe',
        amountRefunded: 5000,
        total: 21000,
        amountRemaining: 16000,
      })

      await waitForInvoiceLoaded()
      expect(screen.getByTestId('invoice-refund-summary')).toBeInTheDocument()

      expect(screen.queryByTestId('invoice-refund-over-total-alert')).not.toBeInTheDocument()
    })

    it('hides credit note list and note ids for user role (prop-driven trimming)', async () => {
      // Regular users should not see internal credit note numbers, operators,
      // or the record-refund entry point; the Dialog hides them via prop.
      const manualNote = makeCreditNote({ id: 'cn-user-1', source: 'manual' })
      const stripeNote = makeCreditNote({ id: 'cn-user-stripe-1', source: 'stripe' })

      setupDetailDialog(
        {
          provider: 'manual',
          amountRefunded: 5000,
          amountRemaining: 16000,
          creditNotes: [manualNote, stripeNote],
        },
        { variant: 'user' }
      )

      await waitForInvoiceLoaded()
      // Users still see the high-level refund breakdown per the user detail spec.
      expect(screen.getByTestId('invoice-refund-summary')).toBeInTheDocument()

      expect(screen.queryByTestId('credit-note-list')).not.toBeInTheDocument()
      expect(screen.queryByTestId('credit-note-list-manual')).not.toBeInTheDocument()
      expect(screen.queryByTestId('credit-note-list-stripe')).not.toBeInTheDocument()
    })
  })

  // ==================== Not Found / Error State ====================

  describe('invoice not found', () => {
    it('shows "Invoice Not Found" when API returns 404', async () => {
      server.use(
        http.get(`${BASE_URL}/api/bill/invoices/${INVOICE_ID}`, () => {
          return HttpResponse.json({ message: 'Invoice not found' }, { status: 404 })
        })
      )

      renderWithProviders(
        <InvoiceDetailDialog
          open
          onOpenChange={defaultOnOpenChange}
          realmId={REALM_ID}
          invoiceId={INVOICE_ID}
        />
      )

      // Wait for the error state to render (query retries once, then fails)
      await waitFor(
        () => {
          expect(screen.getByText('Invoice Not Found')).toBeInTheDocument()
        },
        { timeout: 5000 }
      )

      expect(screen.getByText('The requested invoice could not be loaded.')).toBeInTheDocument()
    })

    it('shows "Invoice Not Found" when invoiceId is null', () => {
      renderWithProviders(
        <InvoiceDetailDialog
          open
          onOpenChange={defaultOnOpenChange}
          realmId={REALM_ID}
          invoiceId={null}
        />
      )

      // When invoiceId is null, the query is disabled (enabled: open && !!invoiceId).
      // isLoading stays false and invoice is undefined -> falls into the "not found" branch.
      expect(screen.getByText('Invoice Not Found')).toBeInTheDocument()
    })
  })
})
