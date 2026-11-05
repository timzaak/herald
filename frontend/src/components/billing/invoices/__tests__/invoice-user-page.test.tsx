/**
 * @vitest-environment jsdom
 */

import { describe, it, expect, beforeEach, vi } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { InvoiceUserPage } from '../invoice-user-page'
import type { InvoiceResponse, InvoiceListResponse } from '@/lib/api-generated'
import { server } from '@/test/mocks/server'
import { renderWithProviders } from '@/test/utils/render'

// ==================== Test Helpers ====================

const REALM_ID = 'test-realm'
const BASE_URL = 'http://localhost:3000'

function makeInvoice(overrides: Partial<InvoiceResponse> = {}): InvoiceResponse {
  return {
    id: 'inv-1',
    invoiceNumber: 'INV-001',
    accountId: 'acc-1',
    billingName: 'Test Buyer',
    source: 'user_application',
    provider: 'manual',
    status: 'issued',
    total: 9900,
    amountRefunded: 0,
    currency: 'CNY',
    dueDate: '2025-06-01T00:00:00Z',
    createdAt: '2025-05-01T00:00:00Z',
    ...overrides,
  }
}

function makeListResponse(
  invoices: InvoiceResponse[],
  overrides: Partial<InvoiceListResponse> = {}
): InvoiceListResponse {
  return {
    data: invoices,
    page: 0,
    pageSize: 20,
    total: invoices.length,
    ...overrides,
  }
}

// ==================== Tests ====================

