/**
 * @vitest-environment jsdom
 */

import { describe, expect, it, vi, beforeEach } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { PurchaseHistoryList } from '../purchase-history-list'
import type { PurchaseHistoryItem } from '@/lib/api-generated'
import { server } from '@/test/mocks/server'
import { renderWithProviders } from '@/test/utils/render'

const REALM_ID = 'test-realm'
const BASE_URL = 'http://localhost:3000'
const ATTEMPT_ID = 'attempt-1'

const purchase: PurchaseHistoryItem = {
  attemptId: ATTEMPT_ID,
  targetMappingId: 'mapping-1',
  productName: 'Test Product',
  points: 100,
  amount: 999,
  currency: 'USD',
  // Non-Stripe provider: Stripe invoices are pushed via webhook and the apply
  // button is hidden for Stripe purchases, so the default fixture uses a
  // non-Stripe provider to exercise the manual-fallback apply path.
  paymentProvider: 'manual',
  status: 'Succeeded',
  completedAt: '2025-01-01T00:05:00Z',
  createdAt: '2025-01-01T00:00:00Z',
}

const stripePurchase: PurchaseHistoryItem = {
  ...purchase,
  attemptId: 'attempt-stripe',
  paymentProvider: 'stripe',
}

const BUTTON_TESTID = `purchase-history-invoice-button-${ATTEMPT_ID}`

function eligibilityHandler(
  route: 'manual_fallback' | 'disabled' | 'external_provider',
  overrides: Record<string, unknown> = {}
) {
  return http.get(`${BASE_URL}/api/user/bill/invoices/apply-eligibility`, () => {
    return HttpResponse.json({
      referenceType: 'payment_attempt',
      referenceId: ATTEMPT_ID,
      canApply: route === 'manual_fallback',
      route,
      provider: route === 'external_provider' ? 'stripe' : null,
      reason: route === 'disabled' ? 'Not eligible' : null,
      ...overrides,
    })
  })
}

describe('PurchaseHistoryList invoice action', () => {
  beforeEach(() => {
    server.use(eligibilityHandler('manual_fallback'))
  })

  // The per-row button reflects the apply-eligibility route BEFORE submit.
  describe('eligibility gating', () => {
    it('manual_fallback: enabled, clicking invokes onApplyInvoice with attemptId', async () => {
      const user = userEvent.setup()
      const onApplyInvoice = vi.fn()

      renderWithProviders(
        <PurchaseHistoryList
          purchases={[purchase]}
          isLoading={false}
          onDetailsClick={vi.fn()}
          realmId={REALM_ID}
          onApplyInvoice={onApplyInvoice}
        />
      )

      const button = await screen.findByTestId(BUTTON_TESTID)
      await waitFor(() => {
        expect(button).toBeEnabled()
      })
      await user.click(button)

      expect(onApplyInvoice).toHaveBeenCalledTimes(1)
      expect(onApplyInvoice).toHaveBeenCalledWith(ATTEMPT_ID)
    })

    it('disabled: button disabled with reason surfaced inline', async () => {
      server.use(eligibilityHandler('disabled', { reason: 'Not eligible for invoice' }))

      renderWithProviders(
        <PurchaseHistoryList
          purchases={[purchase]}
          isLoading={false}
          onDetailsClick={vi.fn()}
          realmId={REALM_ID}
          onApplyInvoice={vi.fn()}
        />
      )

      const reason = await screen.findByTestId(`${BUTTON_TESTID}-reason`)
      expect(reason).toHaveTextContent('Not eligible for invoice')

      await waitFor(() => {
        expect(screen.getByTestId(BUTTON_TESTID)).toBeDisabled()
      })
    })

    it('external_provider: button disabled with managed-by message', async () => {
      server.use(eligibilityHandler('external_provider'))

      renderWithProviders(
        <PurchaseHistoryList
          purchases={[purchase]}
          isLoading={false}
          onDetailsClick={vi.fn()}
          realmId={REALM_ID}
          onApplyInvoice={vi.fn()}
        />
      )

      const reason = await screen.findByTestId(`${BUTTON_TESTID}-reason`)
      // Messaging pattern: "Managed by {provider} — see My Invoices."
      expect(reason).toHaveTextContent(/Managed by Stripe/)

      await waitFor(() => {
        expect(screen.getByTestId(BUTTON_TESTID)).toBeDisabled()
      })
    })

    it('does not render invoice button when onApplyInvoice is omitted', () => {
      renderWithProviders(
        <PurchaseHistoryList purchases={[purchase]} isLoading={false} onDetailsClick={vi.fn()} />
      )

      expect(screen.queryByTestId(BUTTON_TESTID)).not.toBeInTheDocument()
    })

    it('does not render invoice button for Stripe purchases (webhook-pushed)', () => {
      // Stripe invoices arrive via webhook; users never apply manually. The
      // button must not be rendered for Stripe payment attempts even when
      // realm-level invoicesVisible is on (realmId + onApplyInvoice provided).
      renderWithProviders(
        <PurchaseHistoryList
          purchases={[stripePurchase]}
          isLoading={false}
          onDetailsClick={vi.fn()}
          realmId={REALM_ID}
          onApplyInvoice={vi.fn()}
        />
      )

      expect(
        screen.queryByTestId(`purchase-history-invoice-button-${stripePurchase.attemptId}`)
      ).not.toBeInTheDocument()
    })
  })
})

describe('PurchaseHistoryList null handling', () => {
  it('renders -- for null points', () => {
    const noPointsPurchase = { ...purchase, attemptId: 'attempt-3', points: null }
    renderWithProviders(
      <PurchaseHistoryList
        purchases={[noPointsPurchase]}
        isLoading={false}
        onDetailsClick={vi.fn()}
      />
    )

    expect(screen.getByText('--')).toBeInTheDocument()
  })

  it('renders i18n fallback text for null productName', () => {
    const noNamePurchase = { ...purchase, attemptId: 'attempt-4', productName: null }
    renderWithProviders(
      <PurchaseHistoryList
        purchases={[noNamePurchase]}
        isLoading={false}
        onDetailsClick={vi.fn()}
      />
    )

    // Should show localized fallback text instead of UUID fragment
    expect(screen.getByText('Unknown Product')).toBeInTheDocument()
  })
})
