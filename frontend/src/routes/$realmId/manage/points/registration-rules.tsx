import { useMemo, useState } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { AccessDenied } from '@/components/shared'
import {
  PointDistributionRuleEditor,
  type PointRuleTriggerOption,
} from '@/components/billing/point-distribution-rule-editor'
import { pointRuleTriggerLabel } from '@/components/billing/provider-product-info'
import {
  creditBucketsListQueryOptions,
  registrationRulesQueryOptions,
  queryKeys,
  updateRegistrationRulesMutation,
} from '@/data/query-options'
import { useAuth } from '@/hooks/use-auth'
import { PERMISSION } from '@/lib/constants/auth-constants'
import type { RegistrationRuleWrite } from '@/lib/api-generated'
import {
  pointDistributionRulesSchema,
  toPointDistributionRuleFormData,
  type PointDistributionRuleFormData,
} from '@/lib/schemas/billing-forms'
import { useOptionalRouteParams, useResolvedRealmId } from '@/lib/realm-routing'
import { getErrorMessage } from '@/lib/error-utils'
import { m } from '@/paraglide/messages'

export const Route = createFileRoute('/$realmId/manage/points/registration-rules')({
  component: RegistrationRulesPage,
})

function toWriteRule(rule: PointDistributionRuleFormData): RegistrationRuleWrite {
  return {
    id: rule.id,
    bucketId: rule.bucketId,
    triggerSources: rule.triggerSources,
    grantMode: rule.grantMode,
    pointsAmount: rule.grantMode === 'fixed' ? rule.pointsAmount : null,
    validityDays: rule.grantMode === 'fixed' ? rule.validityDays : null,
    grantPeriodType: rule.grantPeriodType,
    // `toPointDistributionRuleFormData` already normalized windows to
    // `{ windowSeconds, limit }` on the read side, so forward verbatim here.
    quotaWindows: rule.grantMode === 'quota' ? rule.quotaWindows : null,
    enabled: rule.enabled,
    displayOrder: rule.displayOrder,
  }
}

