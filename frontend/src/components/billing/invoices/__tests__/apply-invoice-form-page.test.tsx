/**
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { ApplyInvoiceFormPage } from '../apply-invoice-form-page'
import { server } from '@/test/mocks/server'
import { renderWithProviders } from '@/test/utils/render'

// ==================== Router Mock ====================

const mockNavigate = vi.fn()

vi.mock('@tanstack/react-router', () => ({
  useNavigate: () => mockNavigate,
}))

// ==================== Test Helpers ====================

const REALM_ID = 'test-realm'
const BASE_URL = 'http://localhost:3000'

const PAYMENT_ATTEMPT_ID = '11111111-1111-1111-1111-111111111111'
const SUBSCRIPTION_ID = '22222222-2222-2222-2222-222222222222'

function sellerConfigHandler() {
  return http.get(`${BASE_URL}/api/bill/${REALM_ID}/invoice-seller-config`, () => {
    return HttpResponse.json({
      sellerName: 'Seller Corp',
      sellerAddress: '789 Seller Ave',
      sellerEmail: 'seller@test.com',
      sellerPhone: null,
      sellerTaxId: 'TAX999',
      defaultPaymentTerms: 'Net 30',
      createdAt: '2025-01-01T00:00:00Z',
      updatedAt: '2025-01-01T00:00:00Z',
    })
  })
}

function applyInvoiceHandler() {
  return http.post(`${BASE_URL}/api/user/bill/invoices`, async () => {
    return HttpResponse.json({ id: 'inv-new' }, { status: 201 })
  })
}

// ==================== Tests ====================

describe('ApplyInvoiceFormPage', () => {
  beforeEach(() => {
    mockNavigate.mockClear()
    server.use(sellerConfigHandler(), applyInvoiceHandler())
  })

  // ==================== Rendering ====================

  describe('rendering', () => {
    it('renders the apply invoice form with all sections and a prefilled reference', async () => {
      renderWithProviders(
        <ApplyInvoiceFormPage
          realmId={REALM_ID}
          prefilledReference={{ type: 'paymentAttempt', id: PAYMENT_ATTEMPT_ID }}
        />
      )

      await waitFor(() => {
        expect(screen.getByTestId('apply-form-page')).toBeInTheDocument()
      })

      // Verify all three section cards
      expect(screen.getByTestId('apply-form-reference-section')).toBeInTheDocument()
      expect(screen.getByTestId('apply-form-billing-section')).toBeInTheDocument()
      expect(screen.getByTestId('apply-form-details-section')).toBeInTheDocument()

      // The manual ID-entry inputs MUST NOT exist anymore (P1-3 removed them).
      // The form relies solely on the pre-filled reference banner.
      expect(screen.queryByTestId('apply-payment-attempt-id-input')).not.toBeInTheDocument()
      expect(screen.queryByTestId('apply-subscription-id-input')).not.toBeInTheDocument()

      // Verify the prefilled-reference banner is shown.
      expect(screen.getByTestId('apply-prefilled-reference')).toBeInTheDocument()
      expect(screen.getByText('Points package purchase')).toBeInTheDocument()

      // Verify key form fields are present
      expect(screen.getByTestId('apply-billing-name-input')).toBeInTheDocument()
      expect(screen.getByTestId('apply-billing-email-input')).toBeInTheDocument()
      expect(screen.getByTestId('apply-billing-address-input')).toBeInTheDocument()
      expect(screen.getByTestId('apply-billing-phone-input')).toBeInTheDocument()
      expect(screen.getByTestId('apply-due-date-input')).toBeInTheDocument()
      expect(screen.getByTestId('apply-notes-input')).toBeInTheDocument()

      // Verify action buttons
      expect(screen.getByTestId('apply-invoice-submit-button')).toBeInTheDocument()
      expect(screen.getByTestId('apply-invoice-cancel-button')).toBeInTheDocument()
      expect(screen.getByTestId('apply-invoice-back-button')).toBeInTheDocument()
    })

    it('shows the subscription banner when subscription reference is prefilled', async () => {
      renderWithProviders(
        <ApplyInvoiceFormPage
          realmId={REALM_ID}
          prefilledReference={{ type: 'subscription', id: SUBSCRIPTION_ID }}
        />
      )

      await waitFor(() => {
        expect(screen.getByTestId('apply-prefilled-reference')).toBeInTheDocument()
      })

      expect(screen.getByText('Subscription')).toBeInTheDocument()
      expect(screen.queryByTestId('apply-payment-attempt-id-input')).not.toBeInTheDocument()
      expect(screen.queryByTestId('apply-subscription-id-input')).not.toBeInTheDocument()
    })
  })

  // ==================== Validation ====================

  describe('validation', () => {
    it('submit without billingName shows validation error', async () => {
      const user = userEvent.setup()
      renderWithProviders(
        <ApplyInvoiceFormPage
          realmId={REALM_ID}
          prefilledReference={{ type: 'paymentAttempt', id: PAYMENT_ATTEMPT_ID }}
        />
      )

      await waitFor(() => {
        expect(screen.getByTestId('apply-form-page')).toBeInTheDocument()
      })

      // Submit with empty billingName (and empty billingAddress, dueDate)
      const submitButton = screen.getByTestId('apply-invoice-submit-button')
      await user.click(submitButton)

      // Should show validation error for billingName
      await waitFor(() => {
        expect(screen.getByText('Billing name is required')).toBeInTheDocument()
      })
    })
  })

  // ==================== Submission ====================

  describe('submission', () => {
    it('submits prefilled payment attempt ID without editable reference inputs', async () => {
      let capturedBody: unknown = null

      server.use(
        sellerConfigHandler(),
        http.post(`${BASE_URL}/api/user/bill/invoices`, async ({ request }) => {
          capturedBody = await request.json()
          return HttpResponse.json({ id: 'inv-new' }, { status: 201 })
        })
      )

      const user = userEvent.setup()
      renderWithProviders(
        <ApplyInvoiceFormPage
          realmId={REALM_ID}
          prefilledReference={{ type: 'paymentAttempt', id: PAYMENT_ATTEMPT_ID }}
        />
      )

      await waitFor(() => {
        expect(screen.getByTestId('apply-prefilled-reference')).toBeInTheDocument()
      })

      await user.type(screen.getByTestId('apply-billing-name-input'), 'John Doe')
      await user.type(screen.getByTestId('apply-billing-address-input'), '123 Billing St')
      await user.type(screen.getByTestId('apply-billing-tax-id-input'), 'TAX123456')
      await user.click(screen.getByTestId('apply-invoice-submit-button'))

      await waitFor(() => {
        expect(capturedBody).not.toBeNull()
      })

      expect(capturedBody).toMatchObject({
        paymentAttemptId: PAYMENT_ATTEMPT_ID,
        billingName: 'John Doe',
        billingTaxId: 'TAX123456',
      })
      expect(capturedBody).not.toHaveProperty('subscriptionId')

      // Should navigate back after successful submit
      expect(mockNavigate).toHaveBeenCalledWith({
        to: '/$realmId/user/invoices',
        params: { realmId: REALM_ID },
      })
    })
  })

  // ==================== Navigation ====================

  describe('cancel navigation', () => {
    it('cancel button navigates back', async () => {
      const user = userEvent.setup()
      renderWithProviders(
        <ApplyInvoiceFormPage
          realmId={REALM_ID}
          prefilledReference={{ type: 'paymentAttempt', id: PAYMENT_ATTEMPT_ID }}
        />
      )

      await waitFor(() => {
        expect(screen.getByTestId('apply-form-page')).toBeInTheDocument()
      })

      // Click the back button in the header
      await user.click(screen.getByTestId('apply-invoice-back-button'))

      expect(mockNavigate).toHaveBeenCalledWith({
        to: '/$realmId/user/invoices',
        params: { realmId: REALM_ID },
      })
    })
  })

  // ==================== Creem Rejection ====================

  describe('Creem MoR rejection', () => {
    it('shows inline rejection alert when backend returns 400', async () => {
      server.use(
        sellerConfigHandler(),
        http.post(`${BASE_URL}/api/user/bill/invoices`, async () => {
          return HttpResponse.json(
            {
              status: 400,
              code: 'mor_provider_invoice_blocked',
              message: 'apple transactions are managed by the platform as Merchant of Record',
            },
            { status: 400 }
          )
        })
      )

      const user = userEvent.setup()
      renderWithProviders(
        <ApplyInvoiceFormPage
          realmId={REALM_ID}
          prefilledReference={{ type: 'paymentAttempt', id: PAYMENT_ATTEMPT_ID }}
        />
      )

      await waitFor(() => {
        expect(screen.getByTestId('apply-form-page')).toBeInTheDocument()
      })

      // Fill required fields and submit
      await user.type(screen.getByTestId('apply-billing-name-input'), 'John Doe')
      await user.type(screen.getByTestId('apply-billing-address-input'), '123 Billing St')
      await user.type(screen.getByTestId('apply-billing-tax-id-input'), 'TAX123456')
      await user.click(screen.getByTestId('apply-invoice-submit-button'))

      // Should show the Creem rejection alert
      await waitFor(() => {
        expect(screen.getByTestId('apply-invoice-mor-rejection')).toBeInTheDocument()
      })

      // Should NOT have navigated (submit failed)
      expect(mockNavigate).not.toHaveBeenCalled()
    })

    it('does not show rejection alert when no error', async () => {
      renderWithProviders(
        <ApplyInvoiceFormPage
          realmId={REALM_ID}
          prefilledReference={{ type: 'paymentAttempt', id: PAYMENT_ATTEMPT_ID }}
        />
      )

      await waitFor(() => {
        expect(screen.getByTestId('apply-form-page')).toBeInTheDocument()
      })

      expect(screen.queryByTestId('apply-invoice-mor-rejection')).not.toBeInTheDocument()
    })
  })
})
