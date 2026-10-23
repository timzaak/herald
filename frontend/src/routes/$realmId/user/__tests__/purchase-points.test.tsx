import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import type { ReactNode } from 'react'
import type { PurchaseOptionView } from '@/lib/api-generated'
import { m } from '@/paraglide/messages'

// --- Mocks ----------------------------------------------------------------

// The purchase-flow store is a persisted zustand store. The mock backs both
// the hook usages and the direct getState() reads the bounce logic performs,
// so tests can stage a tracked purchase (attemptId/targetId) per scenario.
const { mockStoreState, resetMockStoreState, mockNavigate, mockUsePurchaseFlowStore } = vi.hoisted(
  () => {
    const storeState = {
      realmId: null as string | null,
      userId: null as string | null,
      targetType: null as string | null,
      targetId: null as string | null,
      paymentProvider: null as string | null,
      attemptId: null as string | null,
      attemptStatus: null as string | null,
      paymentContext: null as Record<string, never> | null,
      expiresAt: null as string | null,
    }
    // Pristine snapshot: the mock's clearPurchaseState and the per-test reset
    // both restore exactly these fields, so a new store field cannot drift
    // between the two reset sites.
    const initialStoreState = { ...storeState }
    const useStore = Object.assign(
      (selector?: (s: typeof storeState) => unknown) =>
        selector ? selector(storeState) : storeState,
      { getState: () => storeState }
    )
    return {
      mockStoreState: storeState,
      resetMockStoreState: () => Object.assign(storeState, initialStoreState),
      mockNavigate: vi.fn(),
      mockUsePurchaseFlowStore: useStore,
    }
  }
)
// Stable identity across renders, mirroring the real useShallow-bound actions:
// an unstable canRecover would re-fire the recovery effect every render and
// clobber the bounce-driven step. mockStoreState is referenced lazily (the
// hoisted blocks above have already run by the time this is called).
const { mockCanRecover } = vi.hoisted(() => ({
  mockCanRecover: () =>
    !!mockStoreState.attemptId &&
    (mockStoreState.attemptStatus === 'Pending' ||
      mockStoreState.attemptStatus === 'RequiresAction'),
}))
const { mockSetPurchaseState, mockSetPaymentAttempt, mockClearPurchaseState } = vi.hoisted(() => ({
  mockSetPurchaseState: vi.fn(),
  mockSetPaymentAttempt: vi.fn(
    (attemptId: string, status: string, context: object, expiresAt: string) => {
      Object.assign(mockStoreState, {
        attemptId,
        attemptStatus: status,
        paymentContext: context,
        expiresAt,
      })
    }
  ),
  mockClearPurchaseState: vi.fn(() => {
    resetMockStoreState()
  }),
}))
vi.mock('@/stores/purchase-flow-store', () => ({
  usePurchaseFlowActions: () => ({
    setPurchaseState: mockSetPurchaseState,
    setPaymentAttempt: mockSetPaymentAttempt,
    clearPurchaseState: mockClearPurchaseState,
    canRecover: mockCanRecover,
  }),
  usePaymentAttempt: () => ({
    attemptId: mockStoreState.attemptId,
    attemptStatus: mockStoreState.attemptStatus,
    paymentContext: mockStoreState.paymentContext,
    expiresAt: mockStoreState.expiresAt,
  }),
  usePurchaseFlowStore: mockUsePurchaseFlowStore,
}))

// The bounce effect strips consumed params via navigate(replace) — outside a
// real router the hook must be stubbed.
vi.mock('@tanstack/react-router', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@tanstack/react-router')>()
  return { ...actual, useNavigate: () => mockNavigate }
})

// Auth store: minimal user identity.
vi.mock('@/stores/auth-store', () => ({
  useAuthStore: (selector: (s: { user: { id: string } | null }) => unknown) =>
    selector({ user: { id: 'user-1' } }),
}))

