import { m } from '@/paraglide/messages'
import { useState, useEffect, useMemo } from 'react'
import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Button } from '@/components/ui/button'
import { AlertCircle, ArrowLeft, ArrowRight, Loader2, CheckCircle2, XCircle } from 'lucide-react'
import { createPaymentAttempt, cancelPaymentAttempt } from '@/lib/api-generated'
import type { PaymentAttemptStatusResponse } from '@/lib/api-generated'
import {
  purchaseOptionsQueryOptions,
  paymentProvidersQueryOptions,
  paymentAttemptStatusQueryOptions,
  queryKeys,
  requireUserFeature,
} from '@/data/query-options'
import { CurrencyPurchaseGroup } from '@/components/billing/currency-purchase-group'
import { groupByEntitlement } from '@/components/billing/currency-utils'
import { PaymentMethodSelector } from '@/components/purchase/payment-method-selector'
import { PaymentAttemptStatus } from '@/components/purchase/payment-attempt-status'
import { usePurchaseFlowActions, usePaymentAttempt } from '@/stores/purchase-flow-store'
import { usePurchaseFlowStore } from '@/stores/purchase-flow-store'
import { realmPath, useResolvedRealmContext } from '@/lib/realm-routing'
import { useAuthStore } from '@/stores/auth-store'
import { useLocale } from '@/components/shared/locale-provider'
import { initializeAuth } from '@/lib/auth-utils'
import { USER_ACCOUNT_CENTER_CLIENT_ID } from '@/lib/constants/auth-constants'
import { PAYMENT_PROVIDERS } from '@/lib/billing-constants'
import { resolveWechatScene } from '@/lib/wechat-pay-utils'
import { formatInvoiceAmount } from '@/lib/invoice-utils'
import { toast } from 'sonner'
import { purchasePointsSearchSchema } from '@/lib/schemas/search-params'
import { useCurrentSearch, useResolvedRealmId } from '@/lib/realm-routing'
import { getErrorMessage } from '@/lib/error-utils'

// Fallback attempt expiry for responses that carry no server expiresAt
// (attempt creation and the provider bounce both seed it).
const PAYMENT_ATTEMPT_FALLBACK_TTL_MS = 15 * 60 * 1000

export const Route = createFileRoute('/$realmId/user/purchase-points')({
  beforeLoad: async ({ context, params }) => {
    // Route beforeLoads run ahead of the __root loader, so on a cold reload
    // the feature query below would fire before initializeAuth restores the
    // Bearer token (401 → route error boundary). Idempotent: short-circuits
    // once the realm/client is initialized.
    await initializeAuth(params.realmId, USER_ACCOUNT_CENTER_CLIENT_ID)
    await requireUserFeature(context.queryClient, (f) => f.user.pointsVisible, {
      to: '/$realmId/user/points',
      params: { realmId: params.realmId },
    })
  },
  validateSearch: purchasePointsSearchSchema,
  component: PurchasePointsRoute,
})

type PurchaseStep = 'packages' | 'payment' | 'processing' | 'complete' | 'cancelled'

export function PurchasePointsRoute() {
  const realmId = useResolvedRealmId()
  // The provider bounces the user back here with `attemptId` (+ `status`) in
  // the query string after a checkout. This is a UX bounce only — payment
  // status is confirmed via webhook + the polling in PurchasePointsPage. See
  // purchasePointsSearchSchema.
  const {
    attemptId: queryAttemptId,
    status: queryStatus,
    wechatOpenid,
  } = useCurrentSearch<{
    attemptId?: string
    status?: 'success' | 'cancel'
    wechatOpenid?: string
  }>()

  const clientAppId = useAuthStore((state) => state.clientAppId) ?? ''

  if (!clientAppId) {
    return (
      <div className="container" data-testid="purchase-points-page">
        <div className="p-4 text-center text-muted-foreground" data-testid="no-client-app-message">
          {m['billing.subscription_no_client_app']()}
        </div>
      </div>
    )
  }

  return (
    <PurchasePointsPage
      realmId={realmId}
      clientAppId={clientAppId}
      queryAttemptId={queryAttemptId}
      queryStatus={queryStatus}
      wechatOpenid={wechatOpenid}
    />
  )
}