describe('InvoiceUserPage', () => {
  const defaultInvoices: InvoiceResponse[] = [
    makeInvoice({
      id: 'inv-1',
      invoiceNumber: 'INV-001',
      status: 'issued',
      total: 9900,
      currency: 'CNY',
      dueDate: '2025-06-01T00:00:00Z',
      createdAt: '2025-05-01T00:00:00Z',
    }),
    makeInvoice({
      id: 'inv-2',
      invoiceNumber: 'INV-002',
      status: 'paid',
      total: 15000,
      currency: 'USD',
      dueDate: '2025-06-15T00:00:00Z',
      createdAt: '2025-05-05T00:00:00Z',
    }),
    makeInvoice({
      id: 'inv-3',
      invoiceNumber: 'INV-003',
      status: 'overdue',
      total: 20000,
      currency: 'CNY',
      dueDate: '2025-05-01T00:00:00Z',
      createdAt: '2025-04-01T00:00:00Z',
    }),
    makeInvoice({
      id: 'inv-4',
      invoiceNumber: 'INV-004',
      status: 'draft',
      total: 5000,
      currency: 'CNY',
      dueDate: '2025-07-01T00:00:00Z',
      createdAt: '2025-06-01T00:00:00Z',
    }),
    makeInvoice({
      id: 'inv-5',
      invoiceNumber: 'INV-005',
      status: 'void',
      total: 3000,
      currency: 'CNY',
      dueDate: '2025-01-15T00:00:00Z',
      createdAt: '2025-01-01T00:00:00Z',
    }),
  ]

  function setupDefaultHandlers() {
    server.use(
      http.get(`${BASE_URL}/api/user/bill/invoices`, () => {
        return HttpResponse.json(makeListResponse(defaultInvoices))
      })
    )
  }

  beforeEach(() => {
    setupDefaultHandlers()
  })

  // ==================== PDF Download Visibility ====================

  describe('PDF download button visibility', () => {
    it('shows PDF download button for issued invoice', async () => {
      const invoice = makeInvoice({ id: 'inv-issued', status: 'issued' })
      server.use(
        http.get(`${BASE_URL}/api/user/bill/invoices`, () => {
          return HttpResponse.json(makeListResponse([invoice]))
        })
      )

      renderWithProviders(<InvoiceUserPage realmId={REALM_ID} />)

      await waitFor(() => {
        expect(screen.getByTestId('invoice-download-pdf-inv-issued')).toBeInTheDocument()
      })
    })

    it('hides PDF download button for draft invoice', async () => {
      const invoice = makeInvoice({ id: 'inv-draft', status: 'draft' })
      server.use(
        http.get(`${BASE_URL}/api/user/bill/invoices`, () => {
          return HttpResponse.json(makeListResponse([invoice]))
        })
      )

      renderWithProviders(<InvoiceUserPage realmId={REALM_ID} />)

      await waitFor(() => {
        expect(screen.getByTestId('invoice-user-table')).toBeInTheDocument()
      })

      expect(screen.queryByTestId('invoice-download-pdf-inv-draft')).not.toBeInTheDocument()
    })

    it('hides PDF download button for void invoice', async () => {
      const invoice = makeInvoice({ id: 'inv-void', status: 'void' })
      server.use(
        http.get(`${BASE_URL}/api/user/bill/invoices`, () => {
          return HttpResponse.json(makeListResponse([invoice]))
        })
      )

      renderWithProviders(<InvoiceUserPage realmId={REALM_ID} />)

      await waitFor(() => {
        expect(screen.getByTestId('invoice-user-table')).toBeInTheDocument()
      })

      expect(screen.queryByTestId('invoice-download-pdf-inv-void')).not.toBeInTheDocument()
    })
  })

  // The standalone Apply Invoice button (and its realm-level eligibility gating)
  // was removed in P1-3. Apply is now only reachable from history rows with a
  // pre-filled resource, gated by the per-resource apply-eligibility API.

  // ==================== Pagination ====================

  describe('pagination', () => {
    const MANY_INVOICES: InvoiceResponse[] = Array.from({ length: 45 }, (_, i) =>
      makeInvoice({
        id: `inv-p${i}`,
        invoiceNumber: `INV-P${String(i + 1).padStart(3, '0')}`,
        status: 'issued',
      })
    )

    it('clicking next/prev controls change page', async () => {
      const user = userEvent.setup()
      let capturedPage: number | null = null

      server.use(
        http.get(`${BASE_URL}/api/user/bill/invoices`, ({ request }) => {
          const url = new URL(request.url)
          capturedPage = parseInt(url.searchParams.get('page') ?? '0', 10)

          const start = capturedPage * 20
          const end = Math.min(start + 20, MANY_INVOICES.length)
          const pageInvoices = MANY_INVOICES.slice(start, end)

          return HttpResponse.json(
            makeListResponse(pageInvoices, {
              page: capturedPage,
              total: MANY_INVOICES.length,
            })
          )
        })
      )

      renderWithProviders(<InvoiceUserPage realmId={REALM_ID} />)

      // Wait for initial load (page 0)
      await waitFor(() => {
        expect(capturedPage).toBe(0)
      })

      await waitFor(() => {
        expect(screen.getByText('INV-P001')).toBeInTheDocument()
      })

      // Click next
      const nextButton = screen.getByTestId('invoice-user-pagination-next')
      await user.click(nextButton)

      await waitFor(() => {
        expect(capturedPage).toBe(1)
      })

      // Page 0 items should be gone, page 1 items should appear
      await waitFor(() => {
        expect(screen.getByText('INV-P021')).toBeInTheDocument()
      })

      // Click previous to go back to page 0
      const prevButton = screen.getByTestId('invoice-user-pagination-previous')
      await user.click(prevButton)

      // Verify we're back on page 0 by checking page 0 content appears
      await waitFor(() => {
        expect(screen.getByText('INV-P001')).toBeInTheDocument()
      })
    })
  })

  // ==================== External Invoice Provider Badge ====================

  describe('provider badge', () => {
    it('shows provider badge for external invoice (stripe)', async () => {
      const invoice = makeInvoice({
        id: 'inv-stripe',
        provider: 'stripe',
        status: 'issued',
      })
      server.use(
        http.get(`${BASE_URL}/api/user/bill/invoices`, () => {
          return HttpResponse.json(makeListResponse([invoice]))
        })
      )

      renderWithProviders(<InvoiceUserPage realmId={REALM_ID} />)

      await waitFor(() => {
        // Provider badge appears in both the provider column and the status badge
        const stripeBadges = screen.getAllByText('Stripe')
        expect(stripeBadges.length).toBeGreaterThanOrEqual(1)
      })
    })

    it('shows no provider badge for manual invoice', async () => {
      const invoice = makeInvoice({
        id: 'inv-manual',
        provider: 'manual',
        status: 'issued',
      })
      server.use(
        http.get(`${BASE_URL}/api/user/bill/invoices`, () => {
          return HttpResponse.json(makeListResponse([invoice]))
        })
      )

      renderWithProviders(<InvoiceUserPage realmId={REALM_ID} />)

      await waitFor(() => {
        expect(screen.getByTestId('invoice-user-table')).toBeInTheDocument()
      })

      // "Provider" header exists but no badge value in that column
      expect(screen.queryByText('Stripe')).not.toBeInTheDocument()
      expect(screen.queryByText('Creem')).not.toBeInTheDocument()
    })
  })

  // ==================== External Invoice Actions ====================

  describe('external invoice actions', () => {
    it('shows PDF download link for external invoice with externalPdfUrl', async () => {
      const invoice = makeInvoice({
        id: 'inv-ext-pdf',
        provider: 'stripe',
        status: 'issued',
        externalPdfUrl: 'https://stripe.example.com/invoice.pdf',
      })
      server.use(
        http.get(`${BASE_URL}/api/user/bill/invoices`, () => {
          return HttpResponse.json(makeListResponse([invoice]))
        })
      )

      renderWithProviders(<InvoiceUserPage realmId={REALM_ID} />)

      await waitFor(() => {
        const link = screen.getByTestId('invoice-download-pdf-inv-ext-pdf')
        expect(link).toBeInTheDocument()
        expect(link.closest('a')).toHaveAttribute('href', 'https://stripe.example.com/invoice.pdf')
        expect(link.closest('a')).toHaveAttribute('target', '_blank')
      })
    })

    it('shows View link for external invoice with hosted URL but no PDF', async () => {
      const invoice = makeInvoice({
        id: 'inv-ext-hosted',
        provider: 'creem',
        status: 'issued',
        externalPdfUrl: null,
        externalHostedUrl: 'https://creem.example.com/hosted',
      })
      server.use(
        http.get(`${BASE_URL}/api/user/bill/invoices`, () => {
          return HttpResponse.json(makeListResponse([invoice]))
        })
      )

      renderWithProviders(<InvoiceUserPage realmId={REALM_ID} />)

      await waitFor(() => {
        const link = screen.getByTestId('invoice-view-provider-inv-ext-hosted')
        expect(link).toBeInTheDocument()
        expect(link.closest('a')).toHaveAttribute('href', 'https://creem.example.com/hosted')
        expect(link).toHaveTextContent('Creem')
      })
    })

    it('shows provider-managed pending text for external invoice without any URL', async () => {
      const invoice = makeInvoice({
        id: 'inv-ext-nourl',
        provider: 'stripe',
        status: 'issued',
        externalPdfUrl: null,
        externalHostedUrl: null,
      })
      server.use(
        http.get(`${BASE_URL}/api/user/bill/invoices`, () => {
          return HttpResponse.json(makeListResponse([invoice]))
        })
      )

      renderWithProviders(<InvoiceUserPage realmId={REALM_ID} />)

      await waitFor(() => {
        expect(screen.getByTestId('invoice-managed-external-inv-ext-nourl')).toBeInTheDocument()
        expect(screen.getByTestId('invoice-managed-external-inv-ext-nourl')).toHaveTextContent(
          'Stripe is managing this invoice. The invoice link will appear here when available.'
        )
      })
    })

    it('shows regular PDF download button for manual invoice', async () => {
      const invoice = makeInvoice({
        id: 'inv-manual-pdf',
        provider: 'manual',
        status: 'issued',
      })
      server.use(
        http.get(`${BASE_URL}/api/user/bill/invoices`, () => {
          return HttpResponse.json(makeListResponse([invoice]))
        })
      )

      renderWithProviders(<InvoiceUserPage realmId={REALM_ID} />)

      await waitFor(() => {
        const button = screen.getByTestId('invoice-download-pdf-inv-manual-pdf')
        expect(button).toBeInTheDocument()
        // Manual invoice uses onClick button, not <a> link
        expect(button.closest('a')).toBeNull()
      })
    })
  })

  // ==================== Refund Summary Pill ====================

  describe('refund summary pill', () => {
    it.each([
      {
        name: 'stripe invoice with refund shows summary pill',
        invoice: {
          id: 'inv-stripe-refund',
          provider: 'stripe' as const,
          amountRefunded: 5000,
          total: 9900,
          currency: 'CNY',
        },
        expectPill: true,
      },
      {
        name: 'manual invoice with refund shows summary pill',
        invoice: {
          id: 'inv-manual-refund',
          provider: 'manual' as const,
          amountRefunded: 3000,
          total: 9900,
          currency: 'CNY',
        },
        expectPill: true,
      },
      {
        // Creem is Merchant of Record; Herald does not maintain refund credit notes for Creem invoices,
        // so the refund dimension (and its summary pill) is not exposed to users.
        name: 'creem invoice hides pill (MoR excludes refund dimension)',
        invoice: {
          id: 'inv-creem-refund',
          provider: 'creem' as const,
          amountRefunded: 5000,
          total: 9900,
          currency: 'CNY',
        },
        expectPill: false,
      },
      {
        name: 'invoice with amountRefunded=0 hides pill (no refund to summarize)',
        invoice: {
          id: 'inv-no-refund',
          provider: 'stripe' as const,
          amountRefunded: 0,
          total: 9900,
          currency: 'CNY',
        },
        expectPill: false,
      },
    ])('$name', async ({ invoice, expectPill }) => {
      server.use(
        http.get(`${BASE_URL}/api/user/bill/invoices`, () => {
          return HttpResponse.json(makeListResponse([makeInvoice(invoice)]))
        })
      )

      renderWithProviders(<InvoiceUserPage realmId={REALM_ID} />)

      // Wait for actual data row (table testid exists during loading state).
      await waitFor(() => {
        expect(screen.getByText('INV-001')).toBeInTheDocument()
      })

      const pill = screen.queryByTestId(`invoice-refund-summary-${invoice.id}`)
      if (expectPill) {
        expect(pill).toBeInTheDocument()
      } else {
        expect(pill).not.toBeInTheDocument()
      }
    })

    it('summary pill shows refunded amount over total without source color or note id', async () => {
      const invoice = makeInvoice({
        id: 'inv-pill-content',
        provider: 'stripe',
        amountRefunded: 5000,
        total: 9900,
        currency: 'CNY',
      })
      server.use(
        http.get(`${BASE_URL}/api/user/bill/invoices`, () => {
          return HttpResponse.json(makeListResponse([invoice]))
        })
      )

      renderWithProviders(<InvoiceUserPage realmId={REALM_ID} />)

      const pill = await screen.findByTestId(`invoice-refund-summary-${invoice.id}`)

      expect(pill).toHaveTextContent(/Refunded/)
      expect(pill).toHaveTextContent(/50\.00/)
      expect(pill).toHaveTextContent(/99\.00/)
      // Users are not exposed to internal credit note identifiers or provenance tags.
      expect(pill).not.toHaveTextContent(/CN-/i)
      expect(pill).not.toHaveTextContent(/NOTE-/i)
    })

    it('total amount still renders when pill is hidden for creem', async () => {
      const invoice = makeInvoice({
        id: 'inv-creem-total',
        provider: 'creem',
        amountRefunded: 0,
        total: 9900,
        currency: 'CNY',
      })
      server.use(
        http.get(`${BASE_URL}/api/user/bill/invoices`, () => {
          return HttpResponse.json(makeListResponse([invoice]))
        })
      )

      renderWithProviders(<InvoiceUserPage realmId={REALM_ID} />)

      // Wait for actual data row (table testid exists during loading state).
      await waitFor(() => {
        expect(screen.getByText('INV-001')).toBeInTheDocument()
      })

      expect(screen.queryByTestId(`invoice-refund-summary-${invoice.id}`)).not.toBeInTheDocument()
      expect(screen.getByText(/99\.00/)).toBeInTheDocument()
    })
  })
})
