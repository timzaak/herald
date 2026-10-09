import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import { z } from 'zod'
import { updateProfile } from '@/lib/api-generated'
import { useFormMutation } from '@/hooks/use-form-mutation'
import { Button } from '@/components/ui/button'
import { TextField } from '@/components/shared/form-fields/text-field'
import { queryKeys } from '@/data/query-options'
import { m } from '@/paraglide/messages'
import { nicknameFieldSchema } from '@/lib/schemas/nickname'

type NicknameFormData = {
  nickname: string
}

const nicknameSchema = z.object({
  nickname: nicknameFieldSchema,
})

interface NicknameEditFormProps {
  initialNickname: string
}

export function NicknameEditForm({ initialNickname }: NicknameEditFormProps) {
  const form = useAppForm({
    schema: nicknameSchema,
    defaultValues: { nickname: initialNickname },
    onSubmit: async ({ value }) => {
      await mutate(value)
    },
  })

  const { isSubmitting, mutate } = useFormMutation({
    mutationFn: async (data: NicknameFormData) => {
      const response = await updateProfile({ body: { nickname: data.nickname } })
      if (response.error) throw response.error
      return response.data
    },
    getSuccessMessage: () => m['profile.nickname_updated_success'](),
    invalidateQueries: [queryKeys.profile()],
  })

  return (
    <AppForm>
      <form
        onSubmit={(e) => {
          e.preventDefault()
          e.stopPropagation()
          form.handleSubmit()
        }}
        className="max-w-sm space-y-3"
      >
        <TextField
          form={form}
          name="nickname"
          label={m['profile.nickname_label']()}
          dataTestId="nickname-input"
          disabled={isSubmitting}
        />
        <Button
          type="submit"
          disabled={isSubmitting}
          data-testid="nickname-save-button"
          className="h-9 w-full md:w-auto"
        >
          {isSubmitting ? m['realm_config.saving']() : m['realm_config.save']()}
        </Button>
      </form>
    </AppForm>
  )
}
