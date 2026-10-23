import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import { createRealmSchema, type CreateRealmFormData } from '@/lib/schemas/realm'
import { createRealm2 } from '@/lib/api-generated'
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
import { Input } from '@/components/ui/input'
import { PasswordInput } from '@/components/ui/password-input'
import { Textarea } from '@/components/ui/textarea'
import { Label } from '@/components/ui/label'
import { getFieldErrorMessage } from '@/lib/form-utils'
import { queryKeys } from '@/data/query-options'
import { m } from '@/paraglide/messages'

interface CreateRealmDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function CreateRealmDialog({ open, onOpenChange }: CreateRealmDialogProps) {
  const { isSubmitting, mutate } = useFormMutation({
    mutationFn: (data: CreateRealmFormData) =>
      createRealm2({
        body: data,
      }),
    getSuccessMessage: (response) => {
      const name = response.data?.name ?? 'Realm'
      return m['realms.realm_created_success']({ name })
    },
    invalidateQueries: [queryKeys.realmsList()],
    onSuccess: () => {
      onOpenChange(false)
    },
  })

  const form = useAppForm({
    schema: createRealmSchema,
    defaultValues: {
      name: '',
      description: '',
      adminUser: {
        email: '',
        password: '',
      },
    },
    onSubmit: async ({ value }) => {
      await mutate(value)
    },
  })

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[425px]" data-testid="dialog">
        <DialogHeader>
          <DialogTitle data-testid="dialog-title">{m['realms.create_title']()}</DialogTitle>
          <DialogDescription>{m['realms.create_description']()}</DialogDescription>
        </DialogHeader>

        <AppForm>
          <form
            className="space-y-4"
            onSubmit={(e) => {
              e.preventDefault()
              e.stopPropagation()
              form.handleSubmit()
            }}
          >
            {/* Realm ID */}
            <form.Field
              name="id"
              children={(field) => (
                <div className="space-y-2">
                  <Label htmlFor="realm-id">{m['realms.realm_id_label']()}</Label>
                  <Input
                    id="realm-id"
                    value={field.state.value ?? ''}
                    onChange={(e) => field.handleChange(e.target.value)}
                    data-testid="realm-create-id-input"
                  />
                  {(field.state.meta.isTouched || form.state.isSubmitted) &&
                    field.state.meta.errors.length > 0 && (
                      <p className="text-sm text-destructive">
                        {getFieldErrorMessage(field.state.meta)}
                      </p>
                    )}
                </div>
              )}
            />

            {/* Realm Name */}
            <form.Field
              name="name"
              children={(field) => (
                <div className="space-y-2">
                  <Label htmlFor="realm-name">{m['realms.realm_name_label']()}</Label>
                  <Input
                    id="realm-name"
                    value={field.state.value ?? ''}
                    onChange={(e) => field.handleChange(e.target.value)}
                    data-testid="realm-create-name-input"
                  />
                  {(field.state.meta.isTouched || form.state.isSubmitted) &&
                    field.state.meta.errors.length > 0 && (
                      <p className="text-sm text-destructive">
                        {getFieldErrorMessage(field.state.meta)}
                      </p>
                    )}
                </div>
              )}
            />

            {/* Description */}
            <form.Field
              name="description"
              children={(field) => (
                <div className="space-y-2">
                  <Label htmlFor="realm-description">{m['realms.description_label']()}</Label>
                  <Textarea
                    id="realm-description"
                    value={field.state.value ?? ''}
                    onChange={(e) => field.handleChange(e.target.value)}
                    data-testid="realm-create-description-input"
                    rows={3}
                    placeholder={m['realms.description_placeholder']()}
                  />
                </div>
              )}
            />

            {/* Admin Email */}
            <form.Field
              name="adminUser.email"
              children={(field) => (
                <div className="space-y-2">
                  <Label htmlFor="admin-email">{m['realms.admin_email_label']()}</Label>
                  <Input
                    id="admin-email"
                    type="email"
                    value={field.state.value ?? ''}
                    onChange={(e) => field.handleChange(e.target.value)}
                    data-testid="realm-create-admin-email-input"
                  />
                  {(field.state.meta.isTouched || form.state.isSubmitted) &&
                    field.state.meta.errors.length > 0 && (
                      <p className="text-sm text-destructive">
                        {getFieldErrorMessage(field.state.meta)}
                      </p>
                    )}
                </div>
              )}
            />

            {/* Admin Password */}
            <form.Field
              name="adminUser.password"
              children={(field) => (
                <div className="space-y-2">
                  <Label htmlFor="admin-password">{m['realms.admin_password_label']()}</Label>
                  <PasswordInput
                    id="admin-password"
                    value={field.state.value ?? ''}
                    onChange={(e) => field.handleChange(e.target.value)}
                    data-testid="realm-create-admin-password-input"
                  />
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
                variant="outline"
                onClick={() => onOpenChange(false)}
                data-testid="dialog-cancel-button"
              >
                {m['realms.cancel']()}
              </Button>
              <Button type="submit" disabled={isSubmitting} data-testid="dialog-submit-button">
                {isSubmitting ? m['realms.creating']() : m['realms.create_realm']()}
              </Button>
            </DialogFooter>
          </form>
        </AppForm>
      </DialogContent>
    </Dialog>
  )
}
