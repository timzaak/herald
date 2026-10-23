import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { z } from 'zod'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import { PasswordField } from '@/components/shared/form-fields/password-field'
import { deleteAccountMutation } from '@/data/query-options'
import { clearAuthStorage, useAuthStore } from '@/stores/auth-store'
import { m } from '@/paraglide/messages'

const deleteAccountSchema = z.object({
  password: z.string().min(1, { error: () => m['security.delete_account.password_required']() }),
})

type DeleteAccountFormData = z.infer<typeof deleteAccountSchema>

interface DeleteAccountDialogProps {
  realmId: string
  open: boolean
  onOpenChange: (open: boolean) => void
}

interface ApiError {
  status?: number
  message?: string
}

function getErrorMessage(error: ApiError | null): string {
  if (!error) return ''
  switch (error.status) {
    case 401:
      return m['security.delete_account.error_401']()
    case 409:
      return m['security.delete_account.error_409']()
    default:
      return m['security.delete_account.error_generic']()
  }
}

export function DeleteAccountDialog({ realmId, open, onOpenChange }: DeleteAccountDialogProps) {
  const queryClient = useQueryClient()
  const [apiError, setApiError] = useState<ApiError | null>(null)

  const form = useAppForm({
    schema: deleteAccountSchema,
    defaultValues: { password: '' },
    onSubmit: async ({ value }) => {
      setApiError(null)
      try {
        await mutation.mutateAsync(value)
      } catch {
        // Error is handled by mutation.onError and surfaced in the dialog.
      }
    },
  })

  const mutation = useMutation({
    // The form collects the user's password, which is used to obtain a
    // single-use delete_account reauth ticket; the request body only carries
    // the resulting `reauthToken`.
    mutationFn: (data: DeleteAccountFormData) => deleteAccountMutation(data.password),
    onSuccess: () => {
      // Clear all cached server state and persisted auth data so no authenticated
      // frontend state remains after the account is deleted.
      queryClient.clear()
      useAuthStore.getState().reset()
      clearAuthStorage()

      toast.success(m['security.delete_account.success_message']())
      window.location.href = `/${realmId}/auth/login`
    },
    onError: (error: ApiError) => {
      setApiError(error)
    },
  })

  const handleOpenChange = (nextOpen: boolean) => {
    if (!nextOpen) {
      setApiError(null)
      form.reset()
    }
    onOpenChange(nextOpen)
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="sm:max-w-lg" data-testid="delete-account-dialog">
        <DialogHeader>
          <DialogTitle data-testid="delete-account-dialog-title">
            {m['security.delete_account.dialog_title']()}
          </DialogTitle>
          <DialogDescription data-testid="delete-account-dialog-description">
            {m['security.delete_account.dialog_description']()}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <div className="rounded-md border border-destructive/50 bg-destructive/5 p-3">
            <p className="text-sm font-medium text-destructive">
              {m['security.delete_account.consequences_title']()}
            </p>
            <ul className="mt-2 list-disc space-y-1 pl-4 text-sm text-destructive/90">
              <li>{m['security.delete_account.consequence_anonymization']()}</li>
              <li>{m['security.delete_account.consequence_subscriptions']()}</li>
              <li>{m['security.delete_account.consequence_sessions']()}</li>
              <li>{m['security.delete_account.consequence_no_refund']()}</li>
              <li>{m['security.delete_account.consequence_no_recovery']()}</li>
            </ul>
          </div>

          {apiError && (
            <Alert variant="destructive" data-testid="delete-account-error-alert">
              <AlertDescription data-testid="delete-account-error-message">
                {getErrorMessage(apiError)}
              </AlertDescription>
            </Alert>
          )}

          <AppForm>
            <form
              onSubmit={(e) => {
                e.preventDefault()
                e.stopPropagation()
                form.handleSubmit()
              }}
              className="space-y-4"
            >
              <PasswordField
                form={form}
                name="password"
                label={m['security.delete_account.password_label']()}
                dataTestId="delete-account-password-input"
              />

              <DialogFooter className="flex-col-reverse gap-2 sm:flex-row sm:justify-end">
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => handleOpenChange(false)}
                  disabled={mutation.isPending}
                  data-testid="delete-account-cancel-button"
                >
                  {m['common.cancel']()}
                </Button>
                <Button
                  type="submit"
                  variant="destructive"
                  disabled={mutation.isPending}
                  loading={mutation.isPending}
                  data-testid="delete-account-submit-button"
                >
                  {mutation.isPending
                    ? m['security.delete_account.deleting']()
                    : m['security.delete_account.confirm_button']()}
                </Button>
              </DialogFooter>
            </form>
          </AppForm>
        </div>
      </DialogContent>
    </Dialog>
  )
}
