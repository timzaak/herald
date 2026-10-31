import { useState, useCallback, useMemo } from 'react'
import { useNavigate } from '@tanstack/react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { format } from 'date-fns'
import { toast } from 'sonner'
import { Plug2, AlertTriangle } from 'lucide-react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Switch } from '@/components/ui/switch'
import { Skeleton } from '@/components/ui/skeleton'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { PageHeader } from '@/components/shared'
import { ProviderSyncButton } from '@/components/billing/provider-sync-button'
import { formatProviderName } from '@/components/billing/format-provider-name'
import {
  readProviderProductInfo,
  primaryProductLabel,
  mapBillingPeriodLabel,
  isOneTimeMapping,
  isNonRenewingMapping,
  pointRuleTriggersForBillingType,
} from '@/components/billing/provider-product-info'
import { formatInvoiceAmount } from '@/lib/invoice-utils'
import { ProtectedPriceConfirmDialog } from '@/components/billing/entitlement-mapping-detail-dialog'
import { SyncNextStepDialog } from '@/components/billing/sync-next-step-dialog'
import { PointDistributionRuleEditor } from '@/components/billing/point-distribution-rule-editor'
import { RoleSelector } from '@/components/shared/role-selector'
import {
  groupByProduct,
  groupByEntitlementKey,
  productKeyOf,
  isWebhookUnresolvedPrice,
  hasWebhookUnresolvedPrice,
} from '@/components/billing/entitlement-mapping-grouping'
import { deriveSharedKeyColor } from '@/components/billing/shared-key-color'
import {
  adminRolesQueryOptions,
  creditBucketsListQueryOptions,
  entitlementMappingsQueryOptions,
  queryKeys,
} from '@/data/query-options'
import {
  useBatchUpdateEntitlementMappings,
  useUpdateEntitlementMapping,
} from '@/data/entitlement-mapping-mutations'
import {
  batchEntitlementMappingsSchema,
  toPointDistributionRuleFormData,
  type PriceMappingUpdateFormData,
} from '@/lib/schemas/billing-forms'
import { majorUnitsToMinor, minorToMajorUnits } from '@/lib/schemas/create-entitlement-mapping'
import { providerRequiresManualPrice } from '@/lib/billing-constants'
import {
  isProtectedPriceError,
  extractActiveSubscriptions,
} from '@/data/entitlement-mapping-mutations'
import { usePermission } from '@/hooks/use-permission'
import { realmPath, useResolvedRealmContext } from '@/lib/realm-routing'
import { m } from '@/paraglide/messages'
import type {
  EntitlementMappingResponse,
  EntitlementMappingListResponse,
  BatchUpdateEntitlementMappingsRequest,
  BucketResponse,
  PriceMappingUpdate,
} from '@/lib/api-generated'

interface EntitlementMappingsPageProps {
  realmId: string
}

