import { useState } from 'react'
import { Checkbox } from '@/components/ui/checkbox'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { BuiltinBadge } from '@/components/shared/builtin-badge'
import { AlertTriangle, ArrowLeft, ArrowRight, Lock, Search, Shield } from 'lucide-react'
import type { PermissionResponse } from '@/lib/api-generated'
import { m } from '@/paraglide/messages'

interface PermissionTransferProps {
  permissions: PermissionResponse[]
  assignedPermissionIds: string[]
  onTogglePermission: (permissionId: string, checked: boolean) => void
  isBuiltinRole: boolean
  disabled?: boolean
  dataTestId?: string
}

function groupPermissionsByResource(
  permissions: PermissionResponse[]
): Map<string, PermissionResponse[]> {
  const grouped = new Map<string, PermissionResponse[]>()
  permissions.forEach((permission) => {
    const resource = permission.resource
    if (!grouped.has(resource)) {
      grouped.set(resource, [])
    }
    grouped.get(resource)!.push(permission)
  })
  return grouped
}

function permissionMatchesSearch(permission: PermissionResponse, search: string): boolean {
  const query = search.trim().toLowerCase()
  if (!query) return true
  return (
    permission.name.toLowerCase().includes(query) ||
    permission.resource.toLowerCase().includes(query) ||
    (permission.description?.toLowerCase().includes(query) ?? false)
  )
}

function permissionCountText(count: number): string {
  return count === 1
    ? m['roles.permission_count']({ count })
    : m['roles.permission_count_plural']({ count })
}

interface TransferPaneProps {
  title: string
  searchValue: string
  onSearchChange: (value: string) => void
  visiblePermissions: PermissionResponse[]
  checkedIds: string[]
  onToggleItem: (permissionId: string, checked: boolean) => void
  // Bulk mark from the pane header: receives the pane's selectable ids
  // (visible and not locked), so the "selectable" rule lives here only.
  onMarkIds: (permissionIds: string[], checked: boolean) => void
  // Locked items render with a lock icon and cannot be checked (built-in permissions on built-in roles)
  isItemLocked: (permission: PermissionResponse) => boolean
  disabled: boolean
  emptyMessage: string
  pane: 'available' | 'selected'
}

