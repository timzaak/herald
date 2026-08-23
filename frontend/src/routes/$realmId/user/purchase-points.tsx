import { m } from '@/paraglide/messages'
import { useState, useEffect, useMemo } from 'react'
import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Button } from '@/components/ui/button'
import { AlertCircle, ArrowLeft, ArrowRight, Loader2, CheckCircle2 } from 'lucide-react'
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
import { useAuthStore } from '@/stores/auth-store'
import { initializeAuth } from '@/lib/auth-utils'
import { USER_ACCOUNT_CENTER_CLIENT_ID } from '@/lib/constants/auth-constants'
import { PAYMENT_PROVIDERS } from '@/lib/billing-constants'
import { resolveWechatScene } from '@/lib/wechat-pay-utils'
import { formatInvoiceAmount } from '@/lib/invoice-utils'
import { toast } from 'sonner'
import { purchasePointsSearchSchema } from '@/lib/schemas/search-params'
import { useCurrentSearch, useResolvedRealmId } from '@/lib/realm-routing'
import { getErrorMessage } from '@/lib/error-utils'

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

type PurchaseStep = 'packages' | 'payment' | 'processing' | 'complete'

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
  const queryClient = useQueryClient()
  const user = useAuthStore((state) => state.user)

  // Purchase flow state. Selection is now price-level: the clicked card's
  // mappingId is the checkout target directly. The provider is
  // derived from the selected option, not picked separately.
  const [currentStep, setCurrentStep] = useState<PurchaseStep>('packages')
  const [selectedMappingId, setSelectedMappingId] = useState<string | null>(null)

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
  // `?attemptId=...`, sync the store so the polling effect above picks it up
  // — this works even if localStorage was cleared (query is the source of
  // truth, persist is only a same-browser fallback). A `cancel` bounce means
  // the user abandoned checkout at Stripe, so drop the attempt and step back.
  useEffect(() => {
    if (!queryAttemptId) return
    if (queryStatus === 'cancel') {
      clearPurchaseState()
      setCurrentStep('payment')
      return
    }
    // success or no status: ensure the store carries this attemptId so polling
    // (gated on `attemptId`) fires. Skip if the store already tracks it — the
    // webhook/polling will have updated status and we must not clobber it.
    // The paymentContext is empty on bounce: the user already paid, so the
    // checkout URL is irrelevant and PaymentAttemptStatus won't render a
    // redirect prompt once status leaves Pending.
    if (queryAttemptId !== attemptId) {
      setPaymentAttempt(
        queryAttemptId,
        'Pending',
        {},
        new Date(Date.now() + 15 * 60 * 1000).toISOString()
      )
      setCurrentStep('processing')
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [queryAttemptId, queryStatus])

  useEffect(() => {
    if (paymentStatus) {
      if (paymentStatus.status === 'Succeeded') {
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
        setCurrentStep('payment')
        clearPurchaseState()
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
        path: { realmId },
        body: {
          targetType: 'entitlement_mapping',
          targetId: data.mappingId,
          paymentProvider: data.provider,
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

        setPaymentAttempt(
          data.id,
          'Pending',
          data.paymentContext || { paymentProvider: selectedOption?.paymentProvider ?? '' },
          data.expiresAt || new Date(Date.now() + 15 * 60 * 1000).toISOString()
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
      const response = await cancelPaymentAttempt({ path: { realmId, attemptId } })
      if (response.error) throw response.error
      return response.data
    },
    onSuccess: () => {
      clearPurchaseState()
      setCurrentStep('payment')
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

  const handleRetry = () => setCurrentStep('payment')
  const handleCancel = () => cancelPaymentMutation.mutate()
  const handleComplete = () => navigate({ to: `/${realmId}/user/points` })

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

      case 'processing':
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
                <div className="flex gap-2">
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

      case 'complete':
        return (
          <div className="space-y-6 text-center" data-testid="purchase-step-complete">
            <div className="flex justify-center">
              <CheckCircle2 className="h-16 w-16 text-success" />
            </div>
            <div>
              <h2 className="text-base font-semibold">{m['points.purchase_complete_title']()}</h2>
              <p className="text-muted-foreground">{m['points.purchase_complete_description']()}</p>
            </div>
            <Button onClick={handleComplete}>{m['points.purchase_view_points']()}</Button>
          </div>
        )

      default:
        return null
    }
  }

  return (
    <div className="container" data-testid="purchase-points-page">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h1 className="text-xl font-semibold tracking-tight">
          {m['points.purchase_page_title']()}
        </h1>
        <div
          className="flex items-center gap-2 text-sm text-muted-foreground"
          data-testid="purchase-step-indicator"
        >
          <span className={currentStep === 'packages' ? 'font-bold text-primary' : ''}>
            {m['points.purchase_step_select']()}
          </span>
          <span>→</span>
          {/* The "Payment" step only appears when more than one provider
              matches the selected price (see `paymentStepSkipped`). When
              skipped we omit it from the indicator so the trail reflects
              the steps the user will actually visit. */}
          {!paymentStepSkipped && (
            <>
              <span className={currentStep === 'payment' ? 'font-bold text-primary' : ''}>
                {m['points.purchase_step_payment']()}
              </span>
              <span>→</span>
            </>
          )}
          <span className={currentStep === 'processing' ? 'font-bold text-primary' : ''}>
            {m['points.purchase_step_processing']()}
          </span>
          <span>→</span>
          <span className={currentStep === 'complete' ? 'font-bold text-primary' : ''}>
            {m['points.purchase_step_complete']()}
          </span>
        </div>
      </div>
      <div className="mt-4 border-t border-border pt-6">
        <div className="space-y-6">{renderStepContent()}</div>

        {currentStep === 'packages' || currentStep === 'payment' ? (
          <div className="mt-6 flex justify-between">
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