export function EntitlementMappingsPage({ realmId }: EntitlementMappingsPageProps) {
  const queryClient = useQueryClient()
  const navigate = useNavigate()
  const realmContext = useResolvedRealmContext()
  const { hasPermission } = usePermission()
  const canManage = hasPermission('billing.manage')
  const canManagePoints = hasPermission('points.manage')

  const [selectedProductKey, setSelectedProductKey] = useState<string | null>(null)

  // Protected-price 409 confirmation state: surfaces the active-sub count for
  // the rejected batch (the backend rolls back the whole transaction; there is
  // no "force" action, so only the count is retained).
  const [protectedConfirm, setProtectedConfirm] = useState<{
    activeSubscriptions: number
  } | null>(null)

  // Post-sync guidance: synced mappings land as disabled drafts. If, right
  // after a sync, every refetched mapping is still disabled, surface a
  // "next step" dialog once. Decided synchronously in the sync-complete
  // callback (after awaiting the refetch) rather than via an effect, to
  // comply with the no-setState-in-effect rule.
  const [nextStepOpen, setNextStepOpen] = useState(false)

  const { data, isLoading } = useQuery({
    ...entitlementMappingsQueryOptions(realmId, {}),
    select: (rawData) => rawData as EntitlementMappingListResponse | undefined,
  })

  const allMappings: EntitlementMappingResponse[] = useMemo(() => data?.items ?? [], [data])

  const productGroups = useMemo(() => groupByProduct(allMappings), [allMappings])

  // Derive the EFFECTIVE selected product key without a setState effect:
  // if the user's selection still exists in the list, keep it; otherwise
  // fall back to the first product (or null when empty). This avoids
  // cascading renders from a setState-in-effect.
  const effectiveSelectedProductKey: string | null = useMemo(() => {
    if (productGroups.length === 0) return null
    const exists = productGroups.some((g) => productKeyOf(g) === selectedProductKey)
    return exists ? selectedProductKey : productKeyOf(productGroups[0])
  }, [productGroups, selectedProductKey])

  const selectedGroup = useMemo(
    () => productGroups.find((g) => productKeyOf(g) === effectiveSelectedProductKey) ?? null,
    [productGroups, effectiveSelectedProductKey]
  )

  const handleSyncComplete = useCallback(async () => {
    // Refetch the mappings, then decide synchronously whether to surface the
    // "next step" guidance: if every mapping is still disabled (the default
    // draft state for freshly synced products), tell the admin what to do.
    // Done in the callback (not an effect) to avoid setState-in-effect.
    await queryClient.refetchQueries({
      queryKey: queryKeys.entitlementMappings(realmId, {}),
    })
    const fresh = queryClient.getQueryData<EntitlementMappingListResponse>(
      queryKeys.entitlementMappings(realmId, {})
    )
    const items = fresh?.items ?? []
    const allDisabled = items.length > 0 && items.every((mp) => !mp.enabled)
    if (allDisabled) setNextStepOpen(true)
  }, [queryClient, realmId])

  const showWebhookBanner = hasWebhookUnresolvedPrice(allMappings)

  return (
    <div className="space-y-6" data-testid="entitlement-mappings-page">
      <PageHeader
        title={m['billing.entitlement_mappings_title']()}
        // "Entitlement" is billing-domain jargon (Stripe/RevenueCat term);
        // the tooltip defines it for admins who haven't worked with those.
        titleTooltip={m['billing.entitlement_mappings_tooltip']()}
        subtitle={m['billing.entitlement_mappings_subtitle']()}
        headingTestId="entitlement-mappings-heading"
      />

      {!canManage && (
        <div
          className="rounded-md border border-warning/50 bg-warning/10 px-4 py-3 text-sm text-warning"
          data-testid="readonly-perm-banner"
          role="alert"
        >
          {m['billing.readonly_banner']()}
        </div>
      )}

      {showWebhookBanner && (
        <div
          className="rounded-md border border-destructive/50 bg-destructive/10 px-4 py-3 text-sm text-destructive"
          data-testid="webhook-price-unresolved-banner"
          role="alert"
        >
          <AlertTriangle className="mr-2 inline h-4 w-4 align-text-bottom" />
          <span className="font-medium">{m['billing.webhook_price_unresolved_title']()}</span>
          {' — '}
          {m['billing.webhook_price_unresolved_body']()}
        </div>
      )}

      <div className="flex justify-end gap-2">
        {canManage && (
          <Button
            variant="default"
            onClick={() =>
              navigate({ to: realmPath(realmContext, '/manage/billing/entitlement-mappings/new') })
            }
            data-testid="create-mapping-button"
          >
            {m['billing.create_mapping_button']()}
          </Button>
        )}
        <ProviderSyncButton realmId={realmId} onSyncComplete={handleSyncComplete} />
      </div>

      {isLoading ? (
        <LoadingSkeleton />
      ) : allMappings.length === 0 ? (
        <EmptyState onSync={() => undefined} canTriggerSync={canManage} />
      ) : (
        <div className="grid grid-cols-1 gap-6 lg:grid-cols-[320px_1fr]">
          <Card>
            <CardHeader>
              <CardTitle>Products</CardTitle>
            </CardHeader>
            <CardContent className="p-0">
              <ul className="divide-y" data-testid="mapping-product-list">
                {productGroups.map((g) => {
                  const key = productKeyOf(g)
                  const isSelected = key === effectiveSelectedProductKey
                  const primaryColor = deriveSharedKeyColor(g.prices[0]?.entitlementKey ?? '')
                  return (
                    <li key={key}>
                      <button
                        type="button"
                        onClick={() => setSelectedProductKey(key)}
                        className={`flex w-full items-center justify-between gap-2 px-4 py-3 text-left hover:bg-accent ${
                          isSelected ? 'bg-accent' : ''
                        }`}
                        data-testid={`mapping-product-row-${g.externalProductId}`}
                        aria-current={isSelected ? 'true' : undefined}
                      >
                        <span className="flex items-center gap-2 truncate">
                          {primaryColor.hue !== 0 && (
                            <span
                              className="inline-block h-2 w-2 shrink-0 rounded-full"
                              style={{ backgroundColor: `hsl(${primaryColor.hue} 70% 50%)` }}
                              aria-hidden
                            />
                          )}
                          <span className="truncate text-sm font-medium">
                            {primaryProductLabel(g.productName, g.externalProductId) ||
                              m['billing.product_name_empty']()}
                          </span>
                          <Badge variant="secondary" className="font-mono text-xs">
                            {formatProviderName(g.paymentProvider)}
                          </Badge>
                        </span>
                        <Badge variant="outline">{g.prices.length}</Badge>
                      </button>
                    </li>
                  )
                })}
              </ul>
            </CardContent>
          </Card>

          {/* Detail: same-screen multi-price editor.
              The `key` resets the panel's local edit state when the selected
              product changes (React-recommended alternative to a sync-seed
              effect, avoids cascading renders). */}
          {selectedGroup && (
            <DetailPanel
              key={productKeyOf(selectedGroup)}
              realmId={realmId}
              group={selectedGroup}
              canManage={canManage}
              canManagePoints={canManagePoints}
              onProtectedPriceError={(activeSubscriptions) =>
                setProtectedConfirm({ activeSubscriptions })
              }
            />
          )}
        </div>
      )}

      <ProtectedPriceConfirmDialog
        open={protectedConfirm !== null}
        activeSubscriptions={protectedConfirm?.activeSubscriptions ?? null}
        onOpenChange={(open) => {
          if (!open) setProtectedConfirm(null)
        }}
      />

      <SyncNextStepDialog open={nextStepOpen} onOpenChange={setNextStepOpen} />
    </div>
  )
}

