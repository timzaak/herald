import { useEffect } from 'react'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import { updateUserSchema, type UpdateUserFormData } from '@/lib/schemas/common'
import { updateUser } from '@/lib/api-generated'
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
import { getFieldErrorMessage } from '@/lib/form-utils'
import { getWritableUserStatusOptions } from '@/lib/constants/user'
import { useRealmId } from '@/stores/auth-store'
import type { UserResponse } from '@/lib/api-generated'
import { queryKeys } from '@/data/query-options'
import { TextField } from '@/components/shared/form-fields'
import { m } from '@/paraglide/messages'

interface EditUserDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  realmId?: string // Optional for backward compatibility
  user: UserResponse
}

export function EditUserDialog({
  open,
  onOpenChange,
  realmId: realmIdProp,
  user,
}: EditUserDialogProps) {
  const storeRealmId = useRealmId()
  const realmId = realmIdProp ?? storeRealmId
  const { isSubmitting, mutate } = useFormMutation({
    mutationFn: (data: UpdateUserFormData) =>
      updateUser({
        path: { realmId, userId: user.id },
        body: data,
      }),
    getSuccessMessage: () => m['users.user_updated']({ email: user.email }),
    invalidateQueries: [queryKeys.usersList(realmId)],
    onSuccess: () => {
      onOpenChange(false)
    },
  })

  const form = useAppForm({
    schema: updateUserSchema,
    defaultValues: {
      email: user.email,
      nickname: user.nickname ?? '',
      status: user.status,
    },
    onSubmit: async ({ value }) => {
      await mutate(value)
    },
  })

  // Reset form when user data changes or dialog opens
  useEffect(() => {
    if (open) {
      form.reset({
        email: user.email,
        nickname: user.nickname ?? '',
        status: user.status,
      })
    }
  }, [form, open, user])

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[425px]" data-testid="user-edit-dialog">
        <DialogHeader>
          <DialogTitle data-testid="user-edit-dialog-title">{m['users.edit_title']()}</DialogTitle>
          <DialogDescription>{m['users.edit_description']()}</DialogDescription>
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
              dataTestId="user-edit-email-input"
              disabled
              helpText={m['users.form_email_readonly']()}
            />

            <TextField
              form={form}
              name="nickname"
              label={m['users.form_nickname']()}
              inputId="nickname"
              dataTestId="user-edit-nickname-input"
            />

            <form.Field
              name="status"
              children={(field) => (
                <div className="space-y-2">
                  <Label htmlFor="status">{m['users.form_status']()}</Label>
                  <Select
                    value={String(field.state.value ?? '')}
                    onValueChange={(value) => field.handleChange(Number(value))}
                    data-testid="user-edit-status-select"
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

            <DialogFooter>
              <Button
                type="button"
                onClick={() => onOpenChange(false)}
                variant="outline"
                data-testid="user-edit-cancel-button"
              >
                {m['common.cancel']()}
              </Button>
              <Button type="submit" disabled={isSubmitting} data-testid="user-edit-submit-button">
                {isSubmitting ? m['users.form_editing']() : m['users.edit_title']()}
              </Button>
            </DialogFooter>
          </form>
        </AppForm>
      </DialogContent>
    </Dialog>
  )
}