// App locale: controlled per-test — the create-attempt request must carry the
// app's UI language so the hosted checkout follows it (not the browser's
// language), so the tests need to pin exactly which locale the page read.
const { appLocale } = vi.hoisted(() => ({ appLocale: { current: 'en' as string } }))
vi.mock('@/components/shared/locale-provider', () => ({
  useLocale: () => ({ locale: appLocale.current, switchLocale: vi.fn() }),
}))

// Payment attempt API: capture the submitted target so we can assert mappingId.
const { mockCreatePaymentAttempt, mockGetPaymentAttemptStatus } = vi.hoisted(() => ({
  mockCreatePaymentAttempt: vi.fn(),
  mockGetPaymentAttemptStatus: vi.fn(),
}))
vi.mock('@/lib/api-generated', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api-generated')>()
  return {
    ...actual,
    createPaymentAttempt: mockCreatePaymentAttempt,
    cancelPaymentAttempt: vi.fn(),
    getPaymentAttemptStatus: mockGetPaymentAttemptStatus,
  }
})

// PaymentAttemptStatus / PaymentMethodSelector: stub to avoid their query trees.
vi.mock('@/components/purchase/payment-attempt-status', () => ({
  PaymentAttemptStatus: () => <div data-testid="payment-attempt-status" />,
}))
vi.mock('@/components/purchase/payment-method-selector', () => ({
  PaymentMethodSelector: ({ selectedProvider }: { selectedProvider: string | null }) => (
    <div data-testid="payment-method-selector">{selectedProvider}</div>
  ),
}))

import { PurchasePointsPage } from '../purchase-points'
import { disabledReason } from '@/components/billing/currency-purchase-group'
import { makePurchaseOption } from '@/test/fixtures/purchase-option'
import { formatInvoiceAmount } from '@/lib/invoice-utils'

// --- Fixtures --------------------------------------------------------------

// This suite's baseline row differs from the shared factory: a "prod_pro"
// product with the monthly price and one active point rule, matching the
// page-level fixtures the assertions below target.
function makeOption(overrides: Partial<PurchaseOptionView>): PurchaseOptionView {
  return makePurchaseOption({
    externalProductId: 'prod_pro',
    externalPriceId: 'price_monthly',
    pointRules: [
      {
        id: 'rule-1',
        bucketId: 'bucket-a',
        triggerSources: ['subscription_initial'],
        grantMode: 'fixed',
        pointsAmount: 1000,
        validityDays: 30,
        enabled: true,
        displayOrder: 0,
      },
    ],
    ...overrides,
  })
}

function makeQueryClient() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
}

function seedOptions(client: QueryClient, items: PurchaseOptionView[]) {
  // The purchase-options query returns the full list response (items), not a
  // bare array.
  client.setQueryData(['purchase-options', 'realm-1', 'app-1'], { items })
  client.setQueryData(['payment-providers', 'realm-1'], [{ platform: 'stripe', name: 'Stripe' }])
}

function makeWrapper(client: QueryClient) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>
  }
}

function renderPage(
  items: PurchaseOptionView[],
  client?: QueryClient,
  pageProps?: {
    wechatOpenid?: string
    queryAttemptId?: string
    queryStatus?: 'success' | 'cancel'
  }
) {
  const qc = client ?? makeQueryClient()
  seedOptions(qc, items)
  const view = render(
    <PurchasePointsPage
      realmId="realm-1"
      clientAppId="app-1"
      queryAttemptId={pageProps?.queryAttemptId}
      queryStatus={pageProps?.queryStatus}
      wechatOpenid={pageProps?.wechatOpenid}
    />,
    {
      wrapper: makeWrapper(qc),
    }
  )
  return { qc, ...view }
}

beforeEach(() => {
  vi.clearAllMocks()
  appLocale.current = 'en'
  resetMockStoreState()
  // Default: payment attempt creation resolves with an id.
  mockCreatePaymentAttempt.mockResolvedValue({
    data: { id: 'att-1', paymentContext: {}, expiresAt: '2026-01-01T00:00:00Z' },
  })
  // Default: the status poll resolves with nothing yet (still loading).
  mockGetPaymentAttemptStatus.mockResolvedValue({ data: undefined })
})