interface DetailPanelProps {
  realmId: string
  group: {
    paymentProvider: string
    externalProductId: string
    productName?: string
    prices: EntitlementMappingResponse[]
  }
  canManage: boolean
  canManagePoints: boolean
  onProtectedPriceError: (activeSubscriptions: number) => void
}

function DetailPanel({
  realmId,
  group,
  canManage,
  canManagePoints,
  onProtectedPriceError,
}: DetailPanelProps) {
  const batchMutation = useBatchUpdateEntitlementMappings(realmId)

  // Realm-scoped and cached (staleTime 2min). Hoisted out of PriceEditRow so the
  // detail panel mounts a single observer instead of one per price row.
  const { data: buckets = [] } = useQuery(creditBucketsListQueryOptions(realmId))

  // Controlled field-array state: one entry per price row, seeded from the
  // loaded group. The parent remounts this panel via `key` when the selected
  // product changes, so the lazy initializer re-runs fresh (no sync effect).
  const [rows, setRows] = useState<PriceMappingUpdateFormData[]>(() => seedRows(group.prices))

  const keyGroups = useMemo(() => groupByEntitlementKey(group.prices), [group.prices])

  const updateRow = useCallback((mappingId: string, patch: Partial<PriceMappingUpdateFormData>) => {
    setRows((prev) => prev.map((r) => (r.mappingId === mappingId ? { ...r, ...patch } : r)))
  }, [])

  const handleSave = () => {
    const request: BatchUpdateEntitlementMappingsRequest = {
      paymentProvider: group.paymentProvider,
      externalProductId: group.externalProductId,
      updates: rows.map(toPriceMappingUpdate),
    }
    const parsed = batchEntitlementMappingsSchema.safeParse(request)
    if (!parsed.success) {
      // Surface the first validation issue via toast through the mutation's
      // generic error path is not appropriate; alert is simplest for now.
      // The backend will re-validate authoritatively on submit.
      const firstError = parsed.error.issues[0]
      toast.error(firstError ? String(firstError.message) : 'Validation failed')
      return
    }
    batchMutation.mutate(request, {
      onError: (error) => {
        if (isProtectedPriceError(error)) {
          const count = extractActiveSubscriptions(error)
          if (count != null) {
            onProtectedPriceError(count)
          }
        }
        // Non-409 errors are already toasted by the mutation.
      },
    })
  }

  const syncedAt = latestSyncedAt(group.prices)

  return (
    <Card data-testid="mapping-detail-panel">
      <CardHeader>
        <div className="flex items-center justify-between gap-2" data-testid="detail-head">
          <div className="flex items-center gap-2">
            <span className="text-base font-semibold">
              {primaryProductLabel(group.productName, group.externalProductId) ||
                m['billing.product_name_empty']()}
            </span>
            <Badge variant="secondary" className="font-mono text-xs">
              {formatProviderName(group.paymentProvider)}
            </Badge>
          </div>
          <span className="text-xs text-muted-foreground">
            {syncedAt ? format(new Date(syncedAt), 'PP') : '---'}
          </span>
        </div>
      </CardHeader>
      <CardContent className="space-y-6">
        {keyGroups.map((kg) => {
          const color = deriveSharedKeyColor(kg.entitlementKey)
          return (
            <div
              key={kg.entitlementKey}
              className="rounded-md border-l-4 pl-4"
              style={{ borderLeftColor: color.hue !== 0 ? `hsl(${color.hue} 70% 50%)` : undefined }}
            >
              <div className="mb-3 flex items-center gap-2">
                <Badge variant="outline" data-testid={`shared-key-chip-${kg.entitlementKey}`}>
                  {color.hue !== 0 && (
                    <span
                      className="mr-1 inline-block h-2 w-2 rounded-full align-middle"
                      style={{ backgroundColor: `hsl(${color.hue} 70% 50%)` }}
                      aria-hidden
                    />
                  )}
                  <span className="font-mono">{kg.entitlementKey}</span>
                </Badge>
                {kg.prices.length > 1 && (
                  <span className="text-xs text-muted-foreground">
                    {m['billing.shared_across_n_prices']({ count: kg.prices.length })}
                  </span>
                )}
              </div>

              <div className="space-y-4">
                {kg.prices.map((price) => {
                  const row = rows.find((r) => r.mappingId === price.id)
                  if (!row) return null
                  return (
                    <PriceEditRow
                      key={price.id}
                      realmId={realmId}
                      price={price}
                      row={row}
                      buckets={buckets}
                      canManage={canManage}
                      canManagePoints={canManagePoints}
                      isUnresolved={isWebhookUnresolvedPrice(price)}
                      onChange={(patch) => updateRow(price.id, patch)}
                    />
                  )
                })}
              </div>
            </div>
          )
        })}

        {canManage && (
          <div className="flex justify-end">
            <Button
              onClick={handleSave}
              disabled={batchMutation.isPending}
              data-testid="save-mapping-button"
            >
              {batchMutation.isPending ? 'Saving...' : m['shared.save_changes']()}
            </Button>
          </div>
        )}
      </CardContent>
    </Card>
  )
}

