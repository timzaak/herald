import { useMemo, useState, type ReactNode } from 'react'
import { useNavigate } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { toast } from 'sonner'
import { ArrowLeft } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { RoleSelector } from '@/components/shared/role-selector'
import { PointDistributionRuleEditor } from '@/components/billing/point-distribution-rule-editor'
import { formatProviderName } from '@/components/billing/format-provider-name'
import { pointRuleTriggersForBillingType } from '@/components/billing/provider-product-info'
import { creditBucketsListQueryOptions, adminRolesQueryOptions } from '@/data/query-options'
import {
  useCreateEntitlementMapping,
  isCreateMappingDuplicateError,
  isCreateMappingConfigError,
} from '@/data/entitlement-mapping-mutations'
import {
  createEntitlementMappingSchema,
  getCreateEntitlementMappingDefaults,
  majorUnitsToMinor,
  type CreateEntitlementMappingFormData,
} from '@/lib/schemas/create-entitlement-mapping'
import {
  PAYMENT_PROVIDERS,
  BILLING_PERIODS,
  providerAllowsRecurringBilling,
  providerRequiresManualPrice,
  providerShowsExternalPriceId,
} from '@/lib/billing-constants'
import { realmPath, useResolvedRealmContext } from '@/lib/realm-routing'
import type { CreateEntitlementMappingRequest } from '@/lib/api-generated'
import { m } from '@/paraglide/messages'

interface CreateEntitlementMappingPageProps {
  realmId: string
  /** Whether the caller may edit credit-strategy fields (points.manage). */
  canManagePoints: boolean
}

/**
 * Shared label + required marker + field-error scaffolding for the form's
 * standard fields. The form drives its inputs with local `useState` (not a
 * tanstack form instance), so it can't reuse the shared `TextField`/`PasswordField`
 * wrappers — this local wrapper centralizes the markup those fields repeat.
 */
function Field({
  label,
  htmlFor,
  required = false,
  error,
  children,
}: {
  label: string
  htmlFor?: string
  required?: boolean
  error?: string
  children: ReactNode
}) {
  return (
    <div className="space-y-1.5">
      <Label htmlFor={htmlFor}>
        {label}
        {required && <span className="ml-0.5 text-destructive">*</span>}
      </Label>
      {children}
      {error && (
        <p className="text-xs text-destructive" role="alert">
          {error}
        </p>
      )}
    </div>
  )
}

/**
 * Full-page form that hand-builds a single entitlement mapping and POSTs it
 * via `useCreateEntitlementMapping`. Sits at
 * /manage/billing/entitlement-mappings/new (replacing the former create
 * dialog — the provider-conditional fields plus the role multiselect and the
 * points-rule editor outgrew a dialog shell). On success it navigates back
 * to the mappings list (the mutation already invalidates
 * `['entitlement-mappings']` so the list refreshes).
 *
 * - 409 → `billing.create_mapping_duplicate` ("product id already exists").
 * - 23514 / non-4xx → `billing.create_mapping_config_error`
 *   ("configuration error"; DB CHECK / server defense).
 * - other (incl. 400/401/403) → generic toast owned by the mutation.
 */
