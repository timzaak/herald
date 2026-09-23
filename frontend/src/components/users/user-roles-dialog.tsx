import { useState, useEffect, useMemo } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { RoleSelector } from '@/components/shared/role-selector'
import { useRealmId } from '@/stores/auth-store'
import { adminRolesQueryOptions, adminUserRolesQueryOptions, queryKeys } from '@/data/query-options'
import { updateUserRoles } from '@/lib/api-generated'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { m } from '@/paraglide/messages'

interface UserRolesDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  userId: string
  userEmail: string
}

export function UserRolesDialog({ open, onOpenChange, userId, userEmail }: UserRolesDialogProps) {
  const realmId = useRealmId()
  const queryClient = useQueryClient()
  const [selectedRoleIds, setSelectedRoleIds] = useState<string[]>([])
  const [isSaving, setIsSaving] = useState(false)

  // Fetch available roles
  const { data: rolesData, isLoading: isLoadingRoles } = useQuery(adminRolesQueryOptions(realmId))

  // Fetch user's current roles using admin API
  const { data: userRolesResponse, isLoading: isLoadingUserRoles } = useQuery({
    ...adminUserRolesQueryOptions(realmId, userId),
    enabled: open && !!realmId && !!userId,
  })

  // Derive role IDs from userRolesData (memoized to prevent infinite loops)
  const derivedRoleIds = useMemo(() => {
    if (!userRolesResponse?.data?.roles) return []
    // Map role objects to role IDs
    return userRolesResponse.data.roles.map((r) => r.id)
  }, [userRolesResponse])

  // Sync user roles data to local state when dialog opens or data loads
  useEffect(() => {
    const hasSameSelection =
      selectedRoleIds.length === derivedRoleIds.length &&
      selectedRoleIds.every((roleId, index) => roleId === derivedRoleIds[index])

    if (open && derivedRoleIds.length > 0 && !hasSameSelection) {
      setSelectedRoleIds(derivedRoleIds)
    }
  }, [open, derivedRoleIds, selectedRoleIds])

  const handleRoleChange = async (newRoleIds: string[]) => {
    setIsSaving(true)

    try {
      // Use PUT to replace all roles at once
      await updateUserRoles({
        path: { userId: userId },
        body: { roleIds: newRoleIds },
      })

      // Invalidate and refetch
      queryClient.invalidateQueries({ queryKey: queryKeys.adminUserRoles(realmId, userId) })
      queryClient.invalidateQueries({ queryKey: queryKeys.usersList(realmId) })

      setSelectedRoleIds(newRoleIds)
    } catch (error) {
      console.error('Failed to update user roles:', error)
      // Error will be handled by the API layer
    } finally {
      setIsSaving(false)
    }
  }

  const isLoading = isLoadingRoles || isLoadingUserRoles

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md" data-testid="user-roles-dialog-content">
        <DialogHeader>
          <DialogTitle data-testid="user-roles-dialog-title">
            {m['users.manage_roles_title']()}
          </DialogTitle>
          <DialogDescription data-testid="user-roles-dialog-user">{userEmail}</DialogDescription>
        </DialogHeader>

        {isLoading ? (
          <div className="py-8 text-center text-sm text-muted-foreground">
            {m['users.manage_roles_loading']()}
          </div>
        ) : (
          <div className="space-y-4">
            <div>
              <label
                className="mb-2 block text-sm font-medium"
                data-testid="user-roles-dialog-label"
              >
                {m['users.manage_roles_assign_label']()}
              </label>
              <RoleSelector
                roles={rolesData ?? []}
                selectedRoleIds={selectedRoleIds}
                onChange={handleRoleChange}
                disabled={isSaving}
                placeholder={m['users.manage_roles_select_placeholder']()}
              />
              <p className="mt-2 text-xs text-muted-foreground">{m['users.manage_roles_help']()}</p>
            </div>

            <DialogFooter className="border-t pt-4">
              <Button
                type="button"
                onClick={() => onOpenChange(false)}
                disabled={isSaving}
                variant="outline"
                data-testid="user-roles-dialog-cancel"
              >
                {m['common.close']()}
              </Button>
            </DialogFooter>
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}