export function PurchasePointsPage({
  realmId,
  clientAppId,
  queryAttemptId,
  queryStatus,
  wechatOpenid,
}: {
  realmId: string
  clientAppId: string
  // Carried from the route's validated search params on a provider redirect
  // bounce. `queryAttemptId` resumes polling; `queryStatus: 'cancel'` steps
  // back to payment. Undefined on normal navigation. `wechatOpenid` is the
  // caller-provided openid that unlocks WeChat JSAPI ordering in-WeChat.
  queryAttemptId?: string
  queryStatus?: 'success' | 'cancel'
  wechatOpenid?: string
}) {
  const navigate = useNavigate()
  const realmContext = useResolvedRealmContext()
  const queryClient = useQueryClient()
  const user = useAuthStore((state) => state.user)
  // App UI language (locale-provider state, not navigator.language) — sent on
  // the create-attempt request so the provider's hosted checkout page follows
  // the app language; unknown tags fail open server-side.
  const { locale: appLocale } = useLocale()

  // Purchase flow state. Selection is now price-level: the clicked card's
  // mappingId is the checkout target directly. The provider is
  // derived from the selected option, not picked separately.
  const [currentStep, setCurrentStep] = useState<PurchaseStep>('packages')
  const [selectedMappingId, setSelectedMappingId] = useState<string | null>(null)
  // True only after a provider success bounce: the checkout page is behind us
  // and the pending UI must ask the user to wait, not to go pay again.
  const [confirmingCheckout, setConfirmingCheckout] = useState(false)
  // The purchase target of the attempt currently being polled. React state is
  // lost on the provider's full-page bounce, so re-entering the payment step
  // (retry after failure) re-seeds the selection from here.
  const [lastAttemptTargetId, setLastAttemptTargetId] = useState<string | null>(null)
  // Amount line for the complete step, captured from the succeeded status
  // before the flow store is cleared.
  const [paidAmount, setPaidAmount] = useState<{
    amount: number
    currency: string
  } | null>(null)

  // Store actions
  const { setPurchaseState, setPaymentAttempt, clearPurchaseState, canRecover } =
    usePurchaseFlowActions()
  const paymentAttempt = usePaymentAttempt()
  const { attemptId } = paymentAttempt
  const paymentProvider = usePurchaseFlowStore((state) => state.paymentProvider)

  // WeChat scene: JSAPI only inside WeChat's browser and only with a
  // caller-provided openid; everything else uses the Native QR flow. Inside
  // WeChat without an openid, ordering must be refused (a QR cannot be scanned
  // by the same device).
  const wechatSceneDecision = useMemo(
    () => resolveWechatScene(wechatOpenid, navigator.userAgent),
    [wechatOpenid]
  )

  // Fetch purchase options (price-granularity flat list, replaces the former
  // entitlement-key-grouped one-time-mappings source).
  const { data: optionsData, isLoading: optionsLoading } = useQuery(
    purchaseOptionsQueryOptions(realmId, clientAppId)
  )
  const options = useMemo(() => optionsData?.items ?? [], [optionsData])
  // Providers are still fetched so the payment step can render provider context;
  // the selected option's own provider is the one used at submit.
  const { data: providers, isLoading: providersLoading } = useQuery(
    paymentProvidersQueryOptions(realmId)
  )

  // Subscriptions section (recurring) shows all recurring options together;
  // Credit packs section (one_time) is always shown when present. Within a
  // section each entitlement renders as its own currency-switchable block.
  const subscriptionOptions = useMemo(
    () => options.filter((o) => o.billingType === 'recurring'),
    [options]
  )
  const creditPackOptions = useMemo(
    () => options.filter((o) => o.billingType !== 'recurring'),
    [options]
  )
  const subscriptionGroups = useMemo(
    () => groupByEntitlement(subscriptionOptions),
    [subscriptionOptions]
  )
  const creditPackGroups = useMemo(() => groupByEntitlement(creditPackOptions), [creditPackOptions])
  const hasRecurring = useMemo(
    () => (options ?? []).some((o) => o.billingType === 'recurring'),
    [options]
  )
  const hasAnyOptions = options.length > 0

  const selectedOption = useMemo(
    () => options.find((o) => o.mappingId === selectedMappingId),
    [options, selectedMappingId]
  )

  // Poll payment status if attempt exists
  const paymentStatusQuery = useQuery({
    ...paymentAttemptStatusQueryOptions(realmId, attemptId || ''),
    enabled: !!attemptId && currentStep === 'processing',
    refetchInterval: (query) => {
      if (!query || !query.state) {
        return false
      }
      const status = query.state.data as PaymentAttemptStatusResponse | undefined
      if (
        status?.status === 'Succeeded' ||
        status?.status === 'Failed' ||
        status?.status === 'Cancelled' ||
        status?.status === 'Expired'
      ) {
        return false
      }
      // WeChat Native: once the attempt's own payment window has passed the QR
      // is dead, so stop polling even if the server still reports Pending
      // (the expired QR UI offers a regenerate entry instead).
      if (
        paymentProvider === PAYMENT_PROVIDERS.WECHAT &&
        status?.expiresAt &&
        new Date(status.expiresAt).getTime() <= Date.now()
      ) {
        return false
      }
      return 2000
    },
  })
  const paymentStatus = paymentStatusQuery.data as PaymentAttemptStatusResponse | undefined

  useEffect(() => {
    const checkRecovery = () => {
      if (canRecover() && attemptId) {
        setCurrentStep('processing')
      }
    }
    checkRecovery()
  }, [attemptId, canRecover])

  // Resume from a provider redirect bounce. When the user returns with
  // `?attemptId=...`, this is a UX bounce only — payment status is confirmed
  // via webhook + the polling below, never by the URL params.
  useEffect(() => {
    if (!queryAttemptId) return
    if (queryStatus === 'cancel') {
      // The user abandoned checkout at the provider. The bounced attempt is
      // deliberately NOT cancelled server-side: the provider session stays
      // payable for hours, and a Cancelled attempt can no longer be fulfilled
      // if the user completes that payment later (Cancelled → Succeeded is a
      // forbidden transition) — money charged, no points granted. The backend
      // expiry job reclaims the attempt on its own.
      const store = usePurchaseFlowStore.getState()
      // Only unwind the tracked purchase when it belongs to the bounced
      // attempt: another tab may be mid-purchase with a newer attempt, and
      // clearing it here would kill that tab's recovery.
      if (store.attemptId === queryAttemptId) {
        // Restore the purchase target so the cancelled view can offer a
        // one-click retry of the same package.
        if (store.targetId) setSelectedMappingId(store.targetId)
        clearPurchaseState()
      }
      setCurrentStep('cancelled')
    } else {
      // success or no status: the user believes they paid; poll the
      // authoritative server status. A fresh empty paymentContext hides the
      // provider "redirect to checkout" prompt — that page is behind us.
      setLastAttemptTargetId(usePurchaseFlowStore.getState().targetId ?? null)
      setPaymentAttempt(
        queryAttemptId,
        'Pending',
        {},
        new Date(Date.now() + PAYMENT_ATTEMPT_FALLBACK_TTL_MS).toISOString()
      )
      setConfirmingCheckout(true)
      setCurrentStep('processing')
    }
    // The bounce params are consumed — strip them so a refresh or a shared
    // link does not replay the bounce. wechatOpenid must survive: it unlocks
    // JSAPI ordering for a subsequent retry.
    navigate({
      to: '.',
      search: (prev: Record<string, unknown>) => ({
        ...prev,
        attemptId: undefined,
        status: undefined,
      }),
      replace: true,
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [queryAttemptId, queryStatus])

  // Shared re-entry rule for when an attempt ends without success: the
  // payment step needs a selected price for its provider cards to render,
  // so re-seed the selection from the tracked purchase target when the
  // React selection is gone (e.g. after a full-page provider bounce), and
  // fall back to package picking when neither survives.
  const reenterPaymentStep = (restorable: string | null) => {
    if (restorable && !selectedMappingId) setSelectedMappingId(restorable)
    setCurrentStep(restorable || selectedMappingId ? 'payment' : 'packages')
  }

  useEffect(() => {
    if (paymentStatus) {
      if (paymentStatus.status === 'Succeeded') {
        setPaidAmount(
          paymentStatus.amount != null && paymentStatus.currency
            ? { amount: paymentStatus.amount, currency: paymentStatus.currency }
            : null
        )
        setConfirmingCheckout(false)
        setCurrentStep('complete')
        clearPurchaseState()
        if (user?.id) {
          queryClient.invalidateQueries({ queryKey: queryKeys.walletsByBucket(realmId) })
        }
        queryClient.invalidateQueries({ queryKey: queryKeys.purchaseHistory(realmId, {}) })
      } else if (
        paymentStatus.status === 'Failed' ||
        paymentStatus.status === 'Cancelled' ||
        paymentStatus.status === 'Expired'
      ) {
        setConfirmingCheckout(false)
        // After a full-page bounce the React selection is gone, so re-seed it
        // from the tracked purchase target.
        const restorable = usePurchaseFlowStore.getState().targetId ?? lastAttemptTargetId
        clearPurchaseState()
        reenterPaymentStep(restorable)
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [paymentStatus])

  // Whether the dedicated "Select Payment Method" step is reachable. When the
  // selected price's provider resolves to at most one matching provider, the
  // provider is fully determined and that step is redundant (we jump straight
  // from selection to processing). Mirrors the branch in `handleNextStep` so
  // the step indicator only lists steps the user can actually visit.
  const paymentStepSkipped = useMemo(() => {
    if (!selectedOption?.paymentProvider) return true
    const matching = providers?.filter((p) => p.platform === selectedOption.paymentProvider) ?? []
    return matching.length <= 1
  }, [providers, selectedOption?.paymentProvider])

  // Create payment attempt. The targetType/targetId shape is unchanged from the
  // prior flow (entitlement_mapping + mappingId); only the selection model that
  // feeds mappingId changed (price-level vs entitlement-key-resolved).
  const createPaymentMutation = useMutation({
    mutationFn: async (data: { mappingId: string; provider: string }) => {
      const response = await createPaymentAttempt({
        body: {
          targetType: 'entitlement_mapping',
          targetId: data.mappingId,
          paymentProvider: data.provider,
          locale: appLocale,
          ...(data.provider === PAYMENT_PROVIDERS.WECHAT && wechatSceneDecision.scene === 'jsapi'
            ? { paymentScene: 'jsapi', openid: wechatSceneDecision.openid }
            : {}),
        },
      })
      if (response.error) throw response.error
      return response.data
    },
    onSuccess: (data) => {
      if (data && data.id) {
        setPurchaseState({
          realmId,
          userId: user?.id || null,
          targetType: 'entitlement_mapping',
          targetId: selectedMappingId,
          paymentProvider: selectedOption?.paymentProvider ?? null,
        })
        setLastAttemptTargetId(selectedMappingId)
        setPaymentAttempt(
          data.id,
          'Pending',
          data.paymentContext || { paymentProvider: selectedOption?.paymentProvider ?? '' },
          data.expiresAt || new Date(Date.now() + PAYMENT_ATTEMPT_FALLBACK_TTL_MS).toISOString()
        )

        queryClient.invalidateQueries({ queryKey: ['payment-attempt-status', realmId, data.id] })

        // Same-tab redirect to the provider's checkout page — no "processing"
        // interstitial, no new tab. The provider bounces the user back to this
        // route with `?attemptId=...`, which re-enters the processing step
        // solely to poll for the webhook-confirmed final status. WeChat never
        // redirects: its pending UI (Native QR / JSAPI invoke) is the
        // processing step itself. When no checkout URL was returned
        // (degraded), fall back to the processing step so its degraded UI can
        // offer retry/cancel.
        const provider = selectedOption?.paymentProvider ?? null
        const checkoutUrl =
          provider === 'stripe'
            ? (data.paymentContext?.stripeCheckoutUrl ?? null)
            : provider === 'creem'
              ? (data.paymentContext?.creemCheckoutUrl ?? null)
              : null
        if (checkoutUrl) {
          window.location.href = checkoutUrl
          return
        }
        setCurrentStep('processing')
      }
    },
    onError: (error: unknown) => {
      toast.error(m['points.purchase_create_failed']({ message: getErrorMessage(error) }))
    },
  })

  const cancelPaymentMutation = useMutation({
    mutationFn: async () => {
      if (!attemptId) throw new Error('No payment attempt to cancel')
      const response = await cancelPaymentAttempt({ path: { attemptId } })
      if (response.error) throw response.error
      return response.data
    },
    onSuccess: () => {
      // Capture the target before clearing: the payment step needs a selected
      // price to render its provider cards.
      const targetId = usePurchaseFlowStore.getState().targetId
      clearPurchaseState()
      reenterPaymentStep(targetId)
      toast.info(m['points.purchase_cancelled']())
    },
    onError: (error: unknown) => {
      toast.error(m['points.purchase_cancel_failed']({ message: getErrorMessage(error) }))
    },
  })

  // Single gate for dispatching a payment attempt. WeChat inside WeChat's
  // browser without a caller-provided openid must not order at all: a Native
  // QR cannot be scanned by the same device and JSAPI cannot be created.
  const initiatePayment = (mappingId: string, provider: string) => {
    // A new attempt replaces any bounce-driven "confirming" wait.
    setConfirmingCheckout(false)
    if (
      provider === PAYMENT_PROVIDERS.WECHAT &&
      wechatSceneDecision.scene === 'jsapi_unavailable'
    ) {
      toast.error(m['points.payment_wechat_jsapi_missing_openid']())
      return
    }
    createPaymentMutation.mutate({ mappingId, provider })
  }

  const handleNextStep = () => {
    if (currentStep === 'packages' && selectedMappingId) {
      const provider = selectedOption?.paymentProvider
      const availableForOption = providers?.filter((p) => p.platform === provider) ?? []
      // When the selected price determines exactly one (or no) provider, the
      // provider is fully determined and the "Select Payment Method" step is
      // redundant; proceed straight to creating the payment attempt. Only show
      // the payment step when more than one provider actually matches.
      if (provider && availableForOption.length <= 1) {
        initiatePayment(selectedMappingId, provider)
      } else {
        setCurrentStep('payment')
      }
    } else if (currentStep === 'payment' && selectedMappingId && selectedOption?.paymentProvider) {
      initiatePayment(selectedMappingId, selectedOption.paymentProvider)
    }
  }

  const handlePreviousStep = () => {
    if (currentStep === 'payment') {
      setCurrentStep('packages')
    }
  }

  // Retry after a Failed/Expired attempt: the flow store was already cleared
  // by the terminal-status effect, so the selection is re-seeded from the
  // captured attempt target.
  const handleRetry = () => reenterPaymentStep(lastAttemptTargetId)
  const handleCancel = () => cancelPaymentMutation.mutate()
  const handleComplete = () =>
    navigate({ to: realmPath({ ...realmContext, realmId }, '/user/points') })
  const handleBackToHub = () => navigate({ to: realmPath({ ...realmContext, realmId }, '/user') })

  const isNextDisabled = () => {
    if (currentStep === 'packages') return !selectedMappingId
    if (currentStep === 'payment')
      return (
        !selectedMappingId || !selectedOption?.paymentProvider || createPaymentMutation.isPending
      )
    return true
  }

  const renderStepContent = () => {
    switch (currentStep) {
      case 'packages':
        return (
          <div className="space-y-8" data-testid="purchase-step-packages">
            <div>
              <h2 className="text-base font-semibold">{m['purchase.choose_plan']()}</h2>
            </div>

            {optionsLoading ? (
              <div className="flex items-center justify-center py-12">
                <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
              </div>
            ) : !hasAnyOptions ? (
              <div
                className="rounded-lg border border-dashed p-8 text-center text-muted-foreground"
                data-testid="purchase-empty-state"
              >
                {m['points.purchase_no_mappings']()}
              </div>
            ) : (
              <>
                {/* Subscriptions section — recurring only. All recurring options
                    are shown together (monthly + annual); no period toggle.
                    Each entitlement is its own block so multi-currency Stripe
                    products get a currency switcher inside it. */}
                {hasRecurring && (
                  <section className="space-y-4" data-testid="purchase-section-subscriptions">
                    <div>
                      <h3 className="text-lg font-semibold">
                        {m['purchase.section_subscriptions']()}
                      </h3>
                      <p className="text-sm text-muted-foreground">
                        {m['purchase.section_subscriptions_meta']()}
                      </p>
                    </div>

                    <div className="space-y-6" data-testid="purchase-price-grid-subscriptions">
                      {subscriptionGroups.map((group) => (
                        <CurrencyPurchaseGroup
                          key={group.entitlementKey}
                          group={group}
                          selectedMappingId={selectedMappingId}
                          onSelect={setSelectedMappingId}
                        />
                      ))}
                    </div>
                  </section>
                )}

                {/* Credit packs section — one_time only, no period toggle. */}
                {creditPackOptions.length > 0 && (
                  <section className="space-y-4" data-testid="purchase-section-credit-packs">
                    <div>
                      <h3 className="text-lg font-semibold">
                        {m['purchase.section_credit_packs']()}
                      </h3>
                      <p className="text-sm text-muted-foreground">
                        {m['purchase.section_credit_packs_meta']()}
                      </p>
                    </div>
                    <div className="space-y-6" data-testid="purchase-price-grid-credit-packs">
                      {creditPackGroups.map((group) => (
                        <CurrencyPurchaseGroup
                          key={group.entitlementKey}
                          group={group}
                          selectedMappingId={selectedMappingId}
                          onSelect={setSelectedMappingId}
                        />
                      ))}
                    </div>
                  </section>
                )}
              </>
            )}
          </div>
        )

      case 'payment':
        return (
          <div className="space-y-6" data-testid="purchase-step-payment">
            <div>
              <h2 className="text-base font-semibold">{m['points.purchase_payment_title']()}</h2>
              <p className="text-muted-foreground">
                {m['points.purchase_payment_grants_description']({
                  grants: selectedOption
                    ? m['points.purchase_account_grants']({
                        count: selectedOption.pointRules.length,
                      })
                    : '',
                  price: selectedOption
                    ? selectedOption.amount != null && selectedOption.currency
                      ? formatInvoiceAmount(selectedOption.amount, selectedOption.currency)
                      : m['points.purchase_price_at_checkout']()
                    : '',
                })}
              </p>
            </div>
            <PaymentMethodSelector
              availableProviders={
                providers?.filter((p) => p.platform === selectedOption?.paymentProvider) ?? []
              }
              selectedProvider={selectedOption?.paymentProvider ?? null}
              onSelect={() => {
                /* provider is derived from the selected price; no-op */
              }}
              disabled={providersLoading || createPaymentMutation.isPending}
            />
          </div>
        )

      case 'processing': {
        // A success bounce must not show provider "go pay" UI while the
        // webhook lands: intercept every not-yet-final status with a waiting
        // panel that tells the user not to pay twice.
        const awaitingFinalStatus =
          !paymentStatus ||
          paymentStatus.status === 'Pending' ||
          paymentStatus.status === 'RequiresAction'
        if (confirmingCheckout && !paymentStatusQuery.isError && awaitingFinalStatus) {
          return (
            <div className="space-y-6 text-center" data-testid="purchase-confirming">
              <div className="flex justify-center">
                <Loader2 className="h-12 w-12 animate-spin text-primary" />
              </div>
              <div className="space-y-1">
                <h2 className="text-base font-semibold">
                  {m['points.purchase_confirming_title']()}
                </h2>
                <p className="text-sm text-muted-foreground">
                  {m['points.purchase_confirming_description']()}
                </p>
              </div>
              <p className="text-xs text-muted-foreground">
                {m['points.purchase_confirming_slow_hint']()}
              </p>
            </div>
          )
        }
        return (
          <div className="space-y-6" data-testid="purchase-step-processing">
            {paymentStatus && attemptId ? (
              <PaymentAttemptStatus
                status={paymentStatus}
                paymentProvider={paymentProvider}
                paymentContext={paymentAttempt.paymentContext}
                onRetry={handleRetry}
                onCancel={handleCancel}
                isRetrying={createPaymentMutation.isPending}
                isCancelling={cancelPaymentMutation.isPending}
              />
            ) : paymentStatusQuery.isError ? (
              <div className="space-y-4" data-testid="payment-status-error">
                <div className="flex items-center gap-3">
                  <AlertCircle className="h-8 w-8 text-destructive" />
                  <div>
                    <h3 className="text-lg font-semibold">
                      {m['points.purchase_processing_load_failed']()}
                    </h3>
                    <p className="text-sm text-muted-foreground">
                      {m['points.purchase_processing_load_failed_description']()}
                    </p>
                  </div>
                </div>
                <div className="flex flex-wrap gap-2">
                  <Button onClick={() => paymentStatusQuery.refetch()}>
                    {m['common.retry']()}
                  </Button>
                  <Button variant="outline" onClick={handleRetry}>
                    {m['points.purchase_processing_back_payment']()}
                  </Button>
                </div>
              </div>
            ) : (
              <div
                className="flex items-center gap-3 text-muted-foreground"
                data-testid="payment-status-loading"
              >
                <Loader2 className="h-8 w-8 animate-spin" />
                <div>
                  <h3 className="text-lg font-semibold text-foreground">
                    {m['points.purchase_processing_checking']()}
                  </h3>
                  <p className="text-sm">{m['points.purchase_processing_waiting']()}</p>
                </div>
              </div>
            )}
          </div>
        )
      }

      case 'cancelled':
        return (
          <div className="space-y-6 text-center" data-testid="purchase-step-cancelled">
            <div className="flex justify-center">
              <XCircle className="h-16 w-16 text-muted-foreground" />
            </div>
            <div className="space-y-1">
              <h2 className="text-base font-semibold">{m['points.purchase_cancelled']()}</h2>
              <p className="text-sm text-muted-foreground">
                {m['points.purchase_cancel_bounce_description']()}
              </p>
            </div>
            {selectedOption && selectedOption.amount != null && selectedOption.currency ? (
              <p className="text-sm text-muted-foreground" data-testid="purchase-cancel-summary">
                {formatInvoiceAmount(selectedOption.amount, selectedOption.currency)}
              </p>
            ) : null}
            <div className="flex flex-col items-stretch justify-center gap-2 sm:flex-row sm:items-center">
              {selectedOption?.paymentProvider ? (
                <Button
                  onClick={() =>
                    initiatePayment(selectedOption.mappingId, selectedOption.paymentProvider!)
                  }
                  disabled={createPaymentMutation.isPending}
                  className="h-11 w-full sm:h-9 sm:w-auto"
                  data-testid="purchase-cancel-retry"
                >
                  {createPaymentMutation.isPending
                    ? m['points.purchase_processing_button']()
                    : m['points.purchase_cancel_bounce_retry']()}
                </Button>
              ) : null}
              <Button
                variant={selectedOption?.paymentProvider ? 'outline' : 'default'}
                onClick={() => setCurrentStep('packages')}
                className="h-11 w-full sm:h-9 sm:w-auto"
                data-testid="purchase-cancel-repick"
              >
                {m['points.purchase_cancel_bounce_repick']()}
              </Button>
              <Button
                variant="ghost"
                onClick={handleComplete}
                className="h-11 w-full sm:h-9 sm:w-auto"
                data-testid="purchase-cancel-view-points"
              >
                {m['points.purchase_view_points']()}
              </Button>
            </div>
          </div>
        )

      case 'complete':
        return (
          <div className="space-y-6 text-center" data-testid="purchase-step-complete">
            <div className="flex justify-center">
              <CheckCircle2 className="h-16 w-16 text-success" />
            </div>
            <div className="space-y-1">
              <h2 className="text-base font-semibold">{m['points.purchase_complete_title']()}</h2>
              <p className="text-sm text-muted-foreground">
                {m['points.purchase_complete_description']()}
              </p>
            </div>
            {paidAmount ? (
              <p className="text-sm text-muted-foreground" data-testid="purchase-complete-amount">
                {m['points.purchase_paid_amount']({
                  amount: formatInvoiceAmount(paidAmount.amount, paidAmount.currency),
                })}
              </p>
            ) : null}
            <div className="flex flex-col items-stretch justify-center gap-2 sm:flex-row sm:items-center">
              <Button
                onClick={handleComplete}
                className="h-11 w-full sm:h-9 sm:w-auto"
                data-testid="purchase-complete-view-points"
              >
                {m['points.purchase_view_points']()}
              </Button>
              <Button
                variant="outline"
                onClick={handleBackToHub}
                className="h-11 w-full sm:h-9 sm:w-auto"
              >
                {m['profile_hub.back']()}
              </Button>
            </div>
          </div>
        )

      default:
        return null
    }
  }

  // Each chip wraps as a unit, keeping the indicator usable on narrow
  // screens. "Processing" is not a user step — it highlights the final chip
  // while the payment outcome is being confirmed.
  const indicatorSteps = useMemo(() => {
    const steps: Array<{ key: 'packages' | 'payment' | 'complete'; label: string }> = [
      { key: 'packages', label: m['points.purchase_step_select']() },
    ]
    if (!paymentStepSkipped)
      steps.push({ key: 'payment', label: m['points.purchase_step_payment']() })
    steps.push({ key: 'complete', label: m['points.purchase_step_complete']() })
    return steps
  }, [paymentStepSkipped])
  const activeIndicatorKey: 'packages' | 'payment' | 'complete' | null =
    currentStep === 'processing' || currentStep === 'complete'
      ? 'complete'
      : currentStep === 'packages' || currentStep === 'payment'
        ? currentStep
        : null

  return (
    <div className="container" data-testid="purchase-points-page">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h1 className="text-xl font-semibold tracking-tight">
          {m['points.purchase_page_title']()}
        </h1>
        {/* The cancelled result view stands alone — a step trail with nothing
            to highlight would only add noise. */}
        {activeIndicatorKey && (
          <div
            className="flex flex-wrap items-center gap-1.5"
            data-testid="purchase-step-indicator"
          >
            {indicatorSteps.map((step, index) => {
              const isActive = step.key === activeIndicatorKey
              return (
                <span
                  key={step.key}
                  className={`inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs ${
                    isActive
                      ? 'border-primary bg-primary/10 font-medium text-primary'
                      : 'border-border text-muted-foreground'
                  }`}
                >
                  <span className="font-mono">{index + 1}</span>
                  {step.label}
                </span>
              )
            })}
          </div>
        )}
      </div>
      <div className="mt-4 border-t border-border pt-6">
        <div className="space-y-6">{renderStepContent()}</div>

        {currentStep === 'packages' || currentStep === 'payment' ? (
          <div className="mt-6 flex flex-wrap items-center justify-between gap-2">
            <Button
              variant="outline"
              onClick={handlePreviousStep}
              disabled={currentStep === 'packages'}
              data-testid="purchase-back-button"
            >
              <ArrowLeft className="mr-2 h-4 w-4" />
              {m['points.purchase_back']()}
            </Button>
            <Button
              onClick={handleNextStep}
              disabled={isNextDisabled()}
              className="h-11 md:h-9"
              data-testid="purchase-next-button"
            >
              {currentStep === 'payment' ? (
                <>
                  {createPaymentMutation.isPending
                    ? m['points.purchase_processing_button']()
                    : m['purchase.continue_to_checkout']()}
                  <ArrowRight className="ml-2 h-4 w-4" />
                </>
              ) : (
                <>
                  {m['points.purchase_next']()}
                  <ArrowRight className="ml-2 h-4 w-4" />
                </>
              )}
            </Button>
          </div>
        ) : null}
      </div>
    </div>
  )
}
