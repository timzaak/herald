/* eslint-disable react-refresh/only-export-components */
import { useCallback, useEffect, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { X } from 'lucide-react'
import type { AuditSearchParams } from '@/lib/schemas/search-params'
import { toDateInputValue } from '@/lib/date-utils'
import { FILTER_ALL_VALUE } from '@/lib/constants'
import { m } from '@/paraglide/messages'
import { AUDIT_ACTION_LABELS } from '@/lib/audit-utils'

export const CATEGORY_ACTIONS: Record<string, string[]> = {
  user_management: ['user.create', 'user.update', 'user.delete'],
  rbac: [
    'role.create',
    'role.update',
    'role.delete',
    'permission.create',
    'permission.update',
    'permission.delete',
    'role.assign',
    'role.unassign',
    'permission.grant',
    'permission.revoke',
  ],
  realm_management: [
    'realm.create',
    'realm.rbac_init',
    'realm_config.update',
    'realm_config.delete',
  ],
  auth: [
    'auth.login',
    'auth.logout',
    'auth.login_failed',
    'passkey_config.update',
    'totp_config.update',
    'email_otp_config.update',
  ],
}

const CATEGORY_LABELS: Record<string, () => string> = {
  user_management: () => m['audit.category_user_management'](),
  rbac: () => m['audit.category_rbac'](),
  realm_management: () => m['audit.category_realm_management'](),
  auth: () => m['audit.category_auth'](),
}

function getActionsForCategory(category?: string): string[] {
  return category ? (CATEGORY_ACTIONS[category] ?? []) : Object.values(CATEGORY_ACTIONS).flat()
}

export function hasActiveFilters(filters: AuditSearchParams): boolean {
  return !!(
    filters.category ||
    filters.action ||
    filters.actorId ||
    filters.startTime ||
    filters.endTime
  )
}

interface AuditFilterBarProps {
  filters: AuditSearchParams
  onFilterChange: (filters: Partial<AuditSearchParams>) => void
  onClearFilters: () => void
}

export function AuditFilterBar({ filters, onFilterChange, onClearFilters }: AuditFilterBarProps) {
  const [actorInput, setActorInput] = useState(filters.actorId ?? '')
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- sync input when external filter resets
    setActorInput(filters.actorId ?? '')
  }, [filters.actorId])

  const handleCategoryChange = useCallback(
    (value: string) => {
      const category = value === FILTER_ALL_VALUE ? undefined : value
      const currentAction = filters.action
      const validActions = getActionsForCategory(category)
      const action =
        currentAction && validActions.includes(currentAction) ? currentAction : undefined
      onFilterChange({ category, action })
    },
    [filters.action, onFilterChange]
  )

  const handleActionChange = useCallback(
    (value: string) => {
      const action = value === FILTER_ALL_VALUE ? undefined : value
      onFilterChange({ action })
    },
    [onFilterChange]
  )

  const handleActorChange = useCallback(
    (value: string) => {
      setActorInput(value)
      if (debounceRef.current) {
        clearTimeout(debounceRef.current)
      }
      debounceRef.current = setTimeout(() => {
        onFilterChange({ actorId: value || undefined })
      }, 300)
    },
    [onFilterChange]
  )

  useEffect(() => {
    return () => {
      if (debounceRef.current) {
        clearTimeout(debounceRef.current)
      }
    }
  }, [])

  const handleStartDateChange = useCallback(
    (value: string) => {
      onFilterChange({ startTime: value || undefined })
    },
    [onFilterChange]
  )

  const handleEndDateChange = useCallback(
    (value: string) => {
      onFilterChange({ endTime: value || undefined })
    },
    [onFilterChange]
  )

  const availableActions = getActionsForCategory(filters.category)

  return (
    <div
      data-testid="audit-filter-bar"
      className="flex flex-wrap items-end gap-4 rounded-md border p-4"
    >
      <div className="space-y-1">
        <Label>{m['audit.filter_category_label']()}</Label>
        <Select value={filters.category ?? FILTER_ALL_VALUE} onValueChange={handleCategoryChange}>
          <SelectTrigger className="w-[180px]" data-testid="audit-category-select">
            <SelectValue placeholder={m['audit.filter_category_all']()} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={FILTER_ALL_VALUE}>{m['audit.filter_category_all']()}</SelectItem>
            {Object.keys(CATEGORY_ACTIONS).map((cat) => (
              <SelectItem key={cat} value={cat}>
                {CATEGORY_LABELS[cat]()}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="space-y-1">
        <Label>{m['audit.filter_action_label']()}</Label>
        <Select value={filters.action ?? FILTER_ALL_VALUE} onValueChange={handleActionChange}>
          <SelectTrigger className="w-[180px]" data-testid="audit-action-select">
            <SelectValue placeholder={m['audit.filter_action_all']()} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={FILTER_ALL_VALUE}>{m['audit.filter_action_all']()}</SelectItem>
            {availableActions.map((act) => (
              <SelectItem key={act} value={act}>
                {AUDIT_ACTION_LABELS[act] ? AUDIT_ACTION_LABELS[act]() : act}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="space-y-1">
        <Label htmlFor="audit-actor-input">{m['audit.filter_actor_label']()}</Label>
        <Input
          id="audit-actor-input"
          type="text"
          value={actorInput}
          onChange={(e) => handleActorChange(e.target.value)}
          placeholder={m['audit.filter_actor_placeholder']()}
          className="w-[200px]"
          data-testid="audit-actor-input"
        />
      </div>

      <div className="space-y-1">
        <Label htmlFor="audit-start-date">{m['audit.filter_start_date_label']()}</Label>
        <Input
          id="audit-start-date"
          type="date"
          value={toDateInputValue(filters.startTime)}
          onChange={(e) => handleStartDateChange(e.target.value)}
          className="w-[160px]"
          data-testid="audit-start-date-input"
        />
      </div>

      <div className="space-y-1">
        <Label htmlFor="audit-end-date">{m['audit.filter_end_date_label']()}</Label>
        <Input
          id="audit-end-date"
          type="date"
          value={toDateInputValue(filters.endTime)}
          onChange={(e) => handleEndDateChange(e.target.value)}
          className="w-[160px]"
          data-testid="audit-end-date-input"
        />
      </div>

      {hasActiveFilters(filters) && (
        <Button
          variant="ghost"
          size="sm"
          onClick={onClearFilters}
          data-testid="audit-clear-filters-button"
        >
          <X className="mr-1 h-4 w-4" />
          {m['audit.filter_clear']()}
        </Button>
      )}
    </div>
  )
}