// --- Pure helper tests (pinned contract) ----------------------------------

describe('disabledReason', () => {
  it('returns null for an enabled option with a provider', () => {
    expect(disabledReason(makeOption({ enabled: true, paymentProvider: 'stripe' }))).toBeNull()
  })

  it('returns the not_enabled reason when disabled', () => {
    const reason = disabledReason(makeOption({ enabled: false }))
    expect(reason).toEqual({ key: 'purchase.not_enabled_reason' })
  })

  it('returns the not_enabled reason when no payment provider is wired', () => {
    const reason = disabledReason(makeOption({ paymentProvider: '' }))
    expect(reason).toEqual({ key: 'purchase.not_enabled_reason' })
  })

  // Already-owned gating (design §4.2.2). `grantsRole` is true only for the
  // one_time + non-empty granted_role_ids combo, so these cases pin that
  // scoping without the frontend re-checking billing_type.
  describe('already-owned gate', () => {
    it('disables a one-time+role card the user already owns', () => {
      const reason = disabledReason(
        makeOption({ grantsRole: true, alreadyOwned: true, billingType: 'one_time' })
      )
      expect(reason).toEqual({ key: 'purchase.already_owned_reason' })
    })

    it('does not disable when the role gate is active but not yet owned', () => {
      const reason = disabledReason(
        makeOption({ grantsRole: true, alreadyOwned: false, billingType: 'one_time' })
      )
      expect(reason).toBeNull()
    })

    it('does not disable a non-gated option even if alreadyOwned is set', () => {
      // points/subscription options always have grantsRole=false (backend
      // semantics), so alreadyOwned must never trigger the gate for them.
      const reason = disabledReason(
        makeOption({ grantsRole: false, alreadyOwned: true, billingType: 'recurring' })
      )
      expect(reason).toBeNull()
    })

    it('prioritizes not_enabled over the already-owned gate (branch order lock)', () => {
      // When both conditions hold, the not_enabled branch (declared first in
      // disabledReason) must win — pins the helper's branch ordering.
      const reason = disabledReason(
        makeOption({
          enabled: false,
          grantsRole: true,
          alreadyOwned: true,
          billingType: 'one_time',
        })
      )
      expect(reason).toEqual({ key: 'purchase.not_enabled_reason' })
    })
  })
})

// --- Component tests -------------------------------------------------------

