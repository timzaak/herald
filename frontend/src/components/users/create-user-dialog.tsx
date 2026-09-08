import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import { createUserSchema, type CreateUserFormData } from '@/lib/schemas/common'
import { createUser2 } from '@/lib/api-generated'
import { useFormMutation } from '@/hooks/use-form-mutation'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Checkbox } from '@/components/ui/checkbox'
import { getFieldErrorMessage } from '@/lib/form-utils'
import { USER_STATUS, getWritableUserStatusOptions } from '@/lib/constants/user'
import { useQuery } from '@tanstack/react-query'
import { useRealmId } from '@/stores/auth-store'
import { adminRolesQueryOptions, queryKeys } from '@/data/query-options'
import { TextField } from '@/components/shared/form-fields'
import { m } from '@/paraglide/messages'

interface CreateUserDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  realmId?: string // Optional for backward compatibility
}

const DEFAULT_STATUS = USER_STATUS.NORMAL

export function CreateUserDialog({
  open,
  onOpenChange,
  realmId: realmIdProp,
}: CreateUserDialogProps) {
  const storeRealmId = useRealmId()
  const realmId = realmIdProp ?? storeRealmId
  // Fetch roles to get the correct role UUIDs
  const { data: rolesData } = useQuery({
    ...adminRolesQueryOptions(realmId),
    enabled: open,
    select: (data) => data ?? [],
  })

  // Find the default user role
  // Use configurable role name to avoid hardcoding
  const DEFAULT_ROLE_TYPE = import.meta.env.VITE_DEFAULT_ROLE_TYPE || 'user'
  const userRole = rolesData?.find((r) => r.name.toLowerCase() === DEFAULT_ROLE_TYPE.toLowerCase())
  const userRoleId = userRole?.id

  const { isSubmitting, mutate } = useFormMutation({
    mutationFn: (data: CreateUserFormData) =>
      createUser2({
        path: { realmId },
        body: data, // Direct pass - no manual mapping needed
      }),
    getSuccessMessage: (response) => {
      const user = response.data
      return user
        ? m['users.user_created']({ email: user.email })
        : m['users.user_created_fallback']()
    },
    invalidateQueries: [queryKeys.usersList(realmId)],
    onSuccess: () => {
      onOpenChange(false)
    },
  })

  const form = useAppForm({
    schema: createUserSchema,
    defaultValues: {
      email: '',
      password: '',
      nickname: undefined,
      status: DEFAULT_STATUS,
      roleIds: [],
    },
    onSubmit: async ({ value }) => {
      await mutate(value)
    },
  })

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[425px]" data-testid="dialog">
        <DialogHeader>
          <DialogTitle data-testid="dialog-title">{m['users.add_title']()}</DialogTitle>
          <DialogDescription>{m['users.add_description']()}</DialogDescription>
        </DialogHeader>

        <AppForm>
          <form
            onSubmit={(e) => {
              e.preventDefault()
              e.stopPropagation()
              form.handleSubmit()
            }}
            className="space-y-4"
          >
            <TextField
              form={form}
              name="email"
              label={m['users.form_email']()}
              inputId="email"
              type="email"
              dataTestId="email-input"
            />

            <TextField
              form={form}
              name="password"
              label={m['users.form_password']()}
              inputId="password"
              type="password"
              dataTestId="password-input"
            />

            <TextField
              form={form}
              name="nickname"
              label={m['users.form_nickname']()}
              inputId="nickname"
              dataTestId="nickname-input"
            />

            <form.Field
              name="status"
              children={(field) => (
                <div className="space-y-2">
                  <Label htmlFor="status">{m['users.form_status']()}</Label>
                  <Select
                    value={String(field.state.value ?? '')}
                    onValueChange={(value) => field.handleChange(Number(value))}
                    data-testid="user-create-status-select"
                  >
                    <SelectTrigger>
                      <SelectValue placeholder={m['users.form_select_status']()} />
                    </SelectTrigger>
                    <SelectContent>
                      {getWritableUserStatusOptions().map((option) => (
                        <SelectItem key={option.value} value={option.value}>
                          {option.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  {(field.state.meta.isTouched || form.state.isSubmitted) &&
                    field.state.meta.errors.length > 0 && (
                      <p className="text-sm text-destructive">
                        {getFieldErrorMessage(field.state.meta)}
                      </p>
                    )}
                </div>
              )}
            />

            <form.Field
              name="roleIds"
              children={(field) => {
                const roleIdsValue = Array.isArray(field.state.value) ? field.state.value : []
                return (
                  <div className="space-y-2">
                    <Label>{m['users.form_roles']()}</Label>
                    <div className="space-y-2">
                      {userRoleId && (
                        <div className="flex items-center space-x-2">
                          <Checkbox
                            id="role-user"
                            checked={roleIdsValue.includes(userRoleId)}
                            onCheckedChange={(checked) => {
                              const newValue = checked
                                ? [...roleIdsValue, userRoleId]
                                : roleIdsValue.filter((id) => id !== userRoleId)
                              field.handleChange(newValue)
                            }}
                            data-testid="user-create-role-checkbox"
                          />
                          <Label htmlFor="role-user" className="text-sm font-normal">
                            {m['users.form_role_user']()}
                          </Label>
                        </div>
                      )}
                    </div>
                    {(field.state.meta.isTouched || form.state.isSubmitted) &&
                      field.state.meta.errors.length > 0 && (
                        <p className="text-sm text-destructive">
                          {getFieldErrorMessage(field.state.meta)}
                        </p>
                      )}
                  </div>
                )
              }}
            />

            <DialogFooter>
              <Button
                type="button"
                onClick={() => onOpenChange(false)}
                variant="outline"
                data-testid="dialog-cancel-button"
              >
                {m['common.cancel']()}
              </Button>
              <Button type="submit" disabled={isSubmitting} data-testid="dialog-submit-button">
                {isSubmitting ? m['users.form_adding']() : m['users.add_button']()}
              </Button>
            </DialogFooter>
          </form>
        </AppForm>
      </DialogContent>
    </Dialog>
  )
}
