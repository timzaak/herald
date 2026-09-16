import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import { changePasswordSchema, type ChangePasswordFormData } from '@/lib/schemas/common'
import { changeUserPassword } from '@/lib/api-generated'
import { obtainReauthToken } from '@/lib/reauth-flow'
import { useFormMutation } from '@/hooks/use-form-mutation'
import { Button } from '@/components/ui/button'
import { TextField } from '@/components/shared/form-fields/text-field'
import { queryKeys } from '@/data/query-options'
import { m } from '@/paraglide/messages'

export function ChangePasswordForm() {
  const form = useAppForm({
    schema: changePasswordSchema,
    defaultValues: {
      oldPass: '',
      newPass: '',
      confirmPass: '',
    },
    onSubmit: async ({ value }) => {
      await mutate(value)
    },
  })

  const { isSubmitting, mutate } = useFormMutation({
    mutationFn: async (data: ChangePasswordFormData) => {
      // High-risk op (change_password): obtain a single-use reauth ticket using
      // the user's current password, then submit the change with it.
      const reauthToken = await obtainReauthToken('change_password', data.oldPass)
      return changeUserPassword({
        body: { ...data, reauthToken },
      })
    },
    getSuccessMessage: () => m['profile.password_changed_success'](),
    invalidateQueries: [queryKeys.profile()],
    onSuccess: () => {
      form.reset()
    },
  })

  return (
    <section>
      <h2 className="text-base font-semibold">{m['profile.change_password_title']()}</h2>
      <div className="mt-4 border-t border-border pt-4">
        <AppForm>
          <form
            onSubmit={(e) => {
              e.preventDefault()
              e.stopPropagation()
              form.handleSubmit()
            }}
            className="max-w-sm space-y-4"
          >
            <TextField
              form={form}
              name="oldPass"
              label={m['profile.current_password_label']()}
              type="password"
              dataTestId="change-password-old-input"
            />
            <TextField
              form={form}
              name="newPass"
              label={m['profile.new_password_label']()}
              type="password"
              dataTestId="change-password-new-input"
            />
            <TextField
              form={form}
              name="confirmPass"
              label={m['profile.confirm_password_label']()}
              type="password"
              dataTestId="change-password-confirm-input"
            />

            <Button
              type="submit"
              disabled={isSubmitting}
              data-testid="change-password-submit-button"
              className="h-11 w-full md:h-9 md:w-auto"
            >
              {isSubmitting ? m['profile.changing']() : m['profile.change_password_button']()}
            </Button>
          </form>
        </AppForm>
      </div>
    </section>
  )
}
