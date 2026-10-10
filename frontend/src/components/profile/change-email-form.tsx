import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import { changeEmailSchema, type ChangeEmailFormData } from '@/lib/schemas/common'
import { changeEmailRequest } from '@/lib/api-generated'
import { obtainReauthToken } from '@/lib/reauth-flow'
import { useFormMutation } from '@/hooks/use-form-mutation'
import { publicConfigQueryOptions } from '@/data/query-options'
import { useResolvedRealmId } from '@/lib/realm-routing'
import { Button } from '@/components/ui/button'
import { TextField } from '@/components/shared/form-fields/text-field'
import { m } from '@/paraglide/messages'

export function ChangeEmailForm() {
  const realmId = useResolvedRealmId()
  const { data: publicConfig, isLoading } = useQuery(publicConfigQueryOptions(realmId))
  // The sidebar entry is hidden when the realm has no email channel; the flag
  // is re-checked here so a direct URL visit degrades to an explanation
  // instead of a dead-end 400 after the user already typed a password.
  const emailConfigured = publicConfig?.emailChannelConfigured === true
  const [sentTo, setSentTo] = useState<string | null>(null)

  const form = useAppForm({
    schema: changeEmailSchema,
    defaultValues: {
      newEmail: '',
      currentPass: '',
    },
    onSubmit: async ({ value }) => {
      await mutate(value)
      setSentTo(value.newEmail)
    },
  })

  const { isSubmitting, mutate } = useFormMutation({
    mutationFn: async (data: ChangeEmailFormData) => {
      // The request only mails a confirmation link: the change commits solely
      // when the user opens that link in this logged-in browser.
      const reauthToken = await obtainReauthToken('change_email', data.currentPass)
      return changeEmailRequest({
        path: { realmId },
        body: { newEmail: data.newEmail, reauthToken },
      })
    },
    getSuccessMessage: () => m['profile.change_email_sent_toast'](),
  })

  if (isLoading) {
    return <p data-testid="change-email-loading">{m['profile.loading']()}</p>
  }

  if (!emailConfigured) {
    return (
      <p className="max-w-lg text-sm text-muted-foreground" data-testid="change-email-unavailable">
        {m['profile.change_email_unavailable']()}
      </p>
    )
  }

  if (sentTo) {
    return (
      <p className="max-w-lg text-sm text-muted-foreground" data-testid="change-email-sent-notice">
        {m['profile.change_email_sent_notice']({ email: sentTo })}
      </p>
    )
  }

  return (
    <AppForm>
      <form
        onSubmit={(e) => {
          e.preventDefault()
          e.stopPropagation()
          form.handleSubmit()
        }}
        className="max-w-sm space-y-4"
        data-testid="change-email-form"
      >
        <TextField
          form={form}
          name="newEmail"
          label={m['profile.change_email_new_email_label']()}
          type="email"
          required
          dataTestId="change-email-new-input"
        />
        <TextField
          form={form}
          name="currentPass"
          label={m['profile.current_password_label']()}
          type="password"
          required
          dataTestId="change-email-password-input"
        />

        <Button
          type="submit"
          disabled={isSubmitting}
          data-testid="change-email-submit-button"
          className="h-11 w-full md:h-9 md:w-auto"
        >
          {isSubmitting
            ? m['profile.change_email_submitting']()
            : m['profile.change_email_submit_button']()}
        </Button>
      </form>
    </AppForm>
  )
}