export function RegistrationRulesPage() {
  const fallbackRealmId = useResolvedRealmId()
  const routeParams = useOptionalRouteParams<{ realmId?: string }>(Route)
  const realmId = routeParams.realmId ?? fallbackRealmId
  const auth = useAuth()
  const queryClient = useQueryClient()
  const canView = auth.permissions?.includes(PERMISSION.POINTS_VIEW) ?? false
  const canManage = auth.permissions?.includes(PERMISSION.POINTS_MANAGE) ?? false

  const { data, isLoading, error } = useQuery({
    ...registrationRulesQueryOptions(realmId),
    enabled: canView,
  })
  const { data: buckets = [] } = useQuery({
    ...creditBucketsListQueryOptions(realmId),
    enabled: canView,
  })

  const initialRules = useMemo(
    () => (data?.rules ?? []).map(toPointDistributionRuleFormData),
    [data]
  )
  const [editedRules, setEditedRules] = useState<PointDistributionRuleFormData[] | null>(null)
  const rules = editedRules ?? initialRules

  // Combined rules (both triggers on one rule) are legal server-side, but this
  // page edits the two trigger groups independently — showing a combined rule
  // in both cards would rewrite it to a single trigger and submit its id twice.
  // Keep combined rules out of both cards and forward them untouched on save.
  const isCombined = (rule: PointDistributionRuleFormData) =>
    rule.triggerSources.includes('registration') &&
    rule.triggerSources.includes('free_periodic_grant')
  const inGroup = (rule: PointDistributionRuleFormData, trigger: string) =>
    rule.triggerSources.includes(trigger) && !isCombined(rule)
  const registrationRules = rules.filter((rule) => inGroup(rule, 'registration'))
  const freePeriodicRules = rules.filter((rule) => inGroup(rule, 'free_periodic_grant'))
  const combinedCount = rules.filter(isCombined).length

  // Built per render so trigger labels honor the active locale.
  const registrationTriggers: PointRuleTriggerOption[] = [
    { value: 'registration', label: pointRuleTriggerLabel('registration') },
  ]
  const freePeriodicTriggers: PointRuleTriggerOption[] = [
    { value: 'free_periodic_grant', label: pointRuleTriggerLabel('free_periodic_grant') },
  ]

  const replaceGroup = (
    trigger: 'registration' | 'free_periodic_grant',
    next: PointDistributionRuleFormData[]
  ) => {
    setEditedRules([
      ...rules.filter((rule) => !inGroup(rule, trigger)),
      ...next.map((rule, index) => ({
        ...rule,
        triggerSources: [trigger],
        displayOrder: index,
      })),
    ])
  }

  const mutation = useMutation({
    mutationFn: async () => {
      const parsed = pointDistributionRulesSchema.safeParse(rules)
      if (!parsed.success) {
        throw new Error(
          parsed.error.issues[0]?.message ?? m['points.registration_rules_invalid_rule']()
        )
      }
      return updateRegistrationRulesMutation(realmId, {
        rules: parsed.data.map(toWriteRule),
      })
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: queryKeys.registrationRules(realmId) })
      setEditedRules(null)
      toast.success(m['points.registration_rules_saved']())
    },
    onError: (mutationError) => {
      toast.error(getErrorMessage(mutationError))
    },
  })

  if (!canView) return <AccessDenied />
  if (isLoading) {
    return (
      <p className="text-sm text-muted-foreground">
        {m['points.registration_rules_loading_rules']()}
      </p>
    )
  }
  if (error) return <p className="text-sm text-destructive">{getErrorMessage(error)}</p>

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-semibold">{m['points.registration_rules_page_title']()}</h1>
        <p className="text-sm text-muted-foreground">
          {m['points.registration_rules_page_description']()}
        </p>
      </div>

      {combinedCount > 0 && (
        <p
          className="text-sm text-muted-foreground"
          data-testid="registration-rules-combined-notice"
        >
          {m['points.registration_rules_combined_notice']({ count: combinedCount })}
        </p>
      )}

      <div className="grid items-start gap-6 xl:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle>{m['points.registration_rules_registration_card_title']()}</CardTitle>
            <CardDescription>
              {m['points.registration_rules_registration_card_description']()}
            </CardDescription>
          </CardHeader>
          <CardContent>
            <PointDistributionRuleEditor
              value={registrationRules}
              onChange={(next) => replaceGroup('registration', next)}
              buckets={buckets}
              triggers={registrationTriggers}
              allowQuota={false}
              requirePermanent
              disabled={!canManage || mutation.isPending}
            />
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>{m['points.registration_rules_free_periodic_card_title']()}</CardTitle>
            <CardDescription>
              {m['points.registration_rules_free_periodic_card_description']()}
            </CardDescription>
          </CardHeader>
          <CardContent>
            <PointDistributionRuleEditor
              value={freePeriodicRules}
              onChange={(next) => replaceGroup('free_periodic_grant', next)}
              buckets={buckets}
              triggers={freePeriodicTriggers}
              allowQuota
              allowPeriod
              disabled={!canManage || mutation.isPending}
            />
          </CardContent>
        </Card>
      </div>

      {canManage && (
        <div className="flex items-center justify-end gap-3">
          {editedRules !== null && !mutation.isPending && (
            <span
              className="text-sm text-muted-foreground"
              data-testid="registration-rules-unsaved"
            >
              {m['points.registration_rules_unsaved_changes']()}
            </span>
          )}
          <Button
            type="button"
            onClick={() => mutation.mutate()}
            disabled={mutation.isPending}
            data-testid="registration-rules-save"
          >
            {mutation.isPending
              ? m['shared.saving']()
              : m['points.registration_rules_save_button']()}
          </Button>
        </div>
      )}
    </div>
  )
}
