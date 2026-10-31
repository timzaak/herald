import { Plus, Trash2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { InfoTooltip } from '@/components/shared/info-tooltip'
import { MultiWindowQuotaEditor } from '@/components/billing/MultiWindowQuotaEditor'
import { realmPath, useResolvedRealmContext } from '@/lib/realm-routing'
import type { BucketResponse } from '@/lib/api-generated'
import type { PointDistributionRuleFormData } from '@/lib/schemas/billing-forms'
import type { PointRuleTriggerOption } from '@/components/billing/provider-product-info'
import { m } from '@/paraglide/messages'

// Re-exported so existing importers (e.g. `registration-rules.tsx`) keep a stable path.
export type { PointRuleTriggerOption }

// Localized labels for the grant-period select. Reuses the existing
// `plan_config_form_period_*` catalog entries (Once / Daily / Weekly / Monthly).
function grantPeriodLabel(period: string): string {
  switch (period) {
    case 'daily':
      return m['points.plan_config_form_period_daily']()
    case 'weekly':
      return m['points.plan_config_form_period_weekly']()
    case 'monthly':
      return m['points.plan_config_form_period_monthly']()
    default:
      return m['points.plan_config_form_period_once']()
  }
}

interface PointDistributionRuleEditorProps {
  value: PointDistributionRuleFormData[]
  onChange: (value: PointDistributionRuleFormData[]) => void
  buckets: BucketResponse[]
  triggers: PointRuleTriggerOption[]
  allowQuota: boolean
  allowPeriod?: boolean
  /**
   * Hide the validity-days input and keep fixed grants permanent
   * (validityDays = 0). Required for registration-triggered rules: the backend
   * rejects expiring registration grants (`RegistrationMustBePermanent`).
   */
  requirePermanent?: boolean
  disabled?: boolean
  emptyText?: string
}

// Per-grant-mode field sets. The single source for the fixed/quota defaults so
// `newRule` and the grant-mode switch below stay in sync.
function fixedGrantModeFields() {
  return { grantMode: 'fixed' as const, pointsAmount: 1, validityDays: 0, quotaWindows: null }
}

function quotaGrantModeFields() {
  return {
    grantMode: 'quota' as const,
    pointsAmount: null,
    validityDays: null,
    quotaWindows: [{ windowSeconds: 3600, limit: 0 }],
  }
}

function newRule(trigger: string, allowPeriod: boolean): PointDistributionRuleFormData {
  return {
    bucketId: '',
    triggerSources: trigger ? [trigger] : [],
    ...fixedGrantModeFields(),
    // A free-periodic fixed rule without a period is rejected by the backend
    // (`FreePeriodicRequiresGrantPeriod`), so seed the displayed default.
    ...(allowPeriod ? { grantPeriodType: 'once' } : {}),
    enabled: true,
    displayOrder: 0,
  }
}

export function PointDistributionRuleEditor({
  value,
  onChange,
  buckets,
  triggers,
  allowQuota,
  allowPeriod = false,
  requirePermanent = false,
  disabled = false,
  emptyText = m['points.rule_editor_empty'](),
}: PointDistributionRuleEditorProps) {
  const realmContext = useResolvedRealmContext()
  const updateRule = (index: number, next: PointDistributionRuleFormData) => {
    onChange(value.map((rule, i) => (i === index ? next : rule)))
  }

  const removeRule = (index: number) => {
    const rule = value[index]
    if (!rule) return
    if (rule.id) {
      updateRule(index, { ...rule, enabled: false })
      return
    }
    onChange(value.filter((_, i) => i !== index))
  }

  return (
    <div className="space-y-3" data-testid="point-rule-list">
      {value.length === 0 && <p className="text-sm text-muted-foreground">{emptyText}</p>}

      {value.map((rule, index) => {
        const key = rule.id ?? `new-${index}`
        return (
          <div
            key={key}
            className="space-y-4 rounded-md border p-4"
            data-testid={`point-rule-${rule.id ?? index}`}
          >
            <div className="flex items-center justify-between gap-3">
              <span className="text-sm font-medium">
                {m['points.rule_editor_rule_label']({ index: index + 1 })}
              </span>
              <div className="flex items-center gap-2">
                <Label htmlFor={`point-rule-enabled-${key}`}>{m['common.enabled']()}</Label>
                <Switch
                  id={`point-rule-enabled-${key}`}
                  checked={rule.enabled ?? true}
                  onCheckedChange={(enabled) => updateRule(index, { ...rule, enabled })}
                  disabled={disabled}
                  data-testid={`point-rule-enabled-${key}`}
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  disabled={disabled}
                  onClick={() => removeRule(index)}
                  aria-label={
                    rule.id
                      ? m['points.rule_editor_disable_rule']()
                      : m['points.rule_editor_remove_rule']()
                  }
                  data-testid={`point-rule-remove-${key}`}
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
            </div>

            <div className="grid gap-4 md:grid-cols-2">
              <div className="space-y-2">
                <div className="flex items-center gap-1.5">
                  <Label>{m['points.rule_editor_target_account']()}</Label>
                  <InfoTooltip
                    label={m['points.rule_editor_target_account_hint']()}
                    testId="point-rule-bucket-hint"
                    iconClassName="h-3.5 w-3.5"
                    contentClassName="max-w-64 space-y-1"
                  >
                    <p>{m['points.rule_editor_target_account_hint']()}</p>
                    <a
                      href={realmPath(realmContext, '/manage/billing/credit-buckets')}
                      className="text-sm font-medium text-primary underline-offset-4 hover:underline"
                      data-testid="point-rule-bucket-hint-link"
                    >
                      {m['points.rule_editor_target_account_hint_link']()}
                    </a>
                  </InfoTooltip>
                </div>
                <Select
                  value={rule.bucketId}
                  onValueChange={(bucketId) => updateRule(index, { ...rule, bucketId })}
                  disabled={disabled}
                >
                  <SelectTrigger data-testid="point-rule-bucket">
                    <SelectValue placeholder={m['points.rule_editor_select_account']()} />
                  </SelectTrigger>
                  <SelectContent>
                    {buckets.map((bucket) => (
                      <SelectItem
                        key={bucket.id}
                        value={bucket.id}
                        disabled={!bucket.enabled && bucket.id !== rule.bucketId}
                      >
                        {bucket.name}
                        {!bucket.enabled ? ` (${m['common.disabled']()})` : ''}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>

              <div className="space-y-2">
                <Label>{m['points.rule_editor_grant_mode']()}</Label>
                <Select
                  value={rule.grantMode}
                  onValueChange={(grantMode) => {
                    if (grantMode === 'quota') {
                      updateRule(index, { ...rule, ...quotaGrantModeFields() })
                    } else {
                      updateRule(index, {
                        ...rule,
                        ...fixedGrantModeFields(),
                        // Same seed as newRule: loaded quota rules carry no
                        // period and the period Select below already displays
                        // 'once' for null — without this the payload stays
                        // null while the admin sees "Once", and the backend
                        // rejects the whole save (FreePeriodicRequiresGrantPeriod).
                        ...(allowPeriod && rule.grantPeriodType == null
                          ? { grantPeriodType: 'once' as const }
                          : {}),
                      })
                    }
                  }}
                  disabled={disabled || !allowQuota}
                >
                  <SelectTrigger data-testid="point-rule-mode">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="fixed">
                      {m['points.rule_editor_grant_mode_fixed']()}
                    </SelectItem>
                    {allowQuota && (
                      <SelectItem value="quota">
                        {m['points.rule_editor_grant_mode_quota']()}
                      </SelectItem>
                    )}
                  </SelectContent>
                </Select>
              </div>
            </div>

            <fieldset className="space-y-2" data-testid="point-rule-trigger">
              <legend className="text-sm font-medium">{m['points.rule_editor_triggers']()}</legend>
              <div className="flex flex-wrap gap-4">
                {triggers.map((trigger) => {
                  const checked = rule.triggerSources.includes(trigger.value)
                  return (
                    <label key={trigger.value} className="flex items-center gap-2 text-sm">
                      <Checkbox
                        checked={checked}
                        disabled={disabled}
                        onCheckedChange={(next) => {
                          const triggerSources = next
                            ? [...rule.triggerSources, trigger.value]
                            : rule.triggerSources.filter((item) => item !== trigger.value)
                          updateRule(index, { ...rule, triggerSources })
                        }}
                        data-testid={`point-rule-trigger-${trigger.value}`}
                      />
                      {trigger.label}
                    </label>
                  )
                })}
              </div>
            </fieldset>

            {rule.grantMode === 'fixed' ? (
              <div className="grid gap-4 md:grid-cols-2">
                <div className="space-y-2">
                  <Label htmlFor={`point-rule-amount-${key}`}>
                    {m['points.rule_editor_points_amount']()}
                  </Label>
                  <Input
                    id={`point-rule-amount-${key}`}
                    type="number"
                    min={1}
                    value={rule.pointsAmount ?? ''}
                    onChange={(event) =>
                      updateRule(index, {
                        ...rule,
                        pointsAmount:
                          event.target.value === '' ? 0 : Number.parseInt(event.target.value, 10),
                      })
                    }
                    disabled={disabled}
                    data-testid={`point-rule-amount-${key}`}
                  />
                </div>
                {!requirePermanent && (
                  <div className="space-y-2">
                    <Label htmlFor={`point-rule-validity-${key}`}>
                      {m['points.rule_editor_validity_days']()}
                    </Label>
                    <Input
                      id={`point-rule-validity-${key}`}
                      type="number"
                      min={0}
                      value={rule.validityDays ?? 0}
                      onChange={(event) =>
                        updateRule(index, {
                          ...rule,
                          validityDays: Number.parseInt(event.target.value || '0', 10),
                        })
                      }
                      disabled={disabled}
                      data-testid={`point-rule-validity-${key}`}
                    />
                  </div>
                )}
                {allowPeriod && (
                  <div className="space-y-2">
                    <Label>{m['points.rule_editor_grant_period']()}</Label>
                    <Select
                      value={rule.grantPeriodType ?? 'once'}
                      onValueChange={(grantPeriodType) =>
                        updateRule(index, { ...rule, grantPeriodType })
                      }
                      disabled={disabled}
                    >
                      <SelectTrigger data-testid={`point-rule-period-${key}`}>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {['once', 'daily', 'weekly', 'monthly'].map((period) => (
                          <SelectItem key={period} value={period}>
                            {grantPeriodLabel(period)}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                )}
              </div>
            ) : (
              <MultiWindowQuotaEditor
                value={rule.quotaWindows ?? []}
                onChange={(quotaWindows) => updateRule(index, { ...rule, quotaWindows })}
                disabled={disabled}
                context="entitlement-mapping"
                testIdPrefix={`point-rule-quota-${key}`}
              />
            )}
          </div>
        )
      })}

      <Button
        type="button"
        variant="outline"
        disabled={disabled || triggers.length === 0}
        onClick={() =>
          onChange([
            ...value,
            {
              ...newRule(triggers[0]?.value ?? '', allowPeriod),
              displayOrder: value.length,
            },
          ])
        }
        data-testid="point-rule-add"
      >
        <Plus className="h-4 w-4" />
        {m['points.rule_editor_add_rule']()}
      </Button>
    </div>
  )
}