describe('PurchasePointsPage', () => {
  it('shows fixed and quota grants per bucket without combining their values', () => {
    renderPage([
      makeOption({
        externalPriceId: 'price_multi_wallet',
        pointRules: [
          {
            id: 'rule-fixed',
            bucketId: 'wallet-fixed',
            triggerSources: ['subscription_initial'],
            grantMode: 'fixed',
            pointsAmount: 1000,
            validityDays: 30,
            enabled: true,
            displayOrder: 0,
          },
          {
            id: 'rule-quota',
            bucketId: 'wallet-quota',
            triggerSources: ['subscription_initial'],
            grantMode: 'quota',
            quotaWindows: [{ key: 'hour', windowSeconds: 3600, limit: 25 }],
            enabled: true,
            displayOrder: 1,
          },
        ],
      }),
    ])

    expect(screen.getByTestId('purchase-point-rule-rule-fixed')).toHaveTextContent(
      'wallet-fixed · 1,000 points'
    )
    expect(screen.getByTestId('purchase-point-rule-rule-quota')).toHaveTextContent(
      'wallet-quota · 25 / 3600s'
    )
  })

  it('renders all recurring cards together with no period toggle', async () => {
    renderPage([
      makeOption({
        mappingId: 'm-month',
        externalPriceId: 'price_monthly',
        billingType: 'recurring',
        billingPeriod: 'month',
      }),
      makeOption({
        mappingId: 'm-year',
        externalPriceId: 'price_annual',
        billingType: 'recurring',
        billingPeriod: 'year',
      }),
    ])

    // Both recurring cards render in a single view — no period tab to toggle.
    expect(screen.getByTestId('purchase-price-card-price_monthly')).toBeTruthy()
    expect(screen.getByTestId('purchase-price-card-price_annual')).toBeTruthy()
    expect(screen.queryByTestId('purchase-period-toggle')).toBeNull()
    expect(screen.queryByTestId('purchase-period-toggle-month')).toBeNull()
    expect(screen.queryByTestId('purchase-period-toggle-year')).toBeNull()
  })

  it('shows a disabled reason and disabled CTA for a not-enabled price', () => {
    renderPage([
      makeOption({
        mappingId: 'm-disabled',
        externalPriceId: 'price_disabled',
        enabled: false,
      }),
    ])

    const card = screen.getByTestId('purchase-price-card-price_disabled')
    expect(card).toBeTruthy()
    expect(screen.getByTestId('purchase-price-card-price_disabled-reason')).toBeTruthy()
    expect(screen.getByTestId('purchase-next-button')).toBeDisabled()
  })

  it('shows the already-owned reason and disabled CTA for an owned one-time+role card', () => {
    renderPage([
      makeOption({
        mappingId: 'm-owned',
        externalPriceId: 'price_owned',
        billingType: 'one_time',
        grantsRole: true,
        alreadyOwned: true,
        enabled: true,
      }),
    ])

    expect(screen.getByTestId('purchase-price-card-price_owned')).toBeTruthy()
    // The reason row renders the canonical already-owned copy (not the
    // not_enabled copy), proving the helper routed to the new branch.
    expect(screen.getByTestId('purchase-price-card-price_owned-reason').textContent).toBe(
      m['purchase.already_owned_reason']()
    )
    expect(screen.getByTestId('purchase-next-button')).toBeDisabled()
  })

  it('selects a card and submits the mappingId as the purchase target (single provider skips payment step)', async () => {
    appLocale.current = 'zh-CN'
    const user = userEvent.setup()
    renderPage([
      makeOption({ mappingId: 'm-1', externalPriceId: 'price_m', billingPeriod: 'month' }),
    ])

    await user.click(screen.getByTestId('purchase-price-card-price_m'))
    await waitFor(() => {
      expect(screen.getByTestId('purchase-next-button')).not.toBeDisabled()
    })

    // With a single matching provider the payment-method step is skipped:
    // Next creates the payment attempt directly.
    await user.click(screen.getByTestId('purchase-next-button'))
    expect(screen.queryByTestId('purchase-step-payment')).toBeNull()
    await waitFor(() => {
      expect(mockCreatePaymentAttempt).toHaveBeenCalledTimes(1)
    })
    const call = mockCreatePaymentAttempt.mock.calls[0][0] as {
      body: { targetType: string; targetId: string; paymentProvider: string; locale: string }
    }
    expect(call.body.targetType).toBe('entitlement_mapping')
    expect(call.body.targetId).toBe('m-1')
    expect(call.body.paymentProvider).toBe('stripe')
    // The app's UI language must reach the create request so the hosted
    // checkout follows it (server maps zh-CN → Stripe's zh); dropping this
    // would leave the checkout on the buyer's browser language.
    expect(call.body.locale).toBe('zh-CN')
  })

  describe('provider redirect bounce', () => {
    it('a cancel bounce shows the cancelled result view and retries the restored package in one click', async () => {
      const user = userEvent.setup()
      // The tab tracked the bounced attempt: full-page reload wiped the React
      // selection, but the persisted store still knows the purchase target.
      Object.assign(mockStoreState, {
        attemptId: 'att-9',
        attemptStatus: 'Pending',
        targetId: 'm-1',
        paymentProvider: 'stripe',
      })
      renderPage(
        [makeOption({ mappingId: 'm-1', externalPriceId: 'price_m', billingPeriod: 'month' })],
        undefined,
        { queryAttemptId: 'att-9', queryStatus: 'cancel' }
      )

      expect(await screen.findByTestId('purchase-step-cancelled')).toBeInTheDocument()
      // The pre-redesign bug landed the bounce on the payment step with no
      // selection, dead-ending on "no providers available" — that broken
      // intermediate state must not render.
      expect(screen.queryByTestId('purchase-step-payment')).toBeNull()
      expect(screen.queryByTestId('payment-method-selector')).toBeNull()
      expect(mockClearPurchaseState).toHaveBeenCalledTimes(1)

      // One-click retry re-creates the attempt for the same package/provider.
      await user.click(screen.getByTestId('purchase-cancel-retry'))
      await waitFor(() => {
        expect(mockCreatePaymentAttempt).toHaveBeenCalledTimes(1)
      })
      const call = mockCreatePaymentAttempt.mock.calls[0][0] as {
        body: { targetId: string; paymentProvider: string }
      }
      expect(call.body.targetId).toBe('m-1')
      expect(call.body.paymentProvider).toBe('stripe')
      // The consumed bounce params are stripped from the URL (replace), and
      // the search updater preserves every other param (wechatOpenid).
      expect(mockNavigate).toHaveBeenCalledWith(expect.objectContaining({ replace: true }))
      const searchUpdater = mockNavigate.mock.calls[0][0].search as (
        prev: Record<string, unknown>
      ) => Record<string, unknown>
      expect(searchUpdater({ attemptId: 'att-9', status: 'cancel', wechatOpenid: 'o-1' })).toEqual({
        attemptId: undefined,
        status: undefined,
        wechatOpenid: 'o-1',
      })
    })

    it('a cancel bounce for an attempt this tab is not tracking clears nothing and hides retry', async () => {
      // Another tab may be mid-purchase with a newer attempt — unwinding the
      // store here would kill that tab's recovery.
      Object.assign(mockStoreState, {
        attemptId: 'att-other',
        attemptStatus: 'Pending',
        targetId: 'm-2',
        paymentProvider: 'stripe',
      })
      renderPage([makeOption({ mappingId: 'm-1', externalPriceId: 'price_m' })], undefined, {
        queryAttemptId: 'att-9',
        queryStatus: 'cancel',
      })

      expect(await screen.findByTestId('purchase-step-cancelled')).toBeInTheDocument()
      expect(mockClearPurchaseState).not.toHaveBeenCalled()
      expect(mockStoreState.attemptId).toBe('att-other')
      // Without a restored selection there is nothing to retry — the way
      // forward is picking a package again.
      expect(screen.queryByTestId('purchase-cancel-retry')).toBeNull()
      expect(screen.getByTestId('purchase-cancel-repick')).toBeInTheDocument()
    })

    it('a success bounce waits on a confirming panel without provider redirect prompts or a cancel affordance', async () => {
      mockGetPaymentAttemptStatus.mockResolvedValue({
        data: { status: 'Pending', amount: 10, currency: 'USD', expiresAt: '2099-01-01T00:00:00Z' },
      })
      renderPage([makeOption({ mappingId: 'm-1', externalPriceId: 'price_m' })], undefined, {
        queryAttemptId: 'att-9',
        queryStatus: 'success',
      })

      expect(await screen.findByTestId('purchase-confirming')).toBeInTheDocument()
      // The provider "go pay" prompt and the cancel button are both wrong
      // after the user already paid — neither may render.
      expect(screen.queryByTestId('payment-attempt-status')).toBeNull()
      expect(screen.queryByTestId('payment-cancel-button')).toBeNull()
    })

    it('a success bounce completes with the paid amount once the webhook-confirmed status lands', async () => {
      mockGetPaymentAttemptStatus.mockResolvedValue({
        data: {
          status: 'Succeeded',
          amount: 10,
          currency: 'USD',
          expiresAt: '2099-01-01T00:00:00Z',
        },
      })
      renderPage([makeOption({ mappingId: 'm-1', externalPriceId: 'price_m' })], undefined, {
        queryAttemptId: 'att-9',
        queryStatus: 'success',
      })

      expect(await screen.findByTestId('purchase-step-complete')).toBeInTheDocument()
      expect(screen.getByTestId('purchase-complete-amount').textContent).toBe(
        m['points.purchase_paid_amount']({ amount: formatInvoiceAmount(10, 'USD') })
      )
      expect(mockClearPurchaseState).toHaveBeenCalled()
    })
  })

  describe('WeChat provider branch', () => {
    const WECHAT_UA =
      'Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 MicroMessenger/8.0.40(0x18002830)'

    function renderWechatPage(props?: { wechatOpenid?: string }) {
      return renderPage(
        [makeOption({ mappingId: 'm-wx', externalPriceId: 'price_wx', paymentProvider: 'wechat' })],
        undefined,
        props
      )
    }

    it('orders Native (no scene/openid fields) and stays on the processing step instead of redirecting', async () => {
      // Outside WeChat's browser the scene defaults server-side to native, so
      // the request carries no scene override; unlike stripe/creem there is no
      // checkout URL to redirect to — the QR IS the pending UI, so the page
      // must settle on the processing step.
      const user = userEvent.setup()
      renderWechatPage()

      await user.click(screen.getByTestId('purchase-price-card-price_wx'))
      await waitFor(() => {
        expect(screen.getByTestId('purchase-next-button')).not.toBeDisabled()
      })
      await user.click(screen.getByTestId('purchase-next-button'))

      await waitFor(() => {
        expect(mockCreatePaymentAttempt).toHaveBeenCalledTimes(1)
      })
      const call = mockCreatePaymentAttempt.mock.calls[0][0] as {
        body: Record<string, unknown>
      }
      expect(call.body.paymentProvider).toBe('wechat')
      expect(call.body).not.toHaveProperty('paymentScene')
      expect(call.body).not.toHaveProperty('openid')
      expect(await screen.findByTestId('purchase-step-processing')).toBeInTheDocument()
    })

    it('orders JSAPI with the caller-provided openid inside WeChat', async () => {
      const uaSpy = vi.spyOn(Navigator.prototype, 'userAgent', 'get').mockReturnValue(WECHAT_UA)
      const user = userEvent.setup()
      renderWechatPage({ wechatOpenid: 'openid-1' })

      await user.click(screen.getByTestId('purchase-price-card-price_wx'))
      await waitFor(() => {
        expect(screen.getByTestId('purchase-next-button')).not.toBeDisabled()
      })
      await user.click(screen.getByTestId('purchase-next-button'))

      await waitFor(() => {
        expect(mockCreatePaymentAttempt).toHaveBeenCalledTimes(1)
      })
      const call = mockCreatePaymentAttempt.mock.calls[0][0] as {
        body: Record<string, unknown>
      }
      expect(call.body.paymentScene).toBe('jsapi')
      expect(call.body.openid).toBe('openid-1')
      uaSpy.mockRestore()
    })

    it('refuses to order inside WeChat without an openid and keeps the user on the selection step', async () => {
      // The same device cannot scan a Native QR and JSAPI cannot be created
      // without an openid, so ordering must be blocked, not downgraded.
      const uaSpy = vi.spyOn(Navigator.prototype, 'userAgent', 'get').mockReturnValue(WECHAT_UA)
      const user = userEvent.setup()
      renderWechatPage()

      await user.click(screen.getByTestId('purchase-price-card-price_wx'))
      await waitFor(() => {
        expect(screen.getByTestId('purchase-next-button')).not.toBeDisabled()
      })
      await user.click(screen.getByTestId('purchase-next-button'))

      expect(mockCreatePaymentAttempt).not.toHaveBeenCalled()
      expect(await screen.findByTestId('purchase-step-packages')).toBeInTheDocument()
      uaSpy.mockRestore()
    })
  })
})