export function CreateEntitlementMappingPage({
  realmId,
  canManagePoints,
}: CreateEntitlementMappingPageProps) {
  const navigate = useNavigate()
  const realmContext = useResolvedRealmContext()
  const mappingsPath = realmPath(
    { ...realmContext, realmId },
    '/manage/billing/entitlement-mappings'
  )

  const createMutation = useCreateEntitlementMapping(realmId)

  const [values, setValues] = useState<CreateEntitlementMappingFormData>(
    getCreateEntitlementMappingDefaults
  )
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})
  const [submitError, setSubmitError] = useState<string | null>(null)

  const { data: buckets } = useQuery(creditBucketsListQueryOptions(realmId))
  const { data: roles } = useQuery(adminRolesQueryOptions(realmId))
  const assignableRoles = useMemo(() => (roles ?? []).filter((r) => !r.isBuiltin), [roles])

  const isRecurring = values.billingType === 'recurring'
  const isOneTime = values.billingType === 'one_time'
  const isNonRenewing = values.billingType === 'non_renewing'
  // Provider-scoped form rules (single decision points in billing-constants):
  // WeChat prices by hand (no hosted catalog) and cannot be recurring.
  const isWechat = providerRequiresManualPrice(values.paymentProvider)
  const showExternalPriceId = providerShowsExternalPriceId(values.paymentProvider)
  const canBeRecurring = providerAllowsRecurringBilling(values.paymentProvider)

  const update = <K extends keyof CreateEntitlementMappingFormData>(
    key: K,
    next: CreateEntitlementMappingFormData[K]
  ) => {
    setValues((prev) => ({ ...prev, [key]: next }))
    // Clear a field-level error as soon as the field is edited.
    setFieldErrors((prev) => {
      if (!prev[key as string]) return prev
      const { [key as string]: _removed, ...rest } = prev
      return rest
    })
  }

  // Switching provider invalidates provider-scoped selections: recurring is
  // unreachable for WeChat (no auto-renewal), and a stale manual price must
  // not silently ride along to a catalog provider.
  const updateProvider = (provider: string) => {
    setValues((prev) => ({
      ...prev,
      paymentProvider: provider,
      ...(providerRequiresManualPrice(provider) ? {} : { priceYuan: '', currency: 'CNY' }),
      ...(!providerAllowsRecurringBilling(provider) && prev.billingType === 'recurring'
        ? { billingType: '', billingPeriod: null }
        : {}),
    }))
    setFieldErrors({})
  }

  const handleSubmit = () => {
    const parsed = createEntitlementMappingSchema.safeParse(values)
    if (!parsed.success) {
      const errors: Record<string, string> = {}
      for (const issue of parsed.error.issues) {
        const path = issue.path[0]
        if (typeof path === 'string' && !errors[path]) {
          errors[path] = issue.message
        }
      }
      setFieldErrors(errors)
      return
    }

    const v = parsed.data
    const isWechat = providerRequiresManualPrice(v.paymentProvider)
    const body: CreateEntitlementMappingRequest = {
      paymentProvider: v.paymentProvider,
      externalProductId: v.externalProductId,
      entitlementKey: v.entitlementKey,
      billingType: v.billingType as 'recurring' | 'one_time' | 'non_renewing',
      // externalPriceId: optional; only catalog providers (Stripe/Creem) show it.
      externalPriceId: v.externalPriceId ? v.externalPriceId : null,
      billingPeriod: isRecurring ? (v.billingPeriod ?? null) : null,
      serviceDurationDays: isNonRenewing ? (v.serviceDurationDays ?? null) : null,
      // Manual WeChat price: the schema's superRefine guarantees a valid
      // major-unit price string and currency here; convert to integer minor
      // units for the API. No fallback — if the guarantee ever breaks, the
      // omitted field fails loud at the backend instead of silently writing
      // a wrong price.
      ...(isWechat ? { price: majorUnitsToMinor(v.priceYuan)!, currency: v.currency! } : {}),
      ...(canManagePoints ? { pointRules: v.pointRules } : {}),
      grantedRoleIds: v.grantedRoleIds,
      enabled: v.enabled,
    }

    createMutation.mutate(body, {
      onSuccess: () => {
        toast.success(m['billing.create_mapping_success']())
        navigate({ to: mappingsPath })
      },
      onError: (error) => {
        // 409 duplicate: the only fix is editing the inputs (provider/product/
        // price), so surface on the externalProductId field AND as a toast.
        if (isCreateMappingDuplicateError(error)) {
          const msg = m['billing.create_mapping_duplicate']()
          setSubmitError(msg)
          toast.error(msg)
          return
        }
        // 23514 / non-4xx: DB CHECK / server defense — a configuration error,
        if (isCreateMappingConfigError(error)) {
          const msg = m['billing.create_mapping_config_error']()
          setSubmitError(msg)
          toast.error(msg)
          return
        }
        // Other errors (400 validation, 401/403 authz) are toasted by the
        // mutation's generic onError. Surface inline too for visibility.
        setSubmitError(m['billing.create_mapping_failed']())
      },
    })
  }

  return (
    <div className="space-y-6" data-testid="create-entitlement-mapping-page">
      <div className="flex items-center gap-4">
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={() => navigate({ to: mappingsPath })}
          data-testid="create-mapping-back-button"
        >
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <div>
          <h1 className="text-2xl font-bold" data-testid="create-mapping-page-title">
            {m['billing.create_mapping_title']()}
          </h1>
          <p className="text-muted-foreground text-sm">
            {m['billing.create_mapping_description']()}
          </p>
        </div>
      </div>

      <Card>
        <CardContent className="space-y-4 pt-6">
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
            <Field
              label={m['billing.create_mapping_provider']()}
              htmlFor="create-mapping-provider"
              required
              error={fieldErrors.paymentProvider}
            >
              <Select value={values.paymentProvider ?? ''} onValueChange={updateProvider}>
                <SelectTrigger
                  id="create-mapping-provider"
                  data-testid="create-mapping-provider-select"
                >
                  <SelectValue placeholder={m['billing.create_mapping_provider_placeholder']()} />
                </SelectTrigger>
                <SelectContent>
                  {Object.values(PAYMENT_PROVIDERS).map((p) => (
                    <SelectItem key={p} value={p}>
                      {formatProviderName(p)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Field>

            <Field
              label={
                isWechat
                  ? m['billing.create_mapping_external_product_id_wechat']()
                  : m['billing.create_mapping_external_product_id']()
              }
              htmlFor="create-mapping-external-product-id"
              required
              error={fieldErrors.externalProductId}
            >
              <Input
                id="create-mapping-external-product-id"
                value={values.externalProductId}
                onChange={(e) => update('externalProductId', e.target.value)}
                data-testid="create-mapping-external-product-id-input"
              />
              <p className="text-xs text-muted-foreground">
                {isWechat
                  ? m['billing.create_mapping_external_product_id_wechat_hint']()
                  : m['billing.create_mapping_external_product_id_hint']()}
              </p>
            </Field>

            {/* External Price ID — catalog providers only (Stripe/Creem take it
                from their dashboard; IAP store ids and WeChat's self-defined
                product ids have no price-id counterpart). */}
            {showExternalPriceId && (
              <Field
                label={m['billing.create_mapping_external_price_id']()}
                htmlFor="create-mapping-external-price-id"
              >
                <Input
                  id="create-mapping-external-price-id"
                  value={values.externalPriceId ?? ''}
                  onChange={(e) =>
                    update('externalPriceId', e.target.value === '' ? null : e.target.value)
                  }
                  data-testid="create-mapping-external-price-id-input"
                />
              </Field>
            )}

            {isWechat && (
              <>
                <Field
                  label={m['billing.create_mapping_price_label']()}
                  htmlFor="create-mapping-price"
                  required
                  error={fieldErrors.priceYuan}
                >
                  <Input
                    id="create-mapping-price"
                    inputMode="decimal"
                    placeholder="19.9"
                    value={values.priceYuan ?? ''}
                    onChange={(e) => update('priceYuan', e.target.value)}
                    data-testid="create-mapping-price-input"
                  />
                  <p className="text-xs text-muted-foreground">
                    {m['billing.create_mapping_price_hint']()}
                  </p>
                </Field>
                <Field
                  label={m['billing.create_mapping_currency_label']()}
                  htmlFor="create-mapping-currency"
                  required
                  error={fieldErrors.currency}
                >
                  <Input
                    id="create-mapping-currency"
                    value={values.currency ?? ''}
                    onChange={(e) => update('currency', e.target.value.toUpperCase())}
                    data-testid="create-mapping-currency-input"
                  />
                </Field>
              </>
            )}

            <Field
              label={m['billing.create_mapping_entitlement_key']()}
              htmlFor="create-mapping-entitlement-key"
              required
              error={fieldErrors.entitlementKey}
            >
              <Input
                id="create-mapping-entitlement-key"
                value={values.entitlementKey}
                onChange={(e) => update('entitlementKey', e.target.value)}
                data-testid="create-mapping-entitlement-key-input"
              />
            </Field>

            <Field
              label={m['billing.create_mapping_billing_type']()}
              htmlFor="create-mapping-billing-type"
              required
              error={fieldErrors.billingType}
            >
              <Select
                value={values.billingType ?? ''}
                onValueChange={(v) =>
                  update('billingType', v as 'recurring' | 'one_time' | 'non_renewing')
                }
              >
                <SelectTrigger
                  id="create-mapping-billing-type"
                  data-testid="create-mapping-billing-type-select"
                >
                  <SelectValue
                    placeholder={m['billing.create_mapping_billing_type_placeholder']()}
                  />
                </SelectTrigger>
                <SelectContent>
                  {canBeRecurring && (
                    <SelectItem value="recurring">
                      {m['billing.billing_type_recurring']()}
                    </SelectItem>
                  )}
                  <SelectItem value="one_time">{m['billing.billing_type_one_time']()}</SelectItem>
                  <SelectItem value="non_renewing">
                    {m['billing.billing_type_non_renewing']()}
                  </SelectItem>
                </SelectContent>
              </Select>
            </Field>

            {isRecurring && (
              <Field
                label={m['billing.create_mapping_billing_period']()}
                htmlFor="create-mapping-billing-period"
                required
                error={fieldErrors.billingPeriod}
              >
                <Select
                  value={values.billingPeriod ?? ''}
                  onValueChange={(v) => update('billingPeriod', v)}
                >
                  <SelectTrigger
                    id="create-mapping-billing-period"
                    data-testid="create-mapping-billing-period-select"
                  >
                    <SelectValue
                      placeholder={m['billing.create_mapping_billing_period_placeholder']()}
                    />
                  </SelectTrigger>
                  <SelectContent>
                    {Object.values(BILLING_PERIODS).map((p) => (
                      <SelectItem key={p} value={p}>
                        {p === BILLING_PERIODS.MONTHLY
                          ? m['billing.billing_period_month']()
                          : m['billing.billing_period_year']()}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </Field>
            )}

            {isNonRenewing && (
              <Field
                label={m['billing.create_mapping_service_duration_days']()}
                htmlFor="create-mapping-service-duration-days"
                required
                error={fieldErrors.serviceDurationDays}
              >
                <Input
                  id="create-mapping-service-duration-days"
                  type="number"
                  min={1}
                  value={values.serviceDurationDays ?? ''}
                  onChange={(e) =>
                    update(
                      'serviceDurationDays',
                      e.target.value === '' ? null : Number(e.target.value)
                    )
                  }
                  data-testid="create-mapping-service-duration-days-input"
                />
              </Field>
            )}

            <div className="space-y-1.5 sm:col-span-2" data-testid="create-mapping-granted-roles">
              <Label>{m['billing.create_mapping_granted_roles']()}</Label>
              <RoleSelector
                roles={assignableRoles}
                selectedRoleIds={values.grantedRoleIds ?? []}
                onChange={(ids) => update('grantedRoleIds', ids)}
                placeholder={m['billing.help_granted_roles']()}
              />
            </div>

            {canManagePoints && values.billingType && (
              <div className="space-y-2 sm:col-span-2">
                <Label>Points distribution rules</Label>
                <PointDistributionRuleEditor
                  value={values.pointRules}
                  onChange={(pointRules) => update('pointRules', pointRules)}
                  buckets={buckets ?? []}
                  triggers={pointRuleTriggersForBillingType(values.billingType)}
                  allowQuota={!isOneTime}
                />
              </div>
            )}
          </div>

          {isWechat && (
            <div
              className="rounded-md border bg-muted/40 px-3 py-2 text-xs text-muted-foreground"
              data-testid="create-mapping-wechat-notice"
            >
              {m['billing.create_mapping_wechat_notice']()}
            </div>
          )}

          {submitError && (
            <p
              className="text-sm text-destructive"
              role="alert"
              data-testid="create-mapping-submit-error"
            >
              {submitError}
            </p>
          )}

          <div className="flex justify-end gap-2 border-t pt-4">
            <Button
              variant="outline"
              onClick={() => navigate({ to: mappingsPath })}
              data-testid="create-mapping-cancel-button"
            >
              {m['billing.create_mapping_cancel']()}
            </Button>
            <Button
              onClick={handleSubmit}
              disabled={createMutation.isPending}
              data-testid="create-mapping-submit-button"
            >
              {createMutation.isPending
                ? m['billing.create_mapping_submitting']()
                : m['billing.create_mapping_submit']()}
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  )
}