interface PriceEditRowProps {
  realmId: string
  price: EntitlementMappingResponse
  row: PriceMappingUpdateFormData
  buckets: BucketResponse[]
  canManage: boolean
  canManagePoints: boolean
  isUnresolved: boolean
  onChange: (patch: Partial<PriceMappingUpdateFormData>) => void
}

function PriceEditRow({
  realmId,
  price,
  row,
  buckets,
  canManage,
  canManagePoints,
  isUnresolved,
  onChange,
}: PriceEditRowProps) {
  const editDisabled = !canManage
  const pointsDisabled = !canManage || !canManagePoints
  // Billing type controls which trigger sources are legal for point rules.
  // Non-renewing mappings additionally expose their service duration.
  const isOneTime = isOneTimeMapping(row.billingType)
  const isNonRenewing = isNonRenewingMapping(row.billingType)

  // Instantiated per-row because the hook signature is
  // (realmId, mappingId) and only non-renewing rows trigger it. Decoupled from
  // the batch save (`save-mapping-button`): independent failure / toast /
  const updateMappingMutation = useUpdateEntitlementMapping(realmId, row.mappingId)

  // Per-price synced provider info (JSONB typed `unknown` upstream) — narrowed
  // ONLY here via `readProviderProductInfo` (the single narrowing point).
  const info = readProviderProductInfo(price.providerProductInfo)
  const metadataEntries = buildMetadataEntries(info.productMetadata, info.priceMetadata)

  // WeChat rows price by hand (no catalog to sync from): editable major-unit
  // price + currency, saved through the same single-row PUT path as
  // serviceDurationDays (decoupled from the batch save).
  const isWechat = providerRequiresManualPrice(price.paymentProvider)
  const [wechatPriceText, setWechatPriceText] = useState(() => minorToMajorUnits(info.price))
  const [wechatCurrency, setWechatCurrency] = useState(info.currency ?? 'CNY')
  // Resync the editors when a refetch lands a new stored value (e.g. after a
  // save round-trip or an external change). Adjust-during-render (with a
  // tracked snapshot of the stored value) instead of an effect, so the reset
  // happens before paint rather than as a cascading post-render update.
  const [syncedPrice, setSyncedPrice] = useState({
    price: info.price,
    currency: info.currency,
  })
  if (syncedPrice.price !== info.price || syncedPrice.currency !== info.currency) {
    setSyncedPrice({ price: info.price, currency: info.currency })
    setWechatPriceText(minorToMajorUnits(info.price))
    setWechatCurrency(info.currency ?? 'CNY')
  }

  // `RoleResponse[]` directly (not a paged envelope); builtin roles are filtered
  // out so only admin-defined roles are assignable — mirrors the API-key roles
  // dialog usage. The query is realm-scoped and cached (staleTime 5min).
  const { data: rolesData } = useQuery(adminRolesQueryOptions(realmId))
  const assignableRoles = (rolesData ?? []).filter((r) => !r.isBuiltin)
  const triggers = pointRuleTriggersForBillingType(row.billingType)

  return (
    <div
      className={`rounded-md border p-3 ${
        isUnresolved ? 'border-destructive/60 bg-destructive/5' : ''
      }`}
      data-testid={`price-edit-row-${price.externalPriceId ?? price.id}`}
    >
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <Field label={m['billing.field_price_id']()}>
          <Input value={price.externalPriceId ?? price.id} readOnly className="font-mono text-sm" />
        </Field>

        <Field label="Entitlement Key">
          <Input
            value={price.entitlementKey}
            readOnly
            className="bg-muted/40 font-mono text-sm text-muted-foreground"
          />
        </Field>

        <Field
          label={m['billing.field_billing_type']()}
          required={isUnresolved && !row.billingType}
          hint={!row.billingType ? m['billing.field_billing_type_sync_hint']() : undefined}
        >
          <Input
            value={
              row.billingType === 'recurring'
                ? m['billing.billing_type_recurring']()
                : row.billingType === 'one_time'
                  ? m['billing.billing_type_one_time']()
                  : row.billingType === 'non_renewing'
                    ? m['billing.billing_type_non_renewing']()
                    : ''
            }
            readOnly
            placeholder="—"
            className="bg-muted/40 text-sm text-muted-foreground"
            data-testid={`price-billing-type-${price.externalPriceId ?? price.id}`}
          />
        </Field>

        {isWechat ? (
          <Field
            label={m['billing.create_mapping_price_label']()}
            hint={m['billing.create_mapping_price_hint']()}
          >
            <div className="flex gap-2">
              <Input
                inputMode="decimal"
                value={wechatPriceText}
                onChange={(e) => setWechatPriceText(e.target.value)}
                onBlur={() => {
                  const minor = majorUnitsToMinor(wechatPriceText)
                  if (minor !== null && minor >= 1 && minor !== info.price) {
                    updateMappingMutation.mutate({ price: minor })
                  }
                }}
                disabled={editDisabled || updateMappingMutation.isPending}
                placeholder="—"
                data-testid={`price-manual-price-${price.id}`}
              />
              <Input
                className="w-24"
                value={wechatCurrency}
                onChange={(e) => setWechatCurrency(e.target.value.toUpperCase())}
                onBlur={() => {
                  if (wechatCurrency && wechatCurrency !== info.currency) {
                    updateMappingMutation.mutate({ currency: wechatCurrency })
                  }
                }}
                disabled={editDisabled || updateMappingMutation.isPending}
                data-testid={`price-manual-currency-${price.id}`}
              />
            </div>
          </Field>
        ) : (
          <Field
            label={m['billing.field_price']()}
            // `== null` (not `!info.price`): a synced $0.00 free price is a real
            // value, not an unsynced state — only show the sync hint when price
            // is genuinely absent. Mirrors the value-rendering `price != null` check.
            hint={info.price == null ? m['billing.field_billing_type_sync_hint']() : undefined}
          >
            <Input
              value={
                info.price != null && info.currency
                  ? formatInvoiceAmount(info.price, info.currency)
                  : ''
              }
              readOnly
              placeholder="—"
              className="bg-muted/40 text-sm text-muted-foreground"
            />
          </Field>
        )}

        {!isOneTime && (
          <Field
            label={m['billing.field_period']()}
            hint={!row.billingPeriod ? m['billing.field_billing_type_sync_hint']() : undefined}
          >
            <Input
              value={mapBillingPeriodLabel(row.billingPeriod)}
              readOnly
              placeholder="—"
              className="bg-muted/40 text-sm text-muted-foreground"
            />
          </Field>
        )}

        {isNonRenewing && (
          <Field label={m['billing.field_service_duration_days']()} required>
            <Input
              type="number"
              min={1}
              value={row.serviceDurationDays ?? ''}
              onChange={(e) =>
                onChange({
                  serviceDurationDays: e.target.value === '' ? null : Number(e.target.value),
                })
              }
              onBlur={() => {
                // positive integer; empty/invalid input does not fire and the
                // edit-row local state is left as-is. Decoupled from the batch
                // save button — independent failure / toast / invalidation.
                const v = row.serviceDurationDays
                if (typeof v === 'number' && v >= 1 && Number.isInteger(v)) {
                  updateMappingMutation.mutate({ serviceDurationDays: v })
                }
              }}
              disabled={editDisabled || updateMappingMutation.isPending}
              placeholder="—"
              data-testid={`price-service-duration-days-${price.externalPriceId ?? price.id}`}
            />
          </Field>
        )}

        {/* Role-grant dimension. Orthogonal to billing_type
            and points: empty = no role grant (pure credit / payment record);
            selected = roles auto-granted on successful payment. A realm-scoped
            query supplies the assignable (non-builtin) roles. */}
        <div
          className="sm:col-span-2"
          data-testid={`price-granted-roles-${price.externalPriceId ?? price.id}`}
        >
          <Field
            label={m['billing.label_granted_roles']()}
            hint={m['billing.help_granted_roles_hint']()}
          >
            <RoleSelector
              roles={assignableRoles}
              selectedRoleIds={row.grantedRoleIds ?? []}
              onChange={(ids) => onChange({ grantedRoleIds: ids })}
              disabled={editDisabled}
              placeholder={m['billing.help_granted_roles']()}
            />
          </Field>
        </div>

        <Field label="Enabled">
          <div className="flex items-center gap-2">
            <Switch
              checked={row.enabled ?? false}
              onCheckedChange={(checked: boolean) => onChange({ enabled: checked })}
              disabled={editDisabled}
              data-testid={`price-enabled-toggle-${price.externalPriceId ?? price.id}`}
              aria-label={
                row.enabled ? m['billing.price_enabled']() : m['billing.price_disabled']()
              }
            />
            <span className="text-sm text-muted-foreground">
              {row.enabled ? m['billing.price_enabled']() : m['billing.price_disabled']()}
            </span>
          </div>
          {/* Protected-price active-subs note: surfaced only after a 409 (the
              backend is the source of truth for the active-subscription lock);
              no client field pre-disables the toggle. */}
        </Field>
      </div>

      <div className="mt-4 space-y-2">
        <Label>Points distribution rules</Label>
        <PointDistributionRuleEditor
          value={row.pointRules ?? []}
          onChange={(pointRules) => onChange({ pointRules })}
          buckets={buckets}
          triggers={triggers}
          allowQuota={!isOneTime}
          disabled={pointsDisabled}
        />
      </div>

      {/* Provider metadata block (read-only). Rendered per price row: the
          backend attaches productMetadata to every price row of a product
          (there is no product-level UI node separate from the price rows) and
          priceMetadata is genuinely per-price, so productMetadata will visually
          repeat across the price rows of the same product — this is ACCEPTED.
          Omitted entirely (no placeholder text) when both maps are empty. */}
      {metadataEntries.length > 0 && (
        <div
          className="mt-3 rounded-md border border-border/60 bg-muted/30 p-3"
          data-testid={`price-metadata-block-${price.externalPriceId ?? price.id}`}
        >
          <Label className="mb-2 block text-xs font-medium text-muted-foreground">
            {m['billing.subscription_provider_metadata']()}
          </Label>
          <dl className="grid grid-cols-1 gap-x-4 gap-y-1 sm:grid-cols-2">
            {metadataEntries.map(({ scope, key, value }) => (
              <div key={scope + key} className="flex gap-1 text-xs">
                <dt className="shrink-0 font-medium text-muted-foreground">{key}</dt>
                <dd
                  className="min-w-0 truncate text-foreground"
                  title={value}
                  data-testid={`metadata-entry-${scope}-${key}`}
                >
                  {value}
                </dd>
              </div>
            ))}
          </dl>
        </div>
      )}
    </div>
  )
}