function TransferPane({
  title,
  searchValue,
  onSearchChange,
  visiblePermissions,
  checkedIds,
  onToggleItem,
  onMarkIds,
  isItemLocked,
  disabled,
  emptyMessage,
  pane,
}: TransferPaneProps) {
  const groupedPermissions = groupPermissionsByResource(visiblePermissions)
  const checkedIdSet = new Set(checkedIds)
  const selectableIds = visiblePermissions
    .filter((permission) => !isItemLocked(permission))
    .map((permission) => permission.id)
  const checkedVisibleCount = selectableIds.filter((id) => checkedIdSet.has(id)).length
  const allVisibleChecked = selectableIds.length > 0 && checkedVisibleCount === selectableIds.length

  return (
    <div
      className="flex min-w-0 flex-col rounded-md border"
      data-testid={`permission-${pane}-pane`}
    >
      <div className="flex items-center gap-2 border-b px-3 py-2">
        <Checkbox
          checked={allVisibleChecked ? true : checkedVisibleCount > 0 ? 'indeterminate' : false}
          onCheckedChange={(checked) => onMarkIds(selectableIds, checked === true)}
          disabled={disabled || selectableIds.length === 0}
          aria-label={m['roles.permissions_select_all']()}
          data-testid={`permission-${pane}-select-all`}
        />
        <span className="text-sm font-medium">{title}</span>
        <span className="ml-auto text-xs text-muted-foreground">
          {permissionCountText(visiblePermissions.length)}
        </span>
      </div>
      <div className="border-b p-2">
        <div className="relative">
          <Search className="absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={searchValue}
            onChange={(event) => onSearchChange(event.target.value)}
            placeholder={m['roles.permissions_search_placeholder']()}
            className="h-8 pl-8"
            disabled={disabled}
            data-testid={`permission-${pane}-search`}
          />
        </div>
      </div>
      <div className="h-80 overflow-y-auto p-2">
        {visiblePermissions.length === 0 ? (
          <div className="flex h-full items-center justify-center px-4 text-center text-sm text-muted-foreground">
            {emptyMessage}
          </div>
        ) : (
          Array.from(groupedPermissions.entries()).map(([resource, resourcePermissions]) => (
            <div key={resource} className="mb-3 last:mb-0">
              <div className="flex items-center gap-2 px-1 pb-1">
                <Badge variant="outline" className="font-semibold">
                  {resource}
                </Badge>
                <span className="text-xs text-muted-foreground">
                  ({permissionCountText(resourcePermissions.length)})
                </span>
              </div>
              <div className="space-y-1">
                {resourcePermissions.map((permission) => {
                  const locked = isItemLocked(permission)
                  return (
                    <div
                      key={permission.id}
                      className="flex items-start gap-2 rounded-md p-1.5 transition-colors hover:bg-accent/50"
                      data-testid={`permission-item-${permission.id}`}
                    >
                      <Checkbox
                        id={`permission-${permission.id}`}
                        checked={checkedIdSet.has(permission.id)}
                        onCheckedChange={(checked) => onToggleItem(permission.id, checked === true)}
                        disabled={disabled || locked}
                        aria-label={permission.name}
                        data-testid={`permission-checkbox-${permission.id}`}
                      />
                      <div className="min-w-0 flex-1">
                        <label
                          htmlFor={`permission-${permission.id}`}
                          className="cursor-pointer text-sm font-medium leading-none peer-disabled:cursor-not-allowed"
                        >
                          <span className="flex items-center gap-2">
                            <span className="truncate">{permission.name}</span>
                            {locked && (
                              <Lock className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                            )}
                            <BuiltinBadge isBuiltin={permission.isBuiltin} />
                          </span>
                        </label>
                        {permission.description && (
                          <p className="mt-1 truncate text-xs text-muted-foreground">
                            {permission.description}
                          </p>
                        )}
                      </div>
                    </div>
                  )
                })}
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  )
}

/**
 * Classic transfer (shuttle) picker: the left pane lists unassigned permissions,
 * the right pane lists every assigned permission. Items are checked on either
 * side and moved with the arrow buttons; each pane has its own search box so a
 * permission can be located without scrolling through the whole catalog.
 */
export function PermissionTransfer({
  permissions,
  assignedPermissionIds,
  onTogglePermission,
  isBuiltinRole,
  disabled = false,
  dataTestId = 'permission-transfer',
}: PermissionTransferProps) {
  const [leftSearch, setLeftSearch] = useState('')
  const [rightSearch, setRightSearch] = useState('')
  // A mark flags an item for moving to the other pane; it is separate from
  // assignment state, and items hidden by search keep their mark. Which pane a
  // marked item belongs to is derived from assignment, so the mark follows the
  // item across panes and clears on every move.
  const [markedIds, setMarkedIds] = useState<string[]>([])

  if (permissions.length === 0) {
    return (
      <div
        className="flex flex-col items-center justify-center py-8 text-muted-foreground"
        data-testid={dataTestId}
      >
        <Shield className="mb-2 h-12 w-12 opacity-50" />
        <p>{m['roles.no_permissions_available']()}</p>
      </div>
    )
  }

  const assignedIdSet = new Set(assignedPermissionIds)
  const availablePermissions = permissions.filter((p) => !assignedIdSet.has(p.id))
  const selectedPermissions = permissions.filter((p) => assignedIdSet.has(p.id))
  const visibleAvailable = availablePermissions.filter((p) =>
    permissionMatchesSearch(p, leftSearch)
  )
  const visibleSelected = selectedPermissions.filter((p) => permissionMatchesSearch(p, rightSearch))
  const leftChecked = markedIds.filter((id) => !assignedIdSet.has(id))
  const rightChecked = markedIds.filter((id) => assignedIdSet.has(id))

  // Built-in permissions already assigned to a built-in role can never be removed
  const isLockedOnSelected = (permission: PermissionResponse) =>
    isBuiltinRole && permission.isBuiltin

  const toggleMarked = (id: string, checked: boolean) =>
    setMarkedIds((prev) => (checked ? [...prev, id] : prev.filter((x) => x !== id)))

  const markIds = (ids: string[], checked: boolean) =>
    setMarkedIds((prev) => {
      if (!checked) {
        const idSet = new Set(ids)
        return prev.filter((id) => !idSet.has(id))
      }
      return Array.from(new Set([...prev, ...ids]))
    })

  const move = (checked: boolean) => {
    const moving = checked ? leftChecked : rightChecked
    moving.forEach((id) => onTogglePermission(id, checked))
    setMarkedIds([])
  }

  return (
    <div className="space-y-4" data-testid={dataTestId}>
      {isBuiltinRole && (
        <Alert variant="default">
          <AlertTriangle className="h-4 w-4" />
          <AlertDescription>{m['roles.builtin_permission_alert']()}</AlertDescription>
        </Alert>
      )}

      <div className="grid grid-cols-1 items-stretch gap-3 md:grid-cols-[1fr_auto_1fr]">
        <TransferPane
          title={m['roles.permissions_available']()}
          searchValue={leftSearch}
          onSearchChange={setLeftSearch}
          visiblePermissions={visibleAvailable}
          checkedIds={leftChecked}
          onToggleItem={toggleMarked}
          onMarkIds={markIds}
          isItemLocked={() => false}
          disabled={disabled}
          emptyMessage={
            availablePermissions.length === 0
              ? m['roles.no_permissions_available']()
              : m['roles.permissions_no_match']()
          }
          pane="available"
        />

        <div className="flex items-center justify-center gap-2 md:flex-col">
          <Button
            variant="outline"
            size="icon"
            onClick={() => move(true)}
            disabled={disabled || leftChecked.length === 0}
            title={m['roles.permissions_move_right']({ count: leftChecked.length })}
            aria-label={m['roles.permissions_move_right']({ count: leftChecked.length })}
            data-testid="permission-move-right"
          >
            <ArrowRight className="h-4 w-4" />
          </Button>
          <Button
            variant="outline"
            size="icon"
            onClick={() => move(false)}
            disabled={disabled || rightChecked.length === 0}
            title={m['roles.permissions_move_left']({ count: rightChecked.length })}
            aria-label={m['roles.permissions_move_left']({ count: rightChecked.length })}
            data-testid="permission-move-left"
          >
            <ArrowLeft className="h-4 w-4" />
          </Button>
        </div>

        <TransferPane
          title={m['roles.permissions_selected']()}
          searchValue={rightSearch}
          onSearchChange={setRightSearch}
          visiblePermissions={visibleSelected}
          checkedIds={rightChecked}
          onToggleItem={toggleMarked}
          onMarkIds={markIds}
          isItemLocked={isLockedOnSelected}
          disabled={disabled}
          emptyMessage={
            selectedPermissions.length === 0
              ? m['roles.permissions_selected_empty']()
              : m['roles.permissions_no_match']()
          }
          pane="selected"
        />
      </div>
    </div>
  )
}