function Field({
  label,
  required,
  hint,
  children,
}: {
  label: string
  required?: boolean
  hint?: string
  children: React.ReactNode
}) {
  return (
    <div className="space-y-1">
      <Label className="text-xs font-medium text-muted-foreground">
        {label}
        {required && <span className="ml-0.5 text-destructive">*</span>}
      </Label>
      {children}
      {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
    </div>
  )
}

function EmptyState({ onSync, canTriggerSync }: { onSync: () => void; canTriggerSync: boolean }) {
  return (
    <Card className="border-dashed" data-testid="entitlement-mappings-empty-state">
      <CardContent className="flex flex-col items-center justify-center py-12">
        <Plug2 className="mb-4 h-12 w-12 text-muted-foreground" />
        <p className="text-center text-sm font-medium text-muted-foreground">
          {m['billing.empty_state_title']()}
        </p>
        <p className="mt-1 text-center text-sm text-muted-foreground">
          {m['billing.empty_state_body']()}
        </p>
        {canTriggerSync && (
          <Button onClick={onSync} className="mt-4" data-testid="empty-sync-button">
            {m['billing.empty_sync_button']()}
          </Button>
        )}
      </CardContent>
    </Card>
  )
}

function LoadingSkeleton() {
  return (
    <div className="grid grid-cols-1 gap-6 lg:grid-cols-[320px_1fr]">
      <Card>
        <CardHeader>
          <Skeleton className="h-6 w-32" />
        </CardHeader>
        <CardContent className="space-y-3">
          {Array.from({ length: 4 }).map((_, i) => (
            <Skeleton key={i} className="h-10 w-full" />
          ))}
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <Skeleton className="h-6 w-48" />
        </CardHeader>
        <CardContent className="space-y-4">
          {Array.from({ length: 3 }).map((_, i) => (
            <Skeleton key={i} className="h-24 w-full" />
          ))}
        </CardContent>
      </Card>
    </div>
  )
}

/**
 * Flatten the (optional) `productMetadata` + `priceMetadata` maps from the
 * synced provider product info into a flat list of display entries for the
 * read-only metadata block. Returns `[]` when both are absent/empty, which
 * the caller uses to omit the whole block (no placeholder text).
 *
 * Values are already display strings (the backend coerces metadata to a
 * strict string→string map at sync time); long values truncate visually via
 * the `title` tooltip on the `<dd>`.
 */
function buildMetadataEntries(
  productMetadata: Record<string, string> | null | undefined,
  priceMetadata: Record<string, string> | null | undefined
): { scope: 'product' | 'price'; key: string; value: string }[] {
  const toEntries = (
    scope: 'product' | 'price',
    map: Record<string, string> | null | undefined
  ): { scope: 'product' | 'price'; key: string; value: string }[] => {
    if (!map) return []
    const out: { scope: 'product' | 'price'; key: string; value: string }[] = []
    for (const key of Object.keys(map)) {
      out.push({ scope, key, value: map[key] })
    }
    return out
  }
  return [...toEntries('product', productMetadata), ...toEntries('price', priceMetadata)]
}

function seedRows(prices: EntitlementMappingResponse[]): PriceMappingUpdateFormData[] {
  return prices.map((p) => ({
    mappingId: p.id,
    billingType: p.billingType ?? null,
    billingPeriod: p.billingPeriod ?? null,
    enabled: p.enabled,
    // state only; persisted via a separate single-row PUT onBlur, never enters
    // batch DTO `PriceMappingUpdate` has no such field).
    serviceDurationDays: p.serviceDurationDays ?? null,
    pointRules: p.pointRules.map(toPointDistributionRuleFormData),
    // GET response carries the granted role list as a required array (empty when
    // none configured). Seed as an editable array so the multi-select can mutate
    // it directly; the save path forwards the array verbatim ([] ⟺ clear).
    grantedRoleIds: p.grantedRoleIds ?? [],
  }))
}

function toPriceMappingUpdate(row: PriceMappingUpdateFormData): PriceMappingUpdate {
  return {
    mappingId: row.mappingId,
    billingType: row.billingType ?? undefined,
    enabled: row.enabled ?? undefined,
    pointRules: row.pointRules ?? undefined,
    grantedRoleIds: row.grantedRoleIds ?? undefined,
  }
}

function latestSyncedAt(prices: EntitlementMappingResponse[]): string | null {
  let best: string | null = null
  for (const p of prices) {
    if (p.syncedAt) {
      if (best === null || p.syncedAt > best) best = p.syncedAt
    }
  }
  return best
}
